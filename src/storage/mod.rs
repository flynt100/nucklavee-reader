use crate::Result;
use crate::chunking::Chunk;
use crate::ir::{Document, DocumentId};

pub trait DocumentStore {
    fn upsert_document(&self, document: &Document) -> Result<()>;
    fn get_document(&self, id: DocumentId) -> Result<Document>;
    fn insert_chunks(&self, chunks: &[Chunk]) -> Result<()>;
    fn get_chunks_by_document(&self, id: DocumentId) -> Result<Vec<Chunk>>;
}

pub mod sqlite;
