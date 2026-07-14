//! Shared `DocumentStore` conformance suite, run against both the in-memory
//! and SQLite backends. Any behavioral divergence between the two stores
//! fails here — behavioral expectations live in this one suite, not in
//! backend-specific tests.

use std::path::PathBuf;

use nucklavee::chunking::{Chunk, ChunkBlockType};
use nucklavee::embedder::EmbeddingSpace;
use nucklavee::parsers::markdown::{ParseOptions, parse_markdown};
use nucklavee::storage::DocumentStore;
use nucklavee::storage::memory::InMemoryDocumentStore;
use nucklavee::storage::sqlite::SqliteDocumentStore;
use uuid::Uuid;

/// The vector space every conformance projection is written in.
fn space() -> EmbeddingSpace {
    EmbeddingSpace {
        fingerprint: "conformance-space".to_string(),
        dimension: 3,
    }
}

/// A different model at the SAME dimension — must still be rejected.
fn other_space() -> EmbeddingSpace {
    EmbeddingSpace {
        fingerprint: "other-space".to_string(),
        dimension: 3,
    }
}

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
        .replace_document_projection(&d, &chunks, &embeddings, &space())
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
        .replace_document_projection(&d1, &new_chunks, &new_embeddings, &space())
        .expect("re-replace");
    let after = store.get_chunks_by_document(d1.meta.id).expect("after");
    assert_eq!(after.len(), 2, "old generation fully retired");
    assert!(
        after
            .iter()
            .all(|c| new_chunks.iter().any(|n| n.id == c.id)),
        "only the new generation remains"
    );
    assert_eq!(
        store.get_all_embeddings().expect("embeddings").len(),
        2,
        "old embeddings retired with their chunks"
    );

    // replaying the identical projection is idempotent
    store
        .replace_document_projection(&d1, &new_chunks, &new_embeddings, &space())
        .expect("idempotent replay");
    assert_eq!(
        store.get_chunks_by_document(d1.meta.id).expect("x").len(),
        2
    );
    assert_eq!(store.list_documents().expect("list").len(), 2);

    // empty replacement removes the previous chunk set
    store
        .replace_document_projection(&d1, &[], &[], &space())
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
        .replace_document_projection(&d1, &[foreign], &[embedding_for(0)], &space())
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
            &space(),
        )
        .expect_err("duplicate sequence must be rejected");
    assert!(matches!(err, nucklavee::Error::Consistency(_)), "got {err}");

    // validation: embeddings must match chunk count
    let lone = chunk_for(d1.meta.id, 0);
    let err = store
        .replace_document_projection(&d1, &[lone], &[], &space())
        .expect_err("missing embeddings must be rejected");
    assert!(matches!(err, nucklavee::Error::Consistency(_)), "got {err}");

    // validation: non-finite embedding values are rejected (a stored
    // embedding must always be able to rebuild the index)
    let poisoned = chunk_for(d1.meta.id, 0);
    let err = store
        .replace_document_projection(&d1, &[poisoned], &[vec![f32::NAN, 1.0, 0.0]], &space())
        .expect_err("non-finite embedding must be rejected");
    assert!(matches!(err, nucklavee::Error::Consistency(_)), "got {err}");

    // validation: embeddings with mixed dimensions are rejected
    let mixed_a = chunk_for(d1.meta.id, 0);
    let mixed_b = chunk_for(d1.meta.id, 1);
    let err = store
        .replace_document_projection(
            &d1,
            &[mixed_a, mixed_b],
            &[vec![1.0, 0.0, 0.0], vec![1.0, 0.0]],
            &space(),
        )
        .expect_err("mixed-dimension embeddings must be rejected");
    assert!(matches!(err, nucklavee::Error::Consistency(_)), "got {err}");

    // remove_document drops the doc, chunks, and embeddings, idempotently
    let d3 = store_projection(&store, "# Three\n\nbody three\n", 2);
    let d3_chunk_ids: Vec<Uuid> = store
        .get_chunks_by_document(d3.meta.id)
        .expect("d3 chunks before remove")
        .iter()
        .map(|c| c.id)
        .collect();
    assert_eq!(d3_chunk_ids.len(), 2, "d3 stored with two chunks");
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
            .all(|(id, _)| !d3_chunk_ids.contains(id)),
        "removed document's chunk embeddings are gone"
    );
    store.remove_document(d3.meta.id).expect("remove twice");
}

