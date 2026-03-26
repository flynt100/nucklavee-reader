pub trait Embedder: Send + Sync {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String>;
    fn dimension(&self) -> usize;
}

pub mod api;
