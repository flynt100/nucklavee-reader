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
5. [ ] Add URL-source path validation (`Source::Url`) and metadata/title extraction tests. *(audit Task 4; `<title>`/`<h1>`/`og:title` extraction is already implemented and tested for file/raw ingest)*

## Phase 4 — Persistence, Chunking, Embeddings, Vector Retrieval

Primary goal: make ingestion/search functional end-to-end over stored chunks.

1. [ ] Implement `DocumentStore` SQLite backend:
   - [ ] document upsert and retrieval
   - [ ] chunk insert/retrieval by document
   - [ ] content-hash dedupe behavior integration
2. [ ] Implement structure-aware chunker with provenance (`section_path`, order, block type).
3. [ ] Implement embedder backend contract and at least one working provider path.
4. [ ] Implement vector index backend integration (HNSW/usearch abstraction target).
5. [ ] Wire pipeline: ingest -> parse -> validate/canonicalize -> store -> chunk -> embed -> index.
6. [ ] Implement ranked semantic query returning chunks with provenance.

## Phase 5 — CLI Command Wiring and Operator UX

Primary goal: expose library capabilities via stable CLI workflows.

1. [ ] Replace scaffold CLI with subcommands (`ingest`, `query`, `emit`, `context-window`).
2. [ ] Add source format flags/input mode handling (file/url/raw text).
3. [ ] Add output format controls and machine-readable output option for automation.
4. [ ] Add CLI-level error mapping and actionable diagnostics.
5. [ ] Add smoke tests for core CLI flows.

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
`docs/full-scope-audit-2026-07-06.md`.)

- **Current truth:** Markdown + HTML **parsers and emitters** complete, with
  bidirectional cross-format equivalence gates (html↔markdown via
  `structural_diff_bodies`); shared `SectionPathTracker` across parsers;
  trait surfaces for Phase 4/5 frozen; content-hash ingest dedupe live;
  provenance byte ranges in original-source coordinates.
- **Phase 2B state:** complete (gate command green).
- **Phase 3 state:** **complete except URL ingestion** (audit Tasks 2 & 3
  done; Task 4 remaining).
- **Exact next focus:** audit **Task 4** (URL ingestion) to close Phase 3,
  then the Phase-4 track — Tasks 5/6/8 (plaintext emitter, SQLite store,
  embedder+index) are unblocked and parallelizable, feeding Task 7 (chunker)
  and Task 9 (query/context pipeline).
- **Task plan reference:** `docs/full-scope-audit-2026-07-06.md` (per-task
  scope fences, guardrails, acceptance criteria).
- **Gate reference:** `docs/phase-gates.md` (Phase-1 exit criteria and blocker/warning definitions).
- **Normalization policy reference:** `docs/normalization-deltas.md` (acceptable parse/emit deltas vs semantic regressions).
- **Cross-phase quality gates still open:** all items under **Cross-Phase Quality Gates** remain active and continue as parallel quality work across Phases 3+.
