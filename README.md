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
- **Option-aware deduplication** on ingest (spec §7.4): identical content
  processed identically returns the existing document ID; identical content
  with changed processing options is reprocessed in place under the same ID
  (`content_hash` + `processing_fingerprint`).
- **Document stores**: an in-memory store and a **SQLite** store
  (`rusqlite`, bundled), both implementing the full frozen `DocumentStore`
  contract (hash lookup, listing, removal, chunk retrieval ordered by
  sequence). SQLite persists the full IR as JSON, so a document ingested in
  one process is retrievable and re-emittable in another. A shared
  conformance suite runs against both backends.
- **Structure-aware chunker** (`StructuralChunker`, Task 7): slices a document
  into embedding-ready chunks that never cross a section boundary, reusing the
  parser's `section_path` provenance; tables and code become their own tagged
  chunks, prose accumulates, and oversized content is split within a token
  budget (tiktoken cl100k_base). Stored IR is canonicalized at ingest so
  chunking is consistent across source formats.
- **End-to-end semantic search** (Task 9): `ingest` runs the full pipeline —
  parse → validate → canonicalize → store → chunk → embed → index —, `query`
  embeds a question and returns the nearest chunks ranked with provenance, and
  `context_window` packs ranked chunks into a token-budgeted string with
  `[Source: title > section]` headers for LLM consumption (spec §7.3).
- **Embeddings + vector search backends**: an `ApiEmbedder` (OpenAI-compatible
  `/v1/embeddings`, blocking) and a `UsearchIndex` (cosine HNSW via `usearch`)
  with add/search/remove and file persistence.
- **Reliability model** (2026-07-13 pass): SQLite is authoritative, the
  vector index is a derived, rebuildable projection. Ingest derives
  everything (parse/chunk/embed) before writing anything, then commits the
  document + chunks + embeddings in one transaction — a failed ingest stores
  nothing. `rebuild-index` restores a lost/corrupt index from stored
  embeddings without re-embedding. See
  `docs/adr/0001-persistence-and-index-consistency.md`.
- **CLI** (spec §8, Task 10): `ingest` / `search` / `emit` / `list` / `info`
  / `context` / `remove` / `rebuild-index` over a persistent SQLite +
  usearch library, TOML config, `--json` machine output.

## What Is Not Implemented Yet

- PDF ingestion (Tasks 11–12) — the next phase.

Unsupported paths return explicit, tested `unsupported format` /
`invalid input` errors (see `docs/cli.md`).

## Implementation Order

Per-task scopes, guardrails, and acceptance criteria live in
`docs/audits/2026-07-06-full-scope.md`; roadmap status lives in `TODO.md`.

1. ~~Phase 1 / 2A / 2B — markdown core loop + hardening~~ ✅
2. ~~Task 0/1 — cleanup, trait freeze, dedupe, provenance remap~~ ✅
3. ~~Task 2 — HTML parser (content extraction + DOM→IR)~~ ✅
4. ~~Task 3 — HTML emitter + cross-format gates (both directions)~~ ✅
5. ~~Task 4 — URL ingestion~~ ✅ **(Phase 3 complete)**
6. ~~Task 5 — PlainText emitter~~ ✅
7. ~~Task 6 — SQLite store~~ ✅ · ~~Task 8 — embedder + vector index~~ ✅
8. ~~Task 7 — structure-aware chunker~~ ✅
9. ~~Task 9 — pipeline wiring + `query` + `context_window`~~ ✅ **(Phase 4 complete)**
10. ~~Task 10 — CLI rework (spec §8 command set)~~ ✅
11. Tasks 11–12 — PDF pipeline (**next**)

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

The CLI is a persistent library backed by SQLite + a usearch index, configured
by a TOML file (`--config`, default `~/.config/forge/config.toml`). Ingest in
one invocation is searchable in the next.

```bash
# Ingest a file or a URL (parse → store → chunk → embed → index)
cargo run --bin nucklavee -- ingest ./notes/valence.md
cargo run --bin nucklavee -- ingest https://example.com/article

# Semantic search and LLM context assembly
cargo run --bin nucklavee -- search "valence in semiconductors" --limit 5
cargo run --bin nucklavee -- context "how does valence affect conductivity" --budget 1500

# Convert, inspect, manage
cargo run --bin nucklavee -- emit <document-id> html
cargo run --bin nucklavee -- list --json
cargo run --bin nucklavee -- info <document-id>
cargo run --bin nucklavee -- remove <document-id>

# Rebuild the vector index from stored embeddings (no re-embedding)
cargo run --bin nucklavee -- rebuild-index
```

`emit` formats: `markdown`, `html`, `text`. `ingest`/`search`/`context`
require a configured embedding endpoint. See `docs/cli.md` for the full
command reference and config schema.

## Reference Docs

- `nucklavee-spec.md` (source of truth for scope and behavior)
- `docs/ir-deltas-from-spec.md` (spec-vs-implementation deltas — read before building)
- `docs/audits/2026-07-06-full-scope.md` (audit + per-task build plan)
- `docs/phase-gates.md` (phase exit criteria and blocker vs warning policy)
- `docs/normalization-deltas.md` (allowed markdown parse/emit deltas vs semantic regressions)
- `docs/cli.md` (CLI command reference and config schema)
- `docs/adr/0001-persistence-and-index-consistency.md` (consistency model: SQLite authoritative, index derived)
- `CHANGELOG.md` (notable changes by date)
