use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Default)]
pub struct AlphaScout;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inputs {
    pub symbol: String,
    pub price: f64,
    pub fast_ma: f64,
    pub slow_ma: f64,
    pub vol: f64, // e.g., ATR or stdev
}

impl AlphaScout {
    pub fn infer_core(i: &Inputs) -> Result<api::SignalResponse> {
        let spread = i.fast_ma - i.slow_ma;
        let scale = (i.vol.abs() + 1e-8).max(1e-6);
        let raw = (spread / scale).tanh();
        let uncertainty = (1.0 - raw.abs()).clamp(0.0, 1.0);
        let spread_magnitude = spread.abs() / scale;
        let regime_fit = (spread_magnitude * 2.0).min(1.0);

        let data_quality = if i.vol.is_finite() && i.vol > 0.0 && i.price.is_finite() {
            1.0
        } else {
            0.0
        };
        let exec_impact = (i.vol * 0.01).clamp(0.001, 0.05);
        let ev_net = (raw * regime_fit) - exec_impact;
        let rationale = if spread > 0.0 {
            "fast>slow trend up"
        } else if spread < 0.0 {
            "fast<slow trend down"
        } else {
            "neutral"
        };
        Ok(api::SignalResponse {
            trace_id: uuid::Uuid::new_v4().to_string(),
            score: raw,
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

impl api::Agent for AlphaScout {
    fn agent_id(&self) -> &'static str {
        "alphascout"
    }
    fn infer(&self, req: &api::InferRequest) -> Result<api::SignalResponse> {
        let g = |k: &str| match req.features.get(k) {
            Some(ir::FeatureValue::F64(v)) => Some(*v),
            _ => None,
        };
        let price = g("price").unwrap_or(0.0);
        let fast_ma = g("fast_ma").unwrap_or(0.0);
        let slow_ma = g("slow_ma").unwrap_or(0.0);
        let vol = g("vol").unwrap_or(1.0);
        let inputs = Inputs {
            symbol: req.symbol.clone(),
            price,
            fast_ma,
            slow_ma,
            vol,
        };
        Self::infer_core(&inputs)
    }
}

pub fn infer_inputs_to_request(i: &Inputs) -> api::InferRequest {
    let mut features = BTreeMap::new();
    features.insert("price".into(), ir::FeatureValue::F64(i.price));
    features.insert("fast_ma".into(), ir::FeatureValue::F64(i.fast_ma));
    features.insert("slow_ma".into(), ir::FeatureValue::F64(i.slow_ma));
    features.insert("vol".into(), ir::FeatureValue::F64(i.vol));
    api::InferRequest {
        symbol: i.symbol.clone(),
        features,
        metadata: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use api::Agent;

    #[test]
    fn trend_signals_have_expected_sign() {
        let i = Inputs {
            symbol: "TEST".into(),
            price: 100.0,
            fast_ma: 102.0,
            slow_ma: 100.0,
            vol: 1.0,
        };
        let s = AlphaScout::infer_core(&i).unwrap();
        assert!(s.score > 0.0);
        let i2 = Inputs {
            symbol: "TEST".into(),
            price: 100.0,
            fast_ma: 98.0,
            slow_ma: 100.0,
            vol: 1.0,
        };
        let s2 = AlphaScout::infer_core(&i2).unwrap();
        assert!(s2.score < 0.0);
    }
}
