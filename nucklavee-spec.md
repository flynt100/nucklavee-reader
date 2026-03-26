# Nucklavee: Universal Document Transformation Library

## Technical Specification v0.2

**Author:** Gray
**Date:** 2026-03-26
**Status:** Pre-implementation specification for agentic handoff
**Parent document:** forge-spec.md

---

## 1. Purpose

Nucklavee is a Rust library for universal document ingestion, normalization, chunking, embedding, and semantic search. It converts any supported input format (markdown, HTML, PDF) into a common intermediate representation, chunks it structure-aware with provenance metadata, embeds chunks for vector search, and provides ranked retrieval with a context window assembler designed for LLM consumption.

Named after the Orkney chimera — a skinless fusion of horse and rider — because it strips the surface formatting off disparate document formats and merges their structure into a single organism.

### 1.1 Design Philosophy

- **IR-first.** Every capability flows through the intermediate representation. Parsers produce it, emitters consume it, the chunker walks it. No feature bypasses the IR.
- **Provenance everywhere.** Every chunk knows which document it came from, which section hierarchy it sat under, and its position in the document. This enables citation, navigation, and trust.
- **PDF is a first-class citizen, not an afterthought.** The IR is designed with PDF's reconstruction uncertainty in mind. Confidence scoring and generic fallback nodes are structural, not bolted on.
- **Standalone utility.** Nucklavee ships its own CLI and is useful without the Forge application. It depends on nothing internal to the workspace.

---

## 2. Public API Surface

```rust
pub struct Library {
    store: DocumentStore,    // SQLite — metadata, chunks, raw content
    index: VectorIndex,      // HNSW embedding index
}

impl Library {
    /// Ingest a document from any supported source.
    /// Parses into IR, stores, chunks, embeds.
    pub fn ingest(&mut self, source: Source) -> Result<DocumentId>;

    /// Semantic search across all ingested documents.
    /// Returns ranked chunks with provenance metadata.
    pub fn query(&self, text: &str, limit: usize) -> Result<Vec<Chunk>>;

    /// Retrieve a full document by ID.
    pub fn get_document(&self, id: DocumentId) -> Result<Document>;

    /// Emit a document in the specified output format.
    pub fn emit(&self, id: DocumentId, format: Format) -> Result<String>;

    /// Assemble a context window from search results within a token budget.
    /// Ranked by relevance, formatted with provenance headers.
    /// Primary interface for LLM context injection.
    pub fn context_window(&self, query: &str, token_budget: usize) -> Result<String>;
}

pub enum Source {
    File(PathBuf),
    Url(String),
    RawMarkdown(String),
    RawHtml(String),
}

pub enum Format {
    Markdown,
    Html,
    PlainText,
}
```

---

## 3. Intermediate Representation

The IR is a tree of block-level and inline-level nodes with document metadata. All parsers produce this tree. All emitters consume it. The chunker walks it.

### 3.1 Document Metadata

```rust
pub struct Document {
    pub meta: DocumentMeta,
    pub body: Vec<Block>,
}

pub struct DocumentMeta {
    pub id: DocumentId,              // UUID
    pub source: Source,              // where it came from
    pub format: SourceFormat,        // which parser produced it
    pub title: Option<String>,       // extracted from first H1 or <title> or PDF metadata
    pub ingested_at: DateTime<Utc>,
    pub content_hash: String,        // SHA-256 for deduplication
}

pub type DocumentId = uuid::Uuid;

pub enum SourceFormat {
    Markdown,
    Html,
    Pdf,
}
```

### 3.2 Block Nodes

```rust
pub enum Block {
    Heading {
        level: u8,                   // 1–6
        content: Vec<Inline>,
    },
    Paragraph {
        content: Vec<Inline>,
    },
    CodeBlock {
        language: Option<String>,
        content: String,
    },
    Table {
        headers: Vec<Vec<Inline>>,           // each header cell contains inlines
        rows: Vec<Vec<Vec<Inline>>>,         // rows → cells → inlines
    },
    List {
        ordered: bool,
        items: Vec<ListItem>,
    },
    BlockQuote {
        children: Vec<Block>,
    },
    /// Catch-all for content that cannot be cleanly classified.
    /// Expected to be common with PDF and messy HTML ingestion.
    GenericBlock {
        content: Vec<Inline>,
        hint: Option<String>,        // best-guess classification (e.g., "table", "sidebar")
        confidence: f32,             // 0.0–1.0, parser's confidence in structural inference
    },
    ThematicBreak,
}

pub struct ListItem {
    pub content: Vec<Block>,         // items can contain paragraphs, nested lists, code blocks
}
```

