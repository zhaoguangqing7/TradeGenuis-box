use crate::models::{BoxAnalysis, TradePlan};

pub fn generate_trade_plan(
    price: f64,
    box_res: Option<&BoxAnalysis>,
    score: i32,
    volume_ratio: f64,
) -> Option<TradePlan> {
    let b = box_res?;
    if price <= 0.0 || b.box_high <= 0.0 || b.box_low <= 0.0 {
        return None;
    }

    let box_span = b.box_high - b.box_low;
    let dist_to_high_pct = ((price - b.box_high) / b.box_high * 100.0 * 10.0).round() / 10.0;
    let buy_trigger = ((b.box_high * 1.005) * 100.0).round() / 100.0;
    
    // 突破后刚性止损位：箱顶下方 2% 或 箱体 85% 位置
    let stop_loss = ((b.box_high * 0.98).max(b.box_high - box_span * 0.25) * 100.0).round() / 100.0;
    
    // 第一目标位：箱体等幅映射 80%~100%
    let target = ((b.box_high + box_span * 0.85) * 100.0).round() / 100.0;

    // 计算盈亏比 (Reward-to-Risk Ratio)
    let potential_gain = (target - price).max(0.01);
    let risk_loss = (price - stop_loss).max(0.01);
    let rr_ratio = ((potential_gain / risk_loss) * 10.0).round() / 10.0;

    let signal_type = if price >= b.box_high && price <= b.box_high * 1.05 && (volume_ratio >= 1.5 || score >= 60) {
        "突破买入".to_string()
    } else if dist_to_high_pct >= -3.5 && dist_to_high_pct <= 0.0 && b.pos_pct >= 85.0 {
        "蓄势待发".to_string()
    } else if b.tests >= 2 {
        "主力试盘".to_string()
    } else if b.pos_pct >= 50.0 {
        "箱内偏强".to_string()
    } else {
        "箱内震荡".to_string()
    };

    Some(TradePlan {
        signal_type,
        buy_trigger_price: buy_trigger,
        stop_loss_price: stop_loss,
        target_price: target,
        rr_ratio,
        breakout_dist_pct: dist_to_high_pct,
    })
}
