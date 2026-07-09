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
- **HTML emit** (Phase 3, Task 3): IR → semantic HTML (spec §5.2), escaped,
  with Strong/Em/Del style mapping; `--format html` on the CLI, `Format::Html`
  in the library.
- **Plain-text emit** (Task 5): IR → readable, formatting-stripped text
  (spec §5.3 — uppercased headings, `text (url)` links, 4-space-indented code,
  `| ` quote gutters, pipe-delimited tables); `--format text`, `Format::PlainText`.
  This is the rendering the chunker/embeddings will consume.
- **Cross-format integrity, both directions**: html → IR → markdown → IR on
  realistic docs-site / wiki / blog pages, and markdown → IR → html → IR on a
  rich fixture (nested lists, tables, code, quotes, links, images), enforced
  by structural equivalence.
- **Provenance in original-source coordinates**: block byte ranges survive
  the parser's internal input rewrites (markdown); HTML blocks carry heading
  `section_path` provenance.
- **URL ingestion** (Phase 3, Task 4): fetch an `http(s)://` page (blocking
  `reqwest`, redirects followed), auto-detect HTML vs Markdown from the
  `Content-Type` header (with URL-extension and body fallbacks), and record
  the final URL as provenance. Network failures surface as typed errors.
- **Content-hash deduplication** on ingest (spec §7.4): identical content
  returns the existing document ID.
- **Document stores**: an in-memory store and a **SQLite** store
  (`rusqlite`, bundled), both implementing the full frozen `DocumentStore`
  contract (hash lookup, listing, removal, chunk retrieval ordered by
  sequence). SQLite persists the full IR as JSON, so a document ingested in
  one process is retrievable and re-emittable in another. A shared
  conformance suite runs against both backends.
- **Embeddings + vector search primitives**: an `ApiEmbedder` (OpenAI-compatible
  `/v1/embeddings`, blocking) and a `UsearchIndex` (cosine HNSW via `usearch`)
  with add/search/remove and file persistence. Not yet wired into an
  end-to-end `query` — that is Task 9.
- **CLI**: `ingest` / `ingest-emit` over `.md`, `.html`, `.htm` files or
  `http(s)://` URLs, emitting `markdown` or `html`, with locked boundary
  errors for everything else.

## What Is Not Implemented Yet

- Chunking (Task 7) and the end-to-end query/context-window pipeline that
  wires ingest → chunk → embed → index → search (Task 9). The storage,
  embedding, and vector-index building blocks exist but are not yet connected.
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
4. ~~Task 3 — HTML emitter + cross-format gates (both directions)~~ ✅
5. ~~Task 4 — URL ingestion~~ ✅ **(Phase 3 complete)**
6. ~~Task 5 — PlainText emitter~~ ✅
7. ~~Task 6 — SQLite store~~ ✅ · ~~Task 8 — embedder + vector index~~ ✅
8. Task 7 — structure-aware chunker
9. Task 9 — pipeline wiring + `query` + `context_window`
10. Task 10 — CLI rework
11. Tasks 11–12 — PDF pipeline

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

# Either source → HTML
cargo run --bin nucklavee -- ingest-emit tests/fixtures/06_headings.md --format html

# Fetch a URL and emit it as markdown (HTML vs Markdown auto-detected)
cargo run --bin nucklavee -- ingest-emit https://example.com/page --format markdown
```

`--format` supports `markdown`, `html`, and `text`.
If another format is passed, the CLI/runtime error string is:
`unsupported format '<value>'. supported: markdown, html, text`.

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