### 3.3 Inline Nodes

```rust
pub enum Inline {
    Text(String),
    Styled {
        style: Style,
        children: Vec<Inline>,
    },
    Code(String),                    // inline code span
    Link {
        url: String,
        children: Vec<Inline>,
    },
    Image {
        url: String,
        alt: Option<String>,
    },
    LineBreak,
}

pub enum Style {
    Strong,
    Emphasis,
    Strikethrough,
}
```

### 3.4 IR Design Rationale

- **`GenericBlock` with `confidence`:** PDF reconstruction is uncertain. A parser might detect tabular spatial patterns but not be sure — it emits `GenericBlock { hint: Some("table"), confidence: 0.6 }`. Downstream consumers set their own confidence thresholds. This keeps the parser honest instead of forcing wrong classifications.

- **`Table` cells are `Vec<Inline>`, not `String`.** Datasheet tables contain bold part numbers, subscripted units, and inline symbols. Preserving inline formatting in cells is required for faithful emission.

- **`ListItem` contains `Vec<Block>`, not `Vec<Inline>`.** List items in real documents contain paragraphs, nested lists, and code blocks. Flattening to inlines loses structure.

- **No frontmatter or metadata block type.** Metadata lives on `DocumentMeta`, not in the content tree. YAML frontmatter and HTML `<meta>` tags are extracted into `DocumentMeta` during parsing.

- **`confidence` only on `GenericBlock`.** Typed blocks (Heading, Paragraph, Table, etc.) are emitted only when the parser is confident in the classification. If confidence is low, it's a `GenericBlock`. This keeps the type system meaningful — a `Block::Table` is always a real table.

---

## 4. Parsers

Each parser converts a source format into a `Document` (metadata + IR tree).

### 4.1 Markdown Parser

**Crate dependency:** `pulldown-cmark`

`pulldown-cmark` emits a flat SAX-style event stream — start/end tags interleaved with text events. The parser maintains a stack to build the tree.

**Algorithm:**

1. Walk the event stream.
2. On `Start(tag)`, push a new node context onto the stack.
3. On `Text(s)`, append an `Inline::Text` to the current stack frame.
4. On `End(tag)`, pop the stack frame, wrap its contents in the appropriate `Block` or `Inline` variant, and attach it to the parent frame.
5. Extract the first `Heading { level: 1 }` as the document title.

**Acceptance criteria:**

Parse a markdown file into IR, emit it back to markdown, parse the output again, and compare the two IR trees for structural equivalence. Whitespace differences are acceptable. Structural or semantic loss is a failure.

**Test fixtures must include:**

- Nested lists (3+ levels deep)
- Tables with inline formatting in cells (bold, code, links)
- Code blocks with and without language tags
- Blockquotes containing multiple block types
- Mixed heading levels (H1 through H4 minimum)
- Links and images with alt text
- Inline code adjacent to styled text
- Thematic breaks between sections
- Empty list items and empty paragraphs (edge cases)

### 4.2 HTML Parser

**Crate dependency:** `scraper` (built on `html5ever`)

HTML is noisier than markdown — navigation chrome, ads, scripts, and content are mixed. The parser must extract content before mapping structure.

**Algorithm:**

1. **Content extraction.** Readability-style heuristic: score DOM nodes by text density, strip `<nav>`, `<header>`, `<footer>`, `<aside>`, `<script>`, `<style>`. Select the highest-density content container.

2. **DOM-to-IR mapping:**

