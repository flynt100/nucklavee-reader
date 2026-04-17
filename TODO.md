# Phase 1 Status

Phase 1 delivers a trustworthy vertical slice: IR + Markdown roundtrip +
block-level provenance + explicit diagnostics for lossy/unsupported cases.

## Done (Phase 1)

- IR core: `Document`, `DocumentMeta`, `BlockNode`, `Block`, `Inline`,
  `Style`, `ListItem` with controlled `GenericBlock` fallback.
- Provenance model: `Provenance { document_id, section_path, byte_range }`
  attached to every `BlockNode`. Byte ranges come from
  `pulldown-cmark`'s offset iterator.
- Diagnostics: typed `Diagnostic { kind, message, byte_range }` with
  `Unsupported`, `Lossy`, `Normalized` variants; collected on `Document`.
- Validator: heading-level range, generic-block confidence range, table
  row shape, non-empty list, empty link/image URL rejection, provenance
  range in-bounds / non-inverted checks.
- Normalization: adjacent text-node merge, empty text drop.
- Structural equivalence helpers (`structurally_equivalent`,
  `structural_diff`) used by the roundtrip harness.
- Markdown parser (Phase 1 subset): headings, paragraphs, emphasis,
  strong, strikethrough, links, images, inline code, fenced code with
  optional language, unordered/ordered/nested lists, blockquotes,
  tables, thematic breaks, hard line breaks.
- Markdown emitter: deterministic canonical output.
- Title extraction from first H1.
- Fixture-driven roundtrip harness with readable failure output.

## Known Phase 1 limitations (explicit, not silent)

- Raw HTML (`Event::Html`, `Event::InlineHtml`) is skipped with an
  `Unsupported` diagnostic.
- Task-list markers and footnotes are not supported; both produce
  `Unsupported` diagnostics.
- Image alt text is preserved as plain text only. If the source puts
  inline formatting inside image alt, a `Lossy` diagnostic is produced.
- Soft line breaks are normalized to a single space inside paragraph
  text (standard CommonMark rendering).
- Provenance granularity is block-level. Inline-level byte ranges are
  not recorded.
- Emitted Markdown is normalized, not source-identical. Roundtrip is
  verified structurally, not byte-for-byte.
- Indented (non-fenced) code blocks are parsed but always emitted back
  as fenced code. This is a deliberate normalization.

## Deferred (future phases; explicitly out of scope)

- HTML / PDF parsers and emitters
- Chunking, storage, embedding, vector search, context assembly
- CLI beyond the existing scaffold
- Inline-level provenance
