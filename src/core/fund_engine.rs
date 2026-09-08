use crate::models::{ControlAnalysis, FundAnalysis, FundFlow, HolderInfo};

pub const FUND_DAYS: usize = 5;
pub const FUND_INFLOW_REQ: i32 = 3;

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
    let mut ctrl = "—".to_string();
    let mut note = String::new();
    let mut h_ratio = None;
    let mut h_date = None;

    if let Some(h) = holder {
        h_ratio = Some(h.ratio);
        h_date = Some(h.end_date.clone());

        if h.ratio <= -8.0 {
            ctrl = "高".to_string();
            note = format!("户数环比{:.1}%", h.ratio);
        } else if h.ratio <= -3.0 {
            ctrl = "中".to_string();
            note = format!("户数环比{:.1}%", h.ratio);
        } else if h.ratio > 0.0 {
            ctrl = "低".to_string();
            note = format!("户数环比+{:.1}%", h.ratio);
        } else {
            ctrl = "平".to_string();
            note = format!("户数环比{:.1}%", h.ratio);
        }
    } else if turnover > 0.0 {
        if turnover < 1.0 {
            ctrl = "中".to_string();
            note = "换手低".to_string();
        } else if turnover < 3.0 {
            ctrl = "中".to_string();
            note = "换手适中".to_string();
        } else {
            ctrl = "低".to_string();
            note = "换手活跃".to_string();
        }
    }

    ControlAnalysis {
        control: ctrl,
        control_note: note,
        holder_ratio: h_ratio,
        holder_date: h_date,
    }
}
