use std::collections::HashSet;
use std::time::Duration;
use tracing::info;

use crate::server::{run_scan_task, AppState};

pub fn start_scheduler(state: AppState) {
    tokio::spawn(async move {
        let mut executed_slots: HashSet<String> = HashSet::new();

        loop {
            tokio::time::sleep(Duration::from_secs(20)).await;

            let (auto, auto_times) = {
                let cfg = state.config.read().await;
                (cfg.auto, cfg.auto_times.clone())
            };

            if !auto {
                continue;
            }

            let now = chrono::Local::now();
            let weekday = now.format("%u").to_string().parse::<u32>().unwrap_or(1);
            if weekday >= 6 {
                continue; // 周末不触发
            }

            let current_hm = now.format("%H:%M").to_string();
            let current_date = now.format("%Y-%m-%d").to_string();

            for t in &auto_times {
                if &current_hm == t {
                    let slot_key = format!("{} {}", current_date, t);
                    if !executed_slots.contains(&slot_key) {
                        executed_slots.insert(slot_key);
                        info!("自动定时扫描触发（{}）...", t);
                        let s = state.clone();
                        tokio::spawn(async move {
                            run_scan_task(s, "pool".to_string()).await;
                        });
                        break;
                    }
                }
            }
        }
    });
}
