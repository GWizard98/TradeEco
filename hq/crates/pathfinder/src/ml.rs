pub trait ImpactModel {
    fn score(&self, mid_price: f64, qty: f64) -> f64; // returns [0..1] impact risk proxy
}

struct SimpleImpact;
impl ImpactModel for SimpleImpact {
    fn score(&self, mid_price: f64, qty: f64) -> f64 {
        let notional = (qty.max(0.0)) * mid_price.max(0.0);
        let sens: f64 = std::env::var("IMPACT_SENS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1.0);
        let q_term = (qty.max(1.0).log10() / 6.0).clamp(0.0, 1.0);
        let n_term = ((notional.max(1.0).log10() / 9.0) * sens).clamp(0.0, 1.0);
        (0.5 * q_term + 0.5 * n_term).clamp(0.0, 1.0)
    }
}

#[cfg(feature = "onnx")]
mod onnx_impl {
    use super::ImpactModel;
    // Placeholder ONNX-backed model; wire real inference later
    pub struct OnnxImpact;
    impl ImpactModel for OnnxImpact {
        fn score(&self, mid_price: f64, qty: f64) -> f64 {
            // For now, mimic SimpleImpact behavior; replace with real ONNX scoring
            let _ = mid_price; // unused until real model
            (qty.log10() / 6.0).clamp(0.0, 1.0)
        }
    }
}

pub fn get_model() -> Box<dyn ImpactModel + Send + Sync> {
    #[cfg(feature = "onnx")]
    {
        return Box::new(onnx_impl::OnnxImpact);
    }
    Box::new(SimpleImpact)
}
