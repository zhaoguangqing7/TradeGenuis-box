use reqwest::Client;
use serde_json::Value;

use crate::models::{FundFlow, HolderInfo, KlineBar, Quote};

pub const EM_UT: &str = "b2884a393a59ad64002292a3e90d46a5";

pub fn secid(code: &str) -> String {
    if code.starts_with('6') || code.starts_with('9') || code.starts_with('5') {
        format!("1.{}", code)
    } else {
        format!("0.{}", code)
    }
}

pub async fn fetch_quote(client: &Client, code: &str) -> Result<Quote, String> {
    let em_url = format!(
        "https://push2.eastmoney.com/api/qt/stock/get?secid={}&fields=f43,f44,f45,f46,f47,f48,f50,f57,f58,f60,f168,f170",
        secid(code)
    );
    let resp = client
        .get(&em_url)
        .header("Referer", "https://quote.eastmoney.com/")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let val = resp.json::<Value>().await.map_err(|e| e.to_string())?;
    if let Some(d) = val.get("data") {
        let f43 = d.get("f43").and_then(|v| v.as_f64()).unwrap_or(0.0);
        if f43 > 0.0 {
            let f170 = d.get("f170").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let f58 = d.get("f58").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let f168 = d.get("f168").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let f50 = d.get("f50").and_then(|v| v.as_f64()).unwrap_or(0.0);
            return Ok(Quote {
                code: code.to_string(),
                price: f43 / 100.0,
                chg: f170 / 100.0,
                name: f58,
                turnover: f168 / 100.0,
                volume_ratio: f50 / 100.0,
            });
        }
    }
    Err("Quote data empty".to_string())
}

pub async fn fetch_kline(client: &Client, code: &str, lmt: usize) -> Result<Vec<KlineBar>, String> {
    let url = format!(
        "https://push2his.eastmoney.com/api/qt/stock/kline/get?secid={}&fields1=f1,f2,f3,f4,f5,f6&fields2=f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61&klt=101&fqt=1&end=20500101&lmt={}&ut={}",
        secid(code), lmt, EM_UT
    );
    let resp = client
        .get(&url)
        .header("Referer", "https://quote.eastmoney.com/")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let val = resp.json::<Value>().await.map_err(|e| e.to_string())?;
    if let Some(klines) = val.get("data").and_then(|d| d.get("klines")).and_then(|k| k.as_array()) {
        let mut bars = Vec::new();
        for item in klines {
            if let Some(s) = item.as_str() {
                let parts: Vec<&str> = s.split(',').collect();
                if parts.len() >= 6 {
                    let date = parts[0].to_string();
                    let open = parts[1].parse::<f64>().unwrap_or(0.0);
                    let close = parts[2].parse::<f64>().unwrap_or(0.0);
                    let high = parts[3].parse::<f64>().unwrap_or(0.0);
                    let low = parts[4].parse::<f64>().unwrap_or(0.0);
                    let vol = parts[5].parse::<f64>().unwrap_or(0.0);
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
    Err("Kline empty".to_string())
}

pub async fn fetch_fund_flow(client: &Client, code: &str, days: usize) -> Vec<FundFlow> {
    let url = format!(
        "https://push2his.eastmoney.com/api/qt/stock/fflow/kline/get?lmt={}&klt=101&secid={}&fields1=f1,f2,f3,f7&fields2=f51,f52,f53",
        days.max(15), secid(code)
    );
    if let Ok(resp) = client.get(&url).header("Referer", "https://quote.eastmoney.com/").send().await {
        if let Ok(val) = resp.json::<Value>().await {
            if let Some(klines) = val.get("data").and_then(|d| d.get("klines")).and_then(|k| k.as_array()) {
                let mut out = Vec::new();
                for item in klines {
                    if let Some(s) = item.as_str() {
                        let parts: Vec<&str> = s.split(',').collect();
                        if parts.len() >= 3 {
                            let date = parts[0].to_string();
                            let r0 = parts[1].parse::<f64>().unwrap_or(0.0);
                            let r1 = parts[2].parse::<f64>().unwrap_or(0.0);
                            out.push(FundFlow {
                                date,
                                main: r0 + r1,
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
    Vec::new()
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
        if let Ok(val) = resp.json::<Value>().await {
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
        if let Ok(val) = resp.json::<Value>().await {
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
