use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ---------- Instrument & Order Contracts ----------
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstrumentSpec {
    pub tick_size: f64,
    pub lot_step: f64,
    pub min_qty: f64,
    pub max_qty: f64,
    pub min_notional: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderType {
    Market,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRequest {
    pub idempotency_key: String,
    pub symbol: String,
    pub side: OrderSide,
    pub qty: f64,
    pub order_type: OrderType,
    pub ttl_sec: u64,
    // Optional bracket exits (prices in instrument units)
    pub stop_loss: Option<f64>,
    pub take_profit: Option<f64>,
    // Optional partial take-profit fraction [0..1]
    pub tp_partial_pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecReport {
    pub side: String,
    pub idempotency_key: String,
    pub symbol: String,
    pub filled_qty: f64,
    pub avg_price: f64,
    pub slippage_bps: f64,
    pub status: String,
}

pub fn round_step_floor(value: f64, step: f64) -> f64 {
    if step <= 0.0 {
        return value;
    }
    let n = (value / step).floor();
    (n * step).max(0.0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferRequest {
    pub symbol: String,
    // Generic feature bag shared across agents
    pub features: BTreeMap<String, ir::FeatureValue>,
    // Optional metadata (timestamps, regime hints, etc.)
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Hints {
    pub stop_loss: Option<f64>,
    pub take_profit: Option<f64>,
    pub ttl_sec: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalResponse {
    pub trace_id: String,
    pub score: f64,
    pub uncertainty: f64,
    pub regime_fit: f64,
    pub ev_net: f64,
    pub data_quality: f64,
    pub exec_impact: f64,
    pub hints: Option<Hints>,
    pub rationale: String,
}

pub trait Agent: Send + Sync {
    fn agent_id(&self) -> &'static str;
    fn infer(&self, req: &InferRequest) -> Result<SignalResponse>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SizingRequest {
    pub symbol: String,
    pub price: f64,
    pub account_equity: f64,
    pub score: f64,
    pub uncertainty: f64,
    pub ev_net: f64,
    pub regime_fit: f64,
    pub atr: f64,
    pub leverage_max: f64,
    pub symbol_notional_cap: f64,
    pub portfolio_notional_cap: f64,
    // Optional context for advanced risk controls
    pub exec_impact: Option<f64>,
    pub corr_to_portfolio: Option<f64>,
    pub sector_exposure_notional: Option<f64>,
    pub recent_max_drawdown: Option<f64>,
    pub loss_cooldown: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SizingResponse {
    pub qty: f64,
    pub stop_loss: Option<f64>,
    pub take_profit: Option<f64>,
    pub ttl_sec: Option<u64>,
    pub rationale: String,
}

pub trait Sizer: Send + Sync {
    fn name(&self) -> &'static str;
    fn size(&self, req: &SizingRequest) -> Result<SizingResponse>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaFeature {
    pub agent_id: String,
    pub score: f64,
    pub uncertainty: f64,
    pub ev_net: f64,
    pub regime_fit: f64,
    pub data_quality: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaRequest {
    pub symbol: String,
    pub features: Vec<MetaFeature>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaWeight {
    pub agent_id: String,
    pub weight: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaResponse {
    pub weights: Vec<MetaWeight>,
    pub abstain: bool,
}

pub trait MetaWeigher: Send + Sync {
    fn name(&self) -> &'static str;
    fn weigh(&self, req: &MetaRequest) -> Result<MetaResponse>;
}

// ---------- News events (Sentinel -> Oracle) ----------
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NewsSource {
    YouTube,
    Bloomberg,
    FXStreet,
    YahooFinance,
    ForexNews,
    TradingView,
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsEvent {
    pub symbol: String,
    pub text: String,
    pub source: NewsSource,
    pub severity: f64,       // [0..1]
    pub source_quality: f64, // [0..1]
    pub published_ms: i64,
}
