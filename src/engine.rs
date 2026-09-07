use std::collections::HashSet;

use crate::models::{
    BoxAnalysis, Candidate, ControlAnalysis, FundAnalysis, FundFlow, HolderInfo, KlineBar,
    VolumeAnalysis,
};

pub const VOL_MULT: f64 = 1.8;
pub const VOL_DAYS_REQ: i32 = 3;
pub const BOX_LOOK: usize = 60;
pub const BOX_NEAR: f64 = 0.985;
pub const BOX_CLOSE: f64 = 1.005;
pub const BOX_SHADOW: f64 = 0.30;
pub const TEST_VOL: f64 = 0.70;
pub const FUND_DAYS: usize = 5;
pub const FUND_INFLOW_REQ: i32 = 3;
pub const HOLDER_HIGH: f64 = -2.0;
pub const HOLDER_MID: f64 = 0.5;
pub const TURNOVER_CAP: f64 = 15.0;

pub fn compute_volume(bars: &[KlineBar]) -> VolumeAnalysis {
    let n = bars.len();
    let mut ratios = vec![0.0; n];
    for i in 0..n {
        if i >= 5 {
            let sum5: f64 = bars[i - 5..i].iter().map(|b| b.vol).sum();
            let avg5 = sum5 / 5.0;
            if avg5 > 0.0 {
                ratios[i] = (bars[i].vol / avg5 * 1000.0).round() / 1000.0;
            }
        }
    }

    let mut run = 0;
    let mut best = 0;
    let start_idx = if n >= 10 { n - 10 } else { 0 };
    for &r in &ratios[start_idx..] {
        if r >= VOL_MULT {
            run += 1;
            best = best.max(run);
        } else {
            run = 0;
        }
    }

    let last_ratio = if n >= 6 { ratios[n - 1] } else { 0.0 };

    VolumeAnalysis {
        volume_days: best,
        volume_ratio: last_ratio,
        ratios,
    }
}

pub fn compute_box(bars: &[KlineBar]) -> Option<BoxAnalysis> {
    let n = bars.len();
    if n < 40 {
        return None;
    }

    let mut box_end = n;
    let look_start = if n >= 15 { n - 15 } else { 0 };
    for i in look_start..n {
        if i >= 40 {
            let prev_high = bars[i - 40..i]
                .iter()
                .map(|b| b.high)
                .fold(f64::NEG_INFINITY, f64::max);
            if bars[i].close > prev_high * 1.005 {
                box_end = i;
                break;
            }
        }
    }

    let start = if box_end >= BOX_LOOK {
        box_end - BOX_LOOK
    } else {
        0
    };
    let win = &bars[start..box_end];
    if win.is_empty() {
        return None;
    }

    let high = win.iter().map(|b| b.high).fold(f64::NEG_INFINITY, f64::max);
    let low = win.iter().map(|b| b.low).fold(f64::INFINITY, f64::min);
    if high <= low {
        return None;
    }

    let vol_sum: f64 = win.iter().map(|b| b.vol).sum();
    let vol_avg = if !win.is_empty() {
        vol_sum / win.len() as f64
    } else {
        1.0
    };

    let mut tests = 0;
    let mut test_dates = Vec::new();

    for b in win {
        let (h, l, c, o, v) = (b.high, b.low, b.close, b.open, b.vol);
        if h <= l {
            continue;
        }
        if h >= high * BOX_NEAR && c <= high * BOX_CLOSE && v >= TEST_VOL * vol_avg {
            let up_shadow = h - o.max(c);
            if up_shadow > 0.0 && (up_shadow / (h - l) >= BOX_SHADOW || h >= high * 0.995) {
                tests += 1;
                test_dates.push(b.date.clone());
            }
        }
    }

    let price = bars[n - 1].close;
    let pos = if high > low {
        (price - low) / (high - low) * 100.0
    } else {
        0.0
    };
    let span = if low > 0.0 {
        (high - low) / low * 100.0
    } else {
        0.0
    };

    let win_str = format!("{} ~ {}", win.first()?.date, win.last()?.date);
    let keep_dates = if test_dates.len() > 8 {
        test_dates[test_dates.len() - 8..].to_vec()
    } else {
        test_dates
    };

    Some(BoxAnalysis {
        box_low: (low * 100.0).round() / 100.0,
        box_high: (high * 100.0).round() / 100.0,
        tests,
        test_dates: keep_dates,
        pos_pct: (pos.clamp(0.0, 100.0) * 10.0).round() / 10.0,
        span_pct: (span * 10.0).round() / 10.0,
        window: win_str,
        box_end,
    })
}

