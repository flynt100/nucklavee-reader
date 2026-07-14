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
use crate::embedder::EmbeddingSpace;
use crate::ir::{Document, DocumentId, DocumentMeta};
use crate::{Error, Result};

/// Document and chunk persistence contract (frozen 2026-07-06; projection
/// semantics revised 2026-07-13 per the external reliability review;
/// embedding-space binding added 2026-07-14 stabilization gate).
pub trait DocumentStore {
    fn get_document(&self, id: DocumentId) -> Result<Document>;

    /// The single vector space this store's durable embeddings belong to, or
    /// `None` if the store has never held a projection (unbound). Once bound,
    /// the space persists even after every document is removed — silent
    /// rebinding would make accidental model changes too easy.
    fn embedding_space(&self) -> Result<Option<EmbeddingSpace>>;

    /// Whether any durable embeddings exist, without loading them. Backs the
    /// legacy fail-closed check: an unbound store that nevertheless holds
    /// embeddings predates space metadata and cannot be trusted.
    fn has_embeddings(&self) -> Result<bool>;

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
    /// sequence indexes are unique and contiguous from zero, and every
    /// embedding matches `embedding_space.dimension`.
    ///
    /// Space binding happens inside the same atomic operation as the write:
    /// the first projection binds the store to `embedding_space`; every later
    /// projection must supply the identical space or the write is rejected
    /// before any mutation. This closes the race between checking the bound
    /// space and writing a document.
    fn replace_document_projection(
        &self,
        document: &Document,
        chunks: &[Chunk],
        embeddings: &[Vec<f32>],
        embedding_space: &EmbeddingSpace,
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
    embedding_space: &EmbeddingSpace,
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
    // vector must be finite and match the projection's declared embedding
    // space exactly.
    for (i, vector) in embeddings.iter().enumerate() {
        if vector.len() != embedding_space.dimension {
            return Err(Error::Consistency(format!(
                "embedding {i} in projection for document {} has dimension {} \
                 but the projection's embedding space has dimension {}",
                document.meta.id,
                vector.len(),
                embedding_space.dimension
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
    Ok(())
}

/// Shared bind-or-verify decision for projection writes, so both backends
/// reject a space mismatch with the same diagnostic. `bound` is the store's
/// currently persisted space (read inside the backend's own transaction).
/// Returns `true` when the caller must bind `supplied` as the store's space.
pub(crate) fn check_space_binding(
    bound: Option<&EmbeddingSpace>,
    supplied: &EmbeddingSpace,
) -> Result<bool> {
    match bound {
        None => Ok(true),
        Some(existing) if existing == supplied => Ok(false),
        Some(existing) => Err(Error::EmbeddingSpaceMismatch(format!(
            "this library's embeddings belong to the embedding space {existing}, \
             but the projection was produced in the space {supplied}. One library \
             holds exactly one embedding space; rebuild-index cannot convert \
             embeddings between models. Reconfigure the original model or create \
             a new library and re-ingest the documents"
        ))),
    }
}

pub mod memory;
pub mod sqlite;
