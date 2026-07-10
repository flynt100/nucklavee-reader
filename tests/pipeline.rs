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
