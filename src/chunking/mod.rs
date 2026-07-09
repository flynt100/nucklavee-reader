use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Result;
use crate::ir::{Document, DocumentId};

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

/// Chunking configuration. `token_budget` is the maximum tokens per chunk.
#[derive(Debug, Clone)]
pub struct ChunkOptions {
    pub token_budget: usize,
}

impl Default for ChunkOptions {
    fn default() -> Self {
        Self { token_budget: 512 }
    }
}

/// Structure-aware chunker contract (frozen 2026-07-06; see
/// `docs/ir-deltas-from-spec.md`).
///
/// Implementations walk the IR tree (`document.body`), reusing the
/// `section_path` already recorded on each `BlockNode`'s provenance — they
/// must not re-derive heading hierarchies from flattened text.
pub trait Chunker {
    fn chunk(&self, document: &Document, opts: &ChunkOptions) -> Result<Vec<Chunk>>;
}

pub mod structural;
pub use structural::StructuralChunker;
