use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quote {
    pub price: f64,
    pub chg: f64,
    pub name: String,
    pub turnover: f64,
    pub volume_ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KlineBar {
    pub date: String,
    pub open: f64,
    pub close: f64,
    pub high: f64,
    pub low: f64,
    pub vol: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundFlow {
    pub date: String,
    pub main: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConceptBoard {
    pub code: String,
    pub name: String,
    pub chg1: f64,
    pub chg5: f64,
    pub main: f64,
    pub up: i32,
    pub down: i32,
    #[serde(default)]
    pub matched: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HolderInfo {
    pub end_date: String,
    pub ratio: f64,
    pub holder_num: f64,
    pub avg_hold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeAnalysis {
    pub volume_days: i32,
    pub volume_ratio: f64,
    pub ratios: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoxAnalysis {
    pub box_low: f64,
    pub box_high: f64,
    pub tests: i32,
    pub test_dates: Vec<String>,
    pub pos_pct: f64,
    pub span_pct: f64,
    pub window: String,
    pub box_end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundAnalysis {
    pub fund_5d: Option<f64>,
    pub inflow_days: i32,
    pub fund_state: String,
    pub fund_5d_str: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlAnalysis {
    pub control: String,
    pub holder_ratio: Option<f64>,
    pub control_note: String,
    pub holder_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub code: String,
    pub name: String,
    pub market: String,
    pub price: f64,
    pub chg: f64,
    pub turnover: Option<f64>,
    pub volume_ratio: f64,
    pub volume_days: i32,
    pub box_low: Option<f64>,
    pub box_high: Option<f64>,
    pub pos_pct: Option<f64>,
    pub box_span_pct: Option<f64>,
    pub box_window: Option<String>,
    pub tests: i32,
    pub test_dates: Vec<String>,
    pub fund_5d: Option<f64>,
    pub inflow_days: Option<i32>,
    pub fund_state: Option<String>,
    pub fund_5d_str: Option<String>,
    pub control: Option<String>,
    pub control_note: Option<String>,
    pub holder_ratio: Option<f64>,
    pub holder_date: Option<String>,
    pub concepts: Vec<String>,
    pub hot_hits: Vec<String>,
    pub theme_ok: bool,
    pub theme_hint: Option<String>,
    pub score: i32,
    pub flags: Vec<String>,
    pub flag_pairs: Vec<(String, i32)>,
    pub mode: String,
    pub qualified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolItem {
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub theme: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolFile {
    #[serde(default)]
    pub updated: Option<String>,
    pub stocks: Vec<PoolItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanPayload {
    pub as_of: String,
    pub scan_type: String,
    pub total: usize,
    pub qualified: usize,
    pub candidates: Vec<Candidate>,
    #[serde(default)]
    pub hot_topics: Vec<ConceptBoard>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemConfig {
    pub auto: bool,
    pub auto_times: Vec<String>,
    pub tg_token: String,
    pub tg_chat: String,
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            auto: true,
            auto_times: vec!["11:30".to_string(), "15:00".to_string()],
            tg_token: String::new(),
            tg_chat: String::new(),
        }
    }
}