pub fn compute_fund(ffs: &[FundFlow]) -> FundAnalysis {
    let start_idx = ffs.len().saturating_sub(FUND_DAYS);
    let last = &ffs[start_idx..];
    let main5: f64 = last.iter().map(|x| x.main).sum();
    let days = last.iter().filter(|x| x.main > 0.0).count() as i32;

    let state = if last.is_empty() {
        "无数据"
    } else if main5 > 0.0 && days >= FUND_INFLOW_REQ {
        "流入"
    } else if main5 > 0.0 {
        "偏流入"
    } else {
        "流出"
    };

    let fund_5d = if !last.is_empty() {
        Some(((main5 / 10000.0) * 10.0).round() / 10.0)
    } else {
        None
    };

    let fund_5d_str = if !last.is_empty() {
        format!("{:+0.0}万", main5 / 10000.0)
    } else {
        "—".to_string()
    };

    FundAnalysis {
        fund_5d,
        inflow_days: days,
        fund_state: state.to_string(),
        fund_5d_str,
    }
}

pub fn compute_control(turnover: f64, holder: Option<&HolderInfo>) -> ControlAnalysis {
    let mut lvl = "中".to_string();
    let mut note = "无户数数据".to_string();
    let mut holder_ratio = None;
    let mut holder_date = None;

    if let Some(h) = holder {
        holder_ratio = Some(h.ratio);
        holder_date = Some(h.end_date.clone());
        if h.ratio <= HOLDER_HIGH {
            lvl = "高".to_string();
        } else if h.ratio <= HOLDER_MID {
            lvl = "中".to_string();
        } else {
            lvl = "低".to_string();
        }
        note = format!("户数环比{:+.1}%", h.ratio);
    }

    if turnover >= TURNOVER_CAP && lvl == "高" {
        lvl = "中".to_string();
        note.push_str(&format!(" (换手{:.1}%偏高)", turnover));
    }

    ControlAnalysis {
        control: lvl,
        holder_ratio,
        control_note: note,
        holder_date,
    }
}

pub fn match_hot(concepts: &[String], hot_names: &HashSet<String>) -> Vec<String> {
    let mut hits = Vec::new();
    for c in concepts {
        if hot_names.contains(c) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_volume_calculation() {
        let mut bars = Vec::new();
        // 前 5 天均量 1000
        for i in 0..5 {
            bars.push(KlineBar {
                date: format!("2026-01-{:02}", i + 1),
                open: 10.0,
                close: 10.5,
                high: 11.0,
                low: 9.8,
                vol: 1000.0,
            });
        }
        // 后续连续放量倍量启动
        bars.push(KlineBar { date: "2026-01-06".into(), open: 10.0, close: 10.5, high: 11.0, low: 9.8, vol: 2000.0 }); // 2000 / 1000 = 2.0
        bars.push(KlineBar { date: "2026-01-07".into(), open: 10.0, close: 10.5, high: 11.0, low: 9.8, vol: 3000.0 }); // 3000 / 1200 = 2.5
        bars.push(KlineBar { date: "2026-01-08".into(), open: 10.0, close: 10.5, high: 11.0, low: 9.8, vol: 4000.0 }); // 4000 / 1600 = 2.5

        let res = compute_volume(&bars);
        assert_eq!(res.volume_days, 3);
        assert!(res.volume_ratio >= 1.8);
    }

    #[test]
    fn test_box_and_scoring() {
        let mut bars = Vec::new();
        for i in 0..60 {
            let high = if i % 10 == 0 { 15.0 } else { 12.0 };
            let close = if i % 10 == 0 { 12.5 } else { 11.0 };
            bars.push(KlineBar {
                date: format!("2026-01-{:02}", i + 1),
                open: 11.0,
                close,
                high,
                low: 10.0,
                vol: 1000.0,
            });
        }
        let box_res = compute_box(&bars).expect("box should be computed");
        assert_eq!(box_res.box_high, 15.0);
        assert_eq!(box_res.box_low, 10.0);
        assert!(box_res.tests >= 1);

        let cand = Candidate {
            code: "600519".to_string(),
            name: "贵州茅台".to_string(),
            market: "stock".to_string(),
            price: 15.0,
            chg: 3.5,
            turnover: Some(1.2),
            volume_ratio: 2.1,
            volume_days: 4,
            box_low: Some(10.0),
            box_high: Some(15.0),
            pos_pct: Some(100.0),
            box_span_pct: Some(50.0),
            box_window: Some("2026-01-01 ~ 2026-03-01".to_string()),
            tests: 3,
            test_dates: vec!["2026-01-10".to_string(), "2026-01-20".to_string(), "2026-01-30".to_string()],
            fund_5d: Some(5000.0),
            inflow_days: Some(4),
            fund_state: Some("流入".to_string()),
            fund_5d_str: Some("+5000万".to_string()),
            control: Some("高".to_string()),
            control_note: Some("高控盘".to_string()),
            holder_ratio: Some(-3.5),
            holder_date: Some("2025-12-31".to_string()),
            concepts: vec!["白酒".to_string()],
            hot_hits: vec!["白酒".to_string()],
            theme_ok: true,
            theme_hint: None,
            score: 0,
            flags: Vec::new(),
            flag_pairs: Vec::new(),
            mode: String::new(),
            qualified: false,
        };

        let scored = score_candidate(cand);
        assert_eq!(scored.score, 100);
        assert!(scored.qualified);
        assert_eq!(scored.mode, "达标关注");
    }
}
