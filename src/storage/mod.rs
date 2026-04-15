use crate::Result;
use crate::ir::{Document, DocumentId};

pub trait DocumentStore {
    /// Persist a document. Overwrites any existing entry with the same id.
    fn put(&self, document: &Document) -> Result<()>;

    /// Look up a document by id.
    fn get(&self, id: DocumentId) -> Result<Document>;

    /// Return the id of an existing document whose `content_hash` matches,
    /// or `None` if no such document is stored. Used by `Library::ingest` to
    /// dedupe identical content.
    fn find_by_content_hash(&self, hash: &str) -> Result<Option<DocumentId>>;
}

pub mod in_memory;
pub mod sqlite;
