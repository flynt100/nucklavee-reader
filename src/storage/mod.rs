//! Document and chunk persistence.
//!
//! Consistency model (see `docs/adr/0001-persistence-and-index-consistency.md`):
//! the document store is **authoritative** — documents, chunks, and chunk
//! embeddings all live here — while the vector index is a derived,
//! rebuildable projection. Both backends must behave identically; the shared
//! conformance suite (`tests/store_conformance.rs`) is the contract's
//! executable form.

use std::collections::HashSet;

use crate::chunking::{Chunk, ChunkId};
use crate::ir::{Document, DocumentId, DocumentMeta};
use crate::{Error, Result};

/// Document and chunk persistence contract (frozen 2026-07-06; projection
/// semantics revised 2026-07-13 per the external reliability review).
pub trait DocumentStore {
    fn get_document(&self, id: DocumentId) -> Result<Document>;

    /// Return the ID of an already-stored document with this raw-content
    /// hash, if any. Backs ingest-time deduplication (spec §7.4).
    fn find_by_content_hash(&self, content_hash: &str) -> Result<Option<DocumentId>>;

    /// List metadata for every stored document (order unspecified).
    fn list_documents(&self) -> Result<Vec<DocumentMeta>>;

    /// Remove a document, its chunks, and their embeddings. Idempotent:
    /// removing an unknown ID is not an error.
    fn remove_document(&self, id: DocumentId) -> Result<()>;

    /// Atomically upsert the document and **replace** its entire chunk set
    /// and embeddings. The previous chunk generation (and its embeddings) is
    /// removed in the same operation; the store never holds a partial mix of
    /// old and new chunks. Idempotent: replaying the same projection yields
    /// the same stored state.
    ///
    /// Validation (identical across backends, enforced before any mutation):
    /// every chunk belongs to `document`, `embeddings.len() == chunks.len()`,
    /// and sequence indexes are unique and contiguous from zero.
    fn replace_document_projection(
        &self,
        document: &Document,
        chunks: &[Chunk],
        embeddings: &[Vec<f32>],
    ) -> Result<()>;

    /// Chunks for a document, ordered by `sequence_index` (all backends).
    fn get_chunks_by_document(&self, id: DocumentId) -> Result<Vec<Chunk>>;

    /// Fetch chunks by ID, preserving the order of `ids`. Unknown IDs are
    /// skipped (the vector index may lag behind chunk deletion).
    fn get_chunks_by_ids(&self, ids: &[ChunkId]) -> Result<Vec<Chunk>>;

    /// Every stored `(chunk_id, embedding)` pair. Backs vector-index rebuild
    /// without re-calling the embedding provider.
    fn get_all_embeddings(&self) -> Result<Vec<(ChunkId, Vec<f32>)>>;
}

/// Shared projection validation used by every backend so replacement
/// semantics cannot drift between implementations.
pub(crate) fn validate_projection(
    document: &Document,
    chunks: &[Chunk],
    embeddings: &[Vec<f32>],
) -> Result<()> {
    if embeddings.len() != chunks.len() {
        return Err(Error::Consistency(format!(
            "projection for document {} has {} chunks but {} embeddings",
            document.meta.id,
            chunks.len(),
            embeddings.len()
        )));
    }
    let mut seen = HashSet::with_capacity(chunks.len());
    for chunk in chunks {
        if chunk.document_id != document.meta.id {
            return Err(Error::Consistency(format!(
                "chunk {} belongs to document {} but the projection is for {}",
                chunk.id, chunk.document_id, document.meta.id
            )));
        }
        if !seen.insert(chunk.sequence_index) {
            return Err(Error::Consistency(format!(
                "duplicate chunk sequence_index {} in projection for document {}",
                chunk.sequence_index, document.meta.id
            )));
        }
    }
    for expected in 0..chunks.len() {
        if !seen.contains(&expected) {
            return Err(Error::Consistency(format!(
                "chunk sequence indexes for document {} are not contiguous (missing {expected})",
                document.meta.id
            )));
        }
    }
    // The store is authoritative and the vector index is rebuilt from it, so
    // no embedding may be persisted that could not re-enter an index: every
    // vector must be finite and all vectors in a projection must share one
    // dimension. (The library additionally checks that dimension against the
    // configured embedder; the store cannot know the configured value.)
    if let Some(first) = embeddings.first() {
        let dimension = first.len();
        for (i, vector) in embeddings.iter().enumerate() {
            if vector.len() != dimension {
                return Err(Error::Consistency(format!(
                    "embedding {i} in projection for document {} has dimension {} \
                     but the projection's first embedding has dimension {dimension}",
                    document.meta.id,
                    vector.len()
                )));
            }
            if let Some(bad) = vector.iter().find(|v| !v.is_finite()) {
                return Err(Error::Consistency(format!(
                    "embedding {i} in projection for document {} contains a \
                     non-finite value ({bad})",
                    document.meta.id
                )));
            }
        }
    }
    Ok(())
}

pub mod memory;
pub mod sqlite;
