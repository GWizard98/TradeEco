pub trait RegimeModel {
    fn score(&self, vol_n: f64, spread_n: f64, trend_n: f64) -> f64; // [0..1]
}

struct SimpleRegime;
impl RegimeModel for SimpleRegime {
    fn score(&self, vol_n: f64, spread_n: f64, trend_n: f64) -> f64 {
        ((1.0 - vol_n) * 0.5 + (1.0 - spread_n) * 0.2 + trend_n * 0.3).clamp(0.0, 1.0)
    }
}

#[cfg(feature = "onnx")]
mod onnx_impl {
    use super::RegimeModel;
    pub struct OnnxRegime;
    impl RegimeModel for OnnxRegime {
        fn score(&self, vol_n: f64, spread_n: f64, trend_n: f64) -> f64 {
            // Placeholder: wire real ONNX inference later
            ((1.0 - vol_n) * 0.4 + (1.0 - spread_n) * 0.3 + trend_n * 0.3).clamp(0.0, 1.0)
        }
    }
}

pub fn get_model() -> Box<dyn RegimeModel + Send + Sync> {
    #[cfg(feature = "onnx")]
    {
        return Box::new(onnx_impl::OnnxRegime);
    }
    Box::new(SimpleRegime)
}