/// Embedding-space binding behavior, identical across backends: the first
/// projection binds the store; later projections must match exactly; a
/// rejected mismatch mutates nothing; removal never unbinds.
fn space_binding_conformance<S: DocumentStore>(store: S) {
    assert_eq!(
        store.embedding_space().expect("space"),
        None,
        "a fresh store is unbound"
    );
    assert!(
        !store.has_embeddings().expect("has"),
        "fresh store is empty"
    );

    // First projection binds the store's space.
    let d1 = store_projection(&store, "# Bind\n\nbody\n", 2);
    assert_eq!(store.embedding_space().expect("space"), Some(space()));
    assert!(store.has_embeddings().expect("has"));

    // A matching projection succeeds.
    let d2 = store_projection(&store, "# Match\n\nbody\n", 1);

    // A different fingerprint at the SAME dimension is rejected before any
    // mutation, leaving the previous projection intact.
    let d3 = doc("# Mismatch\n\nbody\n");
    let chunks: Vec<Chunk> = (0..2).map(|i| chunk_for(d3.meta.id, i)).collect();
    let embeddings: Vec<Vec<f32>> = (0..2).map(embedding_for).collect();
    let err = store
        .replace_document_projection(&d3, &chunks, &embeddings, &other_space())
        .expect_err("same-dimension model change must be rejected");
    assert!(
        matches!(err, nucklavee::Error::EmbeddingSpaceMismatch(_)),
        "got {err}"
    );
    assert!(
        store.get_document(d3.meta.id).is_err(),
        "rejected projection must not write the document"
    );
    assert_eq!(
        store.list_documents().expect("list").len(),
        2,
        "existing projections unchanged after a rejected mismatch"
    );
    assert_eq!(
        store.embedding_space().expect("space"),
        Some(space()),
        "binding unchanged after a rejected mismatch"
    );

    // A different dimension is also rejected.
    let wrong_dim = EmbeddingSpace {
        fingerprint: space().fingerprint,
        dimension: 4,
    };
    let d4 = doc("# WrongDim\n\nbody\n");
    let chunks: Vec<Chunk> = vec![chunk_for(d4.meta.id, 0)];
    let err = store
        .replace_document_projection(&d4, &chunks, &[vec![0.0; 4]], &wrong_dim)
        .expect_err("dimension change must be rejected");
    assert!(
        matches!(err, nucklavee::Error::EmbeddingSpaceMismatch(_)),
        "got {err}"
    );

    // A projection whose vectors do not match its own declared space is
    // rejected as inconsistent.
    let d5 = doc("# BadVectors\n\nbody\n");
    let chunks: Vec<Chunk> = vec![chunk_for(d5.meta.id, 0)];
    let err = store
        .replace_document_projection(&d5, &chunks, &[vec![0.0; 4]], &space())
        .expect_err("vector dimension must match the declared space");
    assert!(matches!(err, nucklavee::Error::Consistency(_)), "got {err}");

    // Removing every document does NOT silently unbind the space.
    store.remove_document(d1.meta.id).expect("remove d1");
    store.remove_document(d2.meta.id).expect("remove d2");
    assert!(store.list_documents().expect("list").is_empty());
    assert!(!store.has_embeddings().expect("has"));
    assert_eq!(
        store.embedding_space().expect("space"),
        Some(space()),
        "the binding outlives the last document"
    );
    let err = store
        .replace_document_projection(&d3, &[], &[], &other_space())
        .expect_err("an emptied store must still reject a different space");
    assert!(
        matches!(err, nucklavee::Error::EmbeddingSpaceMismatch(_)),
        "got {err}"
    );
}

#[test]
fn in_memory_store_conformance() {
    conformance(InMemoryDocumentStore::default());
}

#[test]
fn in_memory_space_binding_conformance() {
    space_binding_conformance(InMemoryDocumentStore::default());
}

#[test]
fn sqlite_space_binding_conformance() {
    space_binding_conformance(SqliteDocumentStore::open_in_memory().expect("open sqlite"));
}

/// The binding survives closing and reopening the database file.
#[test]
fn sqlite_space_binding_persists_across_reopen() {
    let path = temp_db_path("space_binding");
    {
        let store = SqliteDocumentStore::open(&path).expect("open 1");
        store_projection(&store, "# Bound\n\nbody\n", 1);
    }
    {
        let store = SqliteDocumentStore::open(&path).expect("reopen");
        assert_eq!(store.embedding_space().expect("space"), Some(space()));
        let d = doc("# Later\n\nbody\n");
        let err = store
            .replace_document_projection(&d, &[], &[], &other_space())
            .expect_err("reopened store still enforces its binding");
        assert!(
            matches!(err, nucklavee::Error::EmbeddingSpaceMismatch(_)),
            "got {err}"
        );
    }
    let _ = std::fs::remove_file(&path);
}

