//! Shared no-op trait implementations used by tests and integration fixtures.
//!
//! These are deliberate placeholders for backends that are not implemented
//! yet (vector index, embedder). They are **not part of the stable API** and
//! will be removed or demoted once real backends exist.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use crate::Result;
use crate::chunking::ChunkId;
use crate::embedder::{Embedder, EmbeddingSpace};
use crate::vector::VectorIndex;

/// Vector index that stores nothing and finds nothing. Its default embedding
/// space matches [`NoopEmbedder`], the embedder it is normally paired with;
/// use [`NoopVectorIndex::for_space`] to pair it with anything else.
#[derive(Debug, Clone)]
pub struct NoopVectorIndex {
    space: EmbeddingSpace,
}

impl NoopVectorIndex {
    pub fn for_space(space: EmbeddingSpace) -> Self {
        Self { space }
    }
}

impl Default for NoopVectorIndex {
    fn default() -> Self {
        Self::for_space(NoopEmbedder.embedding_space())
    }
}

impl VectorIndex for NoopVectorIndex {
    fn embedding_space(&self) -> &EmbeddingSpace {
        &self.space
    }

    fn add(&mut self, _id: ChunkId, _vector: Vec<f32>) -> Result<()> {
        Ok(())
    }

    fn remove(&mut self, _id: ChunkId) -> Result<()> {
        Ok(())
    }

    fn search(&self, _query: &[f32], _limit: usize) -> Result<Vec<(ChunkId, f32)>> {
        Ok(Vec::new())
    }

    fn clear(&mut self) -> Result<()> {
        Ok(())
    }

    fn save(&self, _path: &Path) -> Result<()> {
        Ok(())
    }

    fn load(&mut self, _path: &Path) -> Result<()> {
        Ok(())
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

    fn fingerprint(&self) -> String {
        "noop|dim=8".to_string()
    }
}

/// Deterministic offline embedder: hashes each whitespace-delimited word into
/// a fixed-dimension bag-of-words vector (L2-normalized). Texts sharing
/// vocabulary get high cosine similarity, so semantic-search wiring can be
/// tested end to end without a network or model. Not for production use.
#[derive(Debug, Clone)]
pub struct HashEmbedder {
    dimension: usize,
}

impl HashEmbedder {
    pub fn new(dimension: usize) -> Self {
        Self { dimension }
    }

    fn embed_one(&self, text: &str) -> Vec<f32> {
        let mut v = vec![0.0f32; self.dimension];
        for word in text.split_whitespace() {
            let mut hasher = DefaultHasher::new();
            word.to_ascii_lowercase().hash(&mut hasher);
            let idx = (hasher.finish() as usize) % self.dimension;
            v[idx] += 1.0;
        }
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in &mut v {
                *x /= norm;
            }
        } else {
            // Empty text: a fixed unit vector avoids an undefined cosine.
            v[0] = 1.0;
        }
        v
    }
}

impl Embedder for HashEmbedder {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|t| self.embed_one(t)).collect())
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn fingerprint(&self) -> String {
        format!("hash-bow|v1|dim={}", self.dimension)
    }
}
