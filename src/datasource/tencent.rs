use reqwest::Client;
use crate::models::{KlineBar, Quote};

pub fn tx_symbol(code: &str) -> String {
    if code.starts_with('6') || code.starts_with('9') || code.starts_with('5') {
        format!("sh{}", code)
    } else if code.starts_with('0') || code.starts_with('3') {
        format!("sz{}", code)
    } else {
        format!("bj{}", code)
    }
}

pub async fn fetch_tx_quote(client: &Client, sym: &str) -> Result<Quote, String> {
    let url = format!("http://qt.gtimg.cn/q={}", sym);
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    let (cow, _, _) = encoding_rs::GBK.decode(&bytes);

    if let Some(idx) = cow.find('"') {
        let content = &cow[idx + 1..];
        if let Some(end) = content.rfind('"') {
            let fields: Vec<&str> = content[..end].split('~').collect();
            if fields.len() >= 38 {
                let name = fields[1].to_string();
                let price = fields[3].parse::<f64>().unwrap_or(0.0);
                let chg = fields[32].parse::<f64>().unwrap_or(0.0);
                let turnover = fields[38].parse::<f64>().unwrap_or(0.0);
                let volume_ratio = fields[49].parse::<f64>().unwrap_or(1.0);
                return Ok(Quote {
                    code: sym.to_string(),
                    price,
                    chg,
                    name,
                    turnover,
                    volume_ratio,
                });
            }
        }
    }
    Err("Tx quote error".to_string())
}

pub async fn fetch_tx_kline(client: &Client, sym: &str, lmt: usize) -> Result<Vec<KlineBar>, String> {
    let tx_url = format!(
        "https://web.ifzq.gtimg.cn/appstock/app/fqkline/get?param={},day,,,{lmt},qfq",
        sym
    );
    let resp = client.get(&tx_url).send().await.map_err(|e| e.to_string())?;
    let val = resp.json::<serde_json::Value>().await.map_err(|e| e.to_string())?;
    if let Some(node) = val.get("data").and_then(|d| d.get(sym)) {
        let kk = node.get("qfqday").or_else(|| node.get("day"));
        if let Some(arr) = kk.and_then(|v| v.as_array()) {
            let mut bars = Vec::new();
            for item in arr {
                if let Some(row) = item.as_array() {
                    if row.len() >= 6 {
                        let date = row[0].as_str().unwrap_or("").to_string();
                        let open = row[1].as_str().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
                        let close = row[2].as_str().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
                        let high = row[3].as_str().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
                        let low = row[4].as_str().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
                        let vol = row[5].as_str().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
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
            if bars.len() >= 30 {
                return Ok(bars);
            }
        }
    }
    Err("Tx kline empty".to_string())
}
