use anyhow::Result;
use serde::{Deserialize, Serialize};

mod ml;

#[derive(Debug, Default)]
pub struct Sentiment;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inputs {
    pub symbol: String,
    pub sentiment: f64,      // [-1..1] from NLP
    pub severity: f64,       // [0..1] event severity
    pub source_quality: f64, // [0..1] source reliability
    pub recency_min: f64,    // minutes since news
}

impl Sentiment {
    pub fn infer_core(i: &Inputs) -> Result<api::SignalResponse> {
        let s = i.sentiment.clamp(-1.0, 1.0);
        let sev = i.severity.clamp(0.0, 1.0);
        let qual = i.source_quality.clamp(0.0, 1.0);
        let rec = 1.0 - (i.recency_min / 120.0).min(1.0); // 0..1, freshness

        // ML score blend
        let model = ml::get_model();
        let ml_score = model.score(s, sev, qual, rec); // [-1..1]

        let score = (0.5 * s + 0.5 * ml_score).clamp(-1.0, 1.0);
        let confidence = (sev * qual * rec).clamp(0.0, 1.0);
        let uncertainty = (1.0 - confidence).clamp(0.0, 1.0);
        let regime_fit = confidence; // acts as gating
        let ev_net = score.abs() * confidence * 0.1; // placeholder
        let data_quality = qual;
        let exec_impact = sev * 0.1; // events often widen spreads
        let rationale = if score > 0.2 {
            "positive news"
        } else if score < -0.2 {
            "negative news"
        } else {
            "neutral/mixed"
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

impl api::Agent for Sentiment {
    fn agent_id(&self) -> &'static str {
        "oracle"
    }
    fn infer(&self, req: &api::InferRequest) -> Result<api::SignalResponse> {
        let g = |k: &str| match req.features.get(k) {
            Some(ir::FeatureValue::F64(v)) => Some(*v),
            Some(ir::FeatureValue::Text(_)) => None,
            _ => None,
        };
        let s = g("sentiment").unwrap_or(0.0);
        let sev = g("severity").unwrap_or(0.0);
        let qual = g("source_quality").unwrap_or(0.5);
        let rec = g("recency_min").unwrap_or(999.0);
        let inputs = Inputs {
            symbol: req.symbol.clone(),
            sentiment: s,
            severity: sev,
            source_quality: qual,
            recency_min: rec,
        };
        Self::infer_core(&inputs)
    }
}
