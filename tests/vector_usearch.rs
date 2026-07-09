//! usearch VectorIndex tests (audit Task 8): add/search/remove with synthetic
//! vectors and save/load persistence including the UUID⇆u64 keymap sidecar.

use std::path::PathBuf;

use nucklavee::vector::VectorIndex;
use nucklavee::vector::usearch::UsearchIndex;
use uuid::Uuid;

/// A 4-dimensional one-hot vector for a given axis.
fn one_hot(axis: usize) -> Vec<f32> {
    let mut v = vec![0.0f32; 4];
    v[axis] = 1.0;
    v
}

#[test]
fn add_and_search_returns_nearest_first() {
    let mut index = UsearchIndex::new(4).expect("new index");
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let c = Uuid::new_v4();
    index.add(a, one_hot(0)).expect("add a");
    index.add(b, one_hot(1)).expect("add b");
    index.add(c, one_hot(2)).expect("add c");
    assert_eq!(index.len(), 3);

    // Query closest to axis 1.
    let results = index.search(&one_hot(1), 3).expect("search");
    assert!(!results.is_empty());
    assert_eq!(results[0].0, b, "nearest match should be the axis-1 vector");
    // Cosine distance to itself is ~0.
    assert!(results[0].1 < 1e-3, "self-distance should be near zero: {}", results[0].1);
}

#[test]
fn dimension_mismatch_is_an_error() {
    let mut index = UsearchIndex::new(4).expect("new index");
    let err = index
        .add(Uuid::new_v4(), vec![1.0, 0.0])
        .expect_err("wrong-dim add must fail");
    assert!(matches!(err, nucklavee::Error::VectorIndex(_)), "got {err}");

    let err = index.search(&[1.0, 0.0], 1).expect_err("wrong-dim query must fail");
    assert!(matches!(err, nucklavee::Error::VectorIndex(_)), "got {err}");
}

#[test]
fn remove_excludes_from_results_and_is_idempotent() {
    let mut index = UsearchIndex::new(4).expect("new index");
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    index.add(a, one_hot(0)).expect("add a");
    index.add(b, one_hot(1)).expect("add b");

    index.remove(a).expect("remove a");
    assert_eq!(index.len(), 1);
    let results = index.search(&one_hot(0), 5).expect("search");
    assert!(
        results.iter().all(|(id, _)| *id != a),
        "removed id must not appear in results"
    );

    // Removing again and removing an unknown id are both no-ops.
    index.remove(a).expect("remove a twice");
    index.remove(Uuid::new_v4()).expect("remove unknown");
}

#[test]
fn readding_same_id_replaces_vector() {
    let mut index = UsearchIndex::new(4).expect("new index");
    let a = Uuid::new_v4();
    index.add(a, one_hot(0)).expect("add a");
    index.add(a, one_hot(3)).expect("re-add a");
    assert_eq!(index.len(), 1, "re-add must not duplicate the id");

    let results = index.search(&one_hot(3), 1).expect("search");
    assert_eq!(results[0].0, a);
    assert!(results[0].1 < 1e-3, "should now match axis-3");
}

#[test]
fn save_and_load_round_trips_vectors_and_keymap() {
    let path = temp_index_path();
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();

    {
        let mut index = UsearchIndex::new(4).expect("new");
        index.add(a, one_hot(0)).expect("add a");
        index.add(b, one_hot(1)).expect("add b");
        index.save(&path).expect("save");
    }

    let mut reloaded = UsearchIndex::new(4).expect("new for load");
    reloaded.load(&path).expect("load");
    assert_eq!(reloaded.len(), 2, "keymap restored");

    // The restored index resolves back to the original ChunkIds.
    let results = reloaded.search(&one_hot(1), 2).expect("search");
    assert_eq!(results[0].0, b, "u64 key mapped back to original UUID");

    // Adding after load uses fresh keys without colliding.
    let c = Uuid::new_v4();
    reloaded.add(c, one_hot(2)).expect("add after load");
    assert_eq!(reloaded.len(), 3);

    cleanup(&path);
}

fn temp_index_path() -> PathBuf {
    let unique = format!("nucklavee_usearch_{}_{}.idx", std::process::id(), Uuid::new_v4());
    std::env::temp_dir().join(unique)
}

fn cleanup(path: &PathBuf) {
    let _ = std::fs::remove_file(path);
    let mut sidecar = path.as_os_str().to_os_string();
    sidecar.push(".keymap.json");
    let _ = std::fs::remove_file(PathBuf::from(sidecar));
}
