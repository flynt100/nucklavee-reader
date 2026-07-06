use crate::Result;

/// Embedding backend contract (frozen 2026-07-06; see
/// `docs/ir-deltas-from-spec.md`).
///
/// Implementations perform **blocking** I/O (e.g. `reqwest::blocking`).
/// Nucklavee's MVP deliberately has no async runtime; do not introduce
/// tokio/async signatures here without revisiting the design freeze.
pub trait Embedder: Send + Sync {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
    fn dimension(&self) -> usize;
}

pub mod api;
