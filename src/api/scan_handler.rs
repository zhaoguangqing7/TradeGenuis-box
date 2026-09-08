use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use std::sync::atomic::Ordering;

use crate::service::{run_scan_task, AppState};

#[derive(Deserialize, Default)]
pub struct ScanReq {
    pub mode: Option<String>,
}

pub async fn handle_scan(
    State(state): State<AppState>,
    Query(query): Query<ScanReq>,
    body: Option<Json<ScanReq>>,
) -> impl IntoResponse {
    if state.scanning.load(Ordering::SeqCst) {
        return Json(serde_json::json!({ "status": "running", "msg": "扫描进行中" })).into_response();
    }

    let mode = query
        .mode
        .or_else(|| body.and_then(|Json(b)| b.mode))
        .unwrap_or_else(|| "market".to_string());

    let state_clone = state.clone();
    let mode_task = mode.clone();

    tokio::spawn(async move {
        run_scan_task(state_clone, mode_task).await;
    });

    Json(serde_json::json!({ "status": "started", "mode": mode })).into_response()
}
