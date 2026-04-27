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

## Phase 2B — Remaining Phase-2 Hardening

Primary goal: tighten behavior contracts and handoff boundaries around the completed Markdown core loop.

1. [x] Define and document acceptable normalization deltas (whitespace/list-marker canonicalization and semantic-regression boundaries) in `docs/normalization-deltas.md`.
2. [x] Expand fixture coverage deltas for difficult Markdown edge patterns.
3. [x] Define/lock CLI integration boundary for parser/emitter + roundtrip workflow.

## Phase 3 — HTML Parser/Emitter + Cross-Format Integrity

Primary goal: robust content extraction from noisy HTML and parity with markdown IR behavior.

1. [ ] Implement readability-style content extraction before DOM mapping.
2. [ ] Implement DOM-to-IR mapping for core block/inline semantics.
3. [ ] Implement HTML emitter for faithful structural output from IR.
4. [ ] Add cross-format tests:
   - [ ] html -> IR -> markdown
   - [ ] markdown -> IR -> html
   - [ ] structural equivalence checks where semantically valid
5. [ ] Add URL-source path validation (`Source::Url`) and metadata/title extraction tests.

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

- **Current truth:** Markdown parser/emitter and IR roundtrip harness are complete.
- **Exact next focus:** finish Phase 2B hardening: normalization policy docs, fixture expansion deltas, and CLI integration boundary.
- **Gate reference:** `docs/phase-gates.md` (Phase-1 exit criteria and blocker/warning definitions).
- **Normalization policy reference:** `docs/normalization-deltas.md` (acceptable parse/emit deltas vs semantic regressions).
