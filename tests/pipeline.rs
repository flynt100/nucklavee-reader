//! End-to-end pipeline test (audit Task 9): ingest → chunk → embed → index,
//! then `query` and `context_window`. Uses a real usearch index and the
//! deterministic offline `HashEmbedder` so ranking is meaningful without a
//! network or model.

use nucklavee::Library;
use nucklavee::ir::Source;
use nucklavee::storage::memory::InMemoryDocumentStore;
use nucklavee::test_support::HashEmbedder;
use nucklavee::vector::usearch::UsearchIndex;

const DIM: usize = 256;

/// Empty usearch index matching `HashEmbedder::new(DIM)`'s space.
fn hash_index() -> UsearchIndex {
    UsearchIndex::for_embedder(&HashEmbedder::new(DIM)).expect("usearch index")
}

fn library() -> Library<InMemoryDocumentStore, UsearchIndex, HashEmbedder> {
    Library::new(
        InMemoryDocumentStore::default(),
        hash_index(),
        HashEmbedder::new(DIM),
    )
    .expect("build library")
}

const DOC: &str = "\
# Field Guide

## Apples

Apples are a crunchy red fruit that grow on trees in temperate orchards.
They are sweet and often pressed into cider.

## Oceans

The ocean is a vast body of salt water covering most of the planet.
Tides rise and fall with the pull of the moon.

## Rust Programming

Rust is a systems programming language focused on memory safety without a
garbage collector, using an ownership model checked at compile time.
";

#[test]
fn query_retrieves_the_most_relevant_section() {
    let mut lib = library();
    lib.ingest(Source::RawMarkdown(DOC.into())).expect("ingest");

    let results = lib
        .query("crunchy sweet red fruit orchard cider", 3)
        .expect("query");
    assert!(!results.is_empty(), "expected search hits");
    assert!(
        results[0].content.to_lowercase().contains("apples"),
        "apple query should rank the Apples section first, got:\n{}",
        results[0].content
    );
    assert_eq!(
        results[0].section_path,
        vec!["Field Guide".to_string(), "Apples".to_string()],
        "top hit carries its provenance path"
    );

    let rust = lib
        .query("memory safety ownership compile time systems language", 3)
        .expect("query");
    assert!(
        rust[0].content.to_lowercase().contains("rust"),
        "rust query should rank the Rust section first, got:\n{}",
        rust[0].content
    );
}

#[test]
fn query_on_empty_library_returns_nothing() {
    let lib = library();
    assert!(lib.query("anything at all", 5).expect("query").is_empty());
    assert!(lib.query("x", 0).expect("zero limit").is_empty());
}

#[test]
fn context_window_packs_ranked_chunks_with_provenance_headers() {
    let mut lib = library();
    lib.ingest(Source::RawMarkdown(DOC.into())).expect("ingest");

    let ctx = lib
        .context_window("ocean salt water tides moon planet", 500)
        .expect("context window");

    assert!(
        ctx.contains("[Source: Field Guide > Oceans]"),
        "missing provenance header:\n{ctx}"
    );
    assert!(
        ctx.contains("salt water"),
        "missing retrieved content:\n{ctx}"
    );
    // The most relevant section is packed first.
    let oceans_at = ctx.find("Oceans").expect("oceans present");
    let apples_at = ctx.find("Apples");
    if let Some(apples_at) = apples_at {
        assert!(
            oceans_at < apples_at,
            "oceans should be packed before apples:\n{ctx}"
        );
    }
}

#[test]
fn context_window_respects_token_budget() {
    let mut lib = library();
    lib.ingest(Source::RawMarkdown(DOC.into())).expect("ingest");

    let big = lib
        .context_window("fruit water rust", 1000)
        .expect("big ctx");
    let tiny = lib
        .context_window("fruit water rust", 20)
        .expect("tiny ctx");

    assert!(
        tiny.len() < big.len(),
        "a tiny budget must produce less output"
    );
    // A budget of zero yields nothing.
    assert!(
        lib.context_window("fruit", 0)
            .expect("zero budget")
            .is_empty()
    );
}

