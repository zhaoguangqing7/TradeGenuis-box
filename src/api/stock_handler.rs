use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crate::core::compute_box;
use crate::datasource::{fetch_crypto_kline, fetch_hot_topics, fetch_kline, fetch_quote};
use crate::service::AppState;
use crate::storage::{add_pool_stock, get_klines, get_latest_scan, get_pool_stocks, remove_pool_stock, save_system_config};

pub async fn get_watchlist(State(state): State<AppState>) -> impl IntoResponse {
    if let Some(pool) = &state.pg_pool {
        if let Ok(Some(data)) = get_latest_scan(pool, "stock").await {
            return Json(serde_json::to_value(data).unwrap_or_default()).into_response();
        }
    }
    if let Ok(content) = tokio::fs::read_to_string("data/watchlist.json").await {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
            return Json(val).into_response();
        }
    }
    Json(serde_json::json!({ "as_of": null, "candidates": [] })).into_response()
}

pub async fn get_crypto(State(state): State<AppState>) -> impl IntoResponse {
    if let Some(pool) = &state.pg_pool {
        if let Ok(Some(data)) = get_latest_scan(pool, "crypto").await {
            return Json(serde_json::to_value(data).unwrap_or_default()).into_response();
        }
    }
    if let Ok(content) = tokio::fs::read_to_string("data/crypto.json").await {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
            return Json(val).into_response();
        }
    }
    Json(serde_json::json!({ "as_of": null, "candidates": [] })).into_response()
}

pub async fn get_hot(State(state): State<AppState>) -> impl IntoResponse {
    let (hot, _) = fetch_hot_topics(&state.client, 10).await;
    Json(serde_json::json!({ "hot_topics": hot })).into_response()
}

pub async fn get_pool(State(state): State<AppState>) -> impl IntoResponse {
    if let Some(pool) = &state.pg_pool {
        if let Ok(stocks) = get_pool_stocks(pool).await {
            return Json(serde_json::json!({ "stocks": stocks })).into_response();
        }
    }
    if let Ok(content) = tokio::fs::read_to_string("data/pool.json").await {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
            return Json(val).into_response();
        }
    }
    Json(serde_json::json!({ "stocks": [] })).into_response()
}

#[derive(Deserialize)]
pub struct PoolActionReq {
    pub action: String,
    pub code: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub theme: String,
}

pub async fn post_pool(
    State(state): State<AppState>,
    Json(payload): Json<PoolActionReq>,
) -> impl IntoResponse {
    let code = payload.code.trim().to_string();
    if code.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": "code required" }))).into_response();
    }

    if let Some(pool) = &state.pg_pool {
        if payload.action == "add" {
            let mut name = payload.name.trim().to_string();
            if name.is_empty() {
                if let Ok(q) = fetch_quote(&state.client, &code).await {
                    name = q.name;
                } else {
                    name = code.clone();
                }
            }
            let _ = add_pool_stock(pool, &code, &name, &payload.theme).await;
        } else if payload.action == "remove" {
            let _ = remove_pool_stock(pool, &code).await;
        }
        if let Ok(stocks) = get_pool_stocks(pool).await {
            return Json(serde_json::json!({ "ok": true, "stocks": stocks })).into_response();
        }
    }

    Json(serde_json::json!({ "ok": true })).into_response()
}

pub async fn get_status(State(state): State<AppState>) -> impl IntoResponse {
    let scanning = state.scanning.load(Ordering::SeqCst);
    let last_scan = state.last_scan.read().await.clone();
    let logs = state.scan_log.read().await.clone();
    let keep_logs: Vec<String> = if logs.len() > 50 {
        logs[logs.len() - 50..].to_vec()
    } else {
        logs
    };
    let now = chrono::Local::now();
    let time_num = now.hour() * 100 + now.minute();
    let is_trading_time = now.weekday().number_from_monday() <= 5
        && ((time_num >= 930 && time_num <= 1130) || (time_num >= 1300 && time_num <= 1500));

    Json(serde_json::json!({
        "scanning": scanning,
        "last_scan": last_scan,
        "scan_log": keep_logs,
        "is_trading_time": is_trading_time,
    }))
}

use chrono::{Datelike, Timelike};

pub async fn get_config(State(state): State<AppState>) -> impl IntoResponse {
    let cfg = state.config.read().await.clone();
    Json(serde_json::json!({ "config": cfg })).into_response()
}

#[derive(Deserialize)]
pub struct ConfigReq {
    pub auto: Option<bool>,
}

