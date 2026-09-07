use chrono::DateTime;
use regex::Regex;
use reqwest::Client;
use std::collections::HashSet;
use std::sync::LazyLock;
use std::time::Duration;

use crate::models::{ConceptBoard, FundFlow, HolderInfo, KlineBar, Quote};

static JUNK_BOARD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"昨日|涨停|连板|炸板|破板|一字|新高|热股|题材股|强势|活跃|微盘|低价|高价|百元|重仓|预盈|预亏|ST|摘帽|转债|富时|MSCI|标普|罗素|沪股通|深股通|融资融券|专精特新|高送转|破净|高股息|B股|AB股|中证|沪深300|深成|上证|权重|基金|社保|险资|QFII|信托|板块$|股$|个股$").unwrap()
});

const EM_UT: &str = "b2884a393a59ad64002292a3e90d46a5";
const BINANCE_FUTURES: &str = "https://fapi.binance.com";

pub fn tx_symbol(code: &str) -> String {
    if code.starts_with('6') || code.starts_with('9') || code.starts_with('5') {
        format!("sh{}", code)
    } else if code.starts_with('0') || code.starts_with('3') {
        format!("sz{}", code)
    } else {
        format!("bj{}", code)
    }
}

pub fn secid(code: &str) -> String {
    if code.starts_with('6') || code.starts_with('9') || code.starts_with('5') {
        format!("1.{}", code)
    } else {
        format!("0.{}", code)
    }
}

pub fn create_client() -> Client {
    Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36")
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default()
}

pub async fn fetch_quote(client: &Client, code: &str) -> Result<Quote, String> {
    // 1. 东财 push2 主源
    let em_url = format!(
        "https://push2.eastmoney.com/api/qt/stock/get?secid={}&fields=f43,f44,f45,f46,f47,f48,f50,f57,f58,f60,f168,f170",
        secid(code)
    );
    if let Ok(resp) = client.get(&em_url).send().await {
        if let Ok(val) = resp.json::<serde_json::Value>().await {
            if let Some(d) = val.get("data") {
                let f43 = d.get("f43").and_then(|v| v.as_f64()).unwrap_or(0.0);
                if f43 > 0.0 {
                    let f170 = d.get("f170").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let f58 = d.get("f58").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let f168 = d.get("f168").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let f50 = d.get("f50").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    return Ok(Quote {
                        price: f43 / 100.0,
                        chg: f170 / 100.0,
                        name: f58,
                        turnover: f168 / 100.0,
                        volume_ratio: f50 / 100.0,
                    });
                }
            }
        }
    }

    // 2. 腾讯行情 qt 兜底
    let tx_url = format!("https://qt.gtimg.cn/q={}", tx_symbol(code));
    if let Ok(resp) = client.get(&tx_url).send().await {
        if let Ok(bytes) = resp.bytes().await {
            let (cow, _, _) = encoding_rs::GBK.decode(&bytes);
            let parts: Vec<&str> = cow.split('~').collect();
            if parts.len() >= 40 && !parts[3].is_empty() {
                let price = parts[3].parse::<f64>().unwrap_or(0.0);
                let name = parts[1].to_string();
                let chg = parts[32].parse::<f64>().unwrap_or(0.0);
                let turnover = parts[38].parse::<f64>().unwrap_or(0.0);
                let volume_ratio = parts.get(49).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
                if price > 0.0 {
                    return Ok(Quote {
                        price,
                        chg,
                        name,
                        turnover,
                        volume_ratio,
                    });
                }
            }
        }
    }

    Err(format!("无法获取股票 {} 行情", code))
}