| HTML Element | IR Node |
|-------------|---------|
| `<h1>` – `<h6>` | `Block::Heading` |
| `<p>` | `Block::Paragraph` |
| `<pre><code>` | `Block::CodeBlock` (language from `class="language-*"`) |
| `<table>` | `Block::Table` |
| `<ul>`, `<ol>` | `Block::List` |
| `<blockquote>` | `Block::BlockQuote` |
| `<strong>`, `<b>` | `Inline::Styled { style: Strong }` |
| `<em>`, `<i>` | `Inline::Styled { style: Emphasis }` |
| `<a href>` | `Inline::Link` |
| `<img>` | `Inline::Image` |
| `<br>` | `Inline::LineBreak` |
| `<hr>` | `Block::ThematicBreak` |
| `<div>` (no semantic mapping) | Recurse into children, flatten the wrapper |
| Unclassifiable elements | `Block::GenericBlock` with `hint` from class names |

3. **Title extraction.** Check `<title>` tag, then `<h1>`, then `<meta property="og:title">`.

**URL ingestion path:** `reqwest` fetches the page. The HTML parser processes the response body. `Source::Url(url)` is preserved in document metadata.

### 4.3 PDF Parser

**Crate dependencies:** `pdf-extract`, `lopdf`

PDF parsing is a reconstruction problem. PDFs contain positioned glyphs on a page, not document structure. The parser must infer structure from spatial layout.

#### 4.3.1 Pipeline

**Stage 1: Text extraction.**
`pdf-extract` produces characters with x/y coordinates, font name, and font size. This is the raw material. Every subsequent stage operates on this positioned-glyph data.

**Stage 2: Line reconstruction.**
Group glyphs by y-coordinate within a tolerance threshold (default: half the modal line height). Sort each group by x-coordinate. Output: lines of text with bounding boxes (x_min, y_min, x_max, y_max) and font metadata.

**Stage 3: Column detection.**
Cluster line start x-offsets across the page. If two distinct clusters exist with a gap exceeding a threshold (e.g., 1.5× average word spacing), the page is multi-column. Process each column independently from this point. Column detection operates per-page-region, not per-page — datasheets commonly switch between single-column (tables) and multi-column (body text) on the same page.

**Stage 4: Heading detection.**
A line is classified as a heading when:
- Font size exceeds body text modal size by ≥ 1.2×, OR
- Font weight is bold AND the line is spatially isolated (no adjacent body text within 1.5× standard line spacing)

Heading level is inferred from font size relative to other headings in the document. The largest heading size maps to H1, next largest to H2, etc. Maximum 4 levels inferred.

**Stage 5: Table detection.**
Identify regions where text aligns to a consistent grid:
- **Column detection within table:** Find x-offsets that appear in ≥ 60% of consecutive lines within a spatial region. These are column boundaries.
- **Row detection:** Lines with consistent y-spacing within the table region, where text appears in ≥ 2 detected columns.
- **Header detection:** The first row(s) with bold font weight, or rows separated from the body by a horizontal rule (detected as a thin graphical element).
- **Cell content extraction:** For each row/column intersection, collect all glyphs within the cell bounding box.

Output: `Block::Table` if confidence is high (grid regularity score ≥ 0.7). `Block::GenericBlock { hint: Some("table"), confidence: <score> }` otherwise.

**Stage 6: Paragraph merging.**
Consecutive lines at the same font size, with standard line spacing (within 20% of modal spacing), and no structural break signals (heading, table, large vertical gap) are merged into paragraphs.

**Stage 7: Header/footer removal.**
Text appearing at consistent y-positions across ≥ 3 pages (typically containing page numbers, part numbers, document titles) is classified as header/footer and excluded from body content. Stored in document metadata if useful.

**Stage 8: Assembly.**
Assemble detected blocks (headings, paragraphs, tables, generic blocks) into the IR tree in reading order (top-to-bottom, left-to-right within columns).

#### 4.3.2 Confidence Scoring

Every structural inference produces a confidence score:
- Headings detected by font size change: 0.8–0.9
- Headings detected by spatial isolation alone: 0.5–0.7
- Tables with clear grid alignment: 0.8–0.95
- Tables inferred from x-offset regularity alone: 0.4–0.7
- Column detection with clear gap: 0.85–0.95
- Column detection with narrow gap: 0.5–0.7

