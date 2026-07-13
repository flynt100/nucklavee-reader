//! `usearch` HNSW-backed [`VectorIndex`] (spec §7.2).
//!
//! **Key-width bridge.** usearch addresses vectors by `u64` keys, but chunk
//! IDs are 128-bit UUIDs. This wraps the index with a bidirectional
//! `u64 ⇆ ChunkId` map, assigning sequential `u64` keys on insert. The map is
//! persisted alongside the index (the raw usearch file only knows `u64`
//! keys).
//!
//! **Generation-manifest persistence.** A saved index is two artifacts (the
//! usearch binary and the keymap JSON) that must never mix across saves. Each
//! `save` writes both under a fresh generation id
//! (`<path>.g<generation>.usearch` / `<path>.g<generation>.keymap.json`),
//! then atomically replaces the small manifest at `<path>` that names the
//! active generation. A crash between writes leaves the previous manifest —
//! and therefore the previous complete, matched generation — in effect.
//! `load` verifies generation, dimension, and entry count across all three
//! files before installing anything.
//!
//! `search` returns `(ChunkId, distance)` sorted by ascending distance
//! (closest first) using the cosine metric — smaller is more similar.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use usearch::{Index, IndexOptions, MetricKind, ScalarKind};

use crate::chunking::ChunkId;
use crate::vector::VectorIndex;
use crate::{Error, Result};

/// Manifest format version (the file at the caller-supplied index path).
const MANIFEST_VERSION: u32 = 1;

pub struct UsearchIndex {
    index: Index,
    dimension: usize,
    next_key: u64,
    key_to_id: HashMap<u64, ChunkId>,
    id_to_key: HashMap<ChunkId, u64>,
    capacity: usize,
}

/// The small file at the caller-supplied index path: names the active
/// generation. Replaced atomically (write-temp + rename of ONE file), which
/// is what makes the two-artifact save safe on every platform.
#[derive(Serialize, Deserialize)]
struct Manifest {
    version: u32,
    generation: String,
    dimension: usize,
    entry_count: usize,
}

#[derive(Serialize, Deserialize)]
struct KeyMapSidecar {
    /// Must match the manifest's generation — a mismatch means the artifacts
    /// come from different saves and must not be combined.
    generation: String,
    dimension: usize,
    next_key: u64,
    /// `(u64 key, chunk id)` pairs.
    entries: Vec<(u64, ChunkId)>,
}

impl UsearchIndex {
    /// Create an empty cosine-metric index for vectors of `dimension`.
    pub fn new(dimension: usize) -> Result<Self> {
        let index = build_index(dimension)?;
        Ok(Self {
            index,
            dimension,
            next_key: 0,
            key_to_id: HashMap::new(),
            id_to_key: HashMap::new(),
            capacity: 0,
        })
    }

    pub fn dimension(&self) -> usize {
        self.dimension
    }

    pub fn len(&self) -> usize {
        self.id_to_key.len()
    }

    pub fn is_empty(&self) -> bool {
        self.id_to_key.is_empty()
    }

    fn ensure_capacity(&mut self, additional: usize) -> Result<()> {
        let needed = self.index.size() + additional;
        if needed <= self.capacity {
            return Ok(());
        }
        let new_capacity = needed.max(self.capacity.max(16) * 2);
        self.index
            .reserve(new_capacity)
            .map_err(|e| Error::VectorIndex(format!("usearch reserve failed: {e}")))?;
        self.capacity = new_capacity;
        Ok(())
    }

    /// Paths of a generation's two artifacts: `(usearch binary, keymap json)`.
    fn generation_paths(manifest_path: &Path, generation: &str) -> (PathBuf, PathBuf) {
        let mut index = manifest_path.as_os_str().to_os_string();
        index.push(format!(".g{generation}.usearch"));
        let mut keymap = manifest_path.as_os_str().to_os_string();
        keymap.push(format!(".g{generation}.keymap.json"));
        (PathBuf::from(index), PathBuf::from(keymap))
    }

