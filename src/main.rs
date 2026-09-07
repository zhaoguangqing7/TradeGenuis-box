use clap::Parser;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{atomic::AtomicBool, Arc};
use tokio::sync::RwLock;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod crawler;
mod db;
mod engine;
mod models;
mod scheduler;
mod server;

use models::SystemConfig;
use server::AppState;

#[derive(Parser, Debug)]
#[command(name = "tradegenius-box", version = "0.1.0", about = "箱体突破战法看板与扫描器 (Rust + PostgreSQL 版)")]
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
    // 1. 初始化日志
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

    info!("TradeGenuis 箱体突破战法看板启动中 (Rust 2.0)...");

    // 3. 初始化 PostgreSQL 连接
    let db_url = args
        .db
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .unwrap_or_else(|| "postgres://postgres:postgres@localhost:5432/tradegenius".to_string());

    let pg_pool = match db::init_pool(&db_url).await {
        Ok(pool) => {
            info!("PostgreSQL 数据库已就绪: {}", db_url);
            Some(pool)
        }
        Err(e) => {
            warn!("无法连接 PostgreSQL ({}): {}", db_url, e);
            warn!("系统将自动降级为本地 JSON 缓存模式运行。");
            None
        }
    };

    // 4. 加载系统配置
    let config = if let Some(ref pool) = pg_pool {
        db::load_system_config(pool).await.unwrap_or_default()
    } else {
        SystemConfig::default()
    };

    let client = crawler::create_client();

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

    // 5. 若指定了命令行单次扫描参数
    if let Some(mode) = args.scan {
        info!("执行单次命令行扫描模式: {}", mode);
        server::run_scan_task(app_state.clone(), mode).await;
        return Ok(());
    }

    // 6. 启动后台定时任务
    scheduler::start_scheduler(app_state.clone());

    // 7. 启动 Axum HTTP 路由与服务
    let app = server::create_router(app_state);
    let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;
    info!("🚀 看板服务器已启动: http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