Only `GenericBlock` carries the confidence field in the IR. Typed blocks are only emitted when confidence exceeds a configurable threshold (default: 0.7). Below threshold, content falls back to `GenericBlock`.

#### 4.3.3 Known Hard Cases (Datasheets)

These are explicitly expected failure modes to track and iterate on:

- **Pinout tables** with merged cells and symbol columns. Often adjacent to graphical pin diagrams. The parser should detect and skip graphical regions (clusters of very short text fragments at irregular positions).
- **Absolute maximum ratings tables** with footnote references (superscript numbers/symbols). Footnotes should be associated with the table via spatial proximity (within 2× line spacing below the table), not parsed as independent paragraphs.
- **Multi-column body text** where column width varies between sections. Column detection must re-evaluate at vertical region boundaries.
- **Electrical characteristics tables** with merged header rows spanning multiple sub-columns (e.g., "Condition" spanning "Min", "Typ", "Max"). Detection requires identifying merged cells via text width relative to column width.
- **Part number variations** in headers/footers that change per-page. The header/footer detector must use fuzzy matching, not exact string matching.

#### 4.3.4 PDF Title Extraction

Check in order: PDF metadata `Title` field → first detected H1-level heading → filename without extension.

#### 4.3.5 MVP Scope

Get lines, paragraphs, and headings working reliably first. Table detection is second priority. Confidence scoring throughout from the start. Accept that some documents will produce mostly `GenericBlock` output initially — improving accuracy against a real datasheet corpus is an iterative process, not a one-shot implementation.

---

## 5. Emitters

Each emitter walks the IR tree and produces an output format. Emitters are recursive pattern-match tree walks.

### 5.1 Markdown Emitter

| IR Node | Output |
|---------|--------|
| `Heading { level, content }` | `"#".repeat(level)` + ` ` + emit inlines + `\n\n` |
| `Paragraph { content }` | emit inlines + `\n\n` |
| `CodeBlock { language, content }` | `` ``` `` + language + `\n` + content + `\n``` \n\n` |
| `List { ordered: false, items }` | each item: `- ` + emit item blocks (indented for nesting) |
| `List { ordered: true, items }` | each item: `N. ` + emit item blocks (indented for nesting) |
| `BlockQuote { children }` | each child block: prefix every line with `> ` |
| `Table` | pipe-delimited markdown table with alignment row |
| `GenericBlock { content }` | emit inlines (best-effort, lossy for low-confidence blocks) |
| `ThematicBreak` | `---\n\n` |
| `Inline::Text(s)` | write s |
| `Inline::Styled { Strong }` | `**` + emit children + `**` |
| `Inline::Styled { Emphasis }` | `*` + emit children + `*` |
| `Inline::Styled { Strikethrough }` | `~~` + emit children + `~~` |
| `Inline::Code(s)` | `` ` `` + s + `` ` `` |
| `Inline::Link { url, children }` | `[` + emit children + `](` + url + `)` |
| `Inline::Image { url, alt }` | `![` + alt + `](` + url + `)` |
| `Inline::LineBreak` | two trailing spaces + `\n` |

### 5.2 HTML Emitter

Same tree walk, HTML output tokens:

| IR Node | Output |
|---------|--------|
| `Heading { level, content }` | `<hN>` + emit inlines + `</hN>` |
| `Paragraph` | `<p>` + emit inlines + `</p>` |
| `CodeBlock { language, content }` | `<pre><code class="language-{lang}">` + escaped content + `</code></pre>` |
| `Table` | `<table>` with `<thead>`, `<tbody>`, `<tr>`, `<th>`, `<td>` |
| `List { ordered: false }` | `<ul>` with `<li>` items |
| `List { ordered: true }` | `<ol>` with `<li>` items |
| `BlockQuote` | `<blockquote>` + emit children + `</blockquote>` |
| `GenericBlock` | `<div class="generic-block">` + emit inlines + `</div>` |
| `ThematicBreak` | `<hr>` |
| `Inline::Styled { Strong }` | `<strong>` |
| `Inline::Styled { Emphasis }` | `<em>` |
| `Inline::Link` | `<a href="...">` |
| `Inline::Image` | `<img src="..." alt="...">` |
| `Inline::LineBreak` | `<br>` |

