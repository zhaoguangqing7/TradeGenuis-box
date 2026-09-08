use chrono::{Datelike, Local, Timelike};
use std::time::Duration;
use tokio::time::sleep;
use tracing::info;

use crate::service::{run_scan_task, AppState};

pub fn start_scheduler(state: AppState) {
    tokio::spawn(async move {
        info!("🕒 自动扫描调度器已启动（监控交易日 11:30 / 15:00 定时全市场扫描）");
        let mut last_triggered_slot = String::new();

        loop {
            sleep(Duration::from_secs(15)).await;

            let now = Local::now();
            let weekday = now.weekday().number_from_monday();
            if weekday > 5 {
                continue;
            }

            let is_auto = {
                let cfg = state.config.read().await;
                cfg.auto
            };
            if !is_auto {
                continue;
            }

            let hour = now.hour();
            let minute = now.minute();
            let date_str = now.format("%Y-%m-%d").to_string();

            let current_slot = if hour == 11 && minute >= 30 && minute <= 35 {
                format!("{}_1130", date_str)
            } else if hour == 15 && minute <= 5 {
                format!("{}_1500", date_str)
            } else {
                String::new()
            };

            if !current_slot.is_empty() && current_slot != last_triggered_slot {
                last_triggered_slot = current_slot;
                info!("🕒 触发交易日定时自动扫描任务: {}", last_triggered_slot);
                run_scan_task(state.clone(), "market".to_string()).await;
            }
        }
    });
}
