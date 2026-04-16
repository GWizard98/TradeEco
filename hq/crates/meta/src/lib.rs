use anyhow::Result;

#[derive(Default)]
pub struct SimpleMeta;

impl api::MetaWeigher for SimpleMeta {
    fn name(&self) -> &'static str {
        "simple-meta"
    }
    fn weigh(&self, req: &api::MetaRequest) -> Result<api::MetaResponse> {
        // Score each agent by linear combo; then softmax to weights
        let a = 0.6; // ev
        let b = 0.2; // regime
        let c = 0.2; // quality
        let d = 0.2; // confidence (1-unc)
        let mut raw: Vec<(String, f64)> = vec![];
        for f in &req.features {
            let conf = (1.0 - f.uncertainty).clamp(0.0, 1.0);
            let s = (a * f.ev_net + b * f.regime_fit + c * f.data_quality + d * conf).max(0.0);
            raw.push((f.agent_id.clone(), s));
        }
        let sum: f64 = raw.iter().map(|(_, x)| *x).sum();
        let abstain = req.features.iter().map(|f| f.ev_net).sum::<f64>() <= 0.0 || sum <= 1e-9;
        let weights = if sum > 0.0 {
            raw.into_iter()
                .map(|(id, x)| api::MetaWeight {
                    agent_id: id,
                    weight: x / sum,
                })
                .collect()
        } else {
            req.features
                .iter()
                .map(|f| api::MetaWeight {
                    agent_id: f.agent_id.clone(),
                    weight: 0.0,
                })
                .collect()
        };
        Ok(api::MetaResponse { weights, abstain })
    }
}
