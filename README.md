# Nucklavee

Nucklavee is a Rust-first **universal document transformation library** focused on:

- Parsing heterogeneous sources (Markdown, HTML, PDF) into one intermediate representation (IR)
- Structure-aware chunking with provenance metadata
- Embedding and vector search over chunks
- Context-window assembly for LLM/RAG workflows

The target architecture is defined in `nucklavee-spec.md`; the deltas between
the spec's sketches and the implemented types are recorded in
`docs/ir-deltas-from-spec.md`.

## Project Goal

Build a standalone crate and CLI that can ingest source documents and produce reliable, provenance-rich semantic retrieval context.

In short: **normalize anything, preserve structure, and make it searchable.**

## What Works Right Now

- **Markdown ingest → IR → markdown emit**, hardened by a 40+-fixture
  roundtrip harness, golden outputs, and determinism gates. Includes
  Obsidian-flavored input repair (frontmatter, callouts, wikilinks, math
  shielding, TSV-paragraph promotion) surfaced as typed diagnostics.
- **HTML ingest → IR → markdown emit** (Phase 3, Task 2): readability-style
  content extraction (nav/header/footer/aside/script chrome stripped, densest
  `<main>`/`<article>`/container selected), DOM→IR mapping for headings,
  paragraphs, code blocks (with `language-*` detection), tables (header
  promotion + ragged-row padding), nested lists, blockquotes, links, images,
  styled text; title from `<title>` → `<h1>` → `og:title`; unclassifiable
  elements degrade to `GenericBlock` with class-name hints and diagnostics.
- **Cross-format integrity**: html → IR → markdown → IR structural
  equivalence is enforced on realistic docs-site / wiki / blog fixture pages.
- **Provenance in original-source coordinates**: block byte ranges survive
  the parser's internal input rewrites (markdown); HTML blocks carry heading
  `section_path` provenance.
- **Content-hash deduplication** on ingest (spec §7.4): identical content
  returns the existing document ID.
- **In-memory document store** implementing the full frozen `DocumentStore`
  contract (hash lookup, listing, removal, chunk retrieval).
- **CLI**: `ingest` / `ingest-emit` over `.md`, `.html`, `.htm` files with
  locked boundary errors for everything else.

## What Is Not Implemented Yet

- HTML **emitter** (Task 3) and URL ingestion (Task 4)
- PlainText emitter (Task 5)
- SQLite persistence (Task 6), chunking (Task 7), embeddings + vector index
  (Task 8), query/context-window pipeline (Task 9)
- Full CLI command set (`search`, `list`, `info`, `context`, `remove` — Task 10)
- PDF pipeline (Tasks 11–12)

Remaining unimplemented paths return explicit, tested `not implemented` /
`invalid input` errors (see `docs/cli-phase2-boundary.md`).

## Implementation Order

Per-task scopes, guardrails, and acceptance criteria live in
`docs/full-scope-audit-2026-07-06.md`; roadmap status lives in `TODO.md`.

1. ~~Phase 1 / 2A / 2B — markdown core loop + hardening~~ ✅
2. ~~Task 0/1 — cleanup, trait freeze, dedupe, provenance remap~~ ✅
3. ~~Task 2 — HTML parser (content extraction + DOM→IR)~~ ✅
4. Task 3 — HTML emitter + cross-format gate (markdown → IR → html → IR)
5. Task 4 — URL ingestion
6. Tasks 5–9 — plaintext emitter, SQLite store, chunker, embedder + vector
   index, end-to-end query/context pipeline
7. Task 10 — CLI rework
8. Tasks 11–12 — PDF pipeline

## Development

### Build

```bash
cargo build
```

### Run tests

```bash
cargo test
```

### CLI usage

```bash
# Markdown roundtrip
cargo run --bin nucklavee -- ingest-emit tests/corpus/electromagnetic-valence.md --format markdown

# HTML → markdown (content extraction + DOM→IR mapping)
cargo run --bin nucklavee -- ingest-emit tests/fixtures/html/docs_site.html --format markdown
```

`--format` currently supports exactly: `markdown`.
If another format is passed, the CLI/runtime error string is:
`unsupported format '<value>'. supported: markdown`.

Document IDs are process-local (in-memory store); use `ingest-emit` for
reliable single-process behavior. For the complete command boundary and
behavior matrix, see `docs/cli-phase2-boundary.md`.

## Reference Docs

- `nucklavee-spec.md` (source of truth for scope and behavior)
- `docs/ir-deltas-from-spec.md` (spec-vs-implementation deltas — read before building)
- `docs/full-scope-audit-2026-07-06.md` (audit + per-task build plan)
- `docs/phase-gates.md` (phase exit criteria and blocker vs warning policy)
- `docs/normalization-deltas.md` (allowed markdown parse/emit deltas vs semantic regressions)
- `docs/cli-phase2-boundary.md` (CLI command boundary and behavior matrix)
