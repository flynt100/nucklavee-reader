use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Collection-wide identity of a vector space.
///
/// One library (store + index) holds embeddings from exactly **one** space;
/// this type is the value that the configured embedder, the authoritative
/// store, and the persisted index manifest must all agree on. It is distinct
/// from a document's `processing_fingerprint`, which identifies how one
/// document was processed — the space is a collection invariant, not a
/// per-document property.
///
/// Equality is exact: fingerprint and dimension must both match. Equal
/// dimensions never imply compatible vectors.
///
/// The fingerprint must never contain secrets (API keys, auth headers); it is
/// persisted in SQLite and in index manifests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingSpace {
    pub fingerprint: String,
    pub dimension: usize,
}

impl std::fmt::Display for EmbeddingSpace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (dimension {})", self.fingerprint, self.dimension)
    }
}

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

    /// The canonical [`EmbeddingSpace`] value for this embedder. All
    /// space-identity checks go through this one representation; do not
    /// rebuild it from `fingerprint()`/`dimension()` at call sites.
    fn embedding_space(&self) -> EmbeddingSpace {
        EmbeddingSpace {
            fingerprint: self.fingerprint(),
            dimension: self.dimension(),
        }
    }
}

/// Trust-boundary validation for anything an [`Embedder`] returns, applied
/// identically on the ingest and query paths: exact vector count, exact
/// dimension for every vector, and all values finite. The `ApiEmbedder`
/// separately validates provider response *indexes* (it knows the wire
/// format); this validates the resulting trait output, whatever the
/// implementation.
pub(crate) fn validate_embedding_batch(
    vectors: &[Vec<f32>],
    expected_count: usize,
    space: &EmbeddingSpace,
    operation: &str,
) -> Result<()> {
    if vectors.len() != expected_count {
        return Err(Error::Embedding(format!(
            "{operation}: embedder returned {} vectors for {expected_count} inputs",
            vectors.len()
        )));
    }
    for (i, vector) in vectors.iter().enumerate() {
        if vector.len() != space.dimension {
            return Err(Error::Embedding(format!(
                "{operation}: embedder returned a vector of dimension {} for input {i}, \
                 expected {}",
                vector.len(),
                space.dimension
            )));
        }
        if let Some(bad) = vector.iter().find(|v| !v.is_finite()) {
            return Err(Error::Embedding(format!(
                "{operation}: embedder returned a non-finite value ({bad}) in the vector \
                 for input {i}"
            )));
        }
    }
    Ok(())
}

pub mod api;
