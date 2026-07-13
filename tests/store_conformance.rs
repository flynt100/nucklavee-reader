//! Shared `DocumentStore` conformance suite, run against both the in-memory
//! and SQLite backends. Any behavioral divergence between the two stores
//! fails here — behavioral expectations live in this one suite, not in
//! backend-specific tests.

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

fn embedding_for(seq: usize) -> Vec<f32> {
    vec![seq as f32, 1.0, 0.0]
}

/// Store a document with `n` chunks + embeddings via the projection API.
fn store_projection<S: DocumentStore>(store: &S, markdown: &str, n: usize) -> nucklavee::Document {
    let d = doc(markdown);
    let chunks: Vec<Chunk> = (0..n).map(|i| chunk_for(d.meta.id, i)).collect();
    let embeddings: Vec<Vec<f32>> = (0..n).map(embedding_for).collect();
    store
        .replace_document_projection(&d, &chunks, &embeddings)
        .expect("replace projection");
    d
}

/// Exercise the entire trait surface. Runs against any backend.
fn conformance<S: DocumentStore>(store: S) {
    // projection write + get
    let d1 = store_projection(&store, "# Title One\n\nbody one\n", 3);
    let d2 = store_projection(&store, "# Title Two\n\nbody two\n", 0);

    let got = store.get_document(d1.meta.id).expect("get d1");
    assert_eq!(got.meta.id, d1.meta.id);
    assert_eq!(got.meta.title.as_deref(), Some("Title One"));
    assert_eq!(got.body.len(), d1.body.len(), "IR body must round-trip");

    // get unknown → error
    assert!(store.get_document(Uuid::new_v4()).is_err());

    // find_by_content_hash
    assert_eq!(
        store
            .find_by_content_hash(&d1.meta.content_hash)
            .expect("hash"),
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

    // chunks ordered by sequence_index
    let by_doc = store.get_chunks_by_document(d1.meta.id).expect("by doc");
    let seqs: Vec<usize> = by_doc.iter().map(|c| c.sequence_index).collect();
    assert_eq!(seqs, vec![0, 1, 2], "chunks ordered by sequence_index");

    // get_chunks_by_ids preserves request order and skips unknown
    let fetched = store
        .get_chunks_by_ids(&[by_doc[2].id, Uuid::new_v4(), by_doc[0].id])
        .expect("by ids");
    let fetched_ids: Vec<Uuid> = fetched.iter().map(|c| c.id).collect();
    assert_eq!(fetched_ids, vec![by_doc[2].id, by_doc[0].id]);

    // embeddings are durable and retrievable
    let stored_embeddings = store.get_all_embeddings().expect("embeddings");
    assert_eq!(stored_embeddings.len(), 3, "one embedding per chunk");
    let for_c1 = stored_embeddings
        .iter()
        .find(|(id, _)| *id == by_doc[1].id)
        .expect("embedding for chunk 1");
    assert_eq!(for_c1.1, embedding_for(1));

    // replacement retires the previous generation entirely
    let new_chunks: Vec<Chunk> = (0..2).map(|i| chunk_for(d1.meta.id, i)).collect();
    let new_embeddings: Vec<Vec<f32>> = (0..2).map(embedding_for).collect();
    store
        .replace_document_projection(&d1, &new_chunks, &new_embeddings)
        .expect("re-replace");
    let after = store.get_chunks_by_document(d1.meta.id).expect("after");
    assert_eq!(after.len(), 2, "old generation fully retired");
    assert!(
        after.iter().all(|c| new_chunks.iter().any(|n| n.id == c.id)),
        "only the new generation remains"
    );
    assert_eq!(
        store.get_all_embeddings().expect("embeddings").len(),
        2,
        "old embeddings retired with their chunks"
    );

    // replaying the identical projection is idempotent
    store
        .replace_document_projection(&d1, &new_chunks, &new_embeddings)
        .expect("idempotent replay");
    assert_eq!(store.get_chunks_by_document(d1.meta.id).expect("x").len(), 2);
    assert_eq!(store.list_documents().expect("list").len(), 2);

    // empty replacement removes the previous chunk set
    store
        .replace_document_projection(&d1, &[], &[])
        .expect("empty replacement");
    assert!(
        store
            .get_chunks_by_document(d1.meta.id)
            .expect("empty")
            .is_empty()
    );

    // validation: chunk belonging to another document is rejected
    let foreign = chunk_for(Uuid::new_v4(), 0);
    let err = store
        .replace_document_projection(&d1, &[foreign], &[embedding_for(0)])
        .expect_err("foreign chunk must be rejected");
    assert!(matches!(err, nucklavee::Error::Consistency(_)), "got {err}");

    // validation: duplicate sequence indexes are rejected
    let mut dup_a = chunk_for(d1.meta.id, 0);
    let dup_b = chunk_for(d1.meta.id, 0);
    dup_a.sequence_index = 0;
    let err = store
        .replace_document_projection(
            &d1,
            &[dup_a, dup_b],
            &[embedding_for(0), embedding_for(1)],
        )
        .expect_err("duplicate sequence must be rejected");
    assert!(matches!(err, nucklavee::Error::Consistency(_)), "got {err}");

    // validation: embeddings must match chunk count
    let lone = chunk_for(d1.meta.id, 0);
    let err = store
        .replace_document_projection(&d1, &[lone], &[])
        .expect_err("missing embeddings must be rejected");
    assert!(matches!(err, nucklavee::Error::Consistency(_)), "got {err}");

    // remove_document drops the doc, chunks, and embeddings, idempotently
    let d3 = store_projection(&store, "# Three\n\nbody three\n", 2);
    store.remove_document(d3.meta.id).expect("remove");
    assert!(store.get_document(d3.meta.id).is_err());
    assert!(
        store
            .get_chunks_by_document(d3.meta.id)
            .expect("chunks after remove")
            .is_empty()
    );
    assert!(
        store
            .get_all_embeddings()
            .expect("embeddings after remove")
            .iter()
            .all(|(id, _)| !d3.body.is_empty() || *id != d3.meta.id),
        "removed document's embeddings are gone"
    );
    store.remove_document(d3.meta.id).expect("remove twice");
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

/// Persistence: a projection written by one store instance is retrievable —
/// including embeddings — by a second instance opened at the same path.
#[test]
fn sqlite_persists_across_reopen() {
    let path = temp_db_path("persist");

    let id = {
        let store = SqliteDocumentStore::open(&path).expect("open 1");
        let d = store_projection(&store, "# Persisted\n\nsurvives a reopen\n", 1);
        d.meta.id
    };

    {
        let store = SqliteDocumentStore::open(&path).expect("reopen");
        let got = store.get_document(id).expect("get after reopen");
        assert_eq!(got.meta.title.as_deref(), Some("Persisted"));
        assert_eq!(store.get_chunks_by_document(id).expect("chunks").len(), 1);
        assert_eq!(
            store.get_all_embeddings().expect("embeddings").len(),
            1,
            "embeddings persist across reopen"
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
