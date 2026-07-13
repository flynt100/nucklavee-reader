# Changelog

Notable changes to nucklavee, newest first. Phases refer to the roadmap in
`TODO.md`; task numbers refer to `docs/audits/2026-07-06-full-scope.md`.

## 2026-07-13 — Reliability pass (external-review remediation)

Consistency model adopted: **SQLite authoritative, usearch derived** —
rationale and deferred alternatives in
`docs/adr/0001-persistence-and-index-consistency.md`.

- `DocumentStore` rework: transactional `replace_document_projection`
  replaces `upsert_document`/`insert_chunks`; projections validated
  (ownership, contiguous unique sequence indexes, embedding counts); new
  `Error::Consistency` variant; schema v2 adds durable `chunk_embeddings`
  and a unique `(document_id, sequence_index)` index.
- Ingest is derive-first: parse/chunk/embed before any write — a failed
  ingest stores nothing, retry is always safe.
- Option-aware dedupe: `DocumentMeta.processing_fingerprint` alongside
  `content_hash`; changed ingest options reprocess in place under a stable
  `DocumentId`.
- `Library::rebuild_index()` + CLI `rebuild-index`: rebuild the vector index
  from stored embeddings without re-embedding.
- `VectorIndex` gains `clear`; `UsearchIndex::add` validates dimension and
  finiteness before mutating; `save` is atomic (temp file + rename).
- `ApiEmbedder` validates the provider response as a complete permutation
  (index bounds/duplicates, dimension, finite values); remote error bodies
  bounded to 512 chars in diagnostics; HTTP client mechanics shared with URL
  fetching via `net::build_blocking_client`.
- URL format detection layered (media type → URL extension → HTML body
  signal → markdown body signal → fallback) with `DetectionBasis` evidence;
  markdown sniffing requires strong or multiple weak signals.
- `context_window`: oversized chunks skipped (not a stop), duplicate hits
  packed once, storage errors propagate as `Error::Consistency`.
- Tests: projection conformance suite, fault-injection pipeline tests
  (flaky/forbidden embedders, broken store), embedder permutation-validation
  suite. `test-support` cargo feature exposes deterministic doubles.

## 2026-07-09 — Phases 4–5 complete (Tasks 5–10)

- Plain-text emitter (spec §5.3); structure-aware `StructuralChunker` with
  tiktoken budgets; SQLite `DocumentStore`; `UsearchIndex` (cosine HNSW,
  UUID⇆u64 keymap sidecar); OpenAI-compatible `ApiEmbedder`; end-to-end
  `Library` pipeline with `query` and `context_window`; normalize-at-ingest
  (canonical stored IR); full spec §8 CLI (`ingest`/`search`/`emit`/`list`/
  `info`/`context`/`remove`) over TOML config; repo prune (dead scaffolds
  removed, `phase2_contract` → `contract`).

## 2026-07-06 — Phase 3 complete (Tasks 0–4)

- Full-scope audit + per-task build plan; trait surfaces frozen; HTML parser
  with readability-style content extraction; HTML emitter; bidirectional
  markdown↔html structural-equivalence gates; URL ingestion with format
  detection; provenance byte ranges remapped to original-source coordinates.

## Earlier — Phases 1–2

- IR with block-level provenance and typed diagnostics; markdown parser
  (pulldown-cmark) with input-repair heuristics and offset remapping;
  markdown emitter with roundtrip fixtures; validation and normalization
  invariants; phase gates and normalization-delta policy docs.