#[test]
fn dedupe_does_not_double_index() {
    let mut lib = library();
    let a = lib
        .ingest(Source::RawMarkdown(DOC.into()))
        .expect("ingest 1");
    let b = lib
        .ingest(Source::RawMarkdown(DOC.into()))
        .expect("ingest 2 (dupe)");
    assert_eq!(a, b, "identical content dedupes to one document");

    // Still exactly one document's worth of chunks retrievable.
    let results = lib.query("apples crunchy red", 10).expect("query");
    let doc_ids: std::collections::HashSet<_> = results.iter().map(|c| c.document_id).collect();
    assert_eq!(
        doc_ids.len(),
        1,
        "results come from a single deduped document"
    );
}

// --- reliability regression scenarios (external review, 2026-07-13) ---------

use std::sync::atomic::{AtomicUsize, Ordering};

use nucklavee::embedder::Embedder;
use nucklavee::{Error, IngestOptions};

/// Embedder that fails its first `fail_first` embed calls, then delegates.
struct FlakyEmbedder {
    inner: HashEmbedder,
    calls: AtomicUsize,
    fail_first: usize,
}

impl Embedder for FlakyEmbedder {
    fn embed(&self, texts: &[&str]) -> nucklavee::Result<Vec<Vec<f32>>> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        if n < self.fail_first {
            return Err(Error::Embedding("injected transport failure".into()));
        }
        self.inner.embed(texts)
    }

    fn dimension(&self) -> usize {
        self.inner.dimension()
    }

    fn fingerprint(&self) -> String {
        self.inner.fingerprint()
    }
}

#[test]
fn failed_embedding_leaves_no_partial_document_and_retry_succeeds() {
    let store = InMemoryDocumentStore::default();
    let mut lib = Library::new(
        store.clone(),
        hash_index(),
        FlakyEmbedder {
            inner: HashEmbedder::new(DIM),
            calls: AtomicUsize::new(0),
            fail_first: 1,
        },
    )
    .expect("build library");

    use nucklavee::storage::DocumentStore;

    // First ingest: the embed call fails — nothing may be stored.
    let err = lib
        .ingest(Source::RawMarkdown(DOC.into()))
        .expect_err("injected failure must surface");
    assert!(matches!(err, Error::Embedding(_)), "got {err}");
    assert!(
        store.list_documents().expect("list").is_empty(),
        "a failed ingest must not leave a partial document behind"
    );
    assert!(store.get_all_embeddings().expect("emb").is_empty());

    // Retry: dedupe cannot return a phantom (nothing stored), so the full
    // pipeline reruns and completes.
    let id = lib
        .ingest(Source::RawMarkdown(DOC.into()))
        .expect("retry succeeds");
    assert_eq!(store.list_documents().expect("list").len(), 1);
    assert!(
        !store.get_chunks_by_document(id).expect("chunks").is_empty(),
        "retry stores the complete projection"
    );
    let hits = lib
        .query("crunchy sweet red fruit orchard", 3)
        .expect("query");
    assert!(!hits.is_empty(), "retried document is searchable");
}

