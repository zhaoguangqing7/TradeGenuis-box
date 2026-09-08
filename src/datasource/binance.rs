use reqwest::Client;
use crate::models::{KlineBar, Quote};

pub const BINANCE_FUTURES: &str = "https://fapi.binance.com";

pub async fn fetch_crypto_tickers(client: &Client) -> Vec<Quote> {
    let url = format!("{}/fapi/v1/ticker/24hr", BINANCE_FUTURES);
    if let Ok(resp) = client.get(&url).send().await {
        if let Ok(val) = resp.json::<serde_json::Value>().await {
            if let Some(arr) = val.as_array() {
                let mut list = Vec::new();
                for t in arr {
                    let sym = t.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
                    if !sym.ends_with("USDT") {
                        continue;
                    }
                    let chg = t.get("priceChangePercent").and_then(|v| v.as_str()).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
                    let price = t.get("lastPrice").and_then(|v| v.as_str()).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
                    if price <= 0.0 {
                        continue;
                    }
                    list.push(Quote {
                        code: sym.to_string(),
                        price,
                        chg,
                        name: sym.to_string(),
                        turnover: 0.0,
                        volume_ratio: 1.0,
                    });
                }
                list.sort_by(|a, b| b.chg.partial_cmp(&a.chg).unwrap_or(std::cmp::Ordering::Equal));
                return list;
            }
        }
    }
    Vec::new()
}

pub async fn fetch_crypto_kline(client: &Client, symbol: &str, lmt: usize) -> Vec<KlineBar> {
    let url = format!("{}/fapi/v1/klines?symbol={}&interval=1d&limit={}", BINANCE_FUTURES, symbol, lmt.clamp(30, 200));
    if let Ok(resp) = client.get(&url).send().await {
        if let Ok(val) = resp.json::<serde_json::Value>().await {
            if let Some(arr) = val.as_array() {
                let mut bars = Vec::new();
                for item in arr {
                    if let Some(row) = item.as_array() {
                        if row.len() >= 6 {
                            let open_time = row[0].as_i64().unwrap_or(0);
                            let date = chrono::DateTime::from_timestamp_millis(open_time)
                                .map(|dt| dt.format("%Y-%m-%d").to_string())
                                .unwrap_or_default();
                            let open = row[1].as_str().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                            let high = row[2].as_str().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                            let low = row[3].as_str().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                            let close = row[4].as_str().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                            let vol = row[5].as_str().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                            bars.push(KlineBar {
                                date,
                                open,
                                close,
                                high,
                                low,
                                vol,
                            });
                        }
                    }
                }
                return bars;
            }
        }
    }
    Vec::new()
}
