# Nucklavee Roadmap TODO

This file tracks implementation phases across the current scaffold-to-MVP plan.

## Phase 1 — IR Finalization (Week 1)

Phase 1 delivers a trustworthy vertical slice: IR + Markdown roundtrip +
block-level provenance + explicit diagnostics for lossy/unsupported cases.

## Done (Phase 1)

1. [x] **IR validation invariants complete and tested**
   - Validation rules are implemented and exercised by `tests/validation.rs`.
2. [x] **Normalization + structural equivalence stable**
   - Canonicalization/normalization behavior is deterministic and structural
     equivalence remains stable under expected deltas; coverage anchored by
     `tests/validation.rs` and roundtrip semantics in
     `tests/markdown_roundtrip.rs`.
3. [x] **Markdown parse->emit->parse fixtures passing**
   - All committed Markdown fixture cases pass through the roundtrip harness in
     `tests/markdown_roundtrip.rs`.
4. [x] **Diagnostics behavior defined for unsupported/lossy cases**
   - Expected diagnostics taxonomy and behavior are explicit and covered by
     `tests/diagnostics.rs`.

Phase 1 quality gate is satisfied by tests `validation`/`diagnostics`/`markdown_roundtrip`; Phase 2A denotes post-gate packaging/wiring work.

## Phase 2A — Completed Foundation (Markdown Core Loop)

Primary goal delivered: post-gate packaging/wiring for the first working ingest/emit loop on the canonical format.

1. [x] Implement `MarkdownParser` (`pulldown-cmark`) with tree construction.
2. [x] Implement markdown emitter that preserves semantic structure from IR.
3. [x] Add IR roundtrip test harness (parse -> emit -> parse -> semantic compare).

## Phase 2B — Completed Phase-2 Hardening

Primary goal: tighten behavior contracts and handoff boundaries around the completed Markdown core loop.

1. [x] Define and document acceptable normalization deltas (whitespace/list-marker canonicalization and semantic-regression boundaries) in `docs/normalization-deltas.md`.
2. [x] Expand fixture coverage deltas for difficult Markdown edge patterns.
3. [x] Define/lock CLI integration boundary for parser/emitter + roundtrip workflow.

## Phase 3 — HTML Parser/Emitter + Cross-Format Integrity

Primary goal: robust content extraction from noisy HTML and parity with markdown IR behavior.

1. [x] Implement readability-style content extraction before DOM mapping.
   - Chrome skip-list + semantic `<main>`/`<article>`/`[role=main]` shortcut +
     density descent (`src/parsers/html.rs`); covered by `tests/html_ingest.rs`.
2. [x] Implement DOM-to-IR mapping for core block/inline semantics.
   - Headings/paragraphs/code (with `language-*`)/tables (header promotion,
     ragged-row padding)/lists/blockquotes/hr + full inline set; unknown
     elements degrade to `GenericBlock` with class hints and diagnostics.
