use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use std::time::Duration;
use tracing::info;

use crate::models::{Candidate, ConceptBoard, PoolItem, ScanPayload, SystemConfig};

pub async fn init_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    info!("正在连接 PostgreSQL 数据库...");
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect(database_url)
        .await?;

    info!("PostgreSQL 连接成功，开始执行表结构初始化...");
    init_tables(&pool).await?;
    info!("PostgreSQL 表结构检查/初始化完毕");

    Ok(pool)
}

async fn init_tables(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS pool (
            code VARCHAR(20) PRIMARY KEY,
            name VARCHAR(50) NOT NULL,
            theme VARCHAR(100) DEFAULT '',
            created_at TIMESTAMPTZ DEFAULT NOW(),
            updated_at TIMESTAMPTZ DEFAULT NOW()
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS scan_records (
            id BIGSERIAL PRIMARY KEY,
            market VARCHAR(20) NOT NULL,
            scan_type VARCHAR(20) NOT NULL,
            total_scanned INT NOT NULL,
            total_qualified INT NOT NULL,
            hot_topics JSONB,
            created_at TIMESTAMPTZ DEFAULT NOW()
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS scan_candidates (
            id BIGSERIAL PRIMARY KEY,
            scan_record_id BIGINT REFERENCES scan_records(id) ON DELETE CASCADE,
            code VARCHAR(20) NOT NULL,
            name VARCHAR(50) NOT NULL,
            market VARCHAR(20) NOT NULL,
            price DOUBLE PRECISION,
            chg DOUBLE PRECISION,
            turnover DOUBLE PRECISION,
            score INT NOT NULL,
            mode VARCHAR(30) NOT NULL,
            qualified BOOLEAN NOT NULL DEFAULT FALSE,
            box_low DOUBLE PRECISION,
            box_high DOUBLE PRECISION,
            pos_pct DOUBLE PRECISION,
            box_span_pct DOUBLE PRECISION,
            box_window VARCHAR(50),
            tests INT DEFAULT 0,
            test_dates JSONB,
            volume_days INT DEFAULT 0,
            volume_ratio DOUBLE PRECISION,
            fund_5d DOUBLE PRECISION,
            fund_state VARCHAR(20),
            control VARCHAR(20),
            control_note VARCHAR(100),
            theme_ok BOOLEAN DEFAULT FALSE,
            theme_hint VARCHAR(100),
            flags JSONB,
            created_at TIMESTAMPTZ DEFAULT NOW()
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_scan_candidates_record ON scan_candidates(scan_record_id)")
        .execute(pool)
        .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_scan_candidates_code ON scan_candidates(code)")
        .execute(pool)
        .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS klines (
            code VARCHAR(20) NOT NULL,
            market VARCHAR(20) NOT NULL DEFAULT 'stock',
            k_date DATE NOT NULL,
            open DOUBLE PRECISION NOT NULL,
            high DOUBLE PRECISION NOT NULL,
            low DOUBLE PRECISION NOT NULL,
            close DOUBLE PRECISION NOT NULL,
            volume DOUBLE PRECISION NOT NULL,
            created_at TIMESTAMPTZ DEFAULT NOW(),
            PRIMARY KEY (code, market, k_date)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS system_config (
            config_key VARCHAR(50) PRIMARY KEY,
            config_value JSONB NOT NULL,
            updated_at TIMESTAMPTZ DEFAULT NOW()
        )
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
    let total_scanned = candidates.len() as i32;
    let hot_json = serde_json::to_value(hot_topics).unwrap_or(serde_json::Value::Null);

    let row = sqlx::query(
        r#"
        INSERT INTO scan_records (market, scan_type, total_scanned, total_qualified, hot_topics)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id
        "#,
    )
    .bind(market)
    .bind(scan_type)
    .bind(total_scanned)
    .bind(qualified_count)
    .bind(hot_json)
    .fetch_one(pool)
    .await?;

    let record_id: i64 = row.get("id");

    for c in candidates {
        let test_dates_json = serde_json::to_value(&c.test_dates).unwrap_or(serde_json::Value::Null);
        let flags_json = serde_json::to_value(&c.flags).unwrap_or(serde_json::Value::Null);

        sqlx::query(
            r#"
            INSERT INTO scan_candidates (
                scan_record_id, code, name, market, price, chg, turnover,
                score, mode, qualified, box_low, box_high, pos_pct, box_span_pct,
                box_window, tests, test_dates, volume_days, volume_ratio,
                fund_5d, fund_state, control, control_note, theme_ok, theme_hint, flags
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7,
                $8, $9, $10, $11, $12, $13, $14,
                $15, $16, $17, $18, $19,
                $20, $21, $22, $23, $24, $25, $26
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
                   control, control_note, theme_ok, theme_hint, flags
            FROM scan_candidates
            WHERE scan_record_id = $1
            ORDER BY score DESC, chg DESC
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

            let price: Option<f64> = r.get("price");
            let chg: Option<f64> = r.get("chg");
            let turnover: Option<f64> = r.get("turnover");
            let box_low: Option<f64> = r.get("box_low");
            let box_high: Option<f64> = r.get("box_high");
            let pos_pct: Option<f64> = r.get("pos_pct");
            let box_span_pct: Option<f64> = r.get("box_span_pct");
            let volume_ratio: Option<f64> = r.get("volume_ratio");
            let fund_5d: Option<f64> = r.get("fund_5d");

            let fund_5d_str = fund_5d.map(|f| format!("{:+0.0}万", f)).unwrap_or_else(|| "—".to_string());

            candidates.push(Candidate {
                code: r.get("code"),
                name: r.get("name"),
                market: r.get("market"),
                price: price.unwrap_or(0.0),
                chg: chg.unwrap_or(0.0),
                turnover,
                volume_ratio: volume_ratio.unwrap_or(0.0),
                volume_days: r.get("volume_days"),
                box_low,
                box_high,
                pos_pct,
                box_span_pct,
                box_window: r.get("box_window"),
                tests: r.get("tests"),
                test_dates,
                fund_5d,
                inflow_days: None,
                fund_state: r.get("fund_state"),
                fund_5d_str: Some(fund_5d_str),
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
            });
        }

        return Ok(Some(ScanPayload {
            as_of: created_at.to_rfc3339(),
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
        SELECT code, name, theme
        FROM pool
        ORDER BY created_at ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut list = Vec::new();
    for r in rows {
        list.push(PoolItem {
            code: r.get("code"),
            name: r.get("name"),
            theme: r.get("theme"),
        });
    }
    Ok(list)
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