HTML content must be escaped (angle brackets, ampersands, quotes in attributes).

### 5.3 PlainText Emitter

Strip all formatting. Used primarily for embedding input and context window assembly.

- Headings: UPPERCASED, followed by blank line.
- Code blocks: indented 4 spaces.
- Lists: indented with `- ` or `N. `.
- Links: `text (url)`.
- Block quotes: prefixed with `| `.
- Tables: pipe-delimited, no alignment row.
- All inline styling stripped — just the text content.

---

## 6. Chunker

The chunker walks the IR tree and produces embedding-ready segments with provenance.

### 6.1 Chunk Structure

```rust
pub struct Chunk {
    pub id: ChunkId,
    pub document_id: DocumentId,
    pub section_path: Vec<String>,     // breadcrumb of ancestor heading texts
    pub content: String,               // plain text for embedding
    pub block_type: ChunkBlockType,
    pub sequence_index: usize,         // position within document
    pub token_count: usize,
}

pub enum ChunkBlockType {
    Prose,
    Code,
    Table,
}

pub type ChunkId = uuid::Uuid;
```

### 6.2 Algorithm

1. Walk the IR tree, maintaining a stack of heading texts as the current `section_path`.
2. On encountering a `Heading`, update the path — pop to the appropriate level, push the new heading text.
3. Accumulate non-heading blocks under the current section path.
4. When a new heading of equal or higher level is encountered, or end-of-document is reached, flush the accumulated blocks as a chunk (or multiple chunks if they exceed the token budget).

### 6.3 Splitting Rules

- **Token budget per chunk:** configurable, default 512 tokens. Counted with `tiktoken-rs` using cl100k_base encoding (matches OpenAI embeddings; configurable for other encodings).
- **Oversized sections:** Split at paragraph boundaries first. If a single paragraph exceeds the budget, split at sentence boundaries (`. ` followed by uppercase letter or newline). Sentence splitting is the last resort.
- **Code blocks:** Keep intact if under budget. If a single code block exceeds budget, split at blank line boundaries within the code. Tag as `ChunkBlockType::Code`.
- **Tables:** Each table is its own chunk. If a table exceeds budget, split by rows with headers prepended to each chunk. Tag as `ChunkBlockType::Table`.
- **GenericBlocks:** Treated as prose for chunking purposes. Low-confidence blocks are chunked normally — confidence is metadata, not a chunking signal.

### 6.4 Provenance

Every chunk carries `section_path` and `document_id`. When retrieved via search, the consumer knows: which document, which section hierarchy, what position in the document. The LLM can cite sources. The user can navigate to the original.

---

## 7. Embedding and Vector Storage

### 7.1 Embedding Pipeline

**MVP:** External embedding API via `reqwest`. Support OpenAI-compatible API (covers OpenAI, local `llama.cpp` with `--embedding`, and most third-party embedding services).

**Future:** Local inference via `ort` (ONNX Runtime) or `candle` (Hugging Face pure-Rust ML). Deferred — API interface is identical regardless of backend.

```rust
pub trait Embedder: Send + Sync {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
    fn dimension(&self) -> usize;
}

pub struct ApiEmbedder {
    client: reqwest::Client,
    endpoint: String,
    model: String,
    api_key: Option<String>,    // None for local servers
}

impl Embedder for ApiEmbedder { /* ... */ }
```

### 7.2 Vector Storage

**Document and chunk storage:** `rusqlite`.

**Vector index:** `usearch` crate for HNSW approximate nearest neighbor search. Single index file alongside the SQLite database.

**Schema:**

```sql
CREATE TABLE documents (
    id TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    source_format TEXT NOT NULL,
    title TEXT,
    ingested_at TEXT NOT NULL,
    content_hash TEXT NOT NULL UNIQUE
);

CREATE TABLE chunks (
    id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    section_path TEXT NOT NULL,        -- JSON array of heading strings
    content TEXT NOT NULL,
    block_type TEXT NOT NULL,          -- "prose", "code", "table"
    sequence_index INTEGER NOT NULL,
    token_count INTEGER NOT NULL
);

CREATE INDEX idx_chunks_document ON chunks(document_id);
CREATE INDEX idx_chunks_block_type ON chunks(block_type);
```

