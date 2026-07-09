//! Nucklavee: universal document transformation library.

pub mod chunking;
pub mod embedder;
pub mod emitters;
pub mod ir;
pub mod net;
pub mod parsers;
pub mod phase2_contract;
pub mod storage;
pub mod test_support;
pub mod vector;

pub use chunking::{Chunk, ChunkOptions, Chunker, StructuralChunker};
pub use ir::{
    Block, BlockNode, ByteRange, Diagnostic, DiagnosticKind, Document, DocumentId, DocumentMeta,
    Frontmatter, Inline, ListItem, Provenance, Source, SourceFormat, SourceInfo, Style,
    ValidationError, normalize_document, structural_diff, structural_diff_bodies, validate,
};

/// How many chunks a `context_window` query retrieves before packing.
const CONTEXT_SEARCH_K: usize = 50;

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
    chunker: StructuralChunker,
}

#[derive(Debug, Clone, Copy)]
pub struct IngestOptions {
    pub normalize_bare_callouts: bool,
    /// Maximum tokens per chunk (spec §6.3 default: 512).
    pub token_budget: usize,
}

impl Default for IngestOptions {
    fn default() -> Self {
        Self {
            normalize_bare_callouts: false,
            token_budget: 512,
        }
    }
}

impl<S, V, E> Library<S, V, E>
where
    S: storage::DocumentStore,
    V: vector::VectorIndex,
    E: embedder::Embedder,
{
    /// Construct a library. Loads the chunker's tokenizer, so this is
    /// fallible.
    pub fn new(store: S, index: V, embedder: E) -> Result<Self> {
        Ok(Self {
            store,
            index,
            embedder,
            chunker: StructuralChunker::new()?,
        })
    }

    pub fn ingest(&mut self, source: Source) -> Result<DocumentId> {
        self.ingest_with_options(source, IngestOptions::default())
    }

    pub fn ingest_with_options(
        &mut self,
        source: Source,
        options: IngestOptions,
    ) -> Result<DocumentId> {
        enum IngestInput {
            Markdown(String),
            Html(String),
        }

        let (input, source_descriptor) = match source {
            Source::File(path) => {
                let ext = path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let read = |label: &str| {
                    std::fs::read_to_string(&path).map_err(|err| {
                        Error::InvalidInput(format!(
                            "failed reading {label} file '{}': {err}",
                            path.display()
                        ))
                    })
                };
                match ext.as_str() {
                    "md" => (
                        IngestInput::Markdown(read("markdown")?),
                        path.display().to_string(),
                    ),
                    "html" | "htm" => (
                        IngestInput::Html(read("html")?),
                        path.display().to_string(),
                    ),
                    "pdf" => {
                        return Err(phase2_contract::not_implemented(
                            phase2_contract::PDF_PIPELINE_NOT_IMPLEMENTED,
                        ));
                    }
                    other => {
                        return Err(Error::InvalidInput(
                            phase2_contract::unsupported_extension_message(other),
                        ));
                    }
                }
            }
            Source::RawMarkdown(markdown) => (IngestInput::Markdown(markdown), "raw:markdown".to_string()),
            Source::RawHtml(html) => (IngestInput::Html(html), "raw:html".to_string()),
            Source::Url(url) => {
                let fetched = net::fetch(&url)?;
                let input = match fetched.kind {
                    net::FetchedKind::Markdown => IngestInput::Markdown(fetched.body),
                    net::FetchedKind::Html => IngestInput::Html(fetched.body),
                };
                (input, fetched.final_url)
            }
        };

        let (doc, source_len) = match input {
            IngestInput::Markdown(markdown) => {
                let doc = parsers::markdown::parse_markdown(
                    &markdown,
                    parsers::markdown::ParseOptions {
                        source_descriptor: Some(source_descriptor),
                        normalize_repeated_leading_segment: false,
                        normalize_bare_callouts: options.normalize_bare_callouts,
                    },
                );
                (doc, markdown.len())
            }
            IngestInput::Html(html) => {
                let doc = parsers::html::parse_html(
                    &html,
                    parsers::html::HtmlParseOptions {
                        source_descriptor: Some(source_descriptor),
                        ..Default::default()
                    },
                );
                (doc, html.len())
            }
        };

        validate(&doc, Some(source_len))
            .map_err(|err| Error::InvalidInput(format!("validation failed: {err}")))?;

        // Canonicalize the stored IR so every backend and every downstream
        // reader (the chunker in particular) sees one tidy form regardless of
        // source format: merges adjacent text runs and drops empty text
        // nodes. Whitespace-agnostic and provenance-preserving.
        let mut doc = doc;
        normalize_document(&mut doc);

        // Content-hash deduplication (spec §7.4): re-ingesting identical raw
        // content returns the existing document instead of storing a copy.
        // Note: dedupe keys on raw content only, so differing IngestOptions
        // do not bypass it.
        if let Some(existing) = self.store.find_by_content_hash(&doc.meta.content_hash)? {
            return Ok(existing);
        }

        self.store.upsert_document(&doc)?;

        // Chunk → store chunks → embed → index (spec §2 ingest pipeline).
        let chunks = self.chunker.chunk(
            &doc,
            &ChunkOptions {
                token_budget: options.token_budget,
            },
        )?;
        if !chunks.is_empty() {
            self.store.insert_chunks(&chunks)?;
            let texts: Vec<&str> = chunks.iter().map(|c| c.content.as_str()).collect();
            let vectors = self.embedder.embed(&texts)?;
            if vectors.len() != chunks.len() {
                return Err(Error::Embedding(format!(
                    "embedder returned {} vectors for {} chunks",
                    vectors.len(),
                    chunks.len()
                )));
            }
            for (chunk, vector) in chunks.iter().zip(vectors) {
                self.index.add(chunk.id, vector)?;
            }
        }

        Ok(doc.meta.id)
    }

    /// Semantic search: embed `text`, retrieve the nearest chunks, and return
    /// them ranked most-relevant first with their provenance.
    pub fn query(&self, text: &str, limit: usize) -> Result<Vec<Chunk>> {
        self.search_chunks(text, limit)
    }

    /// Embed a query and return the nearest stored chunks in rank order.
    fn search_chunks(&self, text: &str, limit: usize) -> Result<Vec<Chunk>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let query_vector = self
            .embedder
            .embed(&[text])?
            .into_iter()
            .next()
            .ok_or_else(|| Error::Embedding("embedder returned no vector for query".to_string()))?;
        let hits = self.index.search(&query_vector, limit)?;
        let ids: Vec<chunking::ChunkId> = hits.into_iter().map(|(id, _)| id).collect();
        self.store.get_chunks_by_ids(&ids)
    }

    pub fn get_document(&self, id: DocumentId) -> Result<Document> {
        self.store.get_document(id)
    }

    pub fn emit(&self, id: DocumentId, format: Format) -> Result<String> {
        let document = self.get_document(id)?;

        match format {
            Format::Markdown => Ok(emitters::markdown::emit_markdown(&document)),
            Format::Html => Ok(emitters::html::emit_html(&document)),
            Format::PlainText => Ok(emitters::text::emit_text(&document)),
        }
    }

    /// Assemble a relevance-ranked, provenance-headed context window within a
    /// token budget (spec §7.3). Each retrieved chunk is prefixed with a
    /// `[Source: {title} > {section path}]` header; chunks are added in ranked
    /// order until the next one would exceed the budget.
    pub fn context_window(&self, query: &str, token_budget: usize) -> Result<String> {
        let chunks = self.search_chunks(query, CONTEXT_SEARCH_K)?;

        let mut out = String::new();
        let mut used = 0usize;
        let mut titles: std::collections::HashMap<DocumentId, String> =
            std::collections::HashMap::new();

        for chunk in chunks {
            let title = titles
                .entry(chunk.document_id)
                .or_insert_with(|| {
                    self.store
                        .get_document(chunk.document_id)
                        .ok()
                        .and_then(|d| d.meta.title)
                        .unwrap_or_else(|| "untitled".to_string())
                })
                .clone();

            let header = format_source_header(&title, &chunk.section_path);
            let block = format!("{header}\n{}\n", chunk.content);
            let cost = self.chunker.count_tokens(&block);
            if used + cost > token_budget {
                break;
            }
            out.push_str(&block);
            used += cost;
        }
        Ok(out)
    }

    pub fn store(&self) -> &S {
        &self.store
    }
}

/// Format a chunk's provenance header for `context_window` output.
///
/// The section path is the full heading breadcrumb, whose first element is
/// usually the document's H1 — i.e. the title itself. Drop that leading
/// segment when it matches so the header reads `[Source: Title > Section]`
/// rather than repeating the title.
fn format_source_header(title: &str, section_path: &[String]) -> String {
    let path = match section_path.split_first() {
        Some((first, rest)) if first == title => rest,
        _ => section_path,
    };
    if path.is_empty() {
        format!("[Source: {title}]")
    } else {
        format!("[Source: {title} > {}]", path.join(" > "))
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

    #[error("network error: {0}")]
    Network(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