pub async fn post_config(
    State(state): State<AppState>,
    Json(payload): Json<ConfigReq>,
) -> impl IntoResponse {
    let mut cfg = state.config.write().await;
    if let Some(a) = payload.auto {
        cfg.auto = a;
    }
    if let Some(pool) = &state.pg_pool {
        let _ = save_system_config(pool, &cfg).await;
    }
    Json(serde_json::json!({ "ok": true, "config": *cfg })).into_response()
}

#[derive(Deserialize)]
pub struct QuotesQuery {
    pub codes: Option<String>,
}

pub async fn get_quotes(
    State(state): State<AppState>,
    Query(query): Query<QuotesQuery>,
) -> impl IntoResponse {
    let codes_str = query.codes.unwrap_or_default();
    let codes: Vec<String> = codes_str
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .take(100)
        .collect();

    let mut result = HashMap::new();
    let mut need_fetch = Vec::new();

    {
        let cache = state.quote_cache.read().await;
        for c in &codes {
            if let Some((ts, val)) = cache.get(c) {
                if ts.elapsed() < Duration::from_millis(2500) {
                    result.insert(c.clone(), val.clone());
                    continue;
                }
            }
            need_fetch.push(c.clone());
        }
    }

    if !need_fetch.is_empty() {
        let mut futures = Vec::new();
        for c in need_fetch {
            let client = state.client.clone();
            futures.push(tokio::spawn(async move {
                let quote = fetch_quote(&client, &c).await.ok();
                (c, quote)
            }));
        }

        let mut cache = state.quote_cache.write().await;
        for f in futures {
            if let Ok((c, q_opt)) = f.await {
                let val = if let Some(q) = q_opt {
                    serde_json::json!({
                        "price": q.price,
                        "chg": q.chg,
                        "turnover": q.turnover,
                        "volume_ratio": q.volume_ratio,
                    })
                } else {
                    serde_json::Value::Null
                };
                cache.insert(c.clone(), (Instant::now(), val.clone()));
                result.insert(c, val);
            }
        }
    }

    Json(result).into_response()
}

#[derive(Deserialize)]
pub struct KlineQuery {
    pub code: Option<String>,
    pub market: Option<String>,
    pub lmt: Option<usize>,
}

pub async fn get_kline(
    State(state): State<AppState>,
    Query(query): Query<KlineQuery>,
) -> impl IntoResponse {
    let code = match query.code {
        Some(c) if !c.is_empty() => c,
        _ => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": "code required" }))).into_response(),
    };
    let market = query.market.unwrap_or_else(|| "stock".to_string());
    let lmt = query.lmt.unwrap_or(160).clamp(60, 500);

    let cache_key = format!("{}:{}:{}", market, code, lmt);
    {
        let cache = state.kline_cache.read().await;
        if let Some((ts, val)) = cache.get(&cache_key) {
            if ts.elapsed() < Duration::from_secs(60) {
                return Json(val.clone()).into_response();
            }
        }
    }

    let payload = if market == "crypto" {
        let bars = fetch_crypto_kline(&state.client, &code, lmt).await;
        let box_res = compute_box(&bars);
        let last_date = bars.last().map(|b| b.date.clone()).unwrap_or_default();
        let last_close = bars.last().map(|b| b.close).unwrap_or(0.0);
        serde_json::json!({
            "code": code,
            "name": code,
            "price": last_close,
            "chg": null,
            "turnover": null,
            "volume_ratio": null,
            "bar_date": last_date,
            "bars": bars,
            "box": box_res,
        })
    } else {
        let mut bars = if let Some(ref pool) = state.pg_pool {
            get_klines(pool, &code, &market, lmt).await.unwrap_or_default()
        } else {
            Vec::new()
        };

        if bars.len() < 30 {
            if let Ok(c_bars) = fetch_kline(&state.client, &code, lmt).await {
                if !c_bars.is_empty() {
                    bars = c_bars;
                }
            }
        }

        let box_res = compute_box(&bars);
        let last_date = bars.last().map(|b| b.date.clone()).unwrap_or_default();
        let last_close = bars.last().map(|b| b.close).unwrap_or(0.0);

        serde_json::json!({
            "code": code,
            "name": code,
            "price": last_close,
            "chg": null,
            "turnover": null,
            "volume_ratio": null,
            "bar_date": last_date,
            "bars": bars,
            "box": box_res,
        })
    };

    {
        let mut cache = state.kline_cache.write().await;
        cache.insert(cache_key, (Instant::now(), payload.clone()));
    }

    Json(payload).into_response()
}
