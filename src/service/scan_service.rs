use chrono::Local;
use reqwest::Client;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tracing::{error, info};

use crate::core::{
    compute_box, compute_control, compute_fund, compute_volume, generate_trade_plan, match_hot,
    score_candidate,
};
use crate::datasource::{
    fetch_concepts, fetch_crypto_kline, fetch_crypto_tickers, fetch_fund_flow, fetch_holder,
    fetch_hot_topics, fetch_kline, fetch_quote, fetch_universe,
};
use crate::models::{Candidate, Quote, ScanPayload, SystemConfig};
use crate::storage::{add_pool_stock, get_pool_stocks, save_klines, save_scan_record};

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

pub async fn run_scan_task(state: AppState, mode: String) {
    if state.scanning.swap(true, Ordering::SeqCst) {
        return;
    }

    let log_msg = format!("手动/定时触发扫描：{}...", mode);
    {
        let mut logs = state.scan_log.write().await;
        logs.push(format!("{}  {}", Local::now().format("%Y-%m-%d %H:%M:%S"), log_msg));
    }
    info!("{}", log_msg);

    if mode == "crypto" {
        run_crypto_scan(state.clone()).await;
    } else {
        run_stock_scan(state.clone(), &mode).await;
    }

    state.scanning.store(false, Ordering::SeqCst);
}

pub async fn run_stock_scan(state: AppState, mode: &str) {
    let (hot_topics, hot_names) = fetch_hot_topics(&state.client, 10).await;

    let mut targets: Vec<(String, String, String)> = Vec::new();
    if mode == "market" || mode == "quick" {
        {
            let mut logs = state.scan_log.write().await;
            logs.push(format!("{}  正在拉取全市场股票清单...", Local::now().format("%Y-%m-%d %H:%M:%S")));
        }
        if let Ok(mut all_stocks) = fetch_universe(&state.client).await {
            if mode == "quick" {
                all_stocks.sort_by(|a, b| b.volume_ratio.partial_cmp(&a.volume_ratio).unwrap_or(std::cmp::Ordering::Equal));
                all_stocks.truncate(200);
            }
            let total_count = all_stocks.len();
            {
                let mut logs = state.scan_log.write().await;
                logs.push(format!("{}  已获取 {} 只标的，开始并发分析...", Local::now().format("%Y-%m-%d %H:%M:%S"), total_count));
            }
            for s in all_stocks {
                targets.push((s.code, s.name, String::new()));
            }
        }
    } else if mode == "pool" {
        if let Some(pool) = &state.pg_pool {
            if let Ok(stocks) = get_pool_stocks(pool).await {
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
    let completed_count = Arc::new(AtomicUsize::new(0));

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
            let quote = fetch_quote(&client, &code).await.ok()?;
            let bars = fetch_kline(&client, &code, 240).await.ok()?;
            if let Some(ref pool) = pg_pool_c {
                let _ = save_klines(pool, &code, "stock", &bars).await;
            }
            let ffs = fetch_fund_flow(&client, &code, 11).await;
            let holder = fetch_holder(&client, &code).await;
            let concepts = fetch_concepts(&client, &code).await;

            let vol = compute_volume(&bars);
            let box_res = compute_box(&bars);
            let fund = compute_fund(&ffs);
            let ctrl = compute_control(quote.turnover, holder.as_ref());
            let hot_hits = match_hot(&concepts, &hot_names_c);
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
                trade_plan: None,
            };

            let mut scored = score_candidate(cand);
            scored.trade_plan = generate_trade_plan(scored.price, box_res.as_ref(), scored.score, scored.volume_ratio);

            {
                let mut logs = scan_log_c.write().await;
                logs.push(format!(
                    "{}  [{}/{}] 同步 {} {} | 评分: {} ({})",
                    Local::now().format("%H:%M:%S"),
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
        match save_scan_record(pool, mode, "stock", &candidates, &hot_topics).await {
            Ok(rec_id) => info!("扫描结果已持久化至 PostgreSQL, record_id: {}", rec_id),
            Err(e) => error!("保存扫描结果至 PostgreSQL 失败: {}", e),
        }
        for c in candidates.iter().filter(|c| c.qualified || c.score >= 80) {
            let _ = add_pool_stock(pool, &c.code, &c.name, &c.concepts.join("/")).await;
        }
    }

    let now_str = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
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
        logs.push(format!("{}  {}", Local::now().format("%Y-%m-%d %H:%M:%S"), finish_msg));
        *state.last_scan.write().await = Some(now_str);
    }
    info!("{}", finish_msg);
}

pub async fn run_crypto_scan(state: AppState) {
    let tickers = fetch_crypto_tickers(&state.client).await;
    let top_tickers: Vec<Quote> = tickers.into_iter().take(30).collect();

    let mut candidate_futures = Vec::new();
    for t in top_tickers {
        let client = state.client.clone();
        let pg_pool_c = state.pg_pool.clone();
        candidate_futures.push(tokio::spawn(async move {
            let bars = fetch_crypto_kline(&client, &t.name, 240).await;
            if bars.len() < 40 {
                return None;
            }
            if let Some(ref pool) = pg_pool_c {
                let _ = save_klines(pool, &t.name, "crypto", &bars).await;
            }

            let vol = compute_volume(&bars);
            let box_res = compute_box(&bars);

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
                trade_plan: None,
            };

            let mut scored = score_candidate(cand);
            scored.trade_plan = generate_trade_plan(scored.price, box_res.as_ref(), scored.score, scored.volume_ratio);
            Some(scored)
        }));
    }

    let mut candidates = Vec::new();
    for f in candidate_futures {
        if let Ok(Some(cand)) = f.await {
            candidates.push(cand);
        }
    }
    candidates.sort_by(|a, b| b.chg.partial_cmp(&a.chg).unwrap_or(std::cmp::Ordering::Equal));

    let qualified_count = candidates.iter().filter(|c| c.qualified).count();
    let total = candidates.len();

    if let Some(pool) = &state.pg_pool {
        let _ = save_scan_record(pool, "crypto", "crypto", &candidates, &[]).await;
    }

    let now_str = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
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

    info!("币圈扫描完成：共 {} 币，达标 {} 币", total, qualified_count);
}
