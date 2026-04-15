use std::collections::HashMap;
use std::sync::RwLock;

use crate::ir::{Document, DocumentId};
use crate::storage::DocumentStore;
use crate::{Error, Result};

/// In-process document store backed by a `HashMap`. Intended for tests and
/// for exercising the `Library` facade before SQLite is wired up.
#[derive(Debug, Default)]
pub struct InMemoryDocumentStore {
    inner: RwLock<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    documents: HashMap<DocumentId, Document>,
    by_hash: HashMap<String, DocumentId>,
}

impl InMemoryDocumentStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl DocumentStore for InMemoryDocumentStore {
    fn put(&self, document: &Document) -> Result<()> {
        let mut inner = self
            .inner
            .write()
            .map_err(|_| Error::Storage("in-memory store lock poisoned".to_string()))?;
        inner
            .by_hash
            .insert(document.meta.content_hash.clone(), document.meta.id);
        inner.documents.insert(document.meta.id, document.clone());
        Ok(())
    }

    fn get(&self, id: DocumentId) -> Result<Document> {
        let inner = self
            .inner
            .read()
            .map_err(|_| Error::Storage("in-memory store lock poisoned".to_string()))?;
        inner
            .documents
            .get(&id)
            .cloned()
            .ok_or_else(|| Error::Storage(format!("document {id} not found")))
    }

    fn find_by_content_hash(&self, hash: &str) -> Result<Option<DocumentId>> {
        let inner = self
            .inner
            .read()
            .map_err(|_| Error::Storage("in-memory store lock poisoned".to_string()))?;
        Ok(inner.by_hash.get(hash).copied())
    }
}
