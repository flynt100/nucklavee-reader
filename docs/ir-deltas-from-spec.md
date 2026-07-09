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
  inherent-method struct (`parsers::markdown::parse_markdown(input, ParseOptions)`,
  `parsers::html::parse_html(input, HtmlParseOptions)`) because parse options
  are format-specific. Re-introduce a trait only when a cross-format
  abstraction is actually needed.
- Shared parser helpers live in `parsers/mod.rs`: `sha256_hex`,
  `inlines_to_plain`, `GENERIC_BLOCK_DEFAULT_CONFIDENCE`, and
  `SectionPathTracker` (the heading→section-path algorithm used by both
  parsers — do not re-implement it per format).
- `Emitter` trait exists (`emit(&self, &Document) -> Result<String>`).
- The HTML emitter (`emitters::html::emit_html`) produces a **fragment**
  (block sequence, no `<html>`/`<body>` wrapper) so it can be re-ingested and
  embedded. Style mapping is the inverse of the parser's:
  Strong→`<strong>`, Emphasis→`<em>`, Strikethrough→`<del>`. This keeps
  markdown → IR → html → IR structurally stable.
- Cross-format equivalence (either direction) is asserted with
  `ir::structural_diff_bodies`, which compares normalized block trees and
  ignores `meta` (format/title/frontmatter differ legitimately across
  formats). Whitespace-heavy text (e.g. soft-break newlines inside markdown
  blockquotes) is the known "not semantically valid" edge that HTML's
  whitespace collapsing does not preserve; cross-format fixtures avoid it.
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

**HTML documents carry `section_path` provenance only** — `byte_range` is
`None` because html5ever does not expose source offsets. Downstream code must
treat `byte_range` as optional (it already is in the type).

## HTML parser specifics (Phase 3, spec §4.2)

- Entry point: `parsers::html::parse_html(input, HtmlParseOptions)`; never
  fails, degrades with diagnostics (same philosophy as markdown).
- Content extraction: chrome skip-list (`nav`, `header`, `footer`, `aside`,
  `script`, `style`, forms, embeds) + densest
  `<main>`/`<article>`/`[role=main]` shortcut + 80% density-descent fallback.
  `HtmlParseOptions.extract_content = false` maps the whole `<body>`.
- Tables are shape-normalized to satisfy IR validation: a leading all-`<th>`
  row or `<thead>` row becomes the header, headerless tables promote their
  first row (Normalized diagnostic), ragged rows are padded with empty cells
  (Normalized diagnostic), colspan/rowspan flattening is Lossy.
- Unclassified elements: transparent flatten when they contain block
  structure, else `GenericBlock { hint: class names or tag, confidence: 0.5 }`
  — with per-tag deduplicated `Unsupported` diagnostics.
- Cross-format comparisons use `ir::structural_diff_bodies` (body-only diff
  that ignores `meta.format`/`title`/`frontmatter`).

## Library / ingest (spec §2, §7.4)

- `Library<S, V, E>` is generic over store/index/embedder traits, not concrete
  types.
- `ingest` computes `content_hash` (SHA-256 of the raw input) and returns the
  existing `DocumentId` when the hash is already present in the store
  (spec §7.4 dedupe) instead of re-ingesting.
- `Source::Url` ingest goes through `src/net` (blocking `reqwest`): fetch,
  follow redirects, then choose the parser by `Content-Type`
  (markdown/html/xml), falling back to the URL path extension and then a
  leading-`<` body sniff, defaulting to HTML. The post-redirect final URL is
  stored as `DocumentMeta.source.raw_source`. Failures (build/connect/status/
  body) surface as `Error::Network`.
- **Proxy/TLS caveat:** `net::fetch` honors `HTTP(S)_PROXY`/`NO_PROXY`. In a
  sandbox whose egress goes through an intercepting HTTPS proxy with a custom
  CA, live `https://` fetches may need that CA trusted by the process; the
  loopback tests avoid this by using plain HTTP with `use_env_proxy: false`.
- Phase-boundary behavior (which formats/commands are enabled) is governed by
  `src/phase2_contract.rs` + `docs/cli-phase2-boundary.md`, updated per phase.

## CLI (spec §8)

The spec's command set (`ingest`/`search`/`emit`/`list`/`info`/`context`/`remove`)
is the Phase-5 target. Until then the CLI is the narrow Phase-2 boundary
documented in `docs/cli-phase2-boundary.md`.
