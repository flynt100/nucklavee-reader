//! `usearch` HNSW-backed [`VectorIndex`] (spec §7.2).
//!
//! **Key-width bridge.** usearch addresses vectors by `u64` keys, but chunk
//! IDs are 128-bit UUIDs. This wraps the index with a bidirectional
//! `u64 ⇆ ChunkId` map, assigning sequential `u64` keys on insert. The map is
//! persisted in a JSON sidecar next to the index file so `save`/`load` restore
//! the full mapping (the raw usearch file only knows `u64` keys).
//!
//! `search` returns `(ChunkId, distance)` sorted by ascending distance
//! (closest first) using the cosine metric — smaller is more similar.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use usearch::{Index, IndexOptions, MetricKind, ScalarKind};

use crate::chunking::ChunkId;
use crate::vector::VectorIndex;
use crate::{Error, Result};

pub struct UsearchIndex {
    index: Index,
    dimension: usize,
    next_key: u64,
    key_to_id: HashMap<u64, ChunkId>,
    id_to_key: HashMap<ChunkId, u64>,
    capacity: usize,
}

#[derive(Serialize, Deserialize)]
struct KeyMapSidecar {
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

    fn sidecar_path(index_path: &Path) -> std::path::PathBuf {
        let mut name = index_path.as_os_str().to_os_string();
        name.push(".keymap.json");
        std::path::PathBuf::from(name)
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
        if vector.len() != self.dimension {
            return Err(Error::VectorIndex(format!(
                "vector dimension {} does not match index dimension {}",
                vector.len(),
                self.dimension
            )));
        }

        // Re-adding an existing id replaces its vector.
        if let Some(old_key) = self.id_to_key.remove(&id) {
            self.key_to_id.remove(&old_key);
            let _ = self.index.remove(old_key);
        }

        self.ensure_capacity(1)?;
        let key = self.next_key;
        self.next_key += 1;
        self.index
            .add(key, &vector)
            .map_err(|e| Error::VectorIndex(format!("usearch add failed: {e}")))?;
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

    fn save(&self, path: &Path) -> Result<()> {
        self.index
            .save(path_str(path)?)
            .map_err(|e| Error::VectorIndex(format!("usearch save failed: {e}")))?;

        let sidecar = KeyMapSidecar {
            dimension: self.dimension,
            next_key: self.next_key,
            entries: self.key_to_id.iter().map(|(k, id)| (*k, *id)).collect(),
        };
        let json = serde_json::to_string(&sidecar)
            .map_err(|e| Error::VectorIndex(format!("keymap serialize failed: {e}")))?;
        std::fs::write(Self::sidecar_path(path), json)
            .map_err(|e| Error::VectorIndex(format!("keymap write failed: {e}")))?;
        Ok(())
    }

    fn load(&mut self, path: &Path) -> Result<()> {
        let sidecar_raw = std::fs::read_to_string(Self::sidecar_path(path))
            .map_err(|e| Error::VectorIndex(format!("keymap read failed: {e}")))?;
        let sidecar: KeyMapSidecar = serde_json::from_str(&sidecar_raw)
            .map_err(|e| Error::VectorIndex(format!("keymap parse failed: {e}")))?;

        // Rebuild the index at the persisted dimension, then load vectors.
        let index = build_index(sidecar.dimension)?;
        index
            .load(path_str(path)?)
            .map_err(|e| Error::VectorIndex(format!("usearch load failed: {e}")))?;

        self.key_to_id = sidecar.entries.iter().copied().collect();
        self.id_to_key = sidecar.entries.iter().map(|(k, id)| (*id, *k)).collect();
        self.next_key = sidecar.next_key;
        self.dimension = sidecar.dimension;
        self.capacity = index.size();
        self.index = index;
        Ok(())
    }
}
