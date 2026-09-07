use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use reqwest::Client;
use serde::Deserialize;
use sqlx::PgPool;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tracing::{error, info};

use crate::crawler::{self, fetch_hot_topics, fetch_quote};
use crate::db;
use crate::engine::{compute_box, score_candidate};
use crate::models::{Candidate, Quote, ScanPayload, SystemConfig};

#[derive(Clone)]
pub struct AppState {
    pub pg_pool: Option<PgPool>,
    pub client: Client,
    pub quote_cache: Arc<RwLock<HashMap<String, (Instant, serde_json::Value)>>>,
    pub kline_cache: Arc<RwLock<HashMap<String, (Instant, serde_json::Value)>>>,
    pub scanning: Arc<AtomicBool>,
    pub scan_log: Arc<RwLock<Vec<String>>>,
    pub last_scan: Arc<RwLock<Option<String>>>,
    pub config: Arc<RwLock<SystemConfig>>,
}

pub fn create_router(state: AppState) -> Router {
    let static_dir = PathBuf::from("static");
    Router::new()
        .route("/", get(serve_index))
        .route("/index.html", get(serve_index))
        .route("/api/watchlist", get(get_watchlist))
        .route("/api/crypto", get(get_crypto))
        .route("/api/hot", get(get_hot))
        .route("/api/pool", get(get_pool).post(post_pool))
        .route("/api/status", get(get_status))
        .route("/api/config", get(get_config).post(post_config))
        .route("/api/quotes", get(get_quotes))
        .route("/api/kline", get(get_kline))
        .route("/api/scan", post(post_scan))
        .nest_service("/static", ServeDir::new(static_dir))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn serve_index() -> impl IntoResponse {
    match tokio::fs::read_to_string("dashboard.html").await {
        Ok(content) => (StatusCode::OK, [(header::CONTENT_TYPE, "text/html; charset=utf-8")], Html(content)).into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "dashboard.html not found").into_response(),
    }
}

async fn get_watchlist(State(state): State<AppState>) -> impl IntoResponse {
    if let Some(pool) = &state.pg_pool {
        if let Ok(Some(data)) = db::get_latest_scan(pool, "stock").await {
            return Json(serde_json::to_value(data).unwrap_or_default()).into_response();
        }
    }
    // Fallback: Read local watchlist.json if db unavailable
    if let Ok(content) = tokio::fs::read_to_string("data/watchlist.json").await {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
            return Json(val).into_response();
        }
    }
    Json(serde_json::json!({ "as_of": null, "candidates": [] })).into_response()
}

