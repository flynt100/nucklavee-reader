use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Result;
use crate::ir::DocumentId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: ChunkId,
    pub document_id: DocumentId,
    pub section_path: Vec<String>,
    pub content: String,
    pub block_type: ChunkBlockType,
    pub sequence_index: usize,
    pub token_count: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ChunkBlockType {
    Prose,
    Code,
    Table,
}

pub type ChunkId = Uuid;

pub trait Chunker {
    fn chunk(&self, document_id: DocumentId, body_text: &str) -> Result<Vec<Chunk>>;
}
