//! Nucklavee: universal document transformation library.

pub mod chunking;
pub mod context;
pub mod embedder;
pub mod emitters;
pub mod ir;
pub mod parsers;
pub mod pipeline;
pub mod storage;
pub mod vector;

pub use chunking::Chunk;
pub use ir::{Document, DocumentId, Source, SourceFormat};

/// Primary API entrypoint for document ingestion and retrieval.
pub struct Library<S, V, E>
where
    S: storage::DocumentStore,
    V: vector::VectorIndex,
    E: embedder::Embedder,
{
    store: S,
    index: V,
    embedder: E,
}

impl<S, V, E> Library<S, V, E>
where
    S: storage::DocumentStore,
    V: vector::VectorIndex,
    E: embedder::Embedder,
{
    pub fn new(store: S, index: V, embedder: E) -> Self {
        Self {
            store,
            index,
            embedder,
        }
    }

    pub fn ingest(&mut self, _source: Source) -> Result<DocumentId> {
        Err(Error::NotImplemented("ingest"))
    }

    pub fn query(&self, _text: &str, _limit: usize) -> Result<Vec<Chunk>> {
        Err(Error::NotImplemented("query"))
    }

    pub fn get_document(&self, _id: DocumentId) -> Result<Document> {
        Err(Error::NotImplemented("get_document"))
    }

    pub fn emit(&self, _id: DocumentId, _format: Format) -> Result<String> {
        Err(Error::NotImplemented("emit"))
    }

    pub fn context_window(&self, _query: &str, _token_budget: usize) -> Result<String> {
        Err(Error::NotImplemented("context_window"))
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    pub fn index(&self) -> &V {
        &self.index
    }

    pub fn embedder(&self) -> &E {
        &self.embedder
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Format {
    Markdown,
    Html,
    PlainText,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not implemented: {0}")]
    NotImplemented(&'static str),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("parse error: {0}")]
    Parse(String),

    #[error("emit error: {0}")]
    Emit(String),

    #[error("chunking error: {0}")]
    Chunking(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("vector index error: {0}")]
    VectorIndex(String),

    #[error("embedding error: {0}")]
    Embedding(String),
}

/// Crate-wide `Result` alias defaulting to `crate::Error`.
pub type Result<T, E = Error> = std::result::Result<T, E>;
