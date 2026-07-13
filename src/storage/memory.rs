use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::Result;
use crate::chunking::{Chunk, ChunkId};
use crate::ir::{Document, DocumentId, DocumentMeta};
use crate::storage::{DocumentStore, validate_projection};

#[derive(Debug, Clone, Default)]
pub struct InMemoryDocumentStore {
    inner: Arc<Mutex<InMemoryState>>,
}

#[derive(Debug, Default)]
struct InMemoryState {
    documents: HashMap<DocumentId, Document>,
    chunks_by_doc: HashMap<DocumentId, Vec<Chunk>>,
    embeddings: HashMap<ChunkId, Vec<f32>>,
}

impl InMemoryDocumentStore {
    fn state(&self) -> Result<MutexGuard<'_, InMemoryState>> {
        self.inner
            .lock()
            .map_err(|_| crate::Error::Storage("in-memory store lock poisoned".to_string()))
    }
}

impl DocumentStore for InMemoryDocumentStore {
    fn get_document(&self, id: DocumentId) -> Result<Document> {
        let state = self.state()?;
        state
            .documents
            .get(&id)
            .cloned()
            .ok_or_else(|| crate::Error::Storage(format!("document not found: {id}")))
    }

    fn find_by_content_hash(&self, content_hash: &str) -> Result<Option<DocumentId>> {
        let state = self.state()?;
        Ok(state
            .documents
            .values()
            .find(|doc| doc.meta.content_hash == content_hash)
            .map(|doc| doc.meta.id))
    }

    fn list_documents(&self) -> Result<Vec<DocumentMeta>> {
        let state = self.state()?;
        Ok(state.documents.values().map(|doc| doc.meta.clone()).collect())
    }

    fn remove_document(&self, id: DocumentId) -> Result<()> {
        let mut state = self.state()?;
        state.documents.remove(&id);
        if let Some(chunks) = state.chunks_by_doc.remove(&id) {
            for chunk in chunks {
                state.embeddings.remove(&chunk.id);
            }
        }
        Ok(())
    }

    fn replace_document_projection(
        &self,
        document: &Document,
        chunks: &[Chunk],
        embeddings: &[Vec<f32>],
    ) -> Result<()> {
        validate_projection(document, chunks, embeddings)?;
        let mut state = self.state()?;

        // Retire the previous generation's embeddings before installing the
        // new set, mirroring the SQLite transaction.
        if let Some(old) = state.chunks_by_doc.remove(&document.meta.id) {
            for chunk in old {
                state.embeddings.remove(&chunk.id);
            }
        }

        state.documents.insert(document.meta.id, document.clone());
        state
            .chunks_by_doc
            .insert(document.meta.id, chunks.to_vec());
        for (chunk, embedding) in chunks.iter().zip(embeddings) {
            state.embeddings.insert(chunk.id, embedding.clone());
        }
        Ok(())
    }

    fn get_chunks_by_document(&self, id: DocumentId) -> Result<Vec<Chunk>> {
        let state = self.state()?;
        let mut chunks = state.chunks_by_doc.get(&id).cloned().unwrap_or_default();
        // Contract: ordered by sequence_index (matches the SQLite backend).
        chunks.sort_by_key(|c| c.sequence_index);
        Ok(chunks)
    }

    fn get_chunks_by_ids(&self, ids: &[ChunkId]) -> Result<Vec<Chunk>> {
        let state = self.state()?;
        let by_id: HashMap<ChunkId, &Chunk> = state
            .chunks_by_doc
            .values()
            .flatten()
            .map(|chunk| (chunk.id, chunk))
            .collect();
        Ok(ids
            .iter()
            .filter_map(|id| by_id.get(id).map(|chunk| (*chunk).clone()))
            .collect())
    }

    fn get_all_embeddings(&self) -> Result<Vec<(ChunkId, Vec<f32>)>> {
        let state = self.state()?;
        Ok(state
            .embeddings
            .iter()
            .map(|(id, v)| (*id, v.clone()))
            .collect())
    }
}
