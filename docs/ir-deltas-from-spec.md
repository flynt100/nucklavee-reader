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

`StructuralChunker` (`src/chunking/structural.rs`) implements this. Notable
behaviors and deltas:

- **Sections come from `section_path` equality.** Consecutive non-heading
  blocks with the same `section_path` form one section; a chunk never spans a
  section change (hence never crosses an H2 boundary). Sub-headings (deeper
  levels) also start new chunks — finer granularity than the spec's minimum,
  and still spec-valid.
- **Headings are boundaries, not content** (per spec §6.2): their text lives
  only in the `section_path` of the blocks beneath them, so a heading-only
  document (no body) produces zero chunks.
- **Tables and code blocks are their own chunks**, tagged `Table`/`Code`;
  everything else accumulates into `Prose`. Code chunk content is the raw code
  text (not the 4-space-indented plaintext-emitter form), since that is what
  should be embedded.
- **Budget splitting** uses tiktoken cl100k_base (bundled, offline): prose
  splits at block, then sentence, then a decode-by-token-window fallback that
  guarantees every chunk fits; code splits at blank lines; tables split by
  rows, re-prepending the header row to each part.

### Canonical stored IR (normalize-at-ingest)

`Library::ingest` runs `normalize_document` before storing, so the stored IR
is canonical regardless of source format (Markdown parser output was
previously un-normalized while HTML output was already tidy). This merges
adjacent `Inline::Text` runs and drops empty text nodes — whitespace-agnostic
and provenance-preserving — so the chunker and any other reader of stored
documents get consistent input. Documents written directly via
`DocumentStore::replace_document_projection` (bypassing ingest) are stored
as given.

## Storage / vector / embedder traits (spec §7)

Frozen in the 2026-07-06 trait-surface push (Task 1 of the audit plan), then
revised in the 2026-07-13 reliability pass (see
`docs/adr/0001-persistence-and-index-consistency.md`):