    /// Best-effort removal of generation artifacts other than `keep` —
    /// obsolete generations waste disk but are otherwise harmless, so
    /// failures here are ignored.
    fn cleanup_stale_generations(manifest_path: &Path, keep: &str) {
        let Some(parent) = manifest_path.parent() else {
            return;
        };
        let Some(file_name) = manifest_path.file_name().and_then(|n| n.to_str()) else {
            return;
        };
        let prefix = format!("{file_name}.g");
        let keep_prefix = format!("{file_name}.g{keep}");
        let Ok(entries) = std::fs::read_dir(parent) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name.starts_with(&prefix) && !name.starts_with(&keep_prefix) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

fn build_index(dimension: usize) -> Result<Index> {
    let options = IndexOptions {
        dimensions: dimension,
        metric: MetricKind::Cos,
        quantization: ScalarKind::F32,
        // Zero = use the usearch library defaults.
        connectivity: 0,
        expansion_add: 0,
        expansion_search: 0,
        multi: false,
    };
    Index::new(&options)
        .map_err(|e| Error::VectorIndex(format!("usearch index create failed: {e}")))
}

fn path_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| Error::VectorIndex(format!("non-UTF-8 index path: {}", path.display())))
}

impl VectorIndex for UsearchIndex {
    fn add(&mut self, id: ChunkId, vector: Vec<f32>) -> Result<()> {
        // Reject every predictable failure *before* mutating valid state.
        if vector.len() != self.dimension {
            return Err(Error::VectorIndex(format!(
                "vector dimension {} does not match index dimension {}",
                vector.len(),
                self.dimension
            )));
        }
        if let Some(bad) = vector.iter().find(|v| !v.is_finite()) {
            return Err(Error::VectorIndex(format!(
                "vector for chunk {id} contains a non-finite value ({bad})"
            )));
        }
        self.ensure_capacity(1)?;

        // Replacement order: commit the new vector first, then retire the old
        // one — a failed add leaves the previous vector searchable.
        let key = self.next_key;
        self.index
            .add(key, &vector)
            .map_err(|e| Error::VectorIndex(format!("usearch add failed: {e}")))?;
        self.next_key += 1;

        if let Some(old_key) = self.id_to_key.get(&id).copied() {
            // A failed removal must surface: the maps still resolve `id` to
            // the old vector (a consistent view), but the new vector is now
            // an unreachable entry degrading the index — the caller should
            // rebuild rather than silently accumulate orphans.
            self.index.remove(old_key).map_err(|e| {
                Error::VectorIndex(format!(
                    "usearch failed to retire the replaced vector for chunk {id}: {e}"
                ))
            })?;
            self.id_to_key.remove(&id);
            self.key_to_id.remove(&old_key);
        }
        self.key_to_id.insert(key, id);
        self.id_to_key.insert(id, key);
        Ok(())
    }

    fn remove(&mut self, id: ChunkId) -> Result<()> {
        if let Some(key) = self.id_to_key.remove(&id) {
            self.key_to_id.remove(&key);
            self.index
                .remove(key)
                .map_err(|e| Error::VectorIndex(format!("usearch remove failed: {e}")))?;
        }
        Ok(())
    }

    fn search(&self, query: &[f32], limit: usize) -> Result<Vec<(ChunkId, f32)>> {
        if query.len() != self.dimension {
            return Err(Error::VectorIndex(format!(
                "query dimension {} does not match index dimension {}",
                query.len(),
                self.dimension
            )));
        }
        if limit == 0 || self.is_empty() {
            return Ok(Vec::new());
        }

        let matches = self
            .index
            .search(query, limit)
            .map_err(|e| Error::VectorIndex(format!("usearch search failed: {e}")))?;

        let mut out = Vec::with_capacity(matches.keys.len());
        for (key, distance) in matches.keys.iter().zip(matches.distances.iter()) {
            if let Some(id) = self.key_to_id.get(key) {
                out.push((*id, *distance));
            }
        }
        Ok(out)
    }

    fn clear(&mut self) -> Result<()> {
        self.index = build_index(self.dimension)?;
        self.key_to_id.clear();
        self.id_to_key.clear();
        self.next_key = 0;
        self.capacity = 0;
        Ok(())
    }

