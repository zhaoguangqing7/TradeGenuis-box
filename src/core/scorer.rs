use crate::models::Candidate;
use std::collections::HashSet;

pub const VOL_DAYS_REQ: i32 = 3;
pub const VOL_MULT: f64 = 1.8;

pub fn match_hot(concepts: &[String], hot_names: &HashSet<String>) -> Vec<String> {
    let mut hits = Vec::new();
    for c in concepts {
        if hot_names.contains(c) && !hits.contains(c) {
            hits.push(c.clone());
        }
    }
    for c in concepts {
        if hits.contains(c) {
            continue;
        }
        for h in hot_names {
            if (h.len() > 2 && c.contains(h)) || (c.len() > 2 && h.contains(c)) {
                hits.push(c.clone());
                break;
            }
        }
    }
    hits
}

pub fn score_candidate(mut cand: Candidate) -> Candidate {
    let mut pts = 0;
    let mut flags = Vec::new();
    let mut flag_pairs = Vec::new();

    let vd = cand.volume_days;
    let vr = cand.volume_ratio;
    let tests = cand.tests;
    let is_crypto = cand.market == "crypto";

    if is_crypto {
        // 币圈：倍量(34) + 试盘(33) + 24h涨幅强度(33)
        if vd >= VOL_DAYS_REQ && vr >= VOL_MULT {
            pts += 34;
            flags.push(format!("倍量{}日", vd));
            flag_pairs.push((format!("倍量{}日", vd), 1));
        } else if vd >= 2 || vr >= 1.5 {
            pts += 17;
            flags.push(format!("放量不足({}日)", vd));
            flag_pairs.push((format!("放量不足({}日)", vd), 0));
        } else {
            flags.push(format!("量能弱({}日)", vd));
            flag_pairs.push((format!("量能弱({}日)", vd), 0));
        }

        if tests >= 3 {
            pts += 33;
            flags.push(format!("试盘{}次", tests));
            flag_pairs.push((format!("试盘{}次", tests), 1));
        } else if tests >= 2 {
            pts += 16;
            flags.push(format!("试盘{}次", tests));
            flag_pairs.push((format!("试盘{}次", tests), 0));
        } else {
            flags.push(format!("试盘{}次", tests));
            flag_pairs.push((format!("试盘{}次", tests), 0));
        }

        let chg = cand.chg;
        if chg >= 10.0 {
            pts += 33;
            flags.push("24h强劲".to_string());
            flag_pairs.push(("24h强劲".to_string(), 1));
        } else if chg >= 5.0 {
            pts += 16;
            flags.push("24h一般".to_string());
            flag_pairs.push(("24h一般".to_string(), 0));
        } else {
            flags.push("24h偏弱".to_string());
            flag_pairs.push(("24h偏弱".to_string(), 0));
        }
    } else {
        // A 股四条件（各 25 分）
        if cand.theme_ok {
            pts += 25;
            flags.push("热点题材".to_string());
            flag_pairs.push(("热点题材".to_string(), 1));
        } else {
            flags.push("题材弱/非热点".to_string());
            flag_pairs.push(("题材弱/非热点".to_string(), 0));
        }

        if vd >= VOL_DAYS_REQ && vr >= VOL_MULT {
            pts += 25;
            flags.push(format!("倍量{}日", vd));
            flag_pairs.push((format!("倍量{}日", vd), 1));
        } else if vd >= 2 || vr >= 1.5 {
            pts += 12;
            flags.push(format!("放量不足3日({}日)", vd));
            flag_pairs.push((format!("放量不足3日({}日)", vd), 0));
        } else {
            flags.push(format!("量能未达标({}日)", vd));
            flag_pairs.push((format!("量能未达标({}日)", vd), 0));
        }

        let flow = cand.fund_state.as_deref().unwrap_or("");
        let ctrl = cand.control.as_deref().unwrap_or("");
        if flow == "流入" && ctrl == "高" {
            pts += 25;
            flags.push("资金流入+高控盘".to_string());
            flag_pairs.push(("资金流入+高控盘".to_string(), 1));
        } else if flow == "流入" {
            pts += 15;
            flags.push("资金流入/控盘中".to_string());
            flag_pairs.push(("资金流入/控盘中".to_string(), 1));
        } else {
            flags.push("资金/控盘弱".to_string());
            flag_pairs.push(("资金/控盘弱".to_string(), 0));
        }

        if tests >= 3 {
            pts += 25;
            flags.push(format!("试盘{}次", tests));
            flag_pairs.push((format!("试盘{}次", tests), 1));
        } else if tests >= 2 {
            pts += 10;
            flags.push(format!("试盘{}次", tests));
            flag_pairs.push((format!("试盘{}次", tests), 0));
        } else {
            flags.push(format!("试盘{}次", tests));
            flag_pairs.push((format!("试盘{}次", tests), 0));
        }
    }

    let mode = if pts >= 85 {
        "达标关注"
    } else if pts >= 70 {
        "突破观察"
    } else if pts >= 50 {
        "观察"
    } else {
        "箱内/排除"
    };

    cand.score = pts;
    cand.flags = flags;
    cand.flag_pairs = flag_pairs;
    cand.mode = mode.to_string();
    cand.qualified = pts >= 85;

    cand
}