3. [x] Implement HTML emitter for faithful structural output from IR.
   - `src/emitters/html.rs` (spec §5.2 mapping, HTML-escaped, Strong/Em/Del
     style mapping matched to the parser's inverse); wired to `Format::Html`
     in `Library::emit` and CLI `--format html`.
4. [x] Add cross-format tests:
   - [x] html -> IR -> markdown (with structural-equivalence reparse gate)
   - [x] markdown -> IR -> html (`tests/cross_format.rs`, structural-equivalence gate)
   - [x] structural equivalence checks where semantically valid
     (`structural_diff_bodies` + realistic fixture pages + rich md fixture)
5. [x] Add URL-source path validation (`Source::Url`) and metadata/title extraction tests.
   - `src/net/` blocking `reqwest` fetch (redirects, content-type→parser
     sniffing with extension/body fallback, typed `Error::Network`); wired
     into `Library::ingest` and CLI `http(s)://` args; hermetic loopback-server
     tests in `tests/url_ingest.rs`. `<title>`/`<h1>`/`og:title` extraction
     covered by `tests/html_ingest.rs`.

## Phase 4 — Persistence, Chunking, Embeddings, Vector Retrieval

Primary goal: make ingestion/search functional end-to-end over stored chunks.

> **PlainText emitter (audit Task 5) is done** ahead of the rest of Phase 4,
> since the chunker (Task 7) needs it for chunk `content`: spec §5.3 rendering
> in `src/emitters/text.rs`, wired to `Format::PlainText` + CLI `--format text`
> (`SUPPORTED_FORMATS` is now `markdown, html, text`). Covered by
> `tests/text_emit.rs` + module golden tests.

1. [x] Implement `DocumentStore` SQLite backend (audit Task 6):
   - [x] document upsert and retrieval (full IR stored as JSON for cross-process emit)
   - [x] chunk insert/retrieval by document (ordered by `sequence_index`)
   - [x] content-hash dedupe behavior integration (`content_hash UNIQUE`)
   - `src/storage/sqlite.rs` (rusqlite bundled, `PRAGMA user_version`
     migration); shared conformance suite runs against memory + sqlite +
     a persistence-across-reopen test (`tests/store_conformance.rs`).
2. [x] Implement structure-aware chunker with provenance (`section_path`, order, block type) (audit Task 7).
   - `src/chunking/structural.rs` (`StructuralChunker`): groups consecutive
     non-heading blocks by `section_path` (reusing parser provenance, never
     re-deriving), tables/code as their own tagged chunks, prose accumulated;
     token budget via tiktoken cl100k_base with block→sentence→token-window
     splitting; tables split by rows re-prepending the header. Canonical
     stored IR is guaranteed by normalize-at-ingest. Tests in
     `tests/chunking.rs` (§11.2 acceptance: section boundaries, budget,
     provenance, block types).
3. [x] Implement embedder backend contract and at least one working provider path (audit Task 8).
   - `ApiEmbedder` (OpenAI-compatible blocking `reqwest`); mock-server tests
     in `tests/embedder_api.rs`.
4. [x] Implement vector index backend integration (HNSW/usearch abstraction target) (audit Task 8).
   - `UsearchIndex` (cosine HNSW) with a UUID⇆u64 keymap + JSON sidecar for
     `save`/`load`; add/search/remove/persistence tests in
     `tests/vector_usearch.rs`.
5. [x] Wire pipeline: ingest -> parse -> validate/canonicalize -> store -> chunk -> embed -> index (audit Task 9).
6. [x] Implement ranked semantic query returning chunks with provenance (audit Task 9).
   - `Library::query` (embed → index search → store join) and
     `context_window` (spec §7.3 greedy packing with `[Source: title >
     section]` headers, header tokens counted, title de-duplicated against the
     H1 in the section path). End-to-end `tests/pipeline.rs` with a real
     usearch index + deterministic `HashEmbedder`. Dead `IngestionPipeline`
     and `ContextAssembler` scaffold structs removed (behavior lives on
     `Library`).

## Phase 5 — CLI Command Wiring and Operator UX (audit Task 10) — DONE

Primary goal: expose library capabilities via stable CLI workflows.

1. [x] Replace scaffold CLI with the spec §8 command set: `ingest`, `search`,
   `emit`, `list`, `info`, `context`, `remove`.
2. [x] Source/input handling: file (`.md`/`.html`/`.htm`) and `http(s)://` URL,
   auto-detected.
3. [x] Output format controls (`emit <id> <format>`) + `--json` machine output.
4. [x] Config file (`--config`, default `~/.config/nucklavee/config.toml`;
   legacy `forge` path kept as a warned fallback) for the
   SQLite database, vector index, and embedding endpoint; actionable errors.
5. [x] Smoke tests for every subcommand against a temp SQLite db + a loopback
   mock embedding server (`tests/cli.rs`).

The Phase-2 CLI boundary was retired: `phase2_contract` renamed to `contract`
(dead constants pruned), `cli_smoke_contract.rs`/`phase2_contract.rs` tests
removed, `docs/cli-phase2-boundary.md` replaced by `docs/cli.md`.

## Phase 6 — PDF Pipeline (Iterative Heuristics)

Primary goal: practical PDF ingestion with explicit confidence and graceful fallback.

1. [ ] Build staged PDF extraction pipeline (text extraction -> layout inference -> IR mapping).
2. [ ] Emit typed blocks only when confidence is high; otherwise use `GenericBlock` with hints/confidence.
3. [ ] Implement heading/list/table heuristics with conservative thresholds.
4. [ ] Add page/position provenance hooks where feasible for citation UX.
5. [ ] Add regression corpus for real-world PDFs (datasheets, docs, mixed-layout pages).
6. [ ] Document known limitations and confidence semantics for downstream consumers.

## Cross-Phase Quality Gates

1. [ ] Unit tests for each module boundary (parser/emitter/chunker/store/index/embedder).
2. [ ] Golden fixtures for parser/emitter stability.
3. [ ] Property/invariant tests for IR validation and canonicalization.
4. [ ] Error taxonomy consistency (typed errors, no opaque `String` leaks in stable APIs).
5. [ ] Performance baselines for ingest/query on representative corpora.
6. [ ] Documentation updates per phase (README + examples + migration notes).

## Status Snapshot

This section is the single status source for the roadmap. (`NEXT_BUILD_PUSH.md`
was retired in the 2026-07-06 cleanup; per-push task specs now live in
`docs/audits/2026-07-06-full-scope.md`.)

- **Current truth:** Markdown + HTML **parsers and emitters** complete, with
  bidirectional cross-format equivalence gates (html↔markdown via
  `structural_diff_bodies`); shared `SectionPathTracker` across parsers;
  trait surfaces for Phase 4/5 frozen; content-hash ingest dedupe live;
  provenance byte ranges in original-source coordinates.
- **Phase 2B state:** complete (gate command green).
- **Phase 3 state:** **complete** (audit Tasks 2, 3 & 4 done — HTML
  parse/emit, both cross-format gates, and URL ingestion).
- **Phase 4 is complete** (Tasks 5–9): storage, chunking, embeddings, vector
  retrieval, and the end-to-end `ingest → chunk → embed → index` pipeline with
  `query` + `context_window`. Nucklavee now performs semantic search end to
  end at the library level.
- **Phases 1–5 are complete.** The library and CLI perform ingest (md/html/
  url) → convert (md/html/text) → chunk → embed → index → semantic search →
  context assembly, end to end, over persistent SQLite + usearch storage.
- **2026-07-13 reliability pass done** (external-review remediation): SQLite
  authoritative / usearch derived, transactional projection writes,
  derive-first ingest, option-aware dedupe via `processing_fingerprint`,
  durable embeddings + `rebuild_index`, embedder response permutation
  validation. See `docs/adr/0001-persistence-and-index-consistency.md` and
  `CHANGELOG.md`.
- **2026-07-14 final stabilization gate implemented:** collection-wide
  embedding-space identity enforced across SQLite (schema v3), the
  configured embedder, and usearch manifests (v2); query-embedding
  validation; keymap integrity hardening. See ADR 0001 (2026-07-14
  amendments) and `CHANGELOG.md`. **The gate is satisfied only after
  formatting, clippy, and tests pass on both Linux and Windows CI** — PDF
  work must not start before that.
- **Exact next focus:** complete the experimental open-source MVP release gate
  (license, security/privacy boundaries, package validation, documentation),
  then begin **Phase 6 — the PDF pipeline (audit Tasks 11–12)**.
- **Task plan reference:** `docs/audits/2026-07-06-full-scope.md` (per-task
  scope fences, guardrails, acceptance criteria).
- **Gate reference:** `docs/phase-gates.md` (Phase-1 exit criteria and blocker/warning definitions).
- **Normalization policy reference:** `docs/normalization-deltas.md` (acceptable parse/emit deltas vs semantic regressions).
- **Cross-phase quality gates still open:** all items under **Cross-Phase Quality Gates** remain active and continue as parallel quality work across Phases 3+.
