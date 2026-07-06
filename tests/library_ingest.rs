//! Library-level ingest behavior: content-hash deduplication (spec §7.4).

use nucklavee::ir::Source;
use nucklavee::storage::DocumentStore;
use nucklavee::storage::memory::InMemoryDocumentStore;
use nucklavee::test_support::{NoopEmbedder, NoopVectorIndex};
use nucklavee::Library;

#[test]
fn reingesting_identical_content_returns_existing_document_id() {
    let mut lib = Library::new(
        InMemoryDocumentStore::default(),
        NoopVectorIndex,
        NoopEmbedder,
    );

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
    );

    let a = lib
        .ingest(Source::RawMarkdown("# A\n".into()))
        .expect("ingest a");
    let b = lib
        .ingest(Source::RawMarkdown("# B\n".into()))
        .expect("ingest b");

    assert_ne!(a, b);
    assert_eq!(lib.store().list_documents().expect("list").len(), 2);
}
