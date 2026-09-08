use clap::Parser;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{atomic::AtomicBool, Arc};
use tokio::sync::RwLock;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod api;
mod core;
mod datasource;
mod models;
mod scheduler;
mod service;
mod storage;

use models::SystemConfig;
use service::AppState;

#[derive(Parser, Debug)]
#[command(name = "tradegenius-box", version = "0.2.0", about = "TradeGenuis 箱体突破实战交易决策看板 (Rust 2.0 分层架构)")]
struct Args {
    #[arg(short, long, default_value = "127.0.0.1")]
    host: String,

    #[arg(short, long, default_value_t = 8808)]
    port: u16,

    #[arg(long)]
    db: Option<String>,

    #[arg(long)]
    scan: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 初始化日志体系
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "tradegenius_box=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // 2. 加载 .env 环境变量
    let _ = dotenvy::dotenv();

    let args = Args::parse();

    info!("TradeGenuis 箱体突破实战交易决策看板启动中 (Rust 2.0 企业级分层架构)...");

    // 3. 初始化 PostgreSQL 连接与自动建表
    let db_url = args
        .db
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .unwrap_or_else(|| "postgres://postgres:postgres@localhost:5432/tradegenius".to_string());

    let pg_pool = match storage::init_pool(&db_url).await {
        Ok(pool) => {
            info!("PostgreSQL 数据库已就绪: {}", db_url);
            Some(pool)
        }
        Err(e) => {
            warn!("无法连接 PostgreSQL ({}): {}", db_url, e);
            warn!("系统自动降级为本地 JSON 缓存模式运行。");
            None
        }
    };

    // 4. 加载系统配置
    let config = if let Some(ref pool) = pg_pool {
        storage::load_system_config(pool).await.unwrap_or_default()
    } else {
        SystemConfig::default()
    };

    let client = datasource::create_client();

    let app_state = AppState {
        pg_pool: pg_pool.clone(),
        client,
        quote_cache: Arc::new(RwLock::new(HashMap::new())),
        kline_cache: Arc::new(RwLock::new(HashMap::new())),
        scanning: Arc::new(AtomicBool::new(false)),
        scan_log: Arc::new(RwLock::new(Vec::new())),
        last_scan: Arc::new(RwLock::new(None)),
        config: Arc::new(RwLock::new(config)),
    };

    // 5. 命令行单次扫描参数检测
    if let Some(mode) = args.scan {
        info!("执行单次命令行扫描模式: {}", mode);
        service::run_scan_task(app_state.clone(), mode).await;
        return Ok(());
    }

    // 6. 启动交易日定时扫描调度器
    scheduler::start_scheduler(app_state.clone());

    // 7. 启动 Axum HTTP 路由与服务
    let app = api::create_router(app_state);
    let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;
    info!("🚀 交易决策看板已启动: http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
