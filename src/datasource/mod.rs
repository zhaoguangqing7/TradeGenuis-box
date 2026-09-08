pub mod binance;
pub mod eastmoney;
pub mod sina;
pub mod tencent;

use reqwest::Client;
use std::collections::HashSet;
use std::time::Duration;

use crate::models::{ConceptBoard, FundFlow, HolderInfo, KlineBar, Quote};

pub fn create_client() -> Client {
    Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36")
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default()
}

pub async fn fetch_quote(client: &Client, code: &str) -> Result<Quote, String> {
    if let Ok(q) = eastmoney::fetch_quote(client, code).await {
        return Ok(q);
    }
    let sym = tencent::tx_symbol(code);
    if let Ok(q) = tencent::fetch_tx_quote(client, &sym).await {
        return Ok(q);
    }
    sina::fetch_sina_quote(client, &sym).await
}

pub async fn fetch_kline(client: &Client, code: &str, lmt: usize) -> Result<Vec<KlineBar>, String> {
    if let Ok(bars) = eastmoney::fetch_kline(client, code, lmt).await {
        return Ok(bars);
    }
    let sym = tencent::tx_symbol(code);
    if let Ok(bars) = tencent::fetch_tx_kline(client, &sym, lmt).await {
        return Ok(bars);
    }
    sina::fetch_sina_kline(client, &sym, lmt).await
}

pub async fn fetch_fund_flow(client: &Client, code: &str, days: usize) -> Vec<FundFlow> {
    eastmoney::fetch_fund_flow(client, code, days).await
}

pub async fn fetch_concepts(client: &Client, code: &str) -> Vec<String> {
    eastmoney::fetch_concepts(client, code).await
}

pub async fn fetch_holder(client: &Client, code: &str) -> Option<HolderInfo> {
    eastmoney::fetch_holder(client, code).await
}

pub async fn fetch_universe(client: &Client) -> Result<Vec<Quote>, String> {
    let url = "https://push2.eastmoney.com/api/qt/clist/get?pn=1&pz=6000&po=1&np=1&fltt=2&invt=2&fid=f3&fs=m:0+t:6,m:0+t:80,m:1+t:2,m:1+t:23,m:0+t:81+s:2048&fields=f12,f14,f2,f3,f8,f50";
    if let Ok(resp) = client.get(url).header("Referer", "https://quote.eastmoney.com/").send().await {
        if let Ok(val) = resp.json::<serde_json::Value>().await {
            if let Some(diff) = val.get("data").and_then(|d| d.get("diff")).and_then(|d| d.as_array()) {
                let mut quotes = Vec::new();
                for item in diff {
                    let code = item.get("f12").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let name = item.get("f14").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let price = item.get("f2").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let chg = item.get("f3").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let turnover = item.get("f8").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let volume_ratio = item.get("f50").and_then(|v| v.as_f64()).unwrap_or(1.0);

                    if !code.is_empty() && price > 0.0 && !name.contains("ST") && !name.contains("退") {
                        quotes.push(Quote {
                            code,
                            price,
                            chg,
                            name,
                            turnover,
                            volume_ratio,
                        });
                    }
                }
                if quotes.len() > 100 {
                    return Ok(quotes);
                }
            }
        }
    }
    Err("Fetch universe failed".to_string())
}

pub async fn fetch_concept_boards(client: &Client) -> Vec<ConceptBoard> {
    let em_nodes = [
        "https://push2.eastmoney.com",
        "https://82.push2.eastmoney.com",
        "https://54.push2.eastmoney.com",
        "https://push2ex.eastmoney.com",
    ];

    for base in em_nodes {
        let url = format!(
            "{}/api/qt/clist/get?pn=1&pz=80&po=1&np=1&fltt=2&invt=2&fid=f3&fs=m:90+t:3+f:!50&fields=f3,f8,f12,f14,f62,f104,f105,f109",
            base
        );
        if let Ok(resp) = client.get(&url).header("Referer", "https://quote.eastmoney.com/").send().await {
            if let Ok(val) = resp.json::<serde_json::Value>().await {
                if let Some(diff) = val.get("data").and_then(|d| d.get("diff")).and_then(|d| d.as_array()) {
                    let mut boards = Vec::new();
                    for item in diff {
                        let name = item.get("f14").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                        if name.is_empty() {
                            continue;
                        }
                        let code = item.get("f12").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let chg1 = item.get("f3").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let chg5 = item.get("f109").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let main = item.get("f62").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let up = item.get("f104").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                        let down = item.get("f105").and_then(|v| v.as_i64()).unwrap_or(0) as i32;

                        if up + down >= 3 {
                            boards.push(ConceptBoard {
                                code,
                                name,
                                chg1,
                                chg5,
                                main,
                                up,
                                down,
                                matched: false,
                            });
                        }
                    }
                    if boards.len() >= 5 {
                        boards.sort_by(|a, b| b.chg1.partial_cmp(&a.chg1).unwrap_or(std::cmp::Ordering::Equal));
                        return boards;
                    }
                }
            }
        }
    }

    // 新浪概念板块容灾
    let sina_concept_url = "http://money.finance.sina.com.cn/q/view/newFLJK.php?param=class";
    if let Ok(resp) = client.get(sina_concept_url).header("Referer", "http://finance.sina.com.cn").send().await {
        if let Ok(bytes) = resp.bytes().await {
            let (cow, _, _) = encoding_rs::GBK.decode(&bytes);
            let mut boards = sina::parse_sina_boards(&cow);
            if boards.len() >= 5 {
                boards.sort_by(|a, b| b.chg1.partial_cmp(&a.chg1).unwrap_or(std::cmp::Ordering::Equal));
                return boards;
            }
        }
    }

    Vec::new()
}

pub async fn fetch_hot_topics(client: &Client, topn: usize) -> (Vec<ConceptBoard>, HashSet<String>) {
    let boards = fetch_concept_boards(client).await;
    let mut names = HashSet::new();
    let mut top_boards = Vec::new();
    for b in boards.into_iter().take(topn) {
        names.insert(b.name.clone());
        top_boards.push(b);
    }
    (top_boards, names)
}

pub use binance::{fetch_crypto_kline, fetch_crypto_tickers};
