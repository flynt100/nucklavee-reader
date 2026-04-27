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

## Current State

This repository has moved beyond pure scaffolding and now includes a working
Markdown ingest/emit slice plus test harnesses:

- `Library` API scaffold in `src/lib.rs`
- Core IR types in `src/ir/mod.rs`
- Parser trait + format parser paths (`markdown` implemented; `html`/`pdf` still scaffolded)
- Emitter trait + format emitter paths (`markdown` implemented; others still scaffolded)
- Chunking model + trait scaffold
- Storage abstraction + sqlite placeholder
- Vector index abstraction + usearch placeholder
- Embedder abstraction + API embedder config scaffold
- Context/pipeline placeholders
- CLI binary target scaffold (`nucklavee`)
- Markdown roundtrip harness and fixture-driven tests

## What Works Right Now

- The crate compiles.
- The CLI target builds and runs.
- Markdown parser is implemented.
- Markdown emitter is implemented.
- IR roundtrip harness (parse -> emit -> parse -> semantic compare) is implemented.
- Architecture boundaries are in place for incremental implementation.

## What Is Not Implemented Yet

- HTML and PDF parser logic
- Non-markdown emitter implementations
- Chunking algorithm implementation
- SQLite persistence implementation
- Vector index integration
- Embedding API calls
- End-to-end ingest/search/context flows

Remaining unimplemented paths currently return explicit `not implemented` errors.

## Near-Term Implementation Order

1. Phase 2 hardening (normalization policy, fixture deltas, CLI integration boundary)
2. HTML parser/emitter and cross-format tests
3. Store + chunking + embedder + vector index
4. CLI command wiring
5. PDF pipeline (iterative heuristics)

## Development

### Build

```bash
cargo build
```

### Run tests

```bash
cargo test
```

### Run CLI scaffold

```bash
cargo run --bin nucklavee
```

### Phase-2 CLI usage (important)

Phase 2 supports retrieval through `ingest-emit` only.

Persistence semantics are strict:

- The CLI uses an in-memory document store.
- Document IDs are process-local and only valid inside the process that created them.
- IDs printed by `ingest` are not reusable across separate CLI invocations.

Use this command for reliable behavior:

```bash
cargo run --bin nucklavee -- ingest-emit ./fixtures/sample.md --format markdown
```

`--format` currently supports exactly: `markdown`.
If another format is passed, the CLI/runtime error string is:
`unsupported format '<value>'. supported: markdown`.

For the complete command boundary and behavior matrix, see
`docs/cli-phase2-boundary.md`.

## Reference Spec

- `nucklavee-spec.md` (source of truth for scope and behavior)
- `docs/phase-gates.md` (phase exit criteria and blocker vs warning policy)
- `docs/normalization-deltas.md` (allowed markdown parse/emit deltas vs semantic regressions)