#[test]
fn same_content_different_options_reprocesses_with_stable_identity() {
    let store = InMemoryDocumentStore::default();
    let mut lib =
        Library::new(store.clone(), hash_index(), HashEmbedder::new(DIM)).expect("build library");

    use nucklavee::storage::DocumentStore;

    let first = lib
        .ingest_with_options(
            Source::RawMarkdown(DOC.into()),
            IngestOptions {
                token_budget: 512,
                ..Default::default()
            },
        )
        .expect("first ingest");
    let chunks_512 = store.get_chunks_by_document(first).expect("chunks").len();

    // Same content, same options → reuse (no reprocessing).
    let again = lib
        .ingest_with_options(
            Source::RawMarkdown(DOC.into()),
            IngestOptions {
                token_budget: 512,
                ..Default::default()
            },
        )
        .expect("same-options ingest");
    assert_eq!(first, again, "identical content+options dedupes");

    // Same content, different token budget → reprocessed in place with a
    // stable document identity and a replaced chunk projection.
    let tiny = lib
        .ingest_with_options(
            Source::RawMarkdown(DOC.into()),
            IngestOptions {
                token_budget: 16,
                ..Default::default()
            },
        )
        .expect("reprocess ingest");
    assert_eq!(first, tiny, "reprocessing keeps the same document id");
    assert_eq!(store.list_documents().expect("list").len(), 1);

    let chunks_16 = store.get_chunks_by_document(first).expect("chunks").len();
    assert!(
        chunks_16 > chunks_512,
        "a smaller budget must produce more chunks ({chunks_16} vs {chunks_512})"
    );

    // Search reflects the new projection, not stale vectors.
    let hits = lib
        .query("crunchy sweet red fruit orchard", 5)
        .expect("query");
    assert!(!hits.is_empty());
    let doc_chunks: std::collections::HashSet<_> = store
        .get_chunks_by_document(first)
        .expect("chunks")
        .into_iter()
        .map(|c| c.id)
        .collect();
    assert!(
        hits.iter().all(|h| doc_chunks.contains(&h.id)),
        "every hit belongs to the current chunk generation"
    );
}

/// Embedder that panics if used — proves rebuild never re-embeds.
struct ForbiddenEmbedder;

impl Embedder for ForbiddenEmbedder {
    fn embed(&self, _texts: &[&str]) -> nucklavee::Result<Vec<Vec<f32>>> {
        panic!("rebuild_index must not call the embedding provider");
    }

    fn dimension(&self) -> usize {
        DIM
    }

    fn fingerprint(&self) -> String {
        format!("hash-bow|v1|dim={DIM}")
    }
}

#[test]
fn index_rebuilds_from_durable_embeddings_without_reembedding() {
    let store = InMemoryDocumentStore::default();

    // Ingest normally.
    {
        let mut lib = Library::new(store.clone(), hash_index(), HashEmbedder::new(DIM))
            .expect("build library");
        lib.ingest(Source::RawMarkdown(DOC.into())).expect("ingest");
    }

    // Fresh empty index (simulates a missing/corrupt index file) + an
    // embedder that panics if consulted: rebuild must restore search purely
    // from the store's durable embeddings.
    let mut lib =
        Library::new(store.clone(), hash_index(), ForbiddenEmbedder).expect("build library");

    let restored = lib.rebuild_index().expect("rebuild");
    assert!(restored > 0, "rebuild indexed the stored embeddings");

    // Query embeds via HashEmbedder externally to avoid the forbidden one.
    // Instead, verify through a HashEmbedder-backed library sharing state.
    drop(lib);
    let mut rebuilt =
        Library::new(store.clone(), hash_index(), HashEmbedder::new(DIM)).expect("build");
    rebuilt.rebuild_index().expect("rebuild");
    let hits = rebuilt
        .query("ocean salt water tides moon", 3)
        .expect("query after rebuild");
    assert!(
        hits[0].content.to_lowercase().contains("ocean"),
        "rebuilt index ranks correctly:\n{}",
        hits[0].content
    );
}

#[test]
fn oversized_first_result_does_not_starve_later_results() {
    let store = InMemoryDocumentStore::default();
    let mut lib = Library::new(store, hash_index(), HashEmbedder::new(DIM)).expect("build library");

    // The "big" section repeats the query vocabulary heavily so it ranks
    // first; the "small" section shares the vocabulary but is tiny.
    let ocean_words = "whale kraken tide brine seafoam abyss current reef ";
    let big_body = ocean_words.repeat(60);
    let doc = format!(
        "# Sea Notes\n\n## Deep Register\n\n{big_body}\n\n## Short Register\n\nwhale kraken tide brine.\n"
    );
    lib.ingest_with_options(
        Source::RawMarkdown(doc),
        IngestOptions {
            token_budget: 4096,
            ..Default::default()
        },
    )
    .expect("ingest");

    // Budget fits the small chunk but not the big one.
    let ctx = lib.context_window(ocean_words, 60).expect("context window");
    assert!(
        !ctx.is_empty(),
        "an oversized top result must not force an empty context"
    );
    assert!(
        ctx.contains("Short Register"),
        "the smaller later candidate should be packed:\n{ctx}"
    );
}