    fn save(&self, path: &Path) -> Result<()> {
        // Write the new generation's two artifacts under generation-stamped
        // names, then atomically replace the single manifest file. Until the
        // manifest rename commits, the previous generation stays active and
        // complete — no interruption can leave mixed-generation artifacts.
        let generation = uuid::Uuid::new_v4().simple().to_string();
        let (index_path, keymap_path) = Self::generation_paths(path, &generation);

        self.index
            .save(path_str(&index_path)?)
            .map_err(|e| Error::VectorIndex(format!("usearch save failed: {e}")))?;

        let sidecar = KeyMapSidecar {
            generation: generation.clone(),
            dimension: self.dimension,
            next_key: self.next_key,
            entries: self.key_to_id.iter().map(|(k, id)| (*k, *id)).collect(),
        };
        let json = serde_json::to_string(&sidecar)
            .map_err(|e| Error::VectorIndex(format!("keymap serialize failed: {e}")))?;
        std::fs::write(&keymap_path, json)
            .map_err(|e| Error::VectorIndex(format!("keymap write failed: {e}")))?;

        let manifest = Manifest {
            version: MANIFEST_VERSION,
            generation: generation.clone(),
            dimension: self.dimension,
            entry_count: self.id_to_key.len(),
        };
        let manifest_json = serde_json::to_string(&manifest)
            .map_err(|e| Error::VectorIndex(format!("manifest serialize failed: {e}")))?;
        let mut manifest_tmp = path.as_os_str().to_os_string();
        manifest_tmp.push(".tmp");
        let manifest_tmp = PathBuf::from(manifest_tmp);
        std::fs::write(&manifest_tmp, manifest_json)
            .map_err(|e| Error::VectorIndex(format!("manifest write failed: {e}")))?;
        std::fs::rename(&manifest_tmp, path)
            .map_err(|e| Error::VectorIndex(format!("manifest rename failed: {e}")))?;

        // The new generation is committed; retiring older ones is best-effort.
        Self::cleanup_stale_generations(path, &generation);
        Ok(())
    }

    fn load(&mut self, path: &Path) -> Result<()> {
        // Read as bytes: corruption is often not valid UTF-8, and it must
        // land in the "rebuild" diagnostic below, not a read error.
        let manifest_raw = std::fs::read(path)
            .map_err(|e| Error::VectorIndex(format!("index manifest read failed: {e}")))?;
        let manifest: Manifest = serde_json::from_slice(&manifest_raw).map_err(|e| {
            Error::VectorIndex(format!(
                "index manifest at '{}' is not readable ({e}); the file is \
                 corrupt or from an unsupported layout — rebuild the index \
                 from the store (CLI: `nucklavee rebuild-index`)",
                path.display()
            ))
        })?;
        if manifest.version != MANIFEST_VERSION {
            return Err(Error::VectorIndex(format!(
                "index manifest version {} is not supported (expected {MANIFEST_VERSION})",
                manifest.version
            )));
        }

        let (index_path, keymap_path) = Self::generation_paths(path, &manifest.generation);
        let sidecar_raw = std::fs::read_to_string(&keymap_path)
            .map_err(|e| Error::VectorIndex(format!("keymap read failed: {e}")))?;
        let sidecar: KeyMapSidecar = serde_json::from_str(&sidecar_raw)
            .map_err(|e| Error::VectorIndex(format!("keymap parse failed: {e}")))?;

        // All three artifacts must describe the same generation and shape.
        if sidecar.generation != manifest.generation {
            return Err(Error::VectorIndex(format!(
                "keymap generation '{}' does not match manifest generation '{}' \
                 — mixed-generation index artifacts must not be combined",
                sidecar.generation, manifest.generation
            )));
        }
        if sidecar.dimension != manifest.dimension {
            return Err(Error::VectorIndex(format!(
                "keymap dimension {} does not match manifest dimension {}",
                sidecar.dimension, manifest.dimension
            )));
        }
        if sidecar.entries.len() != manifest.entry_count {
            return Err(Error::VectorIndex(format!(
                "keymap has {} entries but the manifest records {}",
                sidecar.entries.len(),
                manifest.entry_count
            )));
        }

        // Rebuild the index at the persisted dimension, then load vectors.
        let index = build_index(sidecar.dimension)?;
        index
            .load(path_str(&index_path)?)
            .map_err(|e| Error::VectorIndex(format!("usearch load failed: {e}")))?;
        if index.size() != manifest.entry_count {
            return Err(Error::VectorIndex(format!(
                "loaded index holds {} vectors but the manifest records {}",
                index.size(),
                manifest.entry_count
            )));
        }

        self.key_to_id = sidecar.entries.iter().copied().collect();
        self.id_to_key = sidecar.entries.iter().map(|(k, id)| (*id, *k)).collect();
        self.next_key = sidecar.next_key;
        self.dimension = sidecar.dimension;
        self.capacity = index.size();
        self.index = index;
        Ok(())
    }
}