async fn get_crypto(State(state): State<AppState>) -> impl IntoResponse {
    if let Some(pool) = &state.pg_pool {
        if let Ok(Some(data)) = db::get_latest_scan(pool, "crypto").await {
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

async fn get_hot(State(state): State<AppState>) -> impl IntoResponse {
    let (hot, _) = fetch_hot_topics(&state.client, 10).await;
    Json(serde_json::json!({ "hot_topics": hot })).into_response()
}

async fn get_pool(State(state): State<AppState>) -> impl IntoResponse {
    if let Some(pool) = &state.pg_pool {
        if let Ok(stocks) = db::get_pool_stocks(pool).await {
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
struct PoolActionReq {
    action: String,
    code: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    theme: String,
}

async fn post_pool(
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
            let _ = db::add_pool_stock(pool, &code, &name, &payload.theme).await;
        } else if payload.action == "remove" {
            let _ = db::remove_pool_stock(pool, &code).await;
        }
        if let Ok(stocks) = db::get_pool_stocks(pool).await {
            return Json(serde_json::json!({ "ok": true, "stocks": stocks })).into_response();
        }
    }

    Json(serde_json::json!({ "ok": true })).into_response()
}

async fn get_status(State(state): State<AppState>) -> impl IntoResponse {
    let scanning = state.scanning.load(Ordering::SeqCst);
    let last_scan = state.last_scan.read().await.clone();
    let logs = state.scan_log.read().await.clone();
    let keep_logs: Vec<String> = if logs.len() > 50 {
        logs[logs.len() - 50..].to_vec()
    } else {
        logs
    };

    Json(serde_json::json!({
        "scanning": scanning,
        "last_scan": last_scan,
        "scan_log": keep_logs,
        "is_trading_time": is_trading_time(),
    })).into_response()
}

fn is_trading_time() -> bool {
    let now = chrono::Local::now();
    let weekday = now.format("%u").to_string().parse::<u32>().unwrap_or(1);
    if weekday >= 6 {
        return false;
    }
    let hm = now.format("%H%M").to_string().parse::<u32>().unwrap_or(0);
    (915..=1505).contains(&hm)
}

async fn get_config(State(state): State<AppState>) -> impl IntoResponse {
    let cfg = state.config.read().await.clone();
    Json(cfg).into_response()
}

async fn post_config(
    State(state): State<AppState>,
    Json(cfg): Json<SystemConfig>,
) -> impl IntoResponse {
    if let Some(pool) = &state.pg_pool {
        let _ = db::save_system_config(pool, &cfg).await;
    }
    *state.config.write().await = cfg.clone();
    Json(serde_json::json!({ "ok": true, "config": cfg })).into_response()
}

#[derive(Deserialize)]
struct QuotesQuery {
    codes: Option<String>,
}

async fn get_quotes(
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
struct KlineQuery {
    code: Option<String>,
    market: Option<String>,
    lmt: Option<usize>,
}

async fn get_kline(
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
        let bars = crawler::fetch_crypto_kline(&state.client, &code, lmt).await;
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
        let quote_res = fetch_quote(&state.client, &code).await.ok();
        let bars = crawler::fetch_kline(&state.client, &code, lmt).await.unwrap_or_default();
        let box_res = compute_box(&bars);
        let last_date = bars.last().map(|b| b.date.clone()).unwrap_or_default();

        let (name, price, chg, turnover, volume_ratio) = if let Some(q) = quote_res {
            (q.name, q.price, q.chg, Some(q.turnover), q.volume_ratio)
        } else {
            (code.clone(), 0.0, 0.0, None, 0.0)
        };

        serde_json::json!({
            "code": code,
            "name": name,
            "price": price,
            "chg": chg,
            "turnover": turnover,
            "volume_ratio": volume_ratio,
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

#[derive(Deserialize)]
struct ScanReq {
    mode: Option<String>,
}

async fn post_scan(
    State(state): State<AppState>,
    body: Option<Json<ScanReq>>,
) -> impl IntoResponse {
    if state.scanning.load(Ordering::SeqCst) {
        return Json(serde_json::json!({ "status": "running", "msg": "扫描进行中" })).into_response();
    }

    let mode = body.and_then(|Json(b)| b.mode).unwrap_or_else(|| "pool".to_string());
    let state_clone = state.clone();

    tokio::spawn(async move {
        run_scan_task(state_clone, mode).await;
    });

    Json(serde_json::json!({ "status": "started" })).into_response()
}

pub async fn run_scan_task(state: AppState, mode: String) {
    if state.scanning.swap(true, Ordering::SeqCst) {
        return;
    }

    let log_msg = format!("手动/定时触发扫描：{}...", mode);
    {
        let mut logs = state.scan_log.write().await;
        logs.push(format!("{}  {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), log_msg));
    }
    info!("{}", log_msg);

    if mode == "crypto" {
        run_crypto_scan(state.clone()).await;
    } else {
        run_stock_scan(state.clone(), &mode).await;
    }

    state.scanning.store(false, Ordering::SeqCst);
}

async fn run_stock_scan(state: AppState, mode: &str) {
    let (hot_topics, hot_names) = crawler::fetch_hot_topics(&state.client, 10).await;
    
    // 获取待扫标的
    let mut targets: Vec<(String, String, String)> = Vec::new();
    if mode == "market" || mode == "quick" {
        {
            let mut logs = state.scan_log.write().await;
            logs.push(format!("{}  正在拉取全市场股票清单...", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")));
        }
        if let Ok(mut all_stocks) = crawler::fetch_universe(&state.client).await {
            if mode == "quick" {
                all_stocks.sort_by(|a, b| b.volume_ratio.partial_cmp(&a.volume_ratio).unwrap_or(std::cmp::Ordering::Equal));
                all_stocks.truncate(200);
            }
            let total_count = all_stocks.len();
            {
                let mut logs = state.scan_log.write().await;
                logs.push(format!("{}  已获取 {} 只标的，开始并发分析...", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), total_count));
            }
            for s in all_stocks {
                targets.push((s.code, s.name, String::new()));
            }
        }
    } else if mode == "pool" {
        if let Some(pool) = &state.pg_pool {
            if let Ok(stocks) = db::get_pool_stocks(pool).await {
                for s in stocks {
                    targets.push((s.code, s.name, s.theme));
                }
            }
        }
        if targets.is_empty() {
            if let Ok(content) = tokio::fs::read_to_string("data/pool.json").await {
                if let Ok(f) = serde_json::from_str::<crate::models::PoolFile>(&content) {
                    for s in f.stocks {
                        targets.push((s.code, s.name, s.theme));
                    }
                }
            }
        }
    }

    if targets.is_empty() {
        // 默认内置测试股票池
        let defaults = vec![
            ("000001", "平安银行", "银行"),
            ("600519", "贵州茅台", "白酒"),
            ("300750", "宁德时代", "锂电池"),
            ("688981", "中芯国际", "半导体"),
            ("002594", "比亚迪", "新能源汽车"),
            ("002475", "立讯精密", "消费电子"),
        ];
        for (c, n, t) in defaults {
            targets.push((c.to_string(), n.to_string(), t.to_string()));
        }
    }

    let semaphore = Arc::new(tokio::sync::Semaphore::new(16));
    let total_targets = targets.len();
    let completed_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let mut candidate_futures = Vec::new();
    for (code, _name, theme) in targets {
        let client = state.client.clone();
        let hot_names_c = hot_names.clone();
        let pg_pool_c = state.pg_pool.clone();
        let sem = semaphore.clone();
        let done_counter = completed_count.clone();
        let scan_log_c = state.scan_log.clone();

        candidate_futures.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.ok()?;
            let quote = crawler::fetch_quote(&client, &code).await.ok()?;
            let bars = crawler::fetch_kline(&client, &code, 240).await.ok()?;
            if let Some(ref pool) = pg_pool_c {
                let _ = db::save_klines(pool, &code, "stock", &bars).await;
            }
            let ffs = crawler::fetch_fund_flow(&client, &code, 11).await;
            let holder = crawler::fetch_holder(&client, &code).await;
            let concepts = crawler::fetch_concepts(&client, &code).await;

            let vol = crate::engine::compute_volume(&bars);
            let box_res = crate::engine::compute_box(&bars);
            let fund = crate::engine::compute_fund(&ffs);
            let ctrl = crate::engine::compute_control(quote.turnover, holder.as_ref());
            let hot_hits = crate::engine::match_hot(&concepts, &hot_names_c);
            let theme_ok = !hot_hits.is_empty();

            let c = done_counter.fetch_add(1, Ordering::SeqCst) + 1;
            info!(
                "[{}/{}] 同步 {} {} (最新价: {}, 240根K线/资金/户数已入库)",
                c, total_targets, code, quote.name, quote.price
            );

            let cand = Candidate {
                code: code.clone(),
                name: quote.name.clone(),
                market: "stock".to_string(),
                price: quote.price,
                chg: quote.chg,
                turnover: Some(quote.turnover),
                volume_ratio: vol.volume_ratio,
                volume_days: vol.volume_days,
                box_low: box_res.as_ref().map(|b| b.box_low),
                box_high: box_res.as_ref().map(|b| b.box_high),
                pos_pct: box_res.as_ref().map(|b| b.pos_pct),
                box_span_pct: box_res.as_ref().map(|b| b.span_pct),
                box_window: box_res.as_ref().map(|b| b.window.clone()),
                tests: box_res.as_ref().map(|b| b.tests).unwrap_or(0),
                test_dates: box_res.as_ref().map(|b| b.test_dates.clone()).unwrap_or_default(),
                fund_5d: fund.fund_5d,
                inflow_days: Some(fund.inflow_days),
                fund_state: Some(fund.fund_state),
                fund_5d_str: Some(fund.fund_5d_str),
                control: Some(ctrl.control),
                control_note: Some(ctrl.control_note),
                holder_ratio: ctrl.holder_ratio,
                holder_date: ctrl.holder_date,
                concepts,
                hot_hits,
                theme_ok,
                theme_hint: Some(theme),
                score: 0,
                flags: Vec::new(),
                flag_pairs: Vec::new(),
                mode: String::new(),
                qualified: false,
            };

            let scored = score_candidate(cand);

            {
                let mut logs = scan_log_c.write().await;
                logs.push(format!(
                    "{}  [{}/{}] 同步 {} {} | 评分: {} ({})",
                    chrono::Local::now().format("%H:%M:%S"),
                    c,
                    total_targets,
                    code,
                    quote.name,
                    scored.score,
                    scored.mode
                ));
                if logs.len() > 60 {
                    let trim = logs.len() - 60;
                    logs.drain(0..trim);
                }
            }

            Some(scored)
        }));
    }

    let mut candidates = Vec::new();
    for f in candidate_futures {
        if let Ok(Some(cand)) = f.await {
            candidates.push(cand);
        }
    }
    candidates.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| b.chg.partial_cmp(&a.chg).unwrap_or(std::cmp::Ordering::Equal)));

    let qualified_count = candidates.iter().filter(|c| c.qualified).count();
    let total = candidates.len();

    // 存储至 PostgreSQL
    if let Some(pool) = &state.pg_pool {
        match db::save_scan_record(pool, mode, "stock", &candidates, &hot_topics).await {
            Ok(rec_id) => info!("扫描结果已持久化至 PostgreSQL, record_id: {}", rec_id),
            Err(e) => error!("保存扫描结果至 PostgreSQL 失败: {}", e),
        }
    }

    // 同时写回本地 JSON 备份
    let now_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let payload = ScanPayload {
        as_of: now_str.clone(),
        scan_type: mode.to_string(),
        total,
        qualified: qualified_count,
        candidates,
        hot_topics,
    };
    if let Ok(json_str) = serde_json::to_string_pretty(&payload) {
        let _ = tokio::fs::create_dir_all("data").await;
        let _ = tokio::fs::write("data/watchlist.json", json_str).await;
    }

    let finish_msg = format!("扫描完成：共 {} 只，达标 {} 只", total, qualified_count);
    {
        let mut logs = state.scan_log.write().await;
        logs.push(format!("{}  {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), finish_msg));
        *state.last_scan.write().await = Some(now_str);
    }
    info!("{}", finish_msg);
}

async fn run_crypto_scan(state: AppState) {
    let tickers = crawler::fetch_crypto_tickers(&state.client).await;
    let top_tickers: Vec<Quote> = tickers.into_iter().take(30).collect();

    let mut candidate_futures = Vec::new();
    for t in top_tickers {
        let client = state.client.clone();
        let pg_pool_c = state.pg_pool.clone();
        candidate_futures.push(tokio::spawn(async move {
            let bars = crawler::fetch_crypto_kline(&client, &t.name, 240).await;
            if bars.len() < 40 {
                return None;
            }
            if let Some(ref pool) = pg_pool_c {
                let _ = db::save_klines(pool, &t.name, "crypto", &bars).await;
            }
            let vol = crate::engine::compute_volume(&bars);
            let box_res = crate::engine::compute_box(&bars);

            let cand = Candidate {
                code: t.name.clone(),
                name: t.name.clone(),
                market: "crypto".to_string(),
                price: t.price,
                chg: t.chg,
                turnover: None,
                volume_ratio: vol.volume_ratio,
                volume_days: vol.volume_days,
                box_low: box_res.as_ref().map(|b| b.box_low),
                box_high: box_res.as_ref().map(|b| b.box_high),
                pos_pct: box_res.as_ref().map(|b| b.pos_pct),
                box_span_pct: box_res.as_ref().map(|b| b.span_pct),
                box_window: box_res.as_ref().map(|b| b.window.clone()),
                tests: box_res.as_ref().map(|b| b.tests).unwrap_or(0),
                test_dates: box_res.as_ref().map(|b| b.test_dates.clone()).unwrap_or_default(),
                fund_5d: None,
                inflow_days: None,
                fund_state: None,
                fund_5d_str: None,
                control: None,
                control_note: None,
                holder_ratio: None,
                holder_date: None,
                concepts: Vec::new(),
                hot_hits: Vec::new(),
                theme_ok: false,
                theme_hint: None,
                score: 0,
                flags: Vec::new(),
                flag_pairs: Vec::new(),
                mode: String::new(),
                qualified: false,
            };

            Some(score_candidate(cand))
        }));
    }

    let mut candidates = Vec::new();
    for f in candidate_futures {
        if let Ok(Some(c)) = f.await {
            candidates.push(c);
        }
    }
    candidates.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| b.chg.partial_cmp(&a.chg).unwrap_or(std::cmp::Ordering::Equal)));

    let qualified_count = candidates.iter().filter(|c| c.qualified).count();
    let total = candidates.len();

    if let Some(pool) = &state.pg_pool {
        let _ = db::save_scan_record(pool, "crypto", "crypto", &candidates, &[]).await;
    }

    let now_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let payload = ScanPayload {
        as_of: now_str.clone(),
        scan_type: "crypto".to_string(),
        total,
        qualified: qualified_count,
        candidates,
        hot_topics: Vec::new(),
    };
    if let Ok(json_str) = serde_json::to_string_pretty(&payload) {
        let _ = tokio::fs::create_dir_all("data").await;
        let _ = tokio::fs::write("data/crypto.json", json_str).await;
    }

    let finish_msg = format!("币圈扫描完成：共 {} 只，达标 {} 只", total, qualified_count);
    {
        let mut logs = state.scan_log.write().await;
        logs.push(format!("{}  {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), finish_msg));
        *state.last_scan.write().await = Some(now_str);
    }
    info!("{}", finish_msg);
}
