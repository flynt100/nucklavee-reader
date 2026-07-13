# ADR 0001 — Persistence and index consistency

- **Status:** Accepted (2026-07-13)
- **Context:** External review of the Phase 1–5 build (2026-07 independent
  audit) identified reliability gaps around partial-ingest states, dedupe
  correctness, and the relationship between the SQLite store and the usearch
  vector index.

## Decision

**SQLite is the single authoritative store. The usearch index is a derived,
rebuildable projection of it.** Every consistency mechanism in the crate
follows from that one sentence.

### 1. Write ordering makes "stored ⇒ fully processed" an invariant

`Library::ingest_with_options` performs **all fallible derivation before any
persistent mutation**:

```
parse → validate → normalize → chunk → embed   (no writes yet)
→ store.replace_document_projection(doc, chunks, embeddings)   (one transaction)
→ index.add(...) per vector                     (derived data only)
```

- If parsing, chunking, or embedding fails, **nothing** was written: no
  phantom document can win a later content-hash dedupe check while lacking
  chunks or vectors. Retrying a failed ingest is always safe.
- The store write is a single transactional projection replacement
  (`replace_document_projection`): document row, chunk set, and chunk
  embeddings change together or not at all. Validation
  (`storage::validate_projection`) rejects chunks belonging to another
  document, non-contiguous or duplicate `sequence_index` values, and
  embedding/chunk count mismatches as `Error::Consistency`.
- Index insertion happens **after** the store commit. A failure there leaves
  the authoritative store correct and surfaces as `Error::Consistency` with
  an explicit instruction to run `rebuild_index()`.

### 2. Embeddings are durable, so the index is genuinely rebuildable

Chunk embeddings are persisted in SQLite (`chunk_embeddings`, cascading with
their chunks). `Library::rebuild_index()` — CLI `rebuild-index` — clears the
vector index and repopulates it from `store.get_all_embeddings()`, **without
re-embedding** (no network, no cost). Index corruption or loss is therefore
an inconvenience, never data loss.

### 3. Dedupe is option-aware; reprocessing keeps document identity

`content_hash` (SHA-256 of the raw input) answers "have I seen these bytes?"
`processing_fingerprint` (hash of the ingest options + embedder dimension)
answers "were they processed the same way?" On a hash hit with a matching
fingerprint, ingest returns the existing ID untouched. With a *different*
fingerprint, the document is reprocessed **in place**: the new parse is
retagged with the existing `DocumentId` (including all provenance
document-ids), old vectors are removed from the index, and the projection is
replaced atomically. Search results always come from the current generation
only.

### 4. Untrusted boundaries are validated, and diagnostics are bounded

The embedding provider's response is validated as a complete permutation of
the inputs (index bounds, duplicates, per-vector dimension, finite values) —
count equality alone does not prove input↔vector correspondence. Remote
error bodies quoted into diagnostics are truncated
(`net::truncate_for_diagnostics`, 512 chars). `UsearchIndex::add` validates
dimension and finiteness *before* mutating, and `save` writes index + keymap
sidecar via temp-file-then-rename.

## Deliberately deferred (and why)

The external review sketched a heavier architecture. These pieces were
**considered and not built**, because the invariants above deliver the same
guarantees at MVP scale without the moving parts:

- **Transactional outbox / pending-operations table.** Unnecessary once
  writes are ordered derive-first: there is no partially-persisted state to
  reconcile, so there is nothing for an outbox to replay.
- **Durable per-document processing-state machine** (`Pending → Chunked →
  Embedded → Indexed`). Same reason — the only observable states are "absent"
  and "fully processed". Revisit if ingestion ever becomes multi-step across
  process restarts (e.g. large PDF batches).
- **`resume_ingestion()` crash recovery.** Subsumed by `rebuild_index()`: the
  store is either consistent or the ingest never happened; only the derived
  index can lag, and rebuild covers that.
- **`ExistingContentPolicy` enum (Skip/Reprocess/Fail).** The fingerprint
  comparison *is* the policy: identical processing skips, changed processing
  reprocesses in place. An explicit policy knob can be added to
  `IngestOptions` later without breaking the trait surface.
- **Stable u64 vector keys stored in SQLite.** The UUID⇆u64 keymap sidecar
  stays with the index file it describes; since the index is rebuildable,
  the keymap needs no independent durability.
- **Structured `ContextWindow` return type.** `context_window` still returns
  a formatted `String`; a structured type is a straightforward additive
  change when a consumer needs machine-readable citations.

## Consequences

- Backup = copy the SQLite file. The `.usearch`/`.keymap.json` pair is
  disposable.
- Any future backend must pass the projection conformance suite
  (`tests/store_conformance.rs`); reliability behavior is regression-tested
  end-to-end in `tests/pipeline.rs` (failed-ingest atomicity, fingerprint
  reprocessing, rebuild-without-re-embedding).
- Schema is versioned via `PRAGMA user_version` with stepwise migrations
  (currently v2: adds `chunk_embeddings` and the unique
  `(document_id, sequence_index)` index).
