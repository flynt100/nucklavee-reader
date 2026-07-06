# IR / API Deltas from `nucklavee-spec.md` (v0.2)

The spec is the source of truth for *scope and intent*, but the implementation
has deliberately diverged from its literal type sketches. Build agents must
code against **this document plus the actual source**, not the spec's Rust
snippets. Spec sections referenced below.

## Document / IR types (spec §3)

| Spec | Implementation | Why |
|---|---|---|
| `Document { meta, body: Vec<Block> }` | `Document { meta, body: Vec<BlockNode>, diagnostics: Vec<Diagnostic> }` (`src/ir/types.rs`) | Block-level provenance and non-fatal parse diagnostics are Phase-1 guarantees. |
| No provenance type | `BlockNode { block: Block, prov: Provenance }`; `Provenance { document_id, section_path, byte_range }` (`src/ir/provenance.rs`) | "Provenance everywhere" is structural, recorded at parse time, not derived later. |
| No diagnostics | `Diagnostic { kind: Unsupported \| Lossy \| Normalized, message, byte_range }` (`src/ir/diagnostic.rs`) | Parsers never fail on unsupported constructs; they degrade with a typed record. |
| §3.4: "No frontmatter or metadata block type" | `DocumentMeta.frontmatter: Option<Frontmatter>` holding the raw YAML payload | Spec intent preserved (metadata is not in the body tree), but the YAML is retained verbatim so markdown emission can round-trip it. |
| `DocumentMeta.source: Source` | `DocumentMeta.source: SourceInfo { raw_source: String }` | `Source` holds owned input payloads (`RawMarkdown(String)`); metadata stores only a descriptor. |
| `ListItem { content: Vec<Block> }` | `ListItem { content: Vec<BlockNode> }` | Provenance wrapping applies inside list items too. |
| — | `Style`, `Inline`, `Block` variants otherwise match the spec | |

## Chunker (spec §6)

Spec sketches `Chunk` fields (matching `src/chunking/mod.rs`) but the trait is:

```rust
pub trait Chunker {
    fn chunk(&self, document: &Document, opts: &ChunkOptions) -> Result<Vec<Chunk>>;
}
```

The chunker walks the IR (`Document.body`), reusing `BlockNode.prov.section_path`
computed at parse time — it does **not** take flat body text and must not
re-derive heading hierarchies.

## Storage / vector / embedder traits (spec §7)

Frozen in the 2026-07-06 trait-surface push (Task 1 of the audit plan):

- `DocumentStore`: `upsert_document`, `get_document`, `find_by_content_hash`,
  `list_documents`, `remove_document` (also removes the document's chunks),
  `insert_chunks`, `get_chunks_by_document`, `get_chunks_by_ids`.
- `VectorIndex`: `add`, `remove`, `search`, `save(&Path)`, `load(&Path)`.
- `Embedder`: unchanged from spec, **blocking I/O** — implementations use
  `reqwest::blocking`; no tokio/async runtime in the MVP.

## Parsers / emitters (spec §4–5)

- There is **no `Parser` trait**. Each format exposes free functions and/or an
  inherent-method struct (`parsers::markdown::parse_markdown(input, ParseOptions)`)
  because parse options are format-specific. Re-introduce a trait only when a
  cross-format abstraction is actually needed.
- `Emitter` trait exists (`emit(&self, &Document) -> Result<String>`).
- The markdown parser applies markdown-specific input-repair heuristics
  (escaped-newline decode, math shielding, unfenced-frontmatter inference,
  TSV-paragraph promotion, bare-callout normalization — see
  `src/parsers/markdown/`). These are **not** part of the generic parse
  contract; HTML/PDF parsers must not import or replicate them.

## Provenance coordinates

`Provenance.byte_range` and `Diagnostic.byte_range` are **original-source byte
offsets**, even when the parser internally rewrites the input (escaped-newline
decode, math shielding). The parser maintains offset maps and remaps all
ranges back before returning the `Document`.

## Library / ingest (spec §2, §7.4)

- `Library<S, V, E>` is generic over store/index/embedder traits, not concrete
  types.
- `ingest` computes `content_hash` (SHA-256 of the raw input) and returns the
  existing `DocumentId` when the hash is already present in the store
  (spec §7.4 dedupe) instead of re-ingesting.
- Phase-boundary behavior (which formats/commands are enabled) is governed by
  `src/phase2_contract.rs` + `docs/cli-phase2-boundary.md`, updated per phase.

## CLI (spec §8)

The spec's command set (`ingest`/`search`/`emit`/`list`/`info`/`context`/`remove`)
is the Phase-5 target. Until then the CLI is the narrow Phase-2 boundary
documented in `docs/cli-phase2-boundary.md`.
