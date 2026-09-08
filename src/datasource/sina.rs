use regex::Regex;
use reqwest::Client;
use std::sync::LazyLock;

use crate::models::{ConceptBoard, KlineBar, Quote};

static JUNK_BOARD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"昨日|涨停|连板|炸板|破板|一字|新高|热股|题材股|强势|活跃|微盘|低价|高价|百元|重仓|预盈|预亏|ST|摘帽|转债|富时|MSCI|标普|罗素|沪股通|深股通|融资融券|专精特新|高送转|破净|高股息|B股|AB股|中证|沪深300|深成|上证|权重|基金|社保|险资|QFII|信托|板块$|股$|个股$").unwrap()
});

pub async fn fetch_sina_quote(client: &Client, sym: &str) -> Result<Quote, String> {
    let url = format!("https://hq.sinajs.cn/list={}", sym);
    let resp = client
        .get(&url)
        .header("Referer", "https://finance.sina.com.cn")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    let (cow, _, _) = encoding_rs::GBK.decode(&bytes);

    if let Some(idx) = cow.find('"') {
        let content = &cow[idx + 1..];
        if let Some(end) = content.rfind('"') {
            let fields: Vec<&str> = content[..end].split(',').collect();
            if fields.len() >= 32 {
                let name = fields[0].to_string();
                let _open = fields[1].parse::<f64>().unwrap_or(0.0);
                let pre_close = fields[2].parse::<f64>().unwrap_or(0.0);
                let price = fields[3].parse::<f64>().unwrap_or(0.0);
                let chg = if pre_close > 0.0 {
                    ((price - pre_close) / pre_close * 100.0 * 100.0).round() / 100.0
                } else {
                    0.0
                };
                return Ok(Quote {
                    code: sym.to_string(),
                    price,
                    chg,
                    name,
                    turnover: 0.0,
                    volume_ratio: 1.0,
                });
            }
        }
    }
    Err("Sina quote error".to_string())
}

pub async fn fetch_sina_kline(client: &Client, sym: &str, lmt: usize) -> Result<Vec<KlineBar>, String> {
    let url = format!(
        "https://money.finance.sina.com.cn/quotes_service/api/json_v2.php/CN_MarketData.getKLineData?symbol={}&scale=240&ma=no&datalen={}",
        sym, lmt
    );
    let resp = client
        .get(&url)
        .header("Referer", "https://finance.sina.com.cn")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let val = resp.json::<serde_json::Value>().await.map_err(|e| e.to_string())?;
    if let Some(arr) = val.as_array() {
        let mut bars = Vec::new();
        for item in arr {
            let date = item.get("day").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let open = item.get("open").and_then(|v| v.as_str()).and_then(|v| v.parse().ok()).unwrap_or(0.0);
            let close = item.get("close").and_then(|v| v.as_str()).and_then(|v| v.parse().ok()).unwrap_or(0.0);
            let high = item.get("high").and_then(|v| v.as_str()).and_then(|v| v.parse().ok()).unwrap_or(0.0);
            let low = item.get("low").and_then(|v| v.as_str()).and_then(|v| v.parse().ok()).unwrap_or(0.0);
            let vol = item.get("volume").and_then(|v| v.as_str()).and_then(|v| v.parse().ok()).unwrap_or(0.0);
            bars.push(KlineBar {
                date,
                open,
                close,
                high,
                low,
                vol,
            });
        }
        if bars.len() >= 30 {
            return Ok(bars);
        }
    }
    Err("Sina kline empty".to_string())
}

pub fn parse_sina_boards(raw: &str) -> Vec<ConceptBoard> {
    let mut boards = Vec::new();
    let re = match Regex::new(r#"["']?(\w+)["']?\s*:\s*["']([^"']+)["']"#) {
        Ok(r) => r,
        Err(_) => return boards,
    };

    for cap in re.captures_iter(raw) {
        if let Some(val) = cap.get(2) {
            let parts: Vec<&str> = val.as_str().split(',').collect();
            if parts.len() >= 4 {
                let code = parts[0].trim().to_string();
                let name = parts[1].trim().to_string();
                if name.is_empty() || JUNK_BOARD_RE.is_match(&name) {
                    continue;
                }
                let count = parts[2].trim().parse::<i32>().unwrap_or(0);
                let chg1 = parts[3].trim().parse::<f64>().unwrap_or(0.0);
                if count >= 3 {
                    boards.push(ConceptBoard {
                        code,
                        name,
                        chg1,
                        chg5: chg1,
                        main: 0.0,
                        up: count / 2,
                        down: count / 2,
                        matched: false,
                    });
                }
            }
        }
    }
    boards
}
