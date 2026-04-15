# Implementation TODO

## Week 1 — IR finalization

1. [x] **Strict typed IR with controlled `GenericBlock` fallback.** `Document::validate_strict()` enforces heading level range, non-empty inline content on headings/paragraphs, and `GenericBlock.confidence` in 0.0..=1.0. Called on every parser output.
2. [ ] **Normalized source metadata (`SourceInfo`) suitable for storage.** Currently a single `raw_source: String`. Needs at minimum a discriminator (file / url / raw) and enough to drive dedupe and re-ingest decisions.
3. [ ] UUID v4 IDs + content-hash dedupe flow contract. (Parser populates `content_hash`; dedupe happens in `Library::ingest` once storage exists.)
4. [x] **Normalized semantic equality for roundtrip tests.** `Document::structural_eq` + whitespace-collapsing `inlines_eq`. Drives `tests/markdown_roundtrip.rs`.
5. [ ] Inline canonicalization pass (merge adjacent text nodes on parse, not just at compare time).
6. [ ] Table cell fidelity invariants/tests (rectangular rows, header/body column-count agreement).
7. [x] **Typed error model across module traits.** `Result<T>` alias + `Error::{Parse, Emit, Chunking, Storage, VectorIndex, Embedding}`.

## Week 1 exit

Markdown parser, markdown emitter, and roundtrip tests land together. Status: parser and emitter implemented against `pulldown-cmark` 0.12; 8 fixture roundtrips green; `Library<S: DocumentStore>` wires `ingest`/`get_document`/`emit` end-to-end against `InMemoryDocumentStore`; dedupe by content hash is live.

## Next

- Week 1 item 2: rename `SourceInfo.raw_source` and give it enough structure for real dedupe/re-ingest decisions.
- Week 1 item 5: inline canonicalization during parse (merge adjacent text nodes at build time, not just at compare time).
- Start Week 2: HTML parser + emitter + cross-format roundtrip.
