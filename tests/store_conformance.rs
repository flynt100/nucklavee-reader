//! Shared `DocumentStore` conformance suite, run against both the in-memory
//! and SQLite backends (audit Task 6). Any behavioral divergence between the
//! two stores fails here.

use std::path::PathBuf;

use nucklavee::chunking::{Chunk, ChunkBlockType};
use nucklavee::parsers::markdown::{ParseOptions, parse_markdown};
use nucklavee::storage::DocumentStore;
use nucklavee::storage::memory::InMemoryDocumentStore;
use nucklavee::storage::sqlite::SqliteDocumentStore;
use uuid::Uuid;

fn doc(markdown: &str) -> nucklavee::Document {
    parse_markdown(markdown, ParseOptions::default())
}

fn chunk_for(doc_id: Uuid, seq: usize) -> Chunk {
    Chunk {
        id: Uuid::new_v4(),
        document_id: doc_id,
        section_path: vec!["A".to_string(), format!("s{seq}")],
        content: format!("chunk {seq}"),
        block_type: ChunkBlockType::Prose,
        sequence_index: seq,
        token_count: seq + 1,
    }
}

/// Exercise the entire trait surface. Runs against any backend.
fn conformance<S: DocumentStore>(store: S) {
    // upsert + get
    let d1 = doc("# Title One\n\nbody one\n");
    let d2 = doc("# Title Two\n\nbody two\n");
    store.upsert_document(&d1).expect("upsert d1");
    store.upsert_document(&d2).expect("upsert d2");

    let got = store.get_document(d1.meta.id).expect("get d1");
    assert_eq!(got.meta.id, d1.meta.id);
    assert_eq!(got.meta.title.as_deref(), Some("Title One"));
    assert_eq!(got.body.len(), d1.body.len(), "IR body must round-trip");

    // get unknown → error
    assert!(store.get_document(Uuid::new_v4()).is_err());

    // find_by_content_hash
    assert_eq!(
        store.find_by_content_hash(&d1.meta.content_hash).expect("hash"),
        Some(d1.meta.id)
    );
    assert_eq!(store.find_by_content_hash("nope").expect("hash"), None);

    // list_documents
    let mut ids: Vec<Uuid> = store
        .list_documents()
        .expect("list")
        .into_iter()
        .map(|m| m.id)
        .collect();
    ids.sort();
    let mut expected = vec![d1.meta.id, d2.meta.id];
    expected.sort();
    assert_eq!(ids, expected);

    // upsert same id again is an update, not a duplicate
    store.upsert_document(&d1).expect("re-upsert d1");
    assert_eq!(store.list_documents().expect("list").len(), 2);

    // chunks
    let c0 = chunk_for(d1.meta.id, 0);
    let c1 = chunk_for(d1.meta.id, 1);
    let c2 = chunk_for(d1.meta.id, 2);
    store
        .insert_chunks(&[c1.clone(), c0.clone(), c2.clone()])
        .expect("insert chunks");

    // get_chunks_by_document is ordered by sequence_index
    let by_doc = store.get_chunks_by_document(d1.meta.id).expect("by doc");
    let seqs: Vec<usize> = by_doc.iter().map(|c| c.sequence_index).collect();
    assert_eq!(seqs, vec![0, 1, 2], "chunks ordered by sequence_index");
    assert_eq!(by_doc[1].section_path, vec!["A".to_string(), "s1".to_string()]);

    // get_chunks_by_ids preserves request order and skips unknown
    let fetched = store
        .get_chunks_by_ids(&[c2.id, Uuid::new_v4(), c0.id])
        .expect("by ids");
    let fetched_ids: Vec<Uuid> = fetched.iter().map(|c| c.id).collect();
    assert_eq!(fetched_ids, vec![c2.id, c0.id]);

    // remove_document drops the doc and its chunks, idempotently
    store.remove_document(d1.meta.id).expect("remove");
    assert!(store.get_document(d1.meta.id).is_err());
    assert!(
        store
            .get_chunks_by_document(d1.meta.id)
            .expect("chunks after remove")
            .is_empty()
    );
    store.remove_document(d1.meta.id).expect("remove twice");
    assert_eq!(store.list_documents().expect("list").len(), 1);
}

#[test]
fn in_memory_store_conformance() {
    conformance(InMemoryDocumentStore::default());
}

#[test]
fn sqlite_store_conformance_in_memory() {
    conformance(SqliteDocumentStore::open_in_memory().expect("open sqlite"));
}

#[test]
fn sqlite_store_conformance_file_backed() {
    let path = temp_db_path("conformance");
    let store = SqliteDocumentStore::open(&path).expect("open file store");
    conformance(store);
    let _ = std::fs::remove_file(&path);
}

/// Persistence: a document ingested by one store instance is retrievable by a
/// second instance opened at the same path (cross-process semantics).
#[test]
fn sqlite_persists_across_reopen() {
    let path = temp_db_path("persist");

    let id = {
        let store = SqliteDocumentStore::open(&path).expect("open 1");
        let d = doc("# Persisted\n\nsurvives a reopen\n");
        store.upsert_document(&d).expect("upsert");
        store
            .insert_chunks(&[chunk_for(d.meta.id, 0)])
            .expect("insert chunk");
        d.meta.id
    };

    {
        let store = SqliteDocumentStore::open(&path).expect("reopen");
        let got = store.get_document(id).expect("get after reopen");
        assert_eq!(got.meta.title.as_deref(), Some("Persisted"));
        assert_eq!(
            store.get_chunks_by_document(id).expect("chunks").len(),
            1,
            "chunks persist across reopen"
        );
    }

    let _ = std::fs::remove_file(&path);
}

fn temp_db_path(tag: &str) -> PathBuf {
    let unique = format!(
        "nucklavee_test_{tag}_{}_{}.sqlite",
        std::process::id(),
        Uuid::new_v4()
    );
    std::env::temp_dir().join(unique)
}
