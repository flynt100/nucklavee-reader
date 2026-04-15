# Nucklavee

Nucklavee is a Rust-first **universal document transformation library** focused on:

- Parsing heterogeneous sources (Markdown, HTML, PDF) into one intermediate representation (IR)
- Structure-aware chunking with provenance metadata
- Embedding and vector search over chunks
- Context-window assembly for LLM/RAG workflows

The target architecture is defined in `nucklavee-spec.md`.

## Project Goal

Build a standalone crate and CLI that can ingest source documents and produce reliable, provenance-rich semantic retrieval context.

In short: **normalize anything, preserve structure, and make it searchable.**

## What Works Right Now

- **Markdown parser + emitter** with roundtrip tests against nested lists, GFM tables, code blocks, blockquotes, inline styles, links, images, and thematic breaks.
- **`Document::validate_strict`** enforces heading level bounds, non-empty inline content, and `GenericBlock.confidence` range.
- **`Document::structural_eq`** compares two IR trees with whitespace-tolerant inline normalization (the roundtrip oracle).
- **`Library<S: DocumentStore>`** wires `ingest`, `get_document`, and `emit` end-to-end. SHA-256 content-hash dedupe is live.
- **`InMemoryDocumentStore`** for tests and for exercising the facade without SQLite.
- **CLI:** `nucklavee parse <path>` prints the IR as JSON; `nucklavee emit <path>` prints re-emitted markdown.

## What Is Not Implemented Yet

- HTML parser / emitter
- PDF parser
- PlainText emitter
- Chunker
- SQLite persistence (scaffold type exists; no impl)
- Vector index (scaffold type exists; no impl)
- Embedder (scaffold types exist; no impl)
- `Library::query` and `Library::context_window` (return `Error::NotImplemented`)

All unimplemented paths return explicit `not implemented` errors.

## Near-Term Implementation Order

1. ~~IR validation and test harness utilities~~ (done)
2. ~~Markdown parser + markdown emitter + roundtrip tests~~ (done)
3. HTML parser/emitter and cross-format tests
4. Chunker + SQLite store + embedder + vector index
5. CLI `ingest` / `search` / `context` commands
6. PDF pipeline (iterative heuristics)

## Development

```bash
cargo build
cargo test
cargo run --bin nucklavee -- parse path/to/file.md
cargo run --bin nucklavee -- emit  path/to/file.md
```

## Reference Spec

- `nucklavee-spec.md` (source of truth for scope and behavior)
