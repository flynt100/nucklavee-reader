//! Nucklavee: universal document transformation library.

pub mod chunking;
pub mod embedder;
pub mod emitters;
pub mod error;
pub mod ir;
pub mod parsers;
pub mod storage;
pub mod vector;

pub use chunking::Chunk;
pub use error::{Error, Result};
pub use ir::{Document, DocumentId, Source, SourceFormat};

use crate::emitters::Emitter;
use crate::emitters::markdown::MarkdownEmitter;
use crate::parsers::Parser;
use crate::parsers::markdown::MarkdownParser;
use crate::storage::DocumentStore;

/// Primary API entrypoint for document ingestion and retrieval.
///
/// Only storage is wired today. `query` and `context_window` return
/// `Error::NotImplemented` until the vector index + embedder land; their
/// generic parameters will be reintroduced at that point.
pub struct Library<S: DocumentStore> {
    store: S,
}

impl<S: DocumentStore> Library<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    /// Parse, validate, and persist a document. If an identical document (by
    /// SHA-256 content hash) is already stored, return its existing id
    /// without re-parsing — this matches the dedupe contract in spec §7.4.
    pub fn ingest(&mut self, source: Source) -> Result<DocumentId> {
        let format = source.detect_format()?;
        let text = source.read_text()?;

        let document = match format {
            SourceFormat::Markdown => MarkdownParser.parse(&text, &source)?,
            SourceFormat::Html => return Err(Error::NotImplemented("html parser")),
            SourceFormat::Pdf => return Err(Error::NotImplemented("pdf parser")),
        };

        document
            .validate_strict()
            .map_err(|e| Error::Parse(format!("parser produced invalid IR: {e}")))?;

        if let Some(existing) = self.store.find_by_content_hash(&document.meta.content_hash)? {
            return Ok(existing);
        }

        let id = document.meta.id;
        self.store.put(&document)?;
        Ok(id)
    }

    pub fn get_document(&self, id: DocumentId) -> Result<Document> {
        self.store.get(id)
    }

    pub fn emit(&self, id: DocumentId, format: Format) -> Result<String> {
        let document = self.store.get(id)?;
        match format {
            Format::Markdown => MarkdownEmitter.emit(&document),
            Format::Html => Err(Error::NotImplemented("html emitter")),
            Format::PlainText => Err(Error::NotImplemented("plaintext emitter")),
        }
    }

    pub fn query(&self, _text: &str, _limit: usize) -> Result<Vec<Chunk>> {
        Err(Error::NotImplemented("query"))
    }

    pub fn context_window(&self, _query: &str, _token_budget: usize) -> Result<String> {
        Err(Error::NotImplemented("context_window"))
    }

    pub fn store(&self) -> &S {
        &self.store
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Format {
    Markdown,
    Html,
    PlainText,
}
