use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::Result;
use crate::chunking::Chunk;
use crate::ir::{Document, DocumentId};
use crate::storage::DocumentStore;

#[derive(Debug, Clone, Default)]
pub struct InMemoryDocumentStore {
    inner: Arc<Mutex<InMemoryState>>,
}

#[derive(Debug, Default)]
struct InMemoryState {
    documents: HashMap<DocumentId, Document>,
    chunks_by_doc: HashMap<DocumentId, Vec<Chunk>>,
}

impl DocumentStore for InMemoryDocumentStore {
    fn upsert_document(&self, document: &Document) -> Result<()> {
        let mut state = self
            .inner
            .lock()
            .map_err(|_| crate::Error::Storage("in-memory store lock poisoned".to_string()))?;
        state.documents.insert(document.meta.id, document.clone());
        Ok(())
    }

    fn get_document(&self, id: DocumentId) -> Result<Document> {
        let state = self
            .inner
            .lock()
            .map_err(|_| crate::Error::Storage("in-memory store lock poisoned".to_string()))?;
        state
            .documents
            .get(&id)
            .cloned()
            .ok_or_else(|| crate::Error::Storage(format!("document not found: {id}")))
    }

    fn insert_chunks(&self, chunks: &[Chunk]) -> Result<()> {
        let mut state = self
            .inner
            .lock()
            .map_err(|_| crate::Error::Storage("in-memory store lock poisoned".to_string()))?;

        for chunk in chunks {
            state
                .chunks_by_doc
                .entry(chunk.document_id)
                .or_default()
                .push(chunk.clone());
        }

        Ok(())
    }

    fn get_chunks_by_document(&self, id: DocumentId) -> Result<Vec<Chunk>> {
        let state = self
            .inner
            .lock()
            .map_err(|_| crate::Error::Storage("in-memory store lock poisoned".to_string()))?;
        Ok(state.chunks_by_doc.get(&id).cloned().unwrap_or_default())
    }
}
