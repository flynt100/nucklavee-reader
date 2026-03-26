use crate::chunking::Chunk;
use crate::ir::{Document, DocumentId};

pub trait DocumentStore {
    fn upsert_document(&self, document: &Document) -> Result<(), String>;
    fn get_document(&self, id: DocumentId) -> Result<Document, String>;
    fn insert_chunks(&self, chunks: &[Chunk]) -> Result<(), String>;
    fn get_chunks_by_document(&self, id: DocumentId) -> Result<Vec<Chunk>, String>;
}

pub mod sqlite;
