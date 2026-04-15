use crate::Result;
use crate::chunking::ChunkId;

pub trait VectorIndex {
    fn add(&mut self, id: ChunkId, vector: Vec<f32>) -> Result<()>;
    fn search(&self, query: &[f32], limit: usize) -> Result<Vec<(ChunkId, f32)>>;
}

pub mod usearch;