pub async fn fetch_kline(client: &Client, code: &str, lmt: usize) -> Result<Vec<KlineBar>, String> {
    let sym = tx_symbol(code);
    // 1. 腾讯主源
    let tx_url = format!(
        "https://web.ifzq.gtimg.cn/appstock/app/fqkline/get?param={},day,,,{lmt},qfq",
        sym
    );
    if let Ok(resp) = client.get(&tx_url).send().await {
        if let Ok(val) = resp.json::<serde_json::Value>().await {
            if let Some(node) = val.get("data").and_then(|d| d.get(&sym)) {
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
        }
    }

    // 2. 新浪兜底
    let sina_url = format!(
        "https://money.finance.sina.com.cn/quotes_service/api/json_v2.php/CN_MarketData.getKLineData?symbol={}&scale=240&ma=no&datalen={}",
        sym, lmt
    );
    if let Ok(resp) = client.get(&sina_url).send().await {
        if let Ok(val) = resp.json::<serde_json::Value>().await {
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
        }
    }

    Err(format!("无法获取股票 {} 的K线数据", code))
}

pub async fn fetch_fund_flow(client: &Client, code: &str, days: usize) -> Vec<FundFlow> {
    // 1. 东财 daykline 主源
    let f2 = "f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61,f62,f63,f64,f65";
    let em_url = format!(
        "https://push2his.eastmoney.com/api/qt/stock/fflow/daykline/get?lmt=0&klt=101&secid={}&fields1=f1,f2,f3,f7&fields2={}&ut={}",
        secid(code), f2, EM_UT
    );
    if let Ok(resp) = client.get(&em_url).send().await {
        if let Ok(val) = resp.json::<serde_json::Value>().await {
            if let Some(klines) = val.get("data").and_then(|d| d.get("klines")).and_then(|k| k.as_array()) {
                let mut out = Vec::new();
                for line in klines {
                    if let Some(s) = line.as_str() {
                        let parts: Vec<&str> = s.split(',').collect();
                        if parts.len() >= 2 && !parts[0].is_empty() {
                            let main = parts[1].parse::<f64>().unwrap_or(0.0);
                            out.push(FundFlow {
                                date: parts[0].to_string(),
                                main,
                            });
                        }
                    }
                }
                out.sort_by(|a, b| a.date.cmp(&b.date));
                if !out.is_empty() {
                    let skip = out.len().saturating_sub(days);
                    return out[skip..].to_vec();
                }
            }
        }
    }

    // 2. 新浪资金流兜底
    let sina_url = format!(
        "https://vip.stock.finance.sina.com.cn/quotes_service/api/json_v2.php/MoneyFlow.ssl_qsfx_zjlrqs?daima={}",
        tx_symbol(code)
    );
    if let Ok(resp) = client.get(&sina_url).send().await {
        if let Ok(val) = resp.json::<serde_json::Value>().await {
            if let Some(arr) = val.as_array() {
                let mut out = Vec::new();
                for item in arr {
                    let opendate = item.get("opendate").and_then(|v| v.as_str()).unwrap_or("");
                    let date = if opendate.len() >= 10 { &opendate[..10] } else { opendate }.to_string();
                    let r0 = item.get("r0_net").and_then(|v| v.as_str()).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
                    let r1 = item.get("r1_net").and_then(|v| v.as_str()).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
                    out.push(FundFlow {
                        date,
                        main: r0 + r1,
                    });
                }
                out.sort_by(|a, b| a.date.cmp(&b.date));
                if !out.is_empty() {
                    let skip = out.len().saturating_sub(days);
                    return out[skip..].to_vec();
                }
            }
        }
    }

    Vec::new()
}

pub async fn fetch_concept_boards(client: &Client) -> Vec<ConceptBoard> {
    let url = "https://push2.eastmoney.com/api/qt/clist/get?pn=1&pz=80&po=1&np=1&fltt=2&invt=2&fid=f3&fs=m:90+t:3+f:!50&fields=f3,f8,f12,f14,f62,f104,f105,f109";
    if let Ok(resp) = client.get(url).send().await {
        if let Ok(val) = resp.json::<serde_json::Value>().await {
            if let Some(diff) = val.get("data").and_then(|d| d.get("diff")).and_then(|d| d.as_array()) {
                let mut boards = Vec::new();
                for item in diff {
                    let name = item.get("f14").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                    if name.is_empty() || JUNK_BOARD_RE.is_match(&name) {
                        continue;
                    }
                    let code = item.get("f12").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let chg1 = item.get("f3").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let chg5 = item.get("f109").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let main = item.get("f62").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let up = item.get("f104").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let down = item.get("f105").and_then(|v| v.as_i64()).unwrap_or(0) as i32;

                    if up + down >= 5 {
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

pub async fn fetch_concepts(client: &Client, code: &str) -> Vec<String> {
    let mkt = if code.starts_with('6') || code.starts_with('9') || code.starts_with('5') {
        "SH"
    } else if code.starts_with('4') || code.starts_with('8') {
        "BJ"
    } else {
        "SZ"
    };
    let url = format!(
        "https://emweb.securities.eastmoney.com/PC_HSF10/CoreConception/PageAjax?code={}{}",
        mkt, code
    );
    if let Ok(resp) = client.get(&url).send().await {
        if let Ok(val) = resp.json::<serde_json::Value>().await {
            if let Some(ssbk) = val.get("ssbk").and_then(|v| v.as_array()) {
                let mut names = Vec::new();
                for item in ssbk {
                    if let Some(n) = item.get("BOARD_NAME").and_then(|v| v.as_str()) {
                        let name = n.trim().to_string();
                        if !name.is_empty() {
                            names.push(name);
                        }
                    }
                }
                return names;
            }
        }
    }
    Vec::new()
}

pub async fn fetch_holder(client: &Client, code: &str) -> Option<HolderInfo> {
    let url = format!(
        "https://datacenter-web.eastmoney.com/api/data/v1/get?reportName=RPT_HOLDERNUM_DET&columns=SECURITY_CODE,END_DATE,HOLDER_NUM,PRE_HOLDER_NUM,HOLDER_NUM_RATIO,AVG_HOLD_NUM&filter=(SECURITY_CODE%3D%22{}%22)&pageNumber=1&pageSize=2&sortTypes=-1&sortColumns=END_DATE",
        code
    );
    if let Ok(resp) = client.get(&url).send().await {
        if let Ok(val) = resp.json::<serde_json::Value>().await {
            if let Some(data) = val.get("result").and_then(|r| r.get("data")).and_then(|d| d.as_array()) {
                if let Some(first) = data.first() {
                    let end_date = first.get("END_DATE").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let date = if end_date.len() >= 10 { &end_date[..10] } else { &end_date }.to_string();
                    let ratio = first.get("HOLDER_NUM_RATIO").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let holder_num = first.get("HOLDER_NUM").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let avg_hold = first.get("AVG_HOLD_NUM").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    return Some(HolderInfo {
                        end_date: date,
                        ratio,
                        holder_num,
                        avg_hold,
                    });
                }
            }
        }
    }
    None
}

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
                        price,
                        chg,
                        name: sym.to_string(),
                        turnover: 0.0,
                        volume_ratio: 0.0,
                    });
                }
                list.sort_by(|a, b| b.chg.partial_cmp(&a.chg).unwrap_or(std::cmp::Ordering::Equal));
                return list;
            }
        }
    }
    Vec::new()
}

pub async fn fetch_crypto_kline(client: &Client, symbol: &str, limit: usize) -> Vec<KlineBar> {
    let url = format!(
        "{}/fapi/v1/klines?symbol={}&interval=1d&limit={}",
        BINANCE_FUTURES, symbol, limit
    );
    if let Ok(resp) = client.get(&url).send().await {
        if let Ok(val) = resp.json::<serde_json::Value>().await {
            if let Some(arr) = val.as_array() {
                let mut bars = Vec::new();
                for item in arr {
                    if let Some(row) = item.as_array() {
                        if row.len() >= 6 {
                            let ts = row[0].as_i64().unwrap_or(0) / 1000;
                            let date = DateTime::from_timestamp(ts, 0)
                                .map(|d| d.format("%Y-%m-%d").to_string())
                                .unwrap_or_default();
                            let open = row[1].as_str().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
                            let high = row[2].as_str().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
                            let low = row[3].as_str().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
                            let close = row[4].as_str().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
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
                return bars;
            }
        }
    }
    Vec::new()
}
