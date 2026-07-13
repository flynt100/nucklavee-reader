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

    /// Stable identity of the **vector space** this embedder produces.
    ///
    /// Two embedders whose vectors are mutually comparable (same provider,
    /// same model, same output space) must return the same string; embedders
    /// whose vectors must never be compared against each other must differ —
    /// equal dimensions do NOT imply a shared vector space. This feeds the
    /// ingest `processing_fingerprint`, so changing models triggers
    /// reprocessing instead of silently reusing incompatible embeddings.
    ///
    /// Must not contain secrets (API keys); it is persisted with documents.
    fn fingerprint(&self) -> String;
}

pub mod api;