/// Store wrapper whose `get_document` always fails — proves context assembly
/// propagates storage failures instead of rendering them as "untitled".
#[derive(Clone)]
struct BrokenDocLookupStore {
    inner: InMemoryDocumentStore,
}

impl nucklavee::storage::DocumentStore for BrokenDocLookupStore {
    fn get_document(&self, _id: nucklavee::DocumentId) -> nucklavee::Result<nucklavee::Document> {
        Err(Error::Storage("injected lookup failure".into()))
    }
    fn find_by_content_hash(&self, h: &str) -> nucklavee::Result<Option<nucklavee::DocumentId>> {
        self.inner.find_by_content_hash(h)
    }
    fn list_documents(&self) -> nucklavee::Result<Vec<nucklavee::DocumentMeta>> {
        self.inner.list_documents()
    }
    fn remove_document(&self, id: nucklavee::DocumentId) -> nucklavee::Result<()> {
        self.inner.remove_document(id)
    }
    fn embedding_space(&self) -> nucklavee::Result<Option<nucklavee::embedder::EmbeddingSpace>> {
        self.inner.embedding_space()
    }
    fn has_embeddings(&self) -> nucklavee::Result<bool> {
        self.inner.has_embeddings()
    }
    fn replace_document_projection(
        &self,
        d: &nucklavee::Document,
        c: &[nucklavee::Chunk],
        e: &[Vec<f32>],
        s: &nucklavee::embedder::EmbeddingSpace,
    ) -> nucklavee::Result<()> {
        self.inner.replace_document_projection(d, c, e, s)
    }
    fn get_chunks_by_document(
        &self,
        id: nucklavee::DocumentId,
    ) -> nucklavee::Result<Vec<nucklavee::Chunk>> {
        self.inner.get_chunks_by_document(id)
    }
    fn get_chunks_by_ids(
        &self,
        ids: &[nucklavee::chunking::ChunkId],
    ) -> nucklavee::Result<Vec<nucklavee::Chunk>> {
        self.inner.get_chunks_by_ids(ids)
    }
    fn get_all_embeddings(
        &self,
    ) -> nucklavee::Result<Vec<(nucklavee::chunking::ChunkId, Vec<f32>)>> {
        self.inner.get_all_embeddings()
    }
}

#[test]
fn context_window_propagates_document_lookup_failures() {
    let broken = BrokenDocLookupStore {
        inner: InMemoryDocumentStore::default(),
    };
    let mut lib =
        Library::new(broken, hash_index(), HashEmbedder::new(DIM)).expect("build library");
    lib.ingest(Source::RawMarkdown(DOC.into())).expect("ingest");

    let err = lib
        .context_window("apples crunchy fruit", 500)
        .expect_err("storage failure must propagate, not render as 'untitled'");
    assert!(matches!(err, Error::Consistency(_)), "got {err}");
}

#[test]
fn missing_title_renders_as_untitled() {
    let mut lib = library();
    // No H1 anywhere → meta.title is None.
    lib.ingest(Source::RawMarkdown(
        "plain kelp forest paragraph with no heading at all\n".into(),
    ))
    .expect("ingest");

    let ctx = lib
        .context_window("kelp forest paragraph", 300)
        .expect("ctx");
    assert!(
        ctx.contains("[Source: untitled]"),
        "missing title is ordinary data and falls back to 'untitled':\n{ctx}"
    );
}

/// HashEmbedder wrapper claiming a distinct model identity — same vectors,
/// same dimension, different vector-space fingerprint.
struct NamedModelEmbedder {
    inner: HashEmbedder,
    model: &'static str,
}

impl NamedModelEmbedder {
    fn new(model: &'static str) -> Self {
        Self {
            inner: HashEmbedder::new(DIM),
            model,
        }
    }
}

