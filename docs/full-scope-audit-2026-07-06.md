# Nucklavee Full-Scope Audit & Build-Task Plan

_Date: 2026-07-06_
_Branch: `claude/nucklavee-reader-audit-v04mgz`_
_Baseline: `main` @ `cc01263` (post Phase 2B QC merge)_

> **Execution status:** Task 0 (all of 0a–0f) and Task 1 (1a trait freeze,
> 1b dedupe, 1c provenance remap + sentinel guard) were executed on this
> branch on 2026-07-06. Findings W1–W3, W5–W6, and the 0a/0f items of W7 are
> resolved; the spec-vs-implementation record now lives in
> `docs/ir-deltas-from-spec.md`. Executing 1c exposed and fixed an
> additional pre-existing bug not listed below: math shielding replaced
> `\[ \]` spans inside inline code with the literal placeholder character in
> both IR and emitted output (symmetric corruption, invisible to roundtrip
> tests).
>
> **Task 2 (HTML parser) is also complete** (same day): `scraper`-based
> content extraction + DOM→IR mapping in `src/parsers/html.rs`, wired into
> `Library::ingest` for `Source::RawHtml` and `.html`/`.htm` files, with
> html→IR→markdown→IR equivalence gates over three realistic fixture pages
> (`tests/html_ingest.rs`, 17 tests) and updated boundary contracts. A known
> limitation is documented for Task 3/4 pickup: math-like `$…$` syntax inside
> link URLs is still shielded by the markdown parser pre-cmark (pre-existing,
> symmetric).
>
> **A behavior-neutral parser-dedup pass** then landed (sanity-check items):
> shared `parsers::SectionPathTracker` + `GENERIC_BLOCK_DEFAULT_CONFIDENCE`,
> consolidated HTML whitespace helpers, and doc comments pinning the
> `ir::normalize` vs HTML `finalize_inlines` distinction. **Task 3 (HTML
> emitter) is complete**: `src/emitters/html.rs` (spec §5.2), wired to
> `Format::Html` + CLI `--format html`, with markdown→IR→html→IR structural
> gates (`tests/cross_format.rs`) closing the second cross-format direction.
>
> **Task 4 (URL ingestion) is complete**, closing Phase 3: `src/net` adds a
> blocking `reqwest` fetch (redirects, `Content-Type`→parser sniffing with
> extension/body fallback, typed `Error::Network`), wired into
> `Library::ingest(Source::Url)` and CLI `http(s)://` arguments, with hermetic
> loopback-server tests (`tests/url_ingest.rs`). Next: the Phase-4 retrieval
> track — Tasks 5/6/8 are parallelizable, feeding Task 7 then Task 9.

---

## Part 1 — Current-State Audit

### Verified health

- `cargo test`: **75/75 passing** (lib 6, cli_smoke_contract 6, diagnostics 8, markdown_roundtrip 43, phase2_contract 3, validation 9).
- `cargo clippy --all-targets`: 7 warnings (6× collapsible-if in `src/emitters/markdown.rs`, 1× let-else→`?` in `src/parsers/markdown.rs`). No errors.
- Phase 1, 2A, 2B are genuinely complete per their gate definitions in `docs/phase-gates.md`.
- The Phase 2B QC report's only functional finding (§3.1, untested `normalize_repeated_leading_segment`) has since been resolved — `tests/diagnostics.rs:110-147` now covers both the passive-diagnostic and active-truncation paths.

### What is real vs. scaffold

