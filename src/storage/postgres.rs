use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use tracing::info;

use crate::models::{Candidate, ConceptBoard, KlineBar, PoolItem, ScanPayload, SystemConfig, TradePlan};

pub async fn init_pool(db_url: &str) -> Result<PgPool, sqlx::Error> {
    info!("正在连接 PostgreSQL 数据库...");
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .min_connections(2)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(db_url)
        .await?;

    info!("PostgreSQL 连接成功，开始执行表结构初始化...");
    init_schema(&pool).await?;
    info!("PostgreSQL 表结构检查/初始化完毕");

    Ok(pool)
}

pub async fn init_schema(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS pool (
            id SERIAL PRIMARY KEY,
            code VARCHAR(20) NOT NULL UNIQUE,
            name VARCHAR(100) NOT NULL,
            theme VARCHAR(255) DEFAULT '',
            created_at TIMESTAMPTZ DEFAULT NOW(),
            updated_at TIMESTAMPTZ DEFAULT NOW()
        );

        CREATE TABLE IF NOT EXISTS scan_records (
            id BIGSERIAL PRIMARY KEY,
            market VARCHAR(20) NOT NULL,
            scan_type VARCHAR(50) NOT NULL,
            total_scanned INT NOT NULL,
            total_qualified INT NOT NULL,
            hot_topics JSONB,
            created_at TIMESTAMPTZ DEFAULT NOW()
        );

        CREATE TABLE IF NOT EXISTS scan_candidates (
            id BIGSERIAL PRIMARY KEY,
            scan_record_id BIGINT REFERENCES scan_records(id) ON DELETE CASCADE,
            code VARCHAR(20) NOT NULL,
            name VARCHAR(100) NOT NULL,
            market VARCHAR(20) NOT NULL,
            price FLOAT8 NOT NULL,
            chg FLOAT8 NOT NULL,
            turnover FLOAT8,
            score INT NOT NULL,
            mode VARCHAR(50) NOT NULL,
            qualified BOOLEAN NOT NULL,
            box_low FLOAT8,
            box_high FLOAT8,
            pos_pct FLOAT8,
            box_span_pct FLOAT8,
            box_window VARCHAR(100),
            tests INT NOT NULL DEFAULT 0,
            test_dates JSONB,
            volume_days INT NOT NULL DEFAULT 0,
            volume_ratio FLOAT8 NOT NULL DEFAULT 0.0,
            fund_5d FLOAT8,
            fund_state VARCHAR(50),
            control VARCHAR(50),
            control_note VARCHAR(255),
            theme_ok BOOLEAN NOT NULL DEFAULT FALSE,
            theme_hint VARCHAR(255),
            flags JSONB,
            trade_plan JSONB,
            created_at TIMESTAMPTZ DEFAULT NOW()
        );

        CREATE TABLE IF NOT EXISTS klines (
            id BIGSERIAL PRIMARY KEY,
            code VARCHAR(20) NOT NULL,
            market VARCHAR(20) NOT NULL,
            k_date DATE NOT NULL,
            open FLOAT8 NOT NULL,
            high FLOAT8 NOT NULL,
            low FLOAT8 NOT NULL,
            close FLOAT8 NOT NULL,
            volume FLOAT8 NOT NULL,
            created_at TIMESTAMPTZ DEFAULT NOW(),
            CONSTRAINT uq_code_market_kdate UNIQUE (code, market, k_date)
        );

        CREATE TABLE IF NOT EXISTS system_config (
            config_key VARCHAR(100) PRIMARY KEY,
            config_value JSONB NOT NULL,
            updated_at TIMESTAMPTZ DEFAULT NOW()
        );

        CREATE INDEX IF NOT EXISTS idx_scan_records_mkt_time ON scan_records(market, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_scan_candidates_rec_score ON scan_candidates(scan_record_id, score DESC, chg DESC);
        CREATE INDEX IF NOT EXISTS idx_klines_lookup ON klines(code, market, k_date DESC);
        "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn save_scan_record(
    pool: &PgPool,
    scan_type: &str,
    market: &str,
    candidates: &[Candidate],
    hot_topics: &[ConceptBoard],
) -> Result<i64, sqlx::Error> {
    let qualified_count = candidates.iter().filter(|c| c.qualified).count() as i32;
    let total_count = candidates.len() as i32;
    let hot_topics_json = serde_json::to_value(hot_topics).unwrap_or(serde_json::Value::Null);

    let row = sqlx::query(
        r#"
        INSERT INTO scan_records (market, scan_type, total_scanned, total_qualified, hot_topics, created_at)
        VALUES ($1, $2, $3, $4, $5, NOW())
        RETURNING id
        "#,
    )
    .bind(market)
    .bind(scan_type)
    .bind(total_count)
    .bind(qualified_count)
    .bind(hot_topics_json)
    .fetch_one(pool)
    .await?;

    let record_id: i64 = row.get("id");

    for c in candidates {
        let test_dates_json = serde_json::to_value(&c.test_dates).unwrap_or(serde_json::Value::Null);
        let flags_json = serde_json::to_value(&c.flags).unwrap_or(serde_json::Value::Null);
        let trade_plan_json = serde_json::to_value(&c.trade_plan).unwrap_or(serde_json::Value::Null);

        sqlx::query(
            r#"
            INSERT INTO scan_candidates (
                scan_record_id, code, name, market, price, chg, turnover,
                score, mode, qualified, box_low, box_high, pos_pct,
                box_span_pct, box_window, tests, test_dates, volume_days, volume_ratio,
                fund_5d, fund_state, control, control_note, theme_ok, theme_hint, flags, trade_plan
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7,
                $8, $9, $10, $11, $12, $13, $14,
                $15, $16, $17, $18, $19,
                $20, $21, $22, $23, $24, $25, $26, $27
            )
            "#,
        )
        .bind(record_id)
        .bind(&c.code)
        .bind(&c.name)
        .bind(&c.market)
        .bind(c.price)
        .bind(c.chg)
        .bind(c.turnover)
        .bind(c.score)
        .bind(&c.mode)
        .bind(c.qualified)
        .bind(c.box_low)
        .bind(c.box_high)
        .bind(c.pos_pct)
        .bind(c.box_span_pct)
        .bind(&c.box_window)
        .bind(c.tests)
        .bind(test_dates_json)
        .bind(c.volume_days)
        .bind(c.volume_ratio)
        .bind(c.fund_5d)
        .bind(&c.fund_state)
        .bind(&c.control)
        .bind(&c.control_note)
        .bind(c.theme_ok)
        .bind(&c.theme_hint)
        .bind(flags_json)
        .bind(trade_plan_json)
        .execute(pool)
        .await?;
    }

    Ok(record_id)
}

pub async fn get_latest_scan(
    pool: &PgPool,
    market: &str,
) -> Result<Option<ScanPayload>, sqlx::Error> {
    let rec_opt = sqlx::query(
        r#"
        SELECT id, market, scan_type, total_scanned, total_qualified, hot_topics, created_at
        FROM scan_records
        WHERE market = $1
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .bind(market)
    .fetch_optional(pool)
    .await?;

    if let Some(rec) = rec_opt {
        let record_id: i64 = rec.get("id");
        let scan_type: String = rec.get("scan_type");
        let total: i32 = rec.get("total_scanned");
        let qualified: i32 = rec.get("total_qualified");
        let created_at: chrono::DateTime<chrono::Utc> = rec.get("created_at");
        let hot_val: Option<serde_json::Value> = rec.get("hot_topics");
        let hot_topics: Vec<ConceptBoard> = hot_val
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();

        let rows = sqlx::query(
            r#"
            SELECT code, name, market, price, chg, turnover, score, mode, qualified,
                   box_low, box_high, pos_pct, box_span_pct, box_window, tests,
                   test_dates, volume_days, volume_ratio, fund_5d, fund_state,
                   control, control_note, theme_ok, theme_hint, flags, trade_plan
            FROM scan_candidates
            WHERE scan_record_id = $1
            ORDER BY score DESC, chg DESC
            LIMIT 150
            "#,
        )
        .bind(record_id)
        .fetch_all(pool)
        .await?;

        let mut candidates = Vec::new();
        for r in rows {
            let test_dates: Vec<String> = r
                .get::<Option<serde_json::Value>, _>("test_dates")
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default();
            let flags: Vec<String> = r
                .get::<Option<serde_json::Value>, _>("flags")
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default();
            let trade_plan: Option<TradePlan> = r
                .get::<Option<serde_json::Value>, _>("trade_plan")
                .and_then(|v| serde_json::from_value(v).ok());

            candidates.push(Candidate {
                code: r.get("code"),
                name: r.get("name"),
                market: r.get("market"),
                price: r.get("price"),
                chg: r.get("chg"),
                turnover: r.get("turnover"),
                volume_ratio: r.get("volume_ratio"),
                volume_days: r.get("volume_days"),
                box_low: r.get("box_low"),
                box_high: r.get("box_high"),
                pos_pct: r.get("pos_pct"),
                box_span_pct: r.get("box_span_pct"),
                box_window: r.get("box_window"),
                tests: r.get("tests"),
                test_dates,
                fund_5d: r.get("fund_5d"),
                inflow_days: None,
                fund_state: r.get("fund_state"),
                fund_5d_str: None,
                control: r.get("control"),
                control_note: r.get("control_note"),
                holder_ratio: None,
                holder_date: None,
                concepts: Vec::new(),
                hot_hits: Vec::new(),
                theme_ok: r.get("theme_ok"),
                theme_hint: r.get("theme_hint"),
                score: r.get("score"),
                flags,
                flag_pairs: Vec::new(),
                mode: r.get("mode"),
                qualified: r.get("qualified"),
                trade_plan,
            });
        }

        return Ok(Some(ScanPayload {
            as_of: created_at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string(),
            scan_type,
            total: total as usize,
            qualified: qualified as usize,
            candidates,
            hot_topics,
        }));
    }

    Ok(None)
}

pub async fn get_pool_stocks(pool: &PgPool) -> Result<Vec<PoolItem>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT code, name, theme, created_at
        FROM pool
        ORDER BY updated_at DESC, id DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut stocks = Vec::new();
    for r in rows {
        let created_at: chrono::DateTime<chrono::Utc> = r.get("created_at");
        stocks.push(PoolItem {
            code: r.get("code"),
            name: r.get("name"),
            theme: r.get("theme"),
            created_at: Some(created_at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string()),
        });
    }
    Ok(stocks)
}

pub async fn add_pool_stock(
    pool: &PgPool,
    code: &str,
    name: &str,
    theme: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO pool (code, name, theme, updated_at)
        VALUES ($1, $2, $3, NOW())
        ON CONFLICT (code) DO UPDATE
        SET name = EXCLUDED.name, theme = EXCLUDED.theme, updated_at = NOW()
        "#,
    )
    .bind(code)
    .bind(name)
    .bind(theme)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn remove_pool_stock(pool: &PgPool, code: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM pool WHERE code = $1")
        .bind(code)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn load_system_config(pool: &PgPool) -> Result<SystemConfig, sqlx::Error> {
    let row_opt = sqlx::query(
        r#"
        SELECT config_value
        FROM system_config
        WHERE config_key = 'main_config'
        "#,
    )
    .fetch_optional(pool)
    .await?;

    if let Some(row) = row_opt {
        let val: serde_json::Value = row.get("config_value");
        if let Ok(cfg) = serde_json::from_value(val) {
            return Ok(cfg);
        }
    }
    Ok(SystemConfig::default())
}

pub async fn save_system_config(pool: &PgPool, cfg: &SystemConfig) -> Result<(), sqlx::Error> {
    let json_val = serde_json::to_value(cfg).unwrap_or(serde_json::Value::Null);
    sqlx::query(
        r#"
        INSERT INTO system_config (config_key, config_value, updated_at)
        VALUES ('main_config', $1, NOW())
        ON CONFLICT (config_key) DO UPDATE
        SET config_value = EXCLUDED.config_value, updated_at = NOW()
        "#,
    )
    .bind(json_val)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn save_klines(
    pool: &PgPool,
    code: &str,
    market: &str,
    bars: &[KlineBar],
) -> Result<(), sqlx::Error> {
    if bars.is_empty() {
        return Ok(());
    }

    let mut codes = Vec::with_capacity(bars.len());
    let mut markets = Vec::with_capacity(bars.len());
    let mut dates = Vec::with_capacity(bars.len());
    let mut opens = Vec::with_capacity(bars.len());
    let mut highs = Vec::with_capacity(bars.len());
    let mut lows = Vec::with_capacity(bars.len());
    let mut closes = Vec::with_capacity(bars.len());
    let mut volumes = Vec::with_capacity(bars.len());

    for b in bars {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(&b.date, "%Y-%m-%d") {
            codes.push(code.to_string());
            markets.push(market.to_string());
            dates.push(d);
            opens.push(b.open);
            highs.push(b.high);
            lows.push(b.low);
            closes.push(b.close);
            volumes.push(b.vol);
        }
    }

    if !dates.is_empty() {
        sqlx::query(
            r#"
            INSERT INTO klines (code, market, k_date, open, high, low, close, volume, created_at)
            SELECT t.code, t.market, t.k_date, t.open, t.high, t.low, t.close, t.volume, NOW()
            FROM UNNEST(
                $1::varchar[],
                $2::varchar[],
                $3::date[],
                $4::float8[],
                $5::float8[],
                $6::float8[],
                $7::float8[],
                $8::float8[]
            ) AS t(code, market, k_date, open, high, low, close, volume)
            ON CONFLICT (code, market, k_date) DO UPDATE
            SET open = EXCLUDED.open, high = EXCLUDED.high, low = EXCLUDED.low, close = EXCLUDED.close, volume = EXCLUDED.volume
            "#,
        )
        .bind(&codes)
        .bind(&markets)
        .bind(&dates)
        .bind(&opens)
        .bind(&highs)
        .bind(&lows)
        .bind(&closes)
        .bind(&volumes)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn get_klines(
    pool: &PgPool,
    code: &str,
    market: &str,
    limit: usize,
) -> Result<Vec<KlineBar>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT k_date, open, high, low, close, volume
        FROM (
            SELECT k_date, open, high, low, close, volume
            FROM klines
            WHERE code = $1 AND market = $2
            ORDER BY k_date DESC
            LIMIT $3
        ) sub
        ORDER BY k_date ASC
        "#,
    )
    .bind(code)
    .bind(market)
    .bind(limit as i64)
    .fetch_all(pool)
    .await?;

    let mut bars = Vec::with_capacity(rows.len());
    for r in rows {
        let d: chrono::NaiveDate = r.get("k_date");
        bars.push(KlineBar {
            date: d.format("%Y-%m-%d").to_string(),
            open: r.get("open"),
            close: r.get("close"),
            high: r.get("high"),
            low: r.get("low"),
            vol: r.get("volume"),
        });
    }
    Ok(bars)
}
