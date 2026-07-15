# Changelog

Notable changes to nucklavee, newest first. Phases refer to the roadmap in
`TODO.md`; task numbers refer to `docs/audits/2026-07-06-full-scope.md`.

## Unreleased — Open-source MVP hardening

- Added the MIT license, package metadata, contributor/security policies, and
  an experimental-v0.1 release posture.
- Bounded URL responses to 10 MiB and blocked non-public destinations by
  default, with redirect revalidation and explicit trusted-local opt-in.
  The opt-in is library-only; CLI URL ingestion remains deny-by-default.
- Added environment-based embedding secrets and retained plaintext TOML only
  as a compatibility fallback.
- Moved the preferred config path to `~/.config/nucklavee/config.toml` with a
  warning-backed legacy Forge fallback.
- Strengthened CI with locked dependencies, read-only permissions, release
  builds, package validation, RustSec advisory scanning, and an explicit
  dependency-license policy.

## 2026-07-14 — Collection-wide embedding-space identity (final pre-PDF gate)

Enforced collection-wide embedding-space identity across SQLite, the
configured embedder, and usearch manifests. Details in ADR 0001's
2026-07-14 amendments.

- **Breaking (trait):** `EmbeddingSpace` is a first-class type;
  `Embedder::embedding_space()` (default from `fingerprint()`/
  `dimension()`), `VectorIndex::embedding_space()`, and
  `DocumentStore::{embedding_space, has_embeddings}` added;
  `replace_document_projection` takes the projection's space;
  `UsearchIndex::new` takes an `EmbeddingSpace` (or use `for_embedder`).
  `ApiEmbedder`'s fingerprint is now versioned (`api-space-v1|…`).
- **Breaking (files):** SQLite schema v3 adds the `library_metadata`
  singleton; index manifest v2 records the space fingerprint in the
  manifest and keymap sidecar (v1 manifests no longer load — rebuild).
- The first stored projection binds a library to one vector space,
  transactionally; later projections, `Library::new`, index load, and
  `rebuild-index` all require an exact match. Same-dimension model changes
  are rejected before any search or mutation with an
  `Error::EmbeddingSpaceMismatch` explaining that rebuild cannot convert
  models. Legacy (pre-v3) stores with unidentified embeddings fail closed.
- Query embeddings validated like ingest embeddings (count, dimension,
  finiteness) via one shared helper; `UsearchIndex::search` additionally
  rejects non-finite query values.
- Keymap loads reject duplicate keys, duplicate chunk IDs, stale
  `next_key`, entry-count disagreement, and manifest/keymap space
  mismatches — corrupt keymaps can no longer silently collapse mappings.

## 2026-07-13 — Stabilization gate (second external review)

Pre-PDF hardening; details in ADR 0001's amendments.

- **Breaking (trait):** `Embedder` gains `fingerprint()` — the stable
  identity of the vector space it produces (never includes API keys).
- **Breaking (files):** vector-index persistence moved to a
  generation-manifest layout (manifest at the configured path + two
  generation-stamped artifacts); saves commit atomically via one manifest
  rename, loads verify generation/dimension/entry-count across all files.
  Pre-manifest index files no longer load — run `nucklavee rebuild-index`.
- Processing fingerprints (`fp2`) now cover source format, parser policy
  version, chunker+tokenizer version, token budget, and embedder identity:
  changing embedding models at the same dimension, or re-ingesting the same
  bytes under a different format, reprocesses instead of reusing
  incompatible derived data.
- Embeddings are validated (dimension, finiteness) at both the library and
  storage boundaries — no arbitrary `Embedder` can persist vectors that
  cannot rebuild the index.
- CLI `rebuild-index` no longer loads the existing index, so it works when
  the index is corrupt or missing; index load failures in other commands
  point at it.
- Reprocessing and removal commit the store before touching the index;
  index-maintenance failures surface as consistency errors directing at
  rebuild. A failed retire of a replaced usearch vector is an error.
- CI added: fmt, clippy (`-D warnings`), and tests on Linux and Windows;
  repository formatted with `cargo fmt`.

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