impl Embedder for NamedModelEmbedder {
    fn embed(&self, texts: &[&str]) -> nucklavee::Result<Vec<Vec<f32>>> {
        self.inner.embed(texts)
    }
    fn dimension(&self) -> usize {
        self.inner.dimension()
    }
    fn fingerprint(&self) -> String {
        format!("named-model|{}|dim={}", self.model, self.inner.dimension())
    }
}

/// A library over `store` configured for a named model, with a matching
/// empty index.
fn named_model_library(
    store: InMemoryDocumentStore,
    model: &'static str,
) -> nucklavee::Result<Library<InMemoryDocumentStore, UsearchIndex, NamedModelEmbedder>> {
    let embedder = NamedModelEmbedder::new(model);
    let index = UsearchIndex::for_embedder(&embedder).expect("index");
    Library::new(store, index, embedder)
}

// --- embedding-space stabilization scenarios (2026-07-14 gate) ---------------

#[test]
fn different_document_under_a_different_model_cannot_enter_the_library() {
    use nucklavee::storage::DocumentStore;

    let store = InMemoryDocumentStore::default();

    // Model A ingests document A and binds the store's space.
    let doc_a = {
        let mut lib = named_model_library(store.clone(), "model-a").expect("build a");
        lib.ingest(Source::RawMarkdown(DOC.into()))
            .expect("ingest a")
    };
    let embeddings_before = store.get_all_embeddings().expect("emb");
    let fingerprint_before = store
        .get_document(doc_a)
        .expect("doc")
        .meta
        .processing_fingerprint;

    // Model B (same dimension) over the same store: construction itself must
    // fail — before any ingest or query is possible.
    let err = named_model_library(store.clone(), "model-b")
        .err()
        .expect("construction under a different model must fail");
    assert!(matches!(err, Error::EmbeddingSpaceMismatch(_)), "got {err}");

    // Document A and its embeddings are untouched; nothing new was written.
    assert_eq!(store.list_documents().expect("list").len(), 1);
    assert_eq!(
        store
            .get_document(doc_a)
            .expect("doc")
            .meta
            .processing_fingerprint,
        fingerprint_before
    );
    let embeddings_after = store.get_all_embeddings().expect("emb");
    assert_eq!(embeddings_before.len(), embeddings_after.len());
}

/// Vector index that panics on `search` — proves no query is dispatched.
struct PanicOnSearchIndex {
    inner: UsearchIndex,
}

impl nucklavee::vector::VectorIndex for PanicOnSearchIndex {
    fn embedding_space(&self) -> &nucklavee::embedder::EmbeddingSpace {
        self.inner.embedding_space()
    }
    fn add(&mut self, id: nucklavee::chunking::ChunkId, vector: Vec<f32>) -> nucklavee::Result<()> {
        self.inner.add(id, vector)
    }
    fn remove(&mut self, id: nucklavee::chunking::ChunkId) -> nucklavee::Result<()> {
        self.inner.remove(id)
    }
    fn search(
        &self,
        _query: &[f32],
        _limit: usize,
    ) -> nucklavee::Result<Vec<(nucklavee::chunking::ChunkId, f32)>> {
        panic!("no query may reach the vector index under a mismatched model");
    }
    fn clear(&mut self) -> nucklavee::Result<()> {
        self.inner.clear()
    }
    fn save(&self, path: &std::path::Path) -> nucklavee::Result<()> {
        self.inner.save(path)
    }
    fn load(&mut self, path: &std::path::Path) -> nucklavee::Result<()> {
        self.inner.load(path)
    }
}

#[test]
fn query_after_model_switch_fails_before_reaching_usearch() {
    let store = InMemoryDocumentStore::default();

    // Persist a model-A library.
    {
        let mut lib = named_model_library(store.clone(), "model-a").expect("build a");
        lib.ingest(Source::RawMarkdown(DOC.into()))
            .expect("ingest a");
    }

    // "Restart" configured for model B at the same dimension. Construction
    // must fail; the panic-on-search index proves usearch never sees a
    // model-B query vector against model-A data.
    let embedder_b = NamedModelEmbedder::new("model-b");
    let index = PanicOnSearchIndex {
        inner: UsearchIndex::for_embedder(&embedder_b).expect("index"),
    };
    let err = Library::new(store, index, embedder_b)
        .err()
        .expect("model-B library over model-A store must not construct");
    assert!(matches!(err, Error::EmbeddingSpaceMismatch(_)), "got {err}");
}

