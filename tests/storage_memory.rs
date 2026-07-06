//! Unit tests for the frozen `DocumentStore` surface on the in-memory store.

use nucklavee::chunking::{Chunk, ChunkBlockType};
use nucklavee::parsers::markdown::{ParseOptions, parse_markdown};
use nucklavee::storage::DocumentStore;
use nucklavee::storage::memory::InMemoryDocumentStore;
use uuid::Uuid;

fn stored_doc(store: &InMemoryDocumentStore, markdown: &str) -> nucklavee::Document {
    let doc = parse_markdown(markdown, ParseOptions::default());
    store.upsert_document(&doc).expect("upsert");
    doc
}

fn chunk_for(doc_id: Uuid, seq: usize) -> Chunk {
    Chunk {
        id: Uuid::new_v4(),
        document_id: doc_id,
        section_path: vec!["A".to_string()],
        content: format!("chunk {seq}"),
        block_type: ChunkBlockType::Prose,
        sequence_index: seq,
        token_count: 2,
    }
}

#[test]
fn find_by_content_hash_returns_stored_document() {
    let store = InMemoryDocumentStore::default();
    let doc = stored_doc(&store, "# A\n\nbody\n");

    let found = store
        .find_by_content_hash(&doc.meta.content_hash)
        .expect("lookup");
    assert_eq!(found, Some(doc.meta.id));

    let missing = store.find_by_content_hash("no-such-hash").expect("lookup");
    assert_eq!(missing, None);
}

#[test]
fn list_documents_returns_all_metadata() {
    let store = InMemoryDocumentStore::default();
    let a = stored_doc(&store, "# A\n");
    let b = stored_doc(&store, "# B\n");

    let mut listed: Vec<Uuid> = store
        .list_documents()
        .expect("list")
        .into_iter()
        .map(|meta| meta.id)
        .collect();
    listed.sort();
    let mut expected = vec![a.meta.id, b.meta.id];
    expected.sort();
    assert_eq!(listed, expected);
}

#[test]
fn remove_document_removes_document_and_chunks_and_is_idempotent() {
    let store = InMemoryDocumentStore::default();
    let doc = stored_doc(&store, "# A\n\nbody\n");
    store
        .insert_chunks(&[chunk_for(doc.meta.id, 0), chunk_for(doc.meta.id, 1)])
        .expect("insert chunks");

    store.remove_document(doc.meta.id).expect("remove");
    assert!(store.get_document(doc.meta.id).is_err());
    assert!(
        store
            .get_chunks_by_document(doc.meta.id)
            .expect("chunks")
            .is_empty()
    );

    // Removing again must not error.
    store.remove_document(doc.meta.id).expect("remove twice");
}

#[test]
fn get_chunks_by_ids_preserves_order_and_skips_unknown() {
    let store = InMemoryDocumentStore::default();
    let doc = stored_doc(&store, "# A\n\nbody\n");
    let c0 = chunk_for(doc.meta.id, 0);
    let c1 = chunk_for(doc.meta.id, 1);
    store
        .insert_chunks(&[c0.clone(), c1.clone()])
        .expect("insert chunks");

    let fetched = store
        .get_chunks_by_ids(&[c1.id, Uuid::new_v4(), c0.id])
        .expect("fetch");
    let ids: Vec<Uuid> = fetched.iter().map(|c| c.id).collect();
    assert_eq!(ids, vec![c1.id, c0.id], "order preserved, unknown skipped");
}
