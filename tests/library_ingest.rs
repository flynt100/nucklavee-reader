//! Library-level API behavior with no-op backends: content-hash dedupe
//! (spec §7.4), emission of all supported formats, and empty search/context.

use nucklavee::ir::Source;
use nucklavee::storage::DocumentStore;
use nucklavee::storage::memory::InMemoryDocumentStore;
use nucklavee::test_support::{NoopEmbedder, NoopVectorIndex};
use nucklavee::{Format, Library};

fn library() -> Library<InMemoryDocumentStore, NoopVectorIndex, NoopEmbedder> {
    Library::new(
        InMemoryDocumentStore::default(),
        NoopVectorIndex,
        NoopEmbedder,
    )
    .expect("build library")
}

#[test]
fn reingesting_identical_content_returns_existing_document_id() {
    let mut lib = Library::new(
        InMemoryDocumentStore::default(),
        NoopVectorIndex,
        NoopEmbedder,
    )
    .expect("build library");

    let first = lib
        .ingest(Source::RawMarkdown("# Title\n\nbody\n".into()))
        .expect("first ingest");
    let second = lib
        .ingest(Source::RawMarkdown("# Title\n\nbody\n".into()))
        .expect("second ingest");

    assert_eq!(first, second, "identical content must dedupe to one ID");
    assert_eq!(
        lib.store().list_documents().expect("list").len(),
        1,
        "store must hold a single document after duplicate ingest"
    );
}

#[test]
fn differing_content_gets_distinct_document_ids() {
    let mut lib = Library::new(
        InMemoryDocumentStore::default(),
        NoopVectorIndex,
        NoopEmbedder,
    )
    .expect("build library");

    let a = lib
        .ingest(Source::RawMarkdown("# A\n".into()))
        .expect("ingest a");
    let b = lib
        .ingest(Source::RawMarkdown("# B\n".into()))
        .expect("ingest b");

    assert_ne!(a, b);
    assert_eq!(lib.store().list_documents().expect("list").len(), 2);
}

#[test]
fn emits_all_three_supported_formats() {
    let mut lib = library();
    let id = lib
        .ingest(Source::RawMarkdown("# title\n\nbody\n".into()))
        .expect("ingest");

    assert!(lib.emit(id, Format::Markdown).expect("md").contains("# title"));
    assert!(
        lib.emit(id, Format::Html)
            .expect("html")
            .contains("<h1>title</h1>")
    );
    assert!(lib.emit(id, Format::PlainText).expect("text").contains("TITLE"));
}

#[test]
fn query_and_context_window_are_empty_without_data() {
    let lib = library();
    // Implemented in Phase 4: with a no-op index and nothing ingested, both
    // return empty rather than erroring.
    assert!(lib.query("anything", 3).expect("query").is_empty());
    assert!(
        lib.context_window("anything", 1000)
            .expect("context")
            .is_empty()
    );
}