- `DocumentStore`: `get_document`, `find_by_content_hash`, `list_documents`,
  `remove_document` (also removes the document's chunks and embeddings),
  **`replace_document_projection(&Document, &[Chunk], &[Vec<f32>])`** — the
  single transactional write path (document + chunk set + embeddings change
  together or not at all; `validate_projection` rejects foreign chunks,
  duplicate/non-contiguous `sequence_index`, and count mismatches as
  `Error::Consistency`), `get_chunks_by_document` (**ordered by
  `sequence_index`** — both backends), `get_chunks_by_ids` (request order
  preserved, unknown IDs skipped), `get_all_embeddings` (feeds
  `Library::rebuild_index`). The former `upsert_document`/`insert_chunks`
  pair was removed — partial writes are no longer expressible.
- `VectorIndex`: `add` (replacement-safe), `remove`, `search`, `clear`,
  `save(&Path)`, `load(&Path)`. The index is a derived projection; the store
  is authoritative.
- `Embedder`: **blocking I/O** — implementations use `reqwest::blocking`;
  no tokio/async runtime in the MVP. Beyond the spec, the trait requires
  `fingerprint()` — a stable identity of the vector space the embedder
  produces (equal dimensions do not imply comparable vectors). It feeds the
  ingest `processing_fingerprint` and must never contain secrets.

### Backend implementations (Tasks 6 & 8) and their edge cases

- **`SqliteDocumentStore`** (`rusqlite`, `bundled`). Schema keeps the spec's
  queryable columns *plus* a `doc_json` column holding the full serialized IR,
  so cross-process `get_document`/`emit` works (the in-memory store cannot).
  Schema version is tracked in `PRAGMA user_version`.
  - *Discrepancy resolved:* `get_chunks_by_document` ordering was initially
    insertion-order in the memory store vs `sequence_index` in SQLite; the
    conformance suite caught it and both now sort by `sequence_index` (the
    documented contract).
  - *Dependency pin:* `rusqlite` is pinned to `0.32` — `0.40`'s
    `libsqlite3-sys` build script uses a nightly-only `cfg_select!` macro that
    does not compile on stable.
- **`UsearchIndex`** (`usearch`, cosine HNSW). **Key-width bridge:** usearch
  addresses vectors by `u64`, but `ChunkId` is a 128-bit UUID, so the index
  keeps a bidirectional `u64 ⇆ ChunkId` map and assigns sequential `u64` keys
  (the raw usearch file only stores `u64` keys). **Generation-manifest
  persistence:** `save` writes generation-stamped index + keymap artifacts
  (`<path>.g<gen>.usearch`, `<path>.g<gen>.keymap.json`) and then atomically
  replaces the small manifest at `<path>` naming the active generation;
  `load` verifies generation, dimension, and entry count across all three
  files and rejects mixed-generation combinations. `search` returns
  `(ChunkId, cosine_distance)` ascending (closest first).
- **`ApiEmbedder`** (OpenAI-compatible `/v1/embeddings`, `reqwest::blocking`).
  Request/response JSON is (de)serialized manually with `serde_json` because
  reqwest's `json` feature is disabled to keep the dependency surface small.
  The provider response is validated as a **complete permutation** of the
  inputs — index bounds, duplicate indexes, per-vector dimension, and finite
  values are all checked (count equality alone does not prove input↔vector
  correspondence). Provider error bodies quoted into diagnostics are bounded
  (`net::truncate_for_diagnostics`). `use_env_proxy` mirrors `crate::net` so
  tests hit a loopback mock directly. HTTP client construction is shared with
  URL fetching via `net::build_blocking_client` so timeout/proxy policy
  cannot drift between the two call sites.

## Parsers / emitters (spec §4–5)

- There is **no `Parser` trait and no `Emitter` trait**. Each format exposes a
  free function: `parsers::markdown::parse_markdown(input, ParseOptions)`,
  `parsers::html::parse_html(input, HtmlParseOptions)`,
  `emitters::{markdown::emit_markdown, html::emit_html, text::emit_text}`.
  Parse options are format-specific and the `Library` dispatches emission on
  the `Format` enum, so neither trait earned its keep. (The unused
  `MarkdownParser`/`HtmlParser` and `*Emitter` scaffold structs were removed
  in the Task 9 prune pass.) Re-introduce a trait only when a real
  cross-format abstraction is needed.
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

## Library / ingest + retrieval (spec §2, §7.3, §7.4)

- `Library<S, V, E>` is generic over store/index/embedder traits, not concrete
  types, and owns a concrete `StructuralChunker` (so `Library::new` is
  fallible — it loads the tokenizer).
- `ingest` runs the full pipeline **derive-first**: parse → validate →
  `normalize_document` → dedupe check → chunk → embed → *then* one
  transactional `replace_document_projection` → `index.add` per vector. All
  fallible work happens before any persistent write, so a failed ingest
  leaves nothing behind and retrying is always safe (see
  `docs/adr/0001-persistence-and-index-consistency.md`).
- Dedupe is two-level: `content_hash` (SHA-256 of the raw input, spec §7.4)
  detects identical bytes; `processing_fingerprint` detects identical
  *processing*. The fingerprint (`fp2`) hashes the whole interpretation
  chain: source format, parser policy version (`PARSER_POLICY_VERSION` per
  parser), normalization options, chunker+tokenizer version
  (`CHUNKER_VERSION`), token budget, and `Embedder::fingerprint()` +
  dimension. Hash hit + matching fingerprint returns the existing ID with no
  work; hash hit + different fingerprint (changed options, changed model,
  changed format, bumped policy version) reprocesses **in place** under the
  same `DocumentId` (provenance retagged; the store commits first, then the
  index is reconciled).
- `rebuild_index()` (CLI `rebuild-index`) repopulates the vector index from
  embeddings stored in SQLite — no re-embedding, no network.
- `query(text, limit)` embeds the query, `index.search`es, and joins the hit
  chunk IDs back through `store.get_chunks_by_ids` (rank order preserved).
- `context_window(query, budget)` retrieves up to `CONTEXT_SEARCH_K` (50)
  chunks and greedily packs them in rank order, each prefixed with a
  `[Source: {title} > {section}]` header. A chunk that does not fit is
  **skipped, not a stop condition** — later, smaller chunks may still fit
  (an oversized top hit cannot starve the window). Duplicate chunk IDs are
  packed once; a failed document-title lookup is an `Error::Consistency`,
  not silently "untitled" (that fallback is reserved for genuinely missing
  titles). Header + content tokens are counted with the chunker's
  tokenizer. **Edge case:** the section path's first element is usually the
  H1, which is also the document title, so the title is dropped from the path
  when they match — the header reads `[Source: Title > Section]`, not
  `[Source: Title > Title > Section]`.
- Embedder and vector-index **dimensions must match**; the `Library` does not
  enforce this at construction, so a caller wiring a real `ApiEmbedder` to a
  `UsearchIndex` must build the index with `embedder.dimension()` (otherwise
  `index.add` errors on the first chunk).
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
- Canonical error strings and helpers live in `src/contract.rs` (formerly
  `phase2_contract`; renamed and pruned in Task 10).

## CLI (spec §8)

The full command set is implemented (Task 10):
`ingest`/`search`/`emit`/`list`/`info`/`context`/`remove`/`rebuild-index`,
configured by a TOML file (`--config`, default `~/.config/forge/config.toml`)
that names the SQLite database, usearch index, and embedding endpoint.
`--json` gives machine-readable output. The vector index is persisted (as a
generation manifest + artifacts, see `UsearchIndex` above) after
`ingest`/`remove`/`rebuild-index`. `rebuild-index` never loads the existing
index files — it is the recovery path when they cannot load — and other
commands whose index load fails print an error pointing at it. See
`docs/cli.md`.
