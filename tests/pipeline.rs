//! End-to-end pipeline test (audit Task 9): ingest → chunk → embed → index,
//! then `query` and `context_window`. Uses a real usearch index and the
//! deterministic offline `HashEmbedder` so ranking is meaningful without a
//! network or model.

use nucklavee::ir::Source;
use nucklavee::storage::memory::InMemoryDocumentStore;
use nucklavee::test_support::HashEmbedder;
use nucklavee::vector::usearch::UsearchIndex;
use nucklavee::Library;

const DIM: usize = 256;

fn library() -> Library<InMemoryDocumentStore, UsearchIndex, HashEmbedder> {
    Library::new(
        InMemoryDocumentStore::default(),
        UsearchIndex::new(DIM).expect("usearch index"),
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

    let results = lib.query("crunchy sweet red fruit orchard cider", 3).expect("query");
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

    assert!(ctx.contains("[Source: Field Guide > Oceans]"), "missing provenance header:\n{ctx}");
    assert!(ctx.contains("salt water"), "missing retrieved content:\n{ctx}");
    // The most relevant section is packed first.
    let oceans_at = ctx.find("Oceans").expect("oceans present");
    let apples_at = ctx.find("Apples");
    if let Some(apples_at) = apples_at {
        assert!(oceans_at < apples_at, "oceans should be packed before apples:\n{ctx}");
    }
}

#[test]
fn context_window_respects_token_budget() {
    let mut lib = library();
    lib.ingest(Source::RawMarkdown(DOC.into())).expect("ingest");

    let big = lib.context_window("fruit water rust", 1000).expect("big ctx");
    let tiny = lib.context_window("fruit water rust", 20).expect("tiny ctx");

    assert!(tiny.len() < big.len(), "a tiny budget must produce less output");
    // A budget of zero yields nothing.
    assert!(lib.context_window("fruit", 0).expect("zero budget").is_empty());
}

#[test]
fn dedupe_does_not_double_index() {
    let mut lib = library();
    let a = lib.ingest(Source::RawMarkdown(DOC.into())).expect("ingest 1");
    let b = lib.ingest(Source::RawMarkdown(DOC.into())).expect("ingest 2 (dupe)");
    assert_eq!(a, b, "identical content dedupes to one document");

    // Still exactly one document's worth of chunks retrievable.
    let results = lib.query("apples crunchy red", 10).expect("query");
    let doc_ids: std::collections::HashSet<_> = results.iter().map(|c| c.document_id).collect();
    assert_eq!(doc_ids.len(), 1, "results come from a single deduped document");
}

// --- reliability regression scenarios (external review, 2026-07-13) ---------

use std::sync::Arc;
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
}

#[test]
fn failed_embedding_leaves_no_partial_document_and_retry_succeeds() {
    let store = InMemoryDocumentStore::default();
    let mut lib = Library::new(
        store.clone(),
        UsearchIndex::new(DIM).expect("index"),
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
        !store
            .get_chunks_by_document(id)
            .expect("chunks")
            .is_empty(),
        "retry stores the complete projection"
    );
    let hits = lib.query("crunchy sweet red fruit orchard", 3).expect("query");
    assert!(!hits.is_empty(), "retried document is searchable");
}

#[test]
fn same_content_different_options_reprocesses_with_stable_identity() {
    let store = InMemoryDocumentStore::default();
    let mut lib = Library::new(
        store.clone(),
        UsearchIndex::new(DIM).expect("index"),
        HashEmbedder::new(DIM),
    )
    .expect("build library");

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
    let hits = lib.query("crunchy sweet red fruit orchard", 5).expect("query");
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
}

#[test]
fn index_rebuilds_from_durable_embeddings_without_reembedding() {
    let store = InMemoryDocumentStore::default();

    // Ingest normally.
    {
        let mut lib = Library::new(
            store.clone(),
            UsearchIndex::new(DIM).expect("index"),
            HashEmbedder::new(DIM),
        )
        .expect("build library");
        lib.ingest(Source::RawMarkdown(DOC.into())).expect("ingest");
    }

    // Fresh empty index (simulates a missing/corrupt index file) + an
    // embedder that panics if consulted: rebuild must restore search purely
    // from the store's durable embeddings.
    let mut lib = Library::new(
        store.clone(),
        UsearchIndex::new(DIM).expect("fresh index"),
        ForbiddenEmbedder,
    )
    .expect("build library");

    let restored = lib.rebuild_index().expect("rebuild");
    assert!(restored > 0, "rebuild indexed the stored embeddings");

    // Query embeds via HashEmbedder externally to avoid the forbidden one.
    // Instead, verify through a HashEmbedder-backed library sharing state.
    drop(lib);
    let mut rebuilt = Library::new(
        store.clone(),
        UsearchIndex::new(DIM).expect("index"),
        HashEmbedder::new(DIM),
    )
    .expect("build");
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
    let mut lib = Library::new(
        store,
        UsearchIndex::new(DIM).expect("index"),
        HashEmbedder::new(DIM),
    )
    .expect("build library");

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
    let ctx = lib
        .context_window(ocean_words, 60)
        .expect("context window");
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
    fn replace_document_projection(
        &self,
        d: &nucklavee::Document,
        c: &[nucklavee::Chunk],
        e: &[Vec<f32>],
    ) -> nucklavee::Result<()> {
        self.inner.replace_document_projection(d, c, e)
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
    let mut lib = Library::new(
        broken,
        UsearchIndex::new(DIM).expect("index"),
        HashEmbedder::new(DIM),
    )
    .expect("build library");
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

    let ctx = lib.context_window("kelp forest paragraph", 300).expect("ctx");
    assert!(
        ctx.contains("[Source: untitled]"),
        "missing title is ordinary data and falls back to 'untitled':\n{ctx}"
    );
}

// Arc is used by FlakyEmbedder's AtomicUsize import group.
#[allow(dead_code)]
fn _keep_arc_import(_x: Arc<()>) {}
