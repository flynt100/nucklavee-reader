//! Nucklavee: universal document transformation library.

pub mod chunking;
pub mod contract;
pub mod embedder;
pub mod emitters;
pub mod ir;
pub mod net;
pub mod parsers;
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
    /// Construct a library. Fallible for two reasons: it loads the chunker's
    /// tokenizer, and it enforces the collection-wide embedding-space
    /// invariant — the configured embedder, the store's bound space, and the
    /// vector index must all describe one vector space, checked here so a
    /// mismatched library cannot even be constructed (per-document
    /// fingerprints never see a search-only session).
    pub fn new(store: S, index: V, embedder: E) -> Result<Self> {
        let configured = embedder.embedding_space();

        if *index.embedding_space() != configured {
            return Err(Error::EmbeddingSpaceMismatch(format!(
                "the vector index is configured for the embedding space {}, \
                 but the embedder produces {}. Construct the index from the \
                 same embedder configuration",
                index.embedding_space(),
                configured
            )));
        }

        match store.embedding_space()? {
            Some(bound) if bound != configured => {
                return Err(Error::EmbeddingSpaceMismatch(format!(
                    "this library's stored embeddings belong to the embedding \
                     space {bound}, but the configured embedder produces \
                     {configured}. Rebuild-index cannot convert embeddings \
                     between models; reconfigure the original model or create \
                     a new library and re-ingest the documents"
                )));
            }
            Some(_) => {}
            None => {
                // Unbound metadata with durable embeddings means the store
                // predates space tracking (schema < 3). Those vectors' model
                // identity cannot be verified, so fail closed instead of
                // adopting the configured model for them.
                if store.has_embeddings()? {
                    return Err(Error::Consistency(
                        "this library contains legacy embeddings without \
                         vector-space metadata; their model identity cannot \
                         be verified. Create a new library and re-ingest the \
                         documents, or run an explicit embedding migration \
                         when one is available"
                            .to_string(),
                    ));
                }
            }
        }

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
                    "html" | "htm" => {
                        (IngestInput::Html(read("html")?), path.display().to_string())
                    }
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
            Source::RawMarkdown(markdown) => {
                (IngestInput::Markdown(markdown), "raw:markdown".to_string())
            }
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
            processing_fingerprint(&options, doc.meta.format, &self.embedder);

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
            // The store is authoritative and the index is rebuilt from it, so
            // every persisted embedding must be valid for the configured
            // index — regardless of which Embedder implementation produced
            // it. Rejecting here keeps invalid vectors out of SQLite.
            embedder::validate_embedding_batch(
                &vectors,
                chunks.len(),
                &self.embedder.embedding_space(),
                "document ingestion",
            )?;
            vectors
        };

        // The previous generation's chunk IDs must be captured *before* the
        // projection replacement retires them from the store.
        let old_chunk_ids: Vec<chunking::ChunkId> = if reprocessing {
            self.store
                .get_chunks_by_document(doc.meta.id)?
                .into_iter()
                .map(|c| c.id)
                .collect()
        } else {
            Vec::new()
        };

        // Atomic in the store: document + chunks + embeddings land together
        // (or not at all). The authoritative commit happens FIRST; only then
        // is the derived index touched, so a failure anywhere in index
        // maintenance leaves the store correct and is repairable by
        // `rebuild_index()`.
        self.store.replace_document_projection(
            &doc,
            &chunks,
            &vectors,
            &self.embedder.embedding_space(),
        )?;

        let consistency = |e: Error| {
            Error::Consistency(format!(
                "document {} is stored but vector-index maintenance failed: {e}; \
                 run rebuild_index() (CLI: `nucklavee rebuild-index`) to repair",
                doc.meta.id
            ))
        };
        for old_id in old_chunk_ids {
            self.index.remove(old_id).map_err(consistency)?;
        }
        for (chunk, vector) in chunks.iter().zip(vectors) {
            self.index.add(chunk.id, vector).map_err(consistency)?;
        }

        Ok(doc.meta.id)
    }

    /// Rebuild the vector index from the store's durable embeddings, without
    /// re-calling the embedding provider. Returns the number of vectors
    /// indexed. Safe to run repeatedly; repairs a missing, stale, or
    /// partially-written index.
    ///
    /// On failure the in-memory index may be partially populated; the fix is
    /// the same operation — run `rebuild_index()` again. Persisted index
    /// files are untouched (callers persist explicitly via [`Self::save_index`]
    /// after a successful rebuild), so a failed rebuild never damages the
    /// on-disk generation.
    pub fn rebuild_index(&mut self) -> Result<usize> {
        // Construction already proved store/index/embedder agree, but rebuild
        // re-checks the store's binding: a rebuild moves durable embeddings
        // into the index wholesale, and must never launder another model's
        // vectors into the configured space.
        let configured = self.embedder.embedding_space();
        match self.store.embedding_space()? {
            Some(bound) if bound != configured => {
                return Err(Error::EmbeddingSpaceMismatch(format!(
                    "cannot rebuild: the store's durable embeddings belong to \
                     the embedding space {bound}, but the configured embedder \
                     produces {configured}. Rebuilding cannot convert \
                     embeddings between models; reconfigure the original \
                     model or create a new library and re-ingest the documents"
                )));
            }
            _ => {}
        }
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
        // Query vectors cross the same trust boundary as ingest vectors: an
        // arbitrary Embedder may return the wrong count, a wrong dimension,
        // or non-finite values, and none of that may reach usearch. Extra
        // vectors are an error, never silently discarded.
        let vectors = self.embedder.embed(&[text])?;
        embedder::validate_embedding_batch(
            &vectors,
            1,
            &self.embedder.embedding_space(),
            "query embedding",
        )?;
        let query_vector = &vectors[0];
        let hits = self.index.search(query_vector, limit)?;
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
    /// index. Idempotent. The authoritative store is updated first; if the
    /// derived index then fails to clean up, the error directs the operator
    /// to `rebuild_index()` rather than leaving a document that is stored but
    /// half-searchable.
    pub fn remove_document(&mut self, id: DocumentId) -> Result<()> {
        let chunk_ids: Vec<chunking::ChunkId> = self
            .store
            .get_chunks_by_document(id)?
            .into_iter()
            .map(|c| c.id)
            .collect();
        self.store.remove_document(id)?;
        for chunk_id in chunk_ids {
            self.index.remove(chunk_id).map_err(|e| {
                Error::Consistency(format!(
                    "document {id} was removed from the store but index cleanup \
                     failed: {e}; run rebuild_index() (CLI: `nucklavee \
                     rebuild-index`) to repair",
                ))
            })?;
        }
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

/// Canonical fingerprint of everything that changes a document's derived
/// output: how the bytes were interpreted (source format + parser policy),
/// how they were normalized and chunked, and which vector space they were
/// embedded into. Two ingests may share a fingerprint only if their stored
/// projections are interchangeable. Format-versioned (`fp2|…`) so future
/// settings extend rather than collide.
fn processing_fingerprint<E: embedder::Embedder>(
    options: &IngestOptions,
    format: SourceFormat,
    embedder: &E,
) -> String {
    let parser_policy = match format {
        SourceFormat::Markdown => parsers::markdown::PARSER_POLICY_VERSION,
        SourceFormat::Html => parsers::html::PARSER_POLICY_VERSION,
        // No PDF parser exists yet; give it a distinct token so the first
        // real implementation cannot collide with text-format fingerprints.
        SourceFormat::Pdf => "pdf0-unimplemented",
    };
    let canonical = format!(
        "fp2|format={format:?}|parser_policy={parser_policy}\
         |normalize_bare_callouts={}|chunker={}|token_budget={}\
         |embedder={}|embed_dimension={}",
        options.normalize_bare_callouts,
        chunking::structural::CHUNKER_VERSION,
        options.token_budget,
        embedder.fingerprint(),
        embedder.dimension()
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

    /// The configured embedder's vector space does not match the space bound
    /// to the store or the persisted index. Distinct from `Consistency`
    /// because the remedy differs: `rebuild-index` repairs derived-index
    /// corruption but can never convert stored embeddings between models —
    /// the fix is reconfiguring the original model or re-ingesting into a
    /// new library.
    #[error("embedding-space mismatch: {0}")]
    EmbeddingSpaceMismatch(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
