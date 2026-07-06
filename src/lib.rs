//! Nucklavee: universal document transformation library.

pub mod chunking;
pub mod context;
pub mod embedder;
pub mod emitters;
pub mod ir;
pub mod parsers;
pub mod phase2_contract;
pub mod pipeline;
pub mod storage;
pub mod test_support;
pub mod vector;

pub use chunking::Chunk;
pub use ir::{
    Block, BlockNode, ByteRange, Diagnostic, DiagnosticKind, Document, DocumentId, DocumentMeta,
    Frontmatter, Inline, ListItem, Provenance, Source, SourceFormat, SourceInfo, Style,
    ValidationError, normalize_document, structural_diff, structurally_equivalent, validate,
};

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

#[derive(Debug, Clone, Copy, Default)]
pub struct IngestOptions {
    pub normalize_bare_callouts: bool,
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

    pub fn ingest(&mut self, source: Source) -> Result<DocumentId> {
        self.ingest_with_options(source, IngestOptions::default())
    }

    pub fn ingest_with_options(
        &mut self,
        source: Source,
        options: IngestOptions,
    ) -> Result<DocumentId> {
        let (markdown, source_descriptor) = match source {
            Source::File(path) => {
                let ext = path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or_default();
                if !ext.eq_ignore_ascii_case("md") {
                    return Err(Error::NotImplemented(
                        "ingest currently supports markdown files only (.md). html/pdf ingest is not implemented in Phase 2",
                    ));
                }

                let markdown = std::fs::read_to_string(&path).map_err(|err| {
                    Error::InvalidInput(format!(
                        "failed reading markdown file '{}': {err}",
                        path.display()
                    ))
                })?;
                (markdown, path.display().to_string())
            }
            Source::RawMarkdown(markdown) => (markdown, "raw:markdown".to_string()),
            Source::Url(_) => {
                return Err(Error::NotImplemented(
                    "URL ingest is not implemented in Phase 2; provide a local .md file",
                ));
            }
            Source::RawHtml(_) => {
                return Err(Error::NotImplemented(
                    "html ingest is not implemented in Phase 2; markdown only",
                ));
            }
        };

        let doc = parsers::markdown::parse_markdown(
            &markdown,
            parsers::markdown::ParseOptions {
                source_descriptor: Some(source_descriptor),
                normalize_repeated_leading_segment: false,
                normalize_bare_callouts: options.normalize_bare_callouts,
            },
        );

        validate(&doc, Some(markdown.len()))
            .map_err(|err| Error::InvalidInput(format!("validation failed: {err}")))?;

        self.store.upsert_document(&doc)?;

        Ok(doc.meta.id)
    }

    pub fn query(&self, _text: &str, _limit: usize) -> Result<Vec<Chunk>> {
        Err(phase2_contract::not_implemented(
            phase2_contract::QUERY_NOT_IMPLEMENTED,
        ))
    }

    pub fn get_document(&self, id: DocumentId) -> Result<Document> {
        self.store.get_document(id)
    }

    pub fn emit(&self, id: DocumentId, format: Format) -> Result<String> {
        let document = self.get_document(id)?;

        match format {
            Format::Markdown => Ok(emitters::markdown::emit_markdown(&document)),
            Format::Html => Err(phase2_contract::invalid_input(
                phase2_contract::unsupported_format_message("html"),
            )),
            Format::PlainText => Err(phase2_contract::invalid_input(
                phase2_contract::unsupported_format_message("text"),
            )),
        }
    }

    pub fn context_window(&self, _query: &str, _token_budget: usize) -> Result<String> {
        Err(phase2_contract::not_implemented(
            phase2_contract::CONTEXT_WINDOW_NOT_IMPLEMENTED,
        ))
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

pub type Result<T, E = Error> = std::result::Result<T, E>;
