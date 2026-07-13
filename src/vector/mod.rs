use std::path::Path;

use crate::Result;
use crate::chunking::ChunkId;

/// Vector index contract (frozen 2026-07-06; see
/// `docs/ir-deltas-from-spec.md`). One index file lives alongside the
/// document store; `save`/`load` handle its persistence.
pub trait VectorIndex {
    /// Add or replace a chunk's vector. Replacement keeps the previous vector
    /// searchable until the new one is committed — a failed replacement must
    /// not lose the old value.
    fn add(&mut self, id: ChunkId, vector: Vec<f32>) -> Result<()>;
    /// Remove a chunk's vector. Idempotent: removing an unknown ID is not an
    /// error.
    fn remove(&mut self, id: ChunkId) -> Result<()>;
    fn search(&self, query: &[f32], limit: usize) -> Result<Vec<(ChunkId, f32)>>;
    /// Discard every vector, leaving an empty index of the same dimension.
    /// Backs full rebuild from the authoritative store.
    fn clear(&mut self) -> Result<()>;
    fn save(&self, path: &Path) -> Result<()>;
    fn load(&mut self, path: &Path) -> Result<()>;
}

pub mod usearch;
