use crate::Result;
use crate::chunking::{Chunk, ChunkId};
use crate::ir::{Document, DocumentId, DocumentMeta};

/// Document and chunk persistence contract (frozen 2026-07-06; see
/// `docs/ir-deltas-from-spec.md`).
pub trait DocumentStore {
    fn upsert_document(&self, document: &Document) -> Result<()>;
    fn get_document(&self, id: DocumentId) -> Result<Document>;
    /// Return the ID of an already-stored document with this raw-content
    /// hash, if any. Backs ingest-time deduplication (spec §7.4).
    fn find_by_content_hash(&self, content_hash: &str) -> Result<Option<DocumentId>>;
    /// List metadata for every stored document (order unspecified).
    fn list_documents(&self) -> Result<Vec<DocumentMeta>>;
    /// Remove a document and all of its chunks. Idempotent: removing an
    /// unknown ID is not an error.
    fn remove_document(&self, id: DocumentId) -> Result<()>;
    fn insert_chunks(&self, chunks: &[Chunk]) -> Result<()>;
    /// Chunks for a document, ordered by `sequence_index` (all backends).
    fn get_chunks_by_document(&self, id: DocumentId) -> Result<Vec<Chunk>>;
    /// Fetch chunks by ID, preserving the order of `ids`. Unknown IDs are
    /// skipped (the vector index may lag behind chunk deletion).
    fn get_chunks_by_ids(&self, ids: &[ChunkId]) -> Result<Vec<Chunk>>;
}

pub mod memory;
pub mod sqlite;
