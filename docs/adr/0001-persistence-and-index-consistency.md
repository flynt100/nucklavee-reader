# ADR 0001 — Persistence and index consistency

- **Status:** Accepted (2026-07-13); amended (2026-07-13 stabilization gate,
  see [Amendments](#amendments-2026-07-13-stabilization-gate))
- **Context:** External review of the Phase 1–5 build (2026-07 independent
  audit) identified reliability gaps around partial-ingest states, dedupe
  correctness, and the relationship between the SQLite store and the usearch
  vector index. A follow-up review of the remediation added four blocking
  findings, addressed in the amendments below.

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

## Amendments (2026-07-13 stabilization gate)

A second review pass found four blocking gaps in the remediation above.
Each amendment strengthens — never replaces — the original decision.

### A1. Fingerprints identify the whole interpretation chain

§3's fingerprint originally covered ingest options + embedding dimension,
which conflated distinct vector spaces (two models with equal dimension) and
distinct interpretations (identical bytes parsed as Markdown vs HTML). The
fingerprint (`fp2`) now covers: source format, per-parser policy version
(`PARSER_POLICY_VERSION`), normalization options, chunker + tokenizer
version (`CHUNKER_VERSION`), token budget, and the embedder's vector-space
identity via the new `Embedder::fingerprint()` (endpoint + model +
dimension for `ApiEmbedder`; never the API key). Version constants must be
bumped whenever behavior changes how an already-ingested document would be
interpreted — that is what turns heuristic improvements (PDF especially)
into reprocessing instead of stale-dedupe bugs.

### A2. No invalid embedding can enter the authoritative store

§4 validated only the `ApiEmbedder`'s own provider responses; an arbitrary
`Embedder` implementation could persist NaN or wrong-dimension vectors that
the index would later reject during rebuild. Validation now runs at **two
boundaries**: the library rejects vectors that don't match
`embedder.dimension()` or contain non-finite values before any write, and
`validate_projection` independently rejects non-finite or mixed-dimension
embeddings inside the store. Invariant: *every embedding accepted by the
authoritative store is valid for rebuilding the configured index.*

### A3. The recovery command must not depend on what it recovers

The CLI loaded the existing index before dispatching any command, so a
corrupt index made `rebuild-index` itself unreachable. `rebuild-index` now
always starts from an empty index and repopulates from the store; every
other command that fails to load the index reports an error pointing at
`nucklavee rebuild-index`.

### A4. Index persistence is generation-consistent

The original save renamed the index file and the keymap sidecar
*separately*, so an interruption between the two renames could leave
artifacts from different saves active together. A saved index is now a
small **manifest** at the configured path (version, generation id,
dimension, entry count) plus two generation-stamped artifacts
(`<path>.g<gen>.usearch`, `<path>.g<gen>.keymap.json`). Save writes the new
generation's artifacts first and then atomically replaces the one manifest
file — the single commit point on every platform. Load verifies generation,
dimension, and entry count across all three files and refuses
mixed-generation combinations. Stale generations are retired best-effort
after the commit.

### Ordering corrections (non-blocking findings)

Reprocessing and `remove_document` previously touched the index before the
store. Both now commit the authoritative store first and then reconcile the
derived index, converting index failures into `Error::Consistency` that
directs at `rebuild_index()`. A failed retire of a replaced usearch vector
is an error rather than silently ignored (orphaned HNSW entries degrade
recall invisibly).

## Consequences

- Backup = copy the SQLite file. The index manifest and its generation
  artifacts are disposable.
- Any future backend must pass the projection conformance suite
  (`tests/store_conformance.rs`); reliability behavior is regression-tested
  end-to-end in `tests/pipeline.rs` (failed-ingest atomicity, fingerprint
  reprocessing — including model-change and format-change cases,
  malformed-embedder rejection, rebuild-without-re-embedding) and
  `tests/cli.rs` (recovery from corrupt/missing index files).
- Schema is versioned via `PRAGMA user_version` with stepwise migrations
  (currently v2: adds `chunk_embeddings` and the unique
  `(document_id, sequence_index)` index).
- Index files from before the manifest layout do not load; the recovery is
  the designed one — `nucklavee rebuild-index`.