| Area | State |
|---|---|
| IR types + provenance + diagnostics (`src/ir/`) | **Implemented, tested** |
| IR validation + normalization + structural equivalence | **Implemented, tested** |
| Markdown parser (`src/parsers/markdown.rs`, 1,484 lines) | **Implemented, tested** — includes several Phase-2B rescue heuristics (see W1) |
| Markdown emitter (`src/emitters/markdown.rs`, 536 lines) | **Implemented, tested** |
| `Library` API (`src/lib.rs`) | Markdown ingest/emit works; `query`/`context_window` return contract errors |
| CLI (`src/cli/main.rs`) | `ingest` / `ingest-emit` work; everything else is boundary-error placeholders |
| In-memory `DocumentStore` | Implemented (chunk methods present but unused) |
| HTML/PDF parsers, HTML/text emitters | Macro-generated `NotImplemented` stubs |
| `SqliteDocumentStore`, `UsearchIndex`, `ApiEmbedder`, `IngestionPipeline`, `ContextAssembler` | Empty marker structs, no trait impls |
| Chunker | Types + trait only, no implementation |

---

## Part 2 — Weak Points & Compressible Refactors

Ordered by how much they threaten future phases, not by current breakage. **Nothing here is a failing contract today** — all findings are about load-bearing surfaces the next phases will build on.

### W1. Phase-2B heuristics are entangled with the core markdown parse walk *(structural, highest leverage)*

`src/parsers/markdown.rs` is ~45% generic cmark-event tree building and ~55% input-rescue heuristics:

- escaped-newline stream decoding (`normalize_preparse_input`)
- math shielding / placeholder / restore machinery (`shield_math_segments`, `MathRestoreState`, ~250 lines)
- unfenced-frontmatter inference (`infer_probable_frontmatter`)
- duplicated-leading-segment detection (`detect_repeated_leading_segment`)
- TSV-paragraph → Table promotion (`detect_phase2b_tabular_fallback`, invoked inside `end_tag`)
- bare-callout normalization (`normalize_bare_callout_paragraph`)