/// Craft a pre-v3 (schema v2) database: full v2 schema, `user_version = 2`,
/// no `library_metadata`, with `with_embeddings` controlling whether a
/// durable embedding row exists.
fn craft_legacy_v2_database(path: &std::path::Path, with_embeddings: bool) {
    let conn = rusqlite::Connection::open(path).expect("open raw sqlite");
    conn.execute_batch(
        r#"
        CREATE TABLE documents (
            id            TEXT PRIMARY KEY,
            source        TEXT NOT NULL,
            source_format TEXT NOT NULL,
            title         TEXT,
            ingested_at   TEXT NOT NULL,
            content_hash  TEXT NOT NULL UNIQUE,
            doc_json      TEXT NOT NULL
        );
        CREATE TABLE chunks (
            id             TEXT PRIMARY KEY,
            document_id    TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
            section_path   TEXT NOT NULL,
            content        TEXT NOT NULL,
            block_type     TEXT NOT NULL,
            sequence_index INTEGER NOT NULL,
            token_count    INTEGER NOT NULL,
            chunk_json     TEXT NOT NULL
        );
        CREATE TABLE chunk_embeddings (
            chunk_id    TEXT PRIMARY KEY REFERENCES chunks(id) ON DELETE CASCADE,
            dimension   INTEGER NOT NULL,
            vector_json TEXT NOT NULL
        );
        CREATE INDEX idx_chunks_document ON chunks(document_id);
        CREATE INDEX idx_chunks_block_type ON chunks(block_type);
        CREATE UNIQUE INDEX idx_chunks_doc_seq ON chunks(document_id, sequence_index);
        PRAGMA user_version = 2;
        "#,
    )
    .expect("create v2 schema");
    if with_embeddings {
        conn.execute_batch(
            r#"
            INSERT INTO documents VALUES
                ('11111111-1111-1111-1111-111111111111', 'legacy.md', 'Markdown',
                 'Legacy', '2026-07-01T00:00:00Z', 'legacyhash', '{}');
            INSERT INTO chunks VALUES
                ('22222222-2222-2222-2222-222222222222',
                 '11111111-1111-1111-1111-111111111111',
                 '[]', 'legacy chunk', 'Prose', 0, 2, '{}');
            INSERT INTO chunk_embeddings VALUES
                ('22222222-2222-2222-2222-222222222222', 3, '[1.0, 0.0, 0.0]');
            "#,
        )
        .expect("insert legacy rows");
    }
}

/// A legacy store WITH embeddings but no space metadata migrates to
/// "unbound with embeddings" and is reported as such — the vectors' model
/// identity is unverifiable, so the caller (Library::new) must fail closed.
#[test]
fn legacy_v2_database_with_embeddings_migrates_to_unbound() {
    let path = temp_db_path("legacy_bound");
    craft_legacy_v2_database(&path, true);

    let store = SqliteDocumentStore::open(&path).expect("open migrates to v3");
    assert_eq!(
        store.embedding_space().expect("space"),
        None,
        "migration must not invent a space for legacy embeddings"
    );
    assert!(
        store.has_embeddings().expect("has"),
        "legacy embeddings are still present"
    );
    let _ = std::fs::remove_file(&path);
}

/// Library construction over a legacy store with unidentified embeddings
/// fails closed: the configured model must never be silently adopted for
/// vectors whose model identity cannot be verified.
#[test]
fn library_over_legacy_embeddings_fails_closed() {
    use nucklavee::test_support::HashEmbedder;
    use nucklavee::vector::usearch::UsearchIndex;

    let path = temp_db_path("legacy_library");
    craft_legacy_v2_database(&path, true);

    let store = SqliteDocumentStore::open(&path).expect("open migrates to v3");
    let embedder = HashEmbedder::new(3);
    let index = UsearchIndex::for_embedder(&embedder).expect("index");
    let err = nucklavee::Library::new(store, index, embedder)
        .err()
        .expect("legacy unidentified embeddings must fail closed");
    assert!(matches!(err, nucklavee::Error::Consistency(_)), "got {err}");
    let message = err.to_string();
    assert!(
        message.contains("legacy embeddings") && message.contains("re-ingest"),
        "diagnostic should explain the migration path: {message}"
    );
    let _ = std::fs::remove_file(&path);
}

/// A legacy store WITHOUT embeddings is safe: it migrates unbound and the
/// first new projection binds it normally.
#[test]
fn legacy_v2_database_without_embeddings_binds_on_first_use() {
    let path = temp_db_path("legacy_empty");
    craft_legacy_v2_database(&path, false);

    let store = SqliteDocumentStore::open(&path).expect("open migrates to v3");
    assert_eq!(store.embedding_space().expect("space"), None);
    assert!(!store.has_embeddings().expect("has"));

    store_projection(&store, "# Fresh\n\nbody\n", 1);
    assert_eq!(store.embedding_space().expect("space"), Some(space()));
    let _ = std::fs::remove_file(&path);
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
