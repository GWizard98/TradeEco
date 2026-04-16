use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FeatureValue {
    F64(f64),
    I64(i64),
    Text(String),
    Flag(bool),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signal {
    pub agent_id: String,
    pub symbol: String,
    pub score: f64,
    pub uncertainty: f64,
    pub features: Vec<(String, FeatureValue)>,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Side {
    Buy,
    Sell,
    Hold,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub symbol: String,
    pub side: Side,
    pub qty: f64,
    pub confidence: f64,
}

pub fn fuse(signals: &[Signal]) -> Decision {
    // Naive placeholder: majority vote by sign(score)
    let mut s = 0.0;
    for sig in signals {
        s += sig.score.signum() * (1.0 - sig.uncertainty.clamp(0.0, 1.0));
    }
    let side = if s > 0.0 {
        Side::Buy
    } else if s < 0.0 {
        Side::Sell
    } else {
        Side::Hold
    };
    Decision {
        symbol: signals
            .first()
            .map(|x| x.symbol.clone())
            .unwrap_or_default(),
        side,
        qty: 0.0,
        confidence: s.abs().min(1.0),
    }
}