#[test]
fn rebuild_under_the_wrong_model_is_refused_and_mutates_nothing() {
    use nucklavee::storage::DocumentStore;

    let store = InMemoryDocumentStore::default();

    // The construction gate is the normal defense for the CLI rebuild path,
    // so reaching `rebuild_index` with a mismatch requires the store to be
    // bound AFTER construction: build the model-B library over the still-
    // unbound store, then let a model-A writer bind it.
    let mut lib_b =
        named_model_library(store.clone(), "model-b").expect("unbound store constructs");
    {
        let mut lib_a = named_model_library(store.clone(), "model-a").expect("build a");
        lib_a
            .ingest(Source::RawMarkdown(DOC.into()))
            .expect("ingest a");
    }
    let embeddings_before = store.get_all_embeddings().expect("emb").len();

    let err = lib_b
        .rebuild_index()
        .expect_err("rebuild must not launder model-A vectors into a model-B index");
    assert!(matches!(err, Error::EmbeddingSpaceMismatch(_)), "got {err}");
    assert_eq!(
        store.get_all_embeddings().expect("emb").len(),
        embeddings_before,
        "stored embeddings remain untouched"
    );

    // And the ordinary restart route is refused at construction.
    let err = named_model_library(store.clone(), "model-b")
        .err()
        .expect("model-B library over a model-A store must not construct");
    assert!(matches!(err, Error::EmbeddingSpaceMismatch(_)), "got {err}");
}

#[test]
fn same_model_restart_loads_and_queries() {
    let store = InMemoryDocumentStore::default();
    {
        let mut lib = named_model_library(store.clone(), "model-a").expect("build");
        lib.ingest(Source::RawMarkdown(DOC.into())).expect("ingest");
    }

    // Restart with an equivalent model-A configuration: construction
    // succeeds, rebuild repopulates the fresh index, and query works.
    let mut lib = named_model_library(store, "model-a").expect("same-model restart");
    lib.rebuild_index().expect("rebuild");
    let hits = lib
        .query("ocean salt water tides moon", 3)
        .expect("query after same-model restart");
    assert!(
        hits[0].content.to_lowercase().contains("ocean"),
        "restarted library ranks correctly:\n{}",
        hits[0].content
    );
}

#[test]
fn same_bytes_ingested_as_markdown_and_html_are_not_conflated() {
    use nucklavee::storage::DocumentStore;

    let store = InMemoryDocumentStore::default();
    let mut lib = Library::new(store.clone(), hash_index(), HashEmbedder::new(DIM)).expect("build");

    // Bytes that are valid in both formats but parse differently.
    let bytes = "# Heading\n\n<p>body paragraph</p>\n";
    let as_md = lib
        .ingest(Source::RawMarkdown(bytes.into()))
        .expect("ingest as markdown");
    let fp_md = store
        .get_document(as_md)
        .expect("doc")
        .meta
        .processing_fingerprint;

    // Identical bytes → same content hash, but the HTML interpretation must
    // not reuse the markdown interpretation: the format is part of the
    // fingerprint, so this reprocesses.
    let as_html = lib
        .ingest(Source::RawHtml(bytes.into()))
        .expect("ingest as html");
    assert_eq!(as_md, as_html, "same bytes keep one stable document id");
    let doc = store.get_document(as_html).expect("doc");
    assert_ne!(
        doc.meta.processing_fingerprint, fp_md,
        "format change must change the fingerprint"
    );
    assert!(
        matches!(doc.meta.format, nucklavee::SourceFormat::Html),
        "stored interpretation reflects the latest ingest"
    );
}

/// Embedder that returns structurally invalid vectors — the library must
/// reject them before anything reaches the authoritative store.
struct MalformedEmbedder {
    dimension: usize,
    poison: fn(usize) -> Vec<f32>,
}

