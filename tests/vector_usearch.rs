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
    assert!(
        results[0].1 < 1e-3,
        "self-distance should be near zero: {}",
        results[0].1
    );
}

#[test]
fn dimension_mismatch_is_an_error() {
    let mut index = UsearchIndex::new(4).expect("new index");
    let err = index
        .add(Uuid::new_v4(), vec![1.0, 0.0])
        .expect_err("wrong-dim add must fail");
    assert!(matches!(err, nucklavee::Error::VectorIndex(_)), "got {err}");

    let err = index
        .search(&[1.0, 0.0], 1)
        .expect_err("wrong-dim query must fail");
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

// --- generation-manifest persistence ----------------------------------------
// A saved index is a manifest at the given path plus two generation-stamped
// artifacts; the manifest replacement is the single atomic commit point.

/// The generation artifacts (`<name>.g<gen>.*`) present next to a manifest.
fn generation_files(path: &std::path::Path) -> Vec<String> {
    let parent = path.parent().expect("parent");
    let prefix = format!(
        "{}.g",
        path.file_name().and_then(|n| n.to_str()).expect("name")
    );
    let mut files: Vec<String> = std::fs::read_dir(parent)
        .expect("read dir")
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|n| n.starts_with(&prefix))
        .collect();
    files.sort();
    files
}

#[test]
fn repeated_saves_stay_loadable_and_retire_old_generations() {
    let path = temp_index_path();
    let a = Uuid::new_v4();
    let mut index = UsearchIndex::new(4).expect("new");
    index.add(a, one_hot(0)).expect("add");

    for round in 0..3 {
        index
            .add(Uuid::new_v4(), one_hot(round % 4))
            .expect("add more");
        index.save(&path).expect("repeated save must succeed");
    }

    // Exactly one generation (two artifacts) remains after the last commit.
    assert_eq!(
        generation_files(&path).len(),
        2,
        "stale generations are cleaned up: {:?}",
        generation_files(&path)
    );

    let mut reloaded = UsearchIndex::new(4).expect("new");
    reloaded.load(&path).expect("load after repeated saves");
    assert_eq!(reloaded.len(), 4);

    cleanup(&path);
}

#[test]
fn corrupt_manifest_is_a_clear_error_pointing_at_rebuild() {
    let path = temp_index_path();
    std::fs::write(&path, b"\x00\x01 not json \xff").expect("write garbage");

    let mut index = UsearchIndex::new(4).expect("new");
    let err = index
        .load(&path)
        .expect_err("corrupt manifest must fail to load");
    let message = err.to_string();
    assert!(
        message.contains("rebuild-index"),
        "load failure should direct the operator to rebuild: {message}"
    );
    cleanup(&path);
}

#[test]
fn missing_generation_artifacts_fail_to_load() {
    let path = temp_index_path();
    {
        let mut index = UsearchIndex::new(4).expect("new");
        index.add(Uuid::new_v4(), one_hot(0)).expect("add");
        index.save(&path).expect("save");
    }
    // Delete the generation artifacts, keeping the manifest.
    let parent = path.parent().expect("parent").to_path_buf();
    for name in generation_files(&path) {
        std::fs::remove_file(parent.join(name)).expect("remove artifact");
    }

    let mut index = UsearchIndex::new(4).expect("new");
    assert!(
        index.load(&path).is_err(),
        "a manifest whose generation artifacts are gone must not load"
    );
    cleanup(&path);
}

#[test]
fn mixed_generation_artifacts_are_rejected() {
    let path = temp_index_path();
    {
        let mut index = UsearchIndex::new(4).expect("new");
        index.add(Uuid::new_v4(), one_hot(0)).expect("add");
        index.save(&path).expect("save");
    }

    // Tamper: rewrite the keymap's generation so it no longer matches the
    // manifest — as if the two artifacts came from different saves.
    let parent = path.parent().expect("parent").to_path_buf();
    let keymap_name = generation_files(&path)
        .into_iter()
        .find(|n| n.ends_with(".keymap.json"))
        .expect("keymap file");
    let keymap_path = parent.join(&keymap_name);
    let mut keymap: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&keymap_path).expect("read")).expect("parse");
    keymap["generation"] = serde_json::Value::String("someothergeneration".into());
    std::fs::write(&keymap_path, keymap.to_string()).expect("write tampered");

    let mut index = UsearchIndex::new(4).expect("new");
    let err = index
        .load(&path)
        .expect_err("mixed generations must be rejected");
    assert!(
        err.to_string().contains("generation"),
        "error should identify the generation mismatch: {err}"
    );
    cleanup(&path);
}

fn temp_index_path() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nucklavee_usearch_{}_{}",
        std::process::id(),
        Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).expect("mkdir");
    dir.join("index.usearch")
}

fn cleanup(path: &std::path::Path) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::remove_dir_all(dir);
    }
}