Vector index managed by `usearch` keyed by chunk ID. On search: query `usearch` for top-k chunk IDs by vector similarity, join against SQLite for metadata and content.

### 7.3 Context Window Assembly

```rust
impl Library {
    pub fn context_window(&self, query: &str, token_budget: usize) -> Result<String> {
        // 1. Embed the query.
        // 2. Retrieve top-k chunks via vector search (k generous, e.g., 50).
        // 3. Rank by similarity score.
        // 4. Greedily pack chunks into the budget:
        //    - Add chunks in ranked order until budget is exhausted.
        //    - Each chunk formatted with provenance header:
        //      "[Source: {document_title} > {section_path joined by " > "}]\n{content}\n"
        //    - Token count includes header overhead.
        // 5. Return assembled string.
    }
}
```

This method is the primary interface between Nucklavee and the session layer. The LLM receives pre-assembled, provenanced, relevance-ranked context without knowing anything about library internals.

### 7.4 Deduplication

On `ingest`, compute SHA-256 of the raw source content. If `content_hash` already exists in `documents`, return the existing `DocumentId` instead of re-ingesting. This prevents duplicate embeddings when the same file or URL is ingested multiple times.

---

## 8. CLI Interface

Nucklavee ships its own binary target for standalone use:

```
nucklavee ingest <path_or_url>           # Ingest a file or URL
nucklavee search <query> [--limit N]     # Semantic search, print ranked chunks with provenance
nucklavee emit <document_id> <format>    # Emit a document as markdown/html/text
nucklavee list                           # List all ingested documents with IDs and titles
nucklavee info <document_id>             # Show document metadata, chunk count, source
nucklavee context <query> [--budget N]   # Assemble and print a context window
nucklavee remove <document_id>           # Remove a document and its chunks from the library
```

All commands read config from `~/.config/forge/config.toml` for database path, embedding endpoint, etc. The CLI is the primary testing and debugging interface during development.

---

## 9. Crate Dependencies

| Crate | Purpose |
|-------|---------|
| `pulldown-cmark` | Markdown event stream parser |
| `scraper` | HTML DOM parsing (built on `html5ever`) |
| `lol_html` | Streaming HTML rewriting (content extraction heuristics) |
| `pdf-extract` | PDF text extraction with glyph positions |
| `lopdf` | Low-level PDF object access |
| `rusqlite` | Document and chunk storage |
| `usearch` | HNSW vector index |
| `reqwest` | HTTP client (URL ingestion + embedding API) |
| `serde` / `serde_json` | Serialization |
| `tiktoken-rs` | Token counting |
| `uuid` | Document and chunk IDs |
| `chrono` | Timestamps |
| `sha2` | Content hashing for deduplication |
| `clap` | CLI argument parsing |
| `tokio` | Async runtime (for reqwest) |

---

## 10. Build Sequence

### Phase 1: IR + Markdown Roundtrip (Week 1)

**Goal:** Prove the IR design with fast feedback cycles.

- Define all IR types: `Document`, `DocumentMeta`, `Block`, `Inline`, `Style`, `ListItem`.
- Implement the markdown parser using `pulldown-cmark`.
- Implement the markdown emitter.
- Write roundtrip tests: parse → emit → parse → compare IR trees structurally.
- Run against test fixtures covering all block and inline types.

**Exit criteria:** All roundtrip tests pass. No structural or semantic loss on any fixture.

### Phase 2: HTML + Cross-Format (Week 2)

**Goal:** Validate the IR is format-agnostic.

- Implement the HTML parser with content extraction heuristics.
- Implement the HTML emitter.
- Cross-format roundtrip tests: markdown → IR → HTML → IR → compare.
- Test against 5–10 real web pages (documentation sites, Wikipedia articles, blog posts).

**Exit criteria:** Cross-format emission preserves all structure. Content extraction correctly strips chrome from test pages.

### Phase 3: Storage + Chunking + Embedding (Week 2–3)

