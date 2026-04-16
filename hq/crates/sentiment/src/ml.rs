pub trait SentModel {
    fn score(&self, s: f64, sev: f64, qual: f64, rec: f64) -> f64;
}
struct Simple;
impl SentModel for Simple {
    fn score(&self, s: f64, sev: f64, qual: f64, rec: f64) -> f64 {
        (s * 0.6 + sev * 0.2 + qual * 0.1 + rec * 0.1).clamp(-1.0, 1.0)
    }
}
#[cfg(feature = "onnx")]
mod onnx_impl {
    use super::SentModel;
    pub struct Onnx;
    impl SentModel for Onnx {
        fn score(&self, s: f64, sev: f64, qual: f64, rec: f64) -> f64 {
            (s * 0.6 + sev * 0.2 + qual * 0.1 + rec * 0.1).clamp(-1.0, 1.0)
        }
    }
}
pub fn get_model() -> Box<dyn SentModel + Send + Sync> {
    #[cfg(feature = "onnx")]
    {
        return Box::new(onnx_impl::Onnx);
    }
    Box::new(Simple)
}
