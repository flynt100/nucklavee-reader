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

## Current State (Scaffold)

This repository now contains the initial crate setup and foundational architecture:

- `Library` API scaffold in `src/lib.rs`
- Core IR types in `src/ir/mod.rs`
- Parser trait + format parser stubs (`markdown`, `html`, `pdf`)
- Emitter trait + format emitter stubs (`markdown`, `html`, `text`)
- Chunking model + trait scaffold
- Storage abstraction + sqlite placeholder
- Vector index abstraction + usearch placeholder
- Embedder abstraction + API embedder config scaffold
- Context/pipeline placeholders
- CLI binary target scaffold (`nucklavee`)

## What Works Right Now

- The crate compiles.
- The CLI target builds and runs.
- The architecture boundaries are in place for incremental implementation.

## What Is Not Implemented Yet

- Actual parser/emitter logic
- Chunking algorithm implementation
- SQLite persistence implementation
- Vector index integration
- Embedding API calls
- End-to-end ingest/search/context flows

All unimplemented paths currently return explicit `not implemented` errors.

## Near-Term Implementation Order

1. IR validation and test harness utilities
2. Markdown parser + markdown emitter + roundtrip tests
3. HTML parser/emitter and cross-format tests
4. Store + chunking + embedder + vector index
5. CLI command wiring
6. PDF pipeline (iterative heuristics)


## Week 1 Tracking

- See `TODO.md` for the seven-item Week 1 IR finalization checklist.
- Item 1 (strict typed IR guardrails + controlled `GenericBlock` fallback) is now implemented at the model/validation layer.

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

## Reference Spec

- `nucklavee-spec.md` (source of truth for scope and behavior)