impl Embedder for MalformedEmbedder {
    fn embed(&self, texts: &[&str]) -> nucklavee::Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|_| (self.poison)(self.dimension))
            .collect())
    }
    fn dimension(&self) -> usize {
        self.dimension
    }
    fn fingerprint(&self) -> String {
        format!("malformed|dim={}", self.dimension)
    }
}

#[test]
fn invalid_embedder_output_never_reaches_the_store() {
    use nucklavee::storage::DocumentStore;

    type Poison = fn(usize) -> Vec<f32>;
    let poisons: [(&str, Poison); 3] = [
        ("NaN values", |d| vec![f32::NAN; d]),
        ("infinite values", |d| vec![f32::INFINITY; d]),
        ("wrong dimension", |d| vec![1.0; d + 3]),
    ];

    for (label, poison) in poisons {
        let store = InMemoryDocumentStore::default();
        let embedder = MalformedEmbedder {
            dimension: DIM,
            poison,
        };
        let index = UsearchIndex::for_embedder(&embedder).expect("index");
        let mut lib = Library::new(store.clone(), index, embedder).expect("build");

        let err = lib
            .ingest(Source::RawMarkdown(DOC.into()))
            .expect_err("malformed vectors must be rejected");
        assert!(
            matches!(err, Error::Embedding(_)),
            "{label}: expected an embedding error, got {err}"
        );
        assert!(
            store.list_documents().expect("list").is_empty(),
            "{label}: nothing may be stored after rejected embeddings"
        );
        assert!(
            store.get_all_embeddings().expect("emb").is_empty(),
            "{label}: no embeddings may be persisted"
        );
    }
}

/// Embedder that behaves normally while ingesting (multi-text batches) but
/// returns poisoned output for single-text (query) calls.
struct QueryPoisonEmbedder {
    inner: HashEmbedder,
    poison: fn(&HashEmbedder) -> Vec<Vec<f32>>,
}

impl Embedder for QueryPoisonEmbedder {
    fn embed(&self, texts: &[&str]) -> nucklavee::Result<Vec<Vec<f32>>> {
        if texts.len() == 1 {
            return Ok((self.poison)(&self.inner));
        }
        self.inner.embed(texts)
    }
    fn dimension(&self) -> usize {
        self.inner.dimension()
    }
    fn fingerprint(&self) -> String {
        self.inner.fingerprint()
    }
}

#[test]
fn malformed_query_embeddings_never_reach_the_index() {
    type Poison = fn(&HashEmbedder) -> Vec<Vec<f32>>;
    let poisons: [(&str, Poison); 6] = [
        ("zero vectors", |_| Vec::new()),
        ("two vectors for one query", |e| {
            vec![vec![1.0; e.dimension()], vec![1.0; e.dimension()]]
        }),
        ("wrong dimension", |e| vec![vec![1.0; e.dimension() + 1]]),
        ("NaN", |e| {
            let mut v = vec![1.0; e.dimension()];
            v[0] = f32::NAN;
            vec![v]
        }),
        ("positive infinity", |e| {
            let mut v = vec![1.0; e.dimension()];
            v[0] = f32::INFINITY;
            vec![v]
        }),
        ("negative infinity", |e| {
            let mut v = vec![1.0; e.dimension()];
            v[0] = f32::NEG_INFINITY;
            vec![v]
        }),
    ];

    for (label, poison) in poisons {
        let embedder = QueryPoisonEmbedder {
            inner: HashEmbedder::new(DIM),
            poison,
        };
        let index = UsearchIndex::for_embedder(&embedder).expect("index");
        let mut lib =
            Library::new(InMemoryDocumentStore::default(), index, embedder).expect("build");
        lib.ingest(Source::RawMarkdown(DOC.into())).expect("ingest");

        let err = lib
            .query("apples", 3)
            .expect_err("malformed query vectors must be rejected before usearch");
        assert!(
            matches!(err, Error::Embedding(_)),
            "{label}: expected an embedding error, got {err}"
        );
    }
}