These are *markdown-input-repair* concerns (Obsidian exports, LLM-paste artifacts), not parsing. Left inline, Phase 3/6 agents face a bad choice: replicate them for HTML/PDF (wrong — they're markdown-specific) or diverge from the file's structure while editing around them.

**Refactor:** split `src/parsers/markdown.rs` into a module directory — `markdown/mod.rs` (event walk + frames), `markdown/preparse.rs` (newline decode, frontmatter extraction/inference, duplicate-segment detection), `markdown/math.rs` (shield/restore), `markdown/heuristics.rs` (TSV fallback, callouts). Zero behavior change; all 75 tests must pass untouched.

### W2. Emitter `escape_text` rewrites content, with a false-positive trap *(correctness)*

`src/emitters/markdown.rs:413-425`: any `[...]` span whose contents match `looks_like_math_inline` (contains any of `\ = ^ _ { }`) is **promoted to display-math delimiters** during emission. A plain paragraph containing `[see chapter_3]` or `[key=value]` emits as `\[see chapter_3\]` / `\[key=value\]`. Roundtrip stays structurally "equivalent" only because the parser's math shielding then treats it as math — the surface text has been semantically re-labeled.

Similarly `collect_math_bracket_span` (emitter) and `try_match_math_span` (parser) encode the same "looks mathy" judgement in two places with different logic.

**Refactor:** represent protected math spans explicitly in the IR (e.g. a `MathSpan`/protected-literal inline carrying the original delimiter form) or at minimum centralize the classifier in one shared module and tighten it. This removes the emitter's content-invention and collapses duplicated heuristics. Should land **before** the chunker (Phase 4), because chunk `content` extraction will otherwise inherit the ambiguity.

### W3. Provenance byte ranges are in preprocessed-input coordinates, not original-source coordinates *(provenance guarantee)*

When preparse normalization changes lengths — escaped-`\n` decoding shrinks the input, math shielding replaces each `$…$`/`\[…\]` payload with a single 2-byte `¤` placeholder — every downstream `ByteRange` refers to the *transformed* string. `Library::ingest` then validates ranges against the **original** input length (`src/lib.rs:103`), which passes only because transforms shrink. Ranges are in-bounds but point at wrong original bytes whenever shielding fires.

Related edge: a **literal `¤` character in source text** desyncs `restore_math_placeholders` — it pops a queued math payload for a character that was never a placeholder, corrupting both spans (there is a diagnostic only for queue underflow, not misalignment).

This undermines the spec's "provenance everywhere / citation" pillar right before Phase 4 builds chunk provenance on top of it.

**Fix:** maintain an offset-map from transformed → original coordinates during preparse/shielding (both transforms are span-local, so a simple sorted vec of (transformed_pos, delta) suffices), and escape pre-existing `¤` (or pick an unused private-use codepoint and reject/escape it on input).

### W4. Trait surfaces will not survive Phase 4 as-is *(design freeze needed)*

- `Chunker::chunk(&self, document_id, body_text: &str)` takes flat text — contradicts the spec's structure-aware IR-walking chunker. Must become `chunk(&self, document: &Document, ...)`.
- `DocumentStore` lacks `find_by_content_hash` (dedupe, spec §7.4), `list_documents`, `remove_document`, `get_chunks_by_ids` (needed for search join + CLI `list`/`info`/`remove`, spec §8).
- `VectorIndex` lacks `remove` and persistence (save/load) hooks.
- `Embedder` is sync while the spec lists `reqwest`+`tokio` — an async-vs-blocking decision is unresolved. Recommendation: **`reqwest::blocking` for MVP**, no tokio; the trait already fits.
- `parsers::Parser::parse(&self, input)` cannot carry `ParseOptions`, so `MarkdownParser` (the trait impl) silently uses defaults while the real entrypoint is the free function.

If each Phase-4 task mutates these traits independently, they'll conflict. Freeze the surfaces in one dedicated push first.

### W5. Content-hash dedupe is computed but never used

`sha256_hex(input)` lands in `DocumentMeta.content_hash`, but re-ingesting the same file mints a new UUID and a second document. Spec §7.4 requires returning the existing ID. Cheap to implement once `DocumentStore::find_by_content_hash` exists (Task 1 + Task 6 below).

### W6. Duplication / dead weight

- `NoopVectorIndex` + `NoopEmbedder` duplicated verbatim in `src/cli/main.rs` and `tests/phase2_contract.rs` (QC §3.2, still true).
- Empty marker structs with no behavior: `IngestionPipeline`, `ContextAssembler`, `SqliteDocumentStore`, `UsearchIndex`, config-only `ApiEmbedder`.
- CLI carries a hidden legacy `Emit` command plus arg-less `Query`/`ContextWindow`/`Html`/`Pdf` placeholder variants that Phase 5 will delete wholesale.

### W7. Docs / repo drift

- `NEXT_BUILD_PUSH.md` still declares "Active implementation target: Phase 2B hardening" — two phases stale; contradicts `TODO.md`. One of them should be the single status source.
- `electromagnetic-valence.md` (473-line Obsidian corpus document) sits in the repo root; it's a test corpus artifact referenced by `docs/cli-phase2-boundary.md`'s verification snippet. Belongs under `tests/corpus/`.
- `nucklavee-spec.md` (v0.2) predates implemented reality: no `BlockNode`/`Provenance`, no `Diagnostic`, no `Frontmatter` on `DocumentMeta`, `Chunker` shape differs. A build agent handed "the spec" will produce types that don't match the codebase. Needs a delta addendum (not a spec rewrite).
- Fixture numeric-prefix collisions (`13_`, `18_`, `19_`, `20_`, `21_`) — cosmetic, accept as-is.

### W8. Contract strings are load-bearing and phase-versioned

`src/phase2_contract.rs` strings are asserted **exactly** in `tests/cli_smoke_contract.rs` and `tests/phase2_contract.rs`, and documented in `docs/cli-phase2-boundary.md` and `README.md`. Every task that enables a new format/command must update all four in the same push, or CI breaks / docs lie. Called out per-task below so no agent trips on it.

---

## Part 3 — Roadmap → Handoff-Ready Build Tasks

Each task below is sized as **one PR / one build-agent handoff**, with explicit scope fences so later tasks never rework or overwrite earlier ones.

**Global guardrails (include verbatim in every handoff):**

1. All 75 existing tests must pass unless the task explicitly says which contract tests it replaces.
2. Markdown emitter output is contract (`docs/normalization-deltas.md`); do not change canonical markdown output unless the task says so.
3. Do not modify trait signatures in `storage`, `vector`, `embedder`, `chunking`, `parsers`, `emitters` outside Task 1.
4. Frontmatter/metadata lives on `DocumentMeta`, never in `Document.body`.
5. Markdown-input heuristics (math shielding, callouts, TSV fallback, etc.) are markdown-parser-private; HTML/PDF parsers must not import or re-implement them.
6. When enabling a new format/command, update `phase2_contract.rs` constants + `tests/cli_smoke_contract.rs` + `docs/cli-phase2-boundary.md` + `README.md` together.

### Task 0 — Housekeeping + parser/emitter refactor prep *(the proposed cleanup, pre-approval)*

**Goal:** shrink and de-risk the surfaces every later task touches. Zero behavior change (except 0e's classifier tightening, which changes only the false-positive case).

- **0a Docs hygiene:** retire `NEXT_BUILD_PUSH.md` (fold its role into `TODO.md`'s Status Snapshot); move `electromagnetic-valence.md` → `tests/corpus/electromagnetic-valence.md` and fix the reference in `docs/cli-phase2-boundary.md`.
- **0b Clippy zero:** fix the 7 warnings (collapsible-ifs, let-else).
- **0c De-duplicate stubs:** add `nucklavee::test_support` (plain `pub mod`, doc-commented as non-API) holding `NoopVectorIndex`/`NoopEmbedder`; use from CLI and `tests/phase2_contract.rs`.
- **0d Parser module split (W1):** `src/parsers/markdown.rs` → `markdown/{mod,preparse,math,heuristics}.rs`. Pure code motion; public paths (`parsers::markdown::{parse_markdown, ParseOptions, MarkdownParser}`) unchanged.
- **0e Math classifier consolidation (W2, minimal version):** single shared `looks_like_math_inline` used by parser and emitter; require at least one *strong* math signal (`\`, `=`, `^`, `{`, `}`) — bare `_` alone no longer promotes `[see chapter_3]` to display math. Add regression fixture. (Full IR-level math span representation is deferred to Task 2-prep if desired.)
- **0f Spec delta addendum (W7):** `docs/ir-deltas-from-spec.md` — table of spec-vs-implementation differences (BlockNode/Provenance, Diagnostic, Frontmatter, Chunker signature, CLI phase boundary).

**Acceptance:** `cargo test` green (75+ tests), `cargo clippy --all-targets` zero warnings, no public API change.

### Task 1 — Trait-surface finalization (design freeze) *(blocks Tasks 5–9)*

**Goal:** land the final Phase-4/5 trait shapes once, with stubs still compiling, so parallel implementation tasks never touch shared signatures.

- `Chunker::chunk(&self, document: &Document, opts: &ChunkOptions) -> Result<Vec<Chunk>>`.
- `DocumentStore` += `find_by_content_hash(&str) -> Result<Option<DocumentId>>`, `list_documents() -> Result<Vec<DocumentMeta>>`, `remove_document(DocumentId) -> Result<()>`, `get_chunks_by_ids(&[ChunkId]) -> Result<Vec<Chunk>>`; implement all on `InMemoryDocumentStore`.
- `VectorIndex` += `remove(ChunkId)`, `save(&Path)`, `load(&Path)` (stub-erroring where backend absent).
- Decide embedding I/O model: **blocking** (`reqwest::blocking`), no tokio; document in the trait.
- `Parser` trait: either add an associated `Options` type or (recommended) drop the trait until a second parser exists and free functions per format are the contract.
- Wire content-hash dedupe into `Library::ingest` against the in-memory store (spec §7.4) — return existing ID on hash hit.
- Provenance offset-map fix (W3): translate byte ranges back to original-source coordinates; guard the `¤` placeholder collision. This is here (not Task 0) because it touches parse output invariants that chunk provenance will freeze.

**Acceptance:** all tests green; new unit tests for dedupe, store extensions, and an offset-map regression test (shielded-math fixture where a block after a `$…$` span has the correct original byte range).

### Task 2 — Phase 3: HTML parser (content extraction + DOM→IR)

**Goal:** `scraper`-based parser producing IR per spec §4.2.

- Deps: `scraper` only (dependency gate lifts for Phase 3).
- Readability-style extraction (strip `nav/header/footer/aside/script/style`, densest container); DOM→IR mapping table from spec; `class="language-*"` on code blocks; title from `<title>` → `<h1>` → `og:title`; unknown elements → `GenericBlock` with class-name hint + diagnostics.
- Wire `Source::RawHtml` and `Source::File(*.html)` in `Library::ingest` (update contract strings per guardrail 6).
- **Out of scope:** URL fetching (Task 4), HTML emitter (Task 3), any change to markdown parser.
- **Acceptance:** fixture suite of saved real pages (docs site, Wikipedia, blog); html→IR→markdown→IR structural-equivalence tests; extraction strips chrome on all fixtures.

### Task 3 — Phase 3: HTML emitter + cross-format integrity

**Goal:** faithful IR→HTML per spec §5.2 + the cross-format gate.

- Escaping (angle brackets, ampersands, attribute quotes); `thead/tbody` tables; `language-*` classes.
- Enable `Format::Html` in `Library::emit` and CLI `--format html`; update `SUPPORTED_FORMATS` + smoke tests + boundary doc together.
- Cross-format tests: markdown→IR→html→IR and html→IR→markdown→IR equivalence where semantically valid.
- **Out of scope:** parser changes beyond what Task 2 landed.

### Task 4 — Phase 3: URL ingestion

**Goal:** `Source::Url` works. Small, isolated.

- `reqwest` (blocking) fetch → HTML parser; preserve URL in `SourceInfo`; content-type sniffing (html vs md); typed network errors (`Error::InvalidInput` vs new `Error::Network`).
- **Acceptance:** mocked-HTTP tests (no live network in CI).

### Task 5 — Phase 4: PlainText emitter

**Goal:** spec §5.3 (UPPERCASE headings, 4-space code indent, `| ` quotes, `text (url)` links). Required by chunker/embedding text and `Format::PlainText`.

- Enable `Format::PlainText` in `Library::emit` + CLI (contract-string update per guardrail 6).
- **Acceptance:** golden fixtures per block/inline type.

### Task 6 — Phase 4: SQLite DocumentStore

**Goal:** implement `SqliteDocumentStore` against the (frozen) trait.

- `rusqlite`; spec §7.2 schema **plus**: serialized IR body (JSON via `serde_json`) so cross-process `emit` works, `frontmatter`, `diagnostics`.
- Migrations: single embedded schema-version pragma; dedupe via `content_hash UNIQUE`.
- **Out of scope:** CLI changes (Phase 5), vector index.
- **Acceptance:** trait-conformance test suite run against **both** memory and sqlite stores (shared test fn), tempfile-backed.

### Task 7 — Phase 4: structure-aware chunker

**Goal:** spec §6 against the frozen `Chunker` trait.

- Walk `Document.body`; **reuse `BlockNode.prov.section_path`** (already computed by the parser) rather than re-deriving heading stacks; token budget (default 512) with `tiktoken-rs` cl100k_base; splitting rules (paragraph → sentence; code at blank lines; tables by rows with header re-prepended); `ChunkBlockType` tagging; plain-text rendering of chunk content via Task 5's emitter.
- **Acceptance:** spec §11.2 tests — no chunk spans two sections, all chunks within budget, provenance paths correct, block types tagged.

### Task 8 — Phase 4: ApiEmbedder + usearch VectorIndex

**Goal:** OpenAI-compatible `/v1/embeddings` blocking client + HNSW index.

- `ApiEmbedder` impl (endpoint/model/api-key config, batch requests, dimension check); `UsearchIndex` impl with save/load to a single file.
- **Acceptance:** embedder tested against a local mock HTTP server; index add/search/remove/persistence round-trip tests with synthetic vectors.

### Task 9 — Phase 4: pipeline wiring + query + context_window

**Goal:** end-to-end: ingest → parse → validate → store → chunk → embed → index; `query` (index top-k → store join → ranked chunks); `context_window` (spec §7.3 greedy packing with `[Source: title > section_path]` headers, header tokens counted in budget).

- Replace `QUERY_NOT_IMPLEMENTED`/`CONTEXT_WINDOW_NOT_IMPLEMENTED` paths (contract-string/test/doc updates per guardrail 6).
- Decide fate of the empty `IngestionPipeline`/`ContextAssembler` structs: implement or delete (recommend: implement `ContextAssembler`, delete `IngestionPipeline` in favor of `Library` methods).
- **Acceptance:** end-to-end test with the deterministic `NoopEmbedder`-style mock (seeded vectors) proving expected chunks rank top-k; budget-boundary tests.

### Task 10 — Phase 5: CLI rework

**Goal:** spec §8 command set: `ingest <path_or_url>`, `search`, `emit`, `list`, `info`, `context`, `remove`; config from `~/.config/forge/config.toml` (db path, embedding endpoint); `--json` machine output; delete the hidden `Emit` and placeholder variants.
- **Retires** the Phase-2 CLI boundary: rewrite `tests/cli_smoke_contract.rs`, `docs/cli-phase2-boundary.md`, `phase2_contract.rs` (rename to a general `contract.rs` or delete), README usage section — in this task only.
- **Acceptance:** smoke tests for every subcommand against a temp sqlite db + mock embedder endpoint.

### Task 11 — Phase 6a: PDF foundations (text, lines, paragraphs, headings)

**Goal:** spec §4.3 stages 1–2, 4 (font-size heading detection), 6, 8 with confidence scoring; typed blocks only above threshold 0.7, else `GenericBlock`.
- Deps: `pdf-extract`, `lopdf`.
- **Acceptance:** spec §10 Phase-4 exit — ≥80% body text as paragraphs, ≤20% heading false positives on a committed 5–10 datasheet corpus (`tests/corpus/pdf/` + annotation files per spec §11.4).

### Task 12 — Phase 6b: PDF tables + hardening

**Goal:** stages 3, 5, 7 (columns, tables, header/footer removal), fuzzy per-page header matching, footnote-to-table association; corpus to 20+ docs; accuracy metrics tracked in a checked-in report.
- **Acceptance:** spec Phase-5 exit thresholds (≥60% table detection, ≥70% cell accuracy).

### Task 13 — Cross-phase: perf baselines + property tests *(parallel, any time after Task 1)*

- `criterion` (or simple timed harness) ingest/query baselines on `tests/corpus/`; `proptest` generators for IR → validate/normalize/equivalence invariants and emit→parse closure.

### Dependency graph

```
Task 0 ──► Task 1 ──► Task 2 ──► Task 3 ──► Task 4
              │
              ├──► Task 5 ──► Task 7 ─┐
              ├──► Task 6 ────────────┼──► Task 9 ──► Task 10
              └──► Task 8 ────────────┘
Task 11 ──► Task 12   (independent of 2–10; needs Task 0/1 only)
Task 13   (parallel after Task 1)
```

Tasks 5/6/8 are mutually independent and can run in parallel once Task 1 freezes the traits; Task 7 needs Task 5; Task 9 needs 5–8; Task 10 needs 9. Tasks 2–4 (HTML) can proceed in parallel with the Phase-4 track after Task 1.
