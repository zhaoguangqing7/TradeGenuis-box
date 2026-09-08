use axum::{
    http::{header, StatusCode},
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use crate::api::scan_handler::handle_scan;
use crate::api::stock_handler::{
    get_config, get_crypto, get_hot, get_kline, get_pool, get_quotes, get_status, get_watchlist,
    post_config, post_pool,
};
use crate::service::AppState;

pub fn create_router(state: AppState) -> Router {
    let static_dir = std::path::Path::new("static");
    Router::new()
        .route("/", get(serve_index))
        .route("/api/watchlist", get(get_watchlist))
        .route("/api/crypto", get(get_crypto))
        .route("/api/hot", get(get_hot))
        .route("/api/status", get(get_status))
        .route("/api/pool", get(get_pool).post(post_pool))
        .route("/api/config", get(get_config).post(post_config))
        .route("/api/quotes", get(get_quotes))
        .route("/api/kline", get(get_kline))
        .route("/api/scan", get(handle_scan).post(handle_scan))
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
