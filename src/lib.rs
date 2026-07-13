//! Nucklavee: universal document transformation library.

pub mod chunking;
pub mod embedder;
pub mod emitters;
pub mod ir;
pub mod net;
pub mod parsers;
pub mod contract;
pub mod storage;
/// Test-only stubs and deterministic fakes. Excluded from production builds;
/// enabled for this crate's own tests via the self dev-dependency.
#[cfg(feature = "test-support")]
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
                        return Err(contract::not_implemented(
                            contract::PDF_PIPELINE_NOT_IMPLEMENTED,
                        ));
                    }
                    other => {
                        return Err(Error::InvalidInput(
                            contract::unsupported_extension_message(other),
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
        doc.meta.processing_fingerprint =
            processing_fingerprint(&options, self.embedder.dimension());

        // Deduplication (spec §7.4) is content **and** configuration aware:
        // identical raw content processed under the same settings reuses the
        // stored document; the same content under different settings is
        // reprocessed in place, keeping a stable document identity.
        let mut reprocessing = false;
        if let Some(existing_id) = self.store.find_by_content_hash(&doc.meta.content_hash)? {
            let stored = self.store.get_document(existing_id)?;
            if stored.meta.processing_fingerprint == doc.meta.processing_fingerprint {
                return Ok(existing_id);
            }
            retag_document(&mut doc, existing_id);
            reprocessing = true;
        }

        // All fallible external work (chunking is local; embedding is the
        // network call) happens BEFORE any storage or index mutation, so a
        // failed ingest leaves no partially-written document behind and a
        // retry starts clean.
        let chunks = self.chunker.chunk(
            &doc,
            &ChunkOptions {
                token_budget: options.token_budget,
            },
        )?;
        let vectors = if chunks.is_empty() {
            Vec::new()
        } else {
            let texts: Vec<&str> = chunks.iter().map(|c| c.content.as_str()).collect();
            let vectors = self.embedder.embed(&texts)?;
            if vectors.len() != chunks.len() {
                return Err(Error::Embedding(format!(
                    "embedder returned {} vectors for {} chunks",
                    vectors.len(),
                    chunks.len()
                )));
            }
            vectors
        };

        if reprocessing {
            for old in self.store.get_chunks_by_document(doc.meta.id)? {
                self.index.remove(old.id)?;
            }
        }

        // Atomic in the store: document + chunks + embeddings land together
        // (or not at all). Embeddings are durable so the index below is a
        // derived projection that can always be rebuilt from the store.
        self.store
            .replace_document_projection(&doc, &chunks, &vectors)?;

        for (chunk, vector) in chunks.iter().zip(vectors) {
            self.index.add(chunk.id, vector).map_err(|e| {
                Error::Consistency(format!(
                    "document {} is stored but vector indexing failed: {e}; \
                     run rebuild_index() (CLI: `nucklavee rebuild-index`) to repair",
                    doc.meta.id
                ))
            })?;
        }

        Ok(doc.meta.id)
    }

    /// Rebuild the vector index from the store's durable embeddings, without
    /// re-calling the embedding provider. Returns the number of vectors
    /// indexed. Safe to run repeatedly; repairs a missing, stale, or
    /// partially-written index.
    pub fn rebuild_index(&mut self) -> Result<usize> {
        self.index.clear()?;
        let pairs = self.store.get_all_embeddings()?;
        let count = pairs.len();
        for (chunk_id, vector) in pairs {
            self.index.add(chunk_id, vector)?;
        }
        Ok(count)
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
    /// `[Source: {title} > {section path}]` header. Packing policy: candidates
    /// are considered in rank order and a candidate that does not fit is
    /// **skipped**, not allowed to end packing — one oversized result must not
    /// starve every later result. Header and separator tokens count against
    /// the budget.
    pub fn context_window(&self, query: &str, token_budget: usize) -> Result<String> {
        let chunks = self.search_chunks(query, CONTEXT_SEARCH_K)?;

        let mut out = String::new();
        let mut used = 0usize;
        let mut selected: std::collections::HashSet<chunking::ChunkId> =
            std::collections::HashSet::new();
        let mut titles: std::collections::HashMap<DocumentId, String> =
            std::collections::HashMap::new();

        for chunk in chunks {
            if used >= token_budget {
                break;
            }
            // A chunk appearing twice in the candidate list must not produce
            // duplicate context.
            if !selected.insert(chunk.id) {
                continue;
            }

            // A missing title is ordinary data (fall back to "untitled");
            // a failed document lookup is a storage/consistency problem and
            // must propagate, not silently render as display text.
            let title = match titles.get(&chunk.document_id) {
                Some(title) => title.clone(),
                None => {
                    let document = self.store.get_document(chunk.document_id).map_err(|e| {
                        Error::Consistency(format!(
                            "chunk {} references document {} which could not be loaded: {e}",
                            chunk.id, chunk.document_id
                        ))
                    })?;
                    let title = document
                        .meta
                        .title
                        .filter(|t| !t.trim().is_empty())
                        .unwrap_or_else(|| "untitled".to_string());
                    titles.insert(chunk.document_id, title.clone());
                    title
                }
            };

            let header = format_source_header(&title, &chunk.section_path);
            let block = format!("{header}\n{}\n", chunk.content);
            let cost = self.chunker.count_tokens(&block);
            if used + cost > token_budget {
                // Skip and keep considering later, smaller candidates.
                continue;
            }
            out.push_str(&block);
            used += cost;
        }
        Ok(out)
    }

    /// Remove a document and its chunks from both the store and the vector
    /// index. Idempotent.
    pub fn remove_document(&mut self, id: DocumentId) -> Result<()> {
        let chunks = self.store.get_chunks_by_document(id)?;
        for chunk in &chunks {
            self.index.remove(chunk.id)?;
        }
        self.store.remove_document(id)?;
        Ok(())
    }

    /// Persist the vector index to `path` (the document store persists itself).
    pub fn save_index(&self, path: &std::path::Path) -> Result<()> {
        self.index.save(path)
    }

    /// Load the vector index from `path`, replacing the in-memory index.
    pub fn load_index(&mut self, path: &std::path::Path) -> Result<()> {
        self.index.load(path)
    }

    pub fn store(&self) -> &S {
        &self.store
    }
}

/// Canonical fingerprint of every ingest setting that changes derived output.
/// Format-versioned (`fp1|…`) so future settings extend rather than collide.
fn processing_fingerprint(options: &IngestOptions, embed_dimension: usize) -> String {
    let canonical = format!(
        "fp1|normalize_bare_callouts={}|token_budget={}|embed_dimension={}",
        options.normalize_bare_callouts, options.token_budget, embed_dimension
    );
    parsers::sha256_hex(&canonical)
}

/// Rewrite a parsed document's identity to `id`, including the document ID
/// recorded in every block's provenance, so reprocessing existing content
/// keeps a stable logical document.
fn retag_document(doc: &mut Document, id: DocumentId) {
    fn retag_node(node: &mut BlockNode, id: DocumentId) {
        node.prov.document_id = id;
        match &mut node.block {
            Block::BlockQuote { children } => {
                for child in children {
                    retag_node(child, id);
                }
            }
            Block::List { items, .. } => {
                for item in items {
                    for child in &mut item.content {
                        retag_node(child, id);
                    }
                }
            }
            _ => {}
        }
    }
    doc.meta.id = id;
    for node in &mut doc.body {
        retag_node(node, id);
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

    /// Cross-store invariant violation (e.g. a chunk referencing a missing
    /// document, or a stored document whose vectors failed to index).
    /// Directs the operator toward verification or `rebuild_index`.
    #[error("consistency error: {0}")]
    Consistency(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
