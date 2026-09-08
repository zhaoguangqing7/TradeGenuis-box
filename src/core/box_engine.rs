use crate::models::{BoxAnalysis, KlineBar, VolumeAnalysis};

pub const BOX_LEN: usize = 60;
pub const BOX_SPAN_MAX: f64 = 38.0;
pub const BOX_NEAR: f64 = 0.94;
pub const BOX_CLOSE: f64 = 0.985;
pub const BOX_SHADOW: f64 = 0.35;
pub const TEST_VOL: f64 = 1.35;

pub fn compute_volume(bars: &[KlineBar]) -> VolumeAnalysis {
    if bars.is_empty() {
        return VolumeAnalysis {
            volume_days: 0,
            volume_ratio: 0.0,
            ratios: Vec::new(),
        };
    }

    let n = bars.len();
    let mut ratios = Vec::new();

    let start = if n > 10 { n - 10 } else { 0 };
    for i in start..n {
        let v = bars[i].vol;
        if i >= 5 {
            let prev5: f64 = bars[i - 5..i].iter().map(|b| b.vol).sum();
            let avg5 = prev5 / 5.0;
            if avg5 > 0.0 {
                ratios.push((v / avg5 * 100.0).round() / 100.0);
            } else {
                ratios.push(1.0);
            }
        } else {
            ratios.push(1.0);
        }
    }

    let mut volume_days = 0;
    for &r in ratios.iter().rev() {
        if r >= 1.6 {
            volume_days += 1;
        } else {
            break;
        }
    }

    let volume_ratio = *ratios.last().unwrap_or(&0.0);

    VolumeAnalysis {
        volume_days,
        volume_ratio,
        ratios,
    }
}

pub fn compute_box(bars: &[KlineBar]) -> Option<BoxAnalysis> {
    let n = bars.len();
    if n < 40 {
        return None;
    }

    let win_len = BOX_LEN.min(n);
    let win = &bars[n - win_len..];

    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    let mut vol_sum = 0.0;

    for b in win {
        if b.low > 0.0 && b.low < low {
            low = b.low;
        }
        if b.high > high {
            high = b.high;
        }
        vol_sum += b.vol;
    }

    if low <= 0.0 || high <= low {
        return None;
    }

    let span_pct = (high - low) / low * 100.0;
    if span_pct > BOX_SPAN_MAX {
        return None;
    }

    let vol_avg = vol_sum / (win.len() as f64);
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
        span_pct: (span_pct * 10.0).round() / 10.0,
        window: win_str,
        box_end: bars.last().map(|b| b.date.clone()),
    })
}
