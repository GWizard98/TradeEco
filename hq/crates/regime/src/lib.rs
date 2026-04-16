use anyhow::Result;
use serde::{Deserialize, Serialize};

mod ml;

#[derive(Debug, Default)]
pub struct Regime;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inputs {
    pub symbol: String,
    pub vol: f64,          // normalized volatility (e.g., ATR/price)
    pub spread_bps: f64,   // bid-ask spread in bps
    pub trend: f64,        // [-1..1] trend strength
    pub depth_liq: f64,    // normalized book depth/liquidity [0..1]
    pub realized_vol: f64, // recent realized vol [0..1]
    pub event_flag: bool,  // true during events/news windows
    pub tod_frac: f64,     // time of day [0..1]
    pub dow: i32,          // day of week 0..6
}

impl Regime {
    pub fn infer_core(i: &Inputs) -> Result<api::SignalResponse> {
        // Extended rules incl. liquidity, realized vol, time-of-day
        let vol_n = (i.vol).clamp(0.0, 1.0);
        let rvol = i.realized_vol.clamp(0.0, 1.0);
        let spread_n = (i.spread_bps / 100.0).clamp(0.0, 1.0); // assume <=100bps
        let trend_n = i.trend.clamp(-1.0, 1.0).abs();
        let liq = i.depth_liq.clamp(0.0, 1.0);
        let tod = i.tod_frac.clamp(0.0, 1.0);

        // Penalize known choppy windows (e.g., lunch), boost open/close liquidity
        let tod_boost = if !(0.15..=0.8).contains(&tod) {
            0.1
        } else {
            -0.05
        };
        let event_pen = if i.event_flag { -0.2 } else { 0.0 };

        let base_fit = ((1.0 - vol_n) * 0.25
            + (1.0 - rvol) * 0.15
            + (1.0 - spread_n) * 0.2
            + trend_n * 0.2
            + liq * 0.2
            + tod_boost
            + event_pen)
            .clamp(0.0, 1.0);
        // Optional ML adjustment
        let model = ml::get_model();
        let ml_fit = model.score(vol_n, spread_n, trend_n);
        let regime_fit = (0.5 * base_fit + 0.5 * ml_fit).clamp(0.0, 1.0);
        let uncertainty = (1.0 - regime_fit).clamp(0.0, 1.0);
        let score = 0.0; // gating only
        let ev_net = 0.0; // not used for regime
        let data_quality = if i.vol.is_finite() && i.spread_bps.is_finite() {
            1.0
        } else {
            0.0
        };
        let exec_impact = i.spread_bps / 10000.0; // proxy
        let rationale = if i.event_flag {
            "event"
        } else if regime_fit > 0.7 {
            "calm-trend"
        } else if regime_fit < 0.3 {
            "high-vol/chop"
        } else {
            "mixed"
        };

        Ok(api::SignalResponse {
            trace_id: uuid::Uuid::new_v4().to_string(),
            score,
            uncertainty,
            regime_fit,
            ev_net,
            data_quality,
            exec_impact,
            hints: None,
            rationale: rationale.into(),
        })
    }
}

impl api::Agent for Regime {
    fn agent_id(&self) -> &'static str {
        "regime"
    }
    fn infer(&self, req: &api::InferRequest) -> Result<api::SignalResponse> {
        let g = |k: &str| match req.features.get(k) {
            Some(ir::FeatureValue::F64(v)) => Some(*v),
            _ => None,
        };
        let vol = g("vol").unwrap_or(0.02);
        let spread_bps = g("spread_bps").unwrap_or(5.0);
        let trend = g("trend").unwrap_or(0.0);
        let depth_liq = g("depth_liq").unwrap_or(0.5);
        let realized_vol = g("realized_vol").unwrap_or(vol);
        let event_flag = match req.features.get("event_flag") {
            Some(ir::FeatureValue::Flag(b)) => *b,
            _ => false,
        };
        let tod_frac = g("tod_frac").unwrap_or(0.5);
        let dow = match req.features.get("dow") {
            Some(ir::FeatureValue::I64(d)) => *d as i32,
            _ => 0,
        };
        let inputs = Inputs {
            symbol: req.symbol.clone(),
            vol,
            spread_bps,
            trend,
            depth_liq,
            realized_vol,
            event_flag,
            tod_frac,
            dow,
        };
        Self::infer_core(&inputs)
    }
}