**Goal:** End-to-end pipeline from ingestion to search.

- Implement `DocumentStore` with `rusqlite`.
- Implement the structure-aware chunker.
- Implement the `Embedder` trait and `ApiEmbedder`.
- Implement `VectorIndex` with `usearch`.
- Implement `context_window` assembly.
- End-to-end test: ingest a set of markdown files → chunk → embed → search → verify expected chunks rank in top-k.
- Ship the CLI.

**Exit criteria:** `nucklavee search` returns relevant, provenanced results on a test corpus. `nucklavee context` assembles a valid context window within budget.

### Phase 4: PDF Parser — Foundations (Week 3–4)

**Goal:** Get text, paragraphs, and headings out of real PDFs.

- Implement text extraction with `pdf-extract`.
- Implement line reconstruction from positioned glyphs.
- Implement heading detection by font size.
- Implement paragraph merging.
- Test against a corpus of 5–10 real component datasheets.
- Measure: what percentage of the document's text content is correctly extracted and structurally classified?

**Exit criteria:** ≥ 80% of body text correctly extracted as paragraphs. Headings detected with ≤ 20% false positive rate. Results ingest into the library and are searchable.

### Phase 5: PDF Parser — Tables + Hardening (Week 4–6)

**Goal:** Extract tabular data from datasheets.

- Implement column detection.
- Implement table detection heuristics.
- Implement header/footer removal.
- Implement confidence scoring throughout.
- Expand the datasheet corpus to 20+ documents across multiple manufacturers.
- Track accuracy metrics: heading detection rate, table detection rate, false positive rate, percentage of content classified as GenericBlock.

**Exit criteria:** Tables detected in ≥ 60% of datasheets that contain them. Extracted table content is correct in ≥ 70% of detected tables. These thresholds are MVP — accuracy improves iteratively post-launch.

---

## 11. Testing Strategy

### 11.1 Roundtrip Tests

Every parser/emitter pair has roundtrip tests. Parse → emit → parse → compare IR trees. This is the fundamental correctness guarantee. Tests run on every commit.

### 11.2 Chunking Tests

- Verify chunk boundaries respect heading structure (no chunk spans two H2 sections).
- Verify all chunks are within token budget.
- Verify provenance paths are correct (section_path matches the heading hierarchy above the chunk).
- Verify code and table block types are correctly tagged.

### 11.3 Search Quality Tests

Ingest a known corpus. Run a set of known queries. Verify expected chunks rank in top-k. This is a regression suite — if search quality degrades after a change, the test catches it.

### 11.4 PDF Regression Corpus

Maintain a directory of datasheets with manually annotated expected structure:
- Expected headings (text and level).
- Expected tables (row count, column count, sample cell values).
- Expected paragraph count.

Run the parser against the corpus and compare output to annotations. Track accuracy metrics over time. This corpus grows as new failure modes are discovered.

---

## 12. Open Questions

1. **Embedding model for technical content.** `text-embedding-3-small` is the default, but may underperform on datasheet content (part numbers, electrical units, terse specification language). Needs evaluation against `nomic-embed-text`, `bge-large`, and `jina-embeddings-v3` on a representative query set.

2. **PDF accuracy floor for MVP.** The spec sets ≥ 60% table detection and ≥ 70% extraction accuracy as MVP thresholds. Are these appropriate, or should MVP accept lower accuracy with a clear improvement roadmap?

3. **Image handling in PDFs.** Datasheets contain pinout diagrams, block diagrams, and timing diagrams that carry critical information. The current spec ignores images entirely. Future work: extract images, store them, and either OCR them or pass them to a vision model for description.

4. **Incremental re-ingestion.** If a source file changes, should `ingest` detect the change (via content hash) and update the existing document, or require explicit removal and re-ingestion? Update-in-place is more convenient but complicates chunk ID stability.

5. **Chunk overlap.** Some RAG systems use overlapping chunks (e.g., 50-token overlap between adjacent chunks) to avoid losing context at boundaries. The current spec uses non-overlapping chunks with heading context in `section_path`. Evaluate whether overlap improves search quality on the test corpus.
