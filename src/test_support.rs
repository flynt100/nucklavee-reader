//! Shared no-op trait implementations used by tests and the scaffold CLI.
//!
//! These are deliberate placeholders for backends that are not implemented
//! yet (vector index, embedder). They are **not part of the stable API** and
//! will be removed or demoted once real backends exist.

use crate::Result;
use crate::chunking::ChunkId;
use crate::embedder::Embedder;
use crate::vector::VectorIndex;

/// Vector index that stores nothing and finds nothing.
#[derive(Debug, Default, Clone)]
pub struct NoopVectorIndex;

impl VectorIndex for NoopVectorIndex {
    fn add(&mut self, _id: ChunkId, _vector: Vec<f32>) -> Result<()> {
        Ok(())
    }

    fn search(&self, _query: &[f32], _limit: usize) -> Result<Vec<(ChunkId, f32)>> {
        Ok(Vec::new())
    }
}

/// Embedder that returns zero vectors of a fixed small dimension.
#[derive(Debug, Default, Clone)]
pub struct NoopEmbedder;

impl Embedder for NoopEmbedder {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(vec![vec![0.0; self.dimension()]; texts.len()])
    }

    fn dimension(&self) -> usize {
        8
    }
}
