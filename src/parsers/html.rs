//! HTML parser built on `scraper` (html5ever).
//!
//! Two stages, per spec §4.2:
//!
//! 1. **Content extraction.** Chrome elements (`nav`, `header`, `footer`,
//!    `aside`, `script`, `style`, …) are skipped everywhere, and the walk is
//!    rooted at the best content container: the densest `<main>` /
//!    `<article>` / `[role=main]` when one exists, otherwise a
//!    density-descent from `<body>` into the child container holding most of
//!    the text.
//! 2. **DOM→IR mapping.** Semantic elements map to typed blocks/inlines;
//!    wrapper elements (`div`, `section`, `span`, …) are transparent;
//!    unclassifiable elements degrade to `GenericBlock` with a class-name
//!    hint plus an `Unsupported` diagnostic (deduplicated per tag).
//!
//! Like the markdown parser, `parse_html` never fails — malformed or
//! unsupported constructs produce diagnostics, not errors.
//!
//! Provenance: HTML blocks carry `section_path` provenance only;
//! `byte_range` is `None` because html5ever does not expose source offsets
//! (see `docs/ir-deltas-from-spec.md`).

use std::collections::HashSet;

use chrono::Utc;
use scraper::{ElementRef, Html, Node, Selector};
use uuid::Uuid;

use crate::ir::{
    Block, BlockNode, Diagnostic, DiagnosticKind, Document, DocumentId, DocumentMeta, Inline,
    ListItem, Provenance, SourceFormat, SourceInfo, Style,
};
use crate::parsers::{
    GENERIC_BLOCK_DEFAULT_CONFIDENCE, SectionPathTracker, inlines_to_plain, sha256_hex,
};

/// Version of this parser's interpretation policy (content extraction,
/// DOM→IR mapping). Bump on any change that makes previously-ingested HTML
/// parse differently — it feeds the ingest `processing_fingerprint`, so a
/// bump triggers reprocessing instead of reusing a stale interpretation.
pub const PARSER_POLICY_VERSION: &str = "html1";

#[derive(Debug, Clone)]
pub struct HtmlParseOptions {
    /// Optional human-readable source descriptor (path, url, etc.) to attach
    /// to `DocumentMeta.source.raw_source`.
    pub source_descriptor: Option<String>,
    /// Run readability-style content extraction before mapping (default
    /// true). Disable to map the full `<body>` as-is (chrome elements are
    /// still skipped).
    pub extract_content: bool,
}

impl Default for HtmlParseOptions {
    fn default() -> Self {
        Self {
            source_descriptor: None,
            extract_content: true,
        }
    }
}

/// Elements whose entire subtree is never content.
const SKIP_TAGS: &[&str] = &[
    "script", "style", "noscript", "template", "nav", "header", "footer", "aside", "form",
    "iframe", "svg", "canvas", "button", "dialog", "select", "input", "textarea",
];

/// Wrapper elements that are structurally transparent at block level.
const TRANSPARENT_BLOCK_TAGS: &[&str] = &[
    "div",
    "section",
    "article",
    "main",
    "body",
    "html",
    "figure",
    "figcaption",
    "details",
    "summary",
    "dl",
    "hgroup",
];

/// Container tags the density-descent heuristic is allowed to descend into.
const DESCENT_TAGS: &[&str] = &["div", "section", "article", "main", "body"];

/// Parse an HTML string into a [`Document`]. Never fails — unsupported or
/// malformed constructs produce diagnostics.
pub fn parse_html(input: &str, opts: HtmlParseOptions) -> Document {
    let dom = Html::parse_document(input);
    let id: DocumentId = Uuid::new_v4();

    let mut ctx = Ctx {
        doc_id: id,
        section: SectionPathTracker::new(),
        diagnostics: Vec::new(),
        reported: HashSet::new(),
    };

    let root = content_root(&dom, opts.extract_content, &mut ctx);
    let body = match root {
        Some(root) => walk_blocks(root, &mut ctx),
        None => Vec::new(),
    };

    let title = extract_html_title(&dom, &body);

    let meta = DocumentMeta {
        id,
        source: SourceInfo {
            raw_source: opts
                .source_descriptor
                .unwrap_or_else(|| "raw:html".to_string()),
        },
        format: SourceFormat::Html,
        title,
        frontmatter: None,
        ingested_at: Utc::now(),
        content_hash: sha256_hex(input),
        processing_fingerprint: String::new(),
    };

    let mut doc = Document::new(meta);
    doc.body = body;
    doc.diagnostics = ctx.diagnostics;
    doc
}

struct Ctx {
    doc_id: DocumentId,
    section: SectionPathTracker,
    diagnostics: Vec<Diagnostic>,
    /// Dedupe key set so per-tag diagnostics are reported once per document.
    reported: HashSet<String>,
}

impl Ctx {
    /// Provenance for a non-heading block: the section it sits under.
    fn prov(&self) -> Provenance {
        Provenance::new(self.doc_id).with_section_path(self.section.current())
    }

    fn report_once(&mut self, key: String, kind: DiagnosticKind, message: String) {
        if self.reported.insert(key) {
            self.diagnostics.push(Diagnostic::new(kind, message));
        }
    }
}

// --- title -------------------------------------------------------------

fn extract_html_title(dom: &Html, body: &[BlockNode]) -> Option<String> {
    // Spec §4.2 order: <title> → first <h1> → <meta property="og:title">.
    let title_sel = Selector::parse("title").expect("static selector");
    if let Some(el) = dom.select(&title_sel).next() {
        let text = collapse_whitespace(&el.text().collect::<String>());
        let text = text.trim();
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }

    for node in body {
        if let Block::Heading { level: 1, content } = &node.block {
            let text = inlines_to_plain(content);
            if !text.trim().is_empty() {
                return Some(text.trim().to_string());
            }
        }
    }

    let og_sel = Selector::parse(r#"meta[property="og:title"]"#).expect("static selector");
    dom.select(&og_sel)
        .find_map(|el| el.value().attr("content"))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

// --- content extraction --------------------------------------------------

fn content_root<'a>(dom: &'a Html, extract: bool, ctx: &mut Ctx) -> Option<ElementRef<'a>> {
    let body_sel = Selector::parse("body").expect("static selector");
    let body = dom.select(&body_sel).next()?;

    if !extract {
        return Some(body);
    }

    // Semantic shortcut: densest <main>/<article>/[role=main].
    let semantic_sel = Selector::parse(r#"main, article, [role="main"]"#).expect("static");
    let semantic = dom
        .select(&semantic_sel)
        .map(|el| (el, content_text_len(el)))
        .filter(|(_, len)| *len > 0)
        .max_by_key(|(_, len)| *len);
    if let Some((el, _)) = semantic {
        note_selected_root(el, body, ctx);
        return Some(el);
    }

    // Density descent: follow the child container that holds >= 80% of the
    // remaining content text.
    let mut current = body;
    loop {
        let total = content_text_len(current);
        if total == 0 {
            break;
        }
        let best = current
            .children()
            .filter_map(ElementRef::wrap)
            .filter(|el| DESCENT_TAGS.contains(&el.value().name()))
            .map(|el| (el, content_text_len(el)))
            .max_by_key(|(_, len)| *len);
        match best {
            Some((el, len)) if len * 10 >= total * 8 => current = el,
            _ => break,
        }
    }
    note_selected_root(current, body, ctx);
    Some(current)
}

fn note_selected_root(root: ElementRef<'_>, body: ElementRef<'_>, ctx: &mut Ctx) {
    if root == body {
        return;
    }
    let el = root.value();
    let mut descriptor = format!("<{}", el.name());
    if let Some(id) = el.attr("id") {
        descriptor.push_str(&format!(" id=\"{id}\""));
    } else if let Some(class) = el.attr("class") {
        descriptor.push_str(&format!(" class=\"{class}\""));
    }
    descriptor.push('>');
    ctx.diagnostics.push(Diagnostic::new(
        DiagnosticKind::Normalized,
        format!("content extraction selected {descriptor} as the content root"),
    ));
}

/// Whitespace-collapsed content text length of an element, excluding text
/// inside chrome (`SKIP_TAGS`) subtrees.
fn content_text_len(el: ElementRef<'_>) -> usize {
    let mut total = 0usize;
    for child in el.children() {
        match child.value() {
            Node::Text(text) => {
                total += text.split_whitespace().map(str::len).sum::<usize>();
            }
            Node::Element(_) => {
                if let Some(child_el) = ElementRef::wrap(child)
                    && !SKIP_TAGS.contains(&child_el.value().name())
                {
                    total += content_text_len(child_el);
                }
            }
            _ => {}
        }
    }
    total
}

// --- block walk ----------------------------------------------------------

const BLOCK_TAGS: &[&str] = &[
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "p",
    "pre",
    "table",
    "ul",
    "ol",
    "blockquote",
    "hr",
    "dt",
    "dd",
    "li",
    "tr",
    "td",
    "th",
    "thead",
    "tbody",
    "tfoot",
    "caption",
];

fn is_transparent_block(name: &str) -> bool {
    TRANSPARENT_BLOCK_TAGS.contains(&name)
}

fn is_known_inline(name: &str) -> bool {
    matches!(
        name,
        "strong"
            | "b"
            | "em"
            | "i"
            | "del"
            | "s"
            | "strike"
            | "code"
            | "kbd"
            | "samp"
            | "a"
            | "img"
            | "br"
            | "span"
            | "abbr"
            | "small"
            | "sub"
            | "sup"
            | "u"
            | "mark"
            | "time"
            | "cite"
            | "q"
            | "wbr"
    )
}

/// Walk an element's children producing blocks. Loose inline content between
/// block elements is wrapped into paragraphs.
fn walk_blocks(el: ElementRef<'_>, ctx: &mut Ctx) -> Vec<BlockNode> {
    let mut blocks: Vec<BlockNode> = Vec::new();
    let mut pending: Vec<Inline> = Vec::new();

    for child in el.children() {
        match child.value() {
            Node::Text(text) => push_collapsed_text(&mut pending, text),
            Node::Element(element) => {
                let name = element.name();
                if SKIP_TAGS.contains(&name) {
                    continue;
                }
                let Some(child_el) = ElementRef::wrap(child) else {
                    continue;
                };
                if is_known_inline(name) {
                    inline_for_element(child_el, &mut pending, ctx);
                } else {
                    flush_pending_paragraph(&mut pending, &mut blocks, ctx);
                    handle_block_element(child_el, &mut blocks, ctx);
                }
            }
            _ => {}
        }
    }
    flush_pending_paragraph(&mut pending, &mut blocks, ctx);
    blocks
}

fn flush_pending_paragraph(pending: &mut Vec<Inline>, blocks: &mut Vec<BlockNode>, ctx: &mut Ctx) {
    let inlines = finalize_inlines(std::mem::take(pending));
    if inlines.is_empty() {
        return;
    }
    blocks.push(BlockNode::new(
        Block::Paragraph { content: inlines },
        ctx.prov(),
    ));
}

fn handle_block_element(el: ElementRef<'_>, blocks: &mut Vec<BlockNode>, ctx: &mut Ctx) {
    let name = el.value().name().to_string();
    match name.as_str() {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let level = name.as_bytes()[1] - b'0';
            let content = collect_inlines(el, ctx);
            if content.is_empty() {
                return;
            }
            // Attribute the heading with the section path above it (shared
            // tracker; see parsers::SectionPathTracker).
            let parent = ctx.section.enter_heading(level, inlines_to_plain(&content));
            let prov = Provenance::new(ctx.doc_id).with_section_path(parent);
            blocks.push(BlockNode::new(Block::Heading { level, content }, prov));
        }
        "p" | "dt" | "dd" | "caption" => {
            let content = collect_inlines(el, ctx);
            if content.is_empty() {
                return;
            }
            blocks.push(BlockNode::new(Block::Paragraph { content }, ctx.prov()));
        }
        "pre" => {
            let content: String = el.text().collect();
            let content = content.trim_end_matches('\n').to_string();
            let language = code_language_of(el);
            blocks.push(BlockNode::new(
                Block::CodeBlock { language, content },
                ctx.prov(),
            ));
        }
        "table" => {
            if let Some(block) = table_block(el, ctx) {
                blocks.push(BlockNode::new(block, ctx.prov()));
            }
        }
        "ul" | "ol" => {
            let items = list_items(el, ctx);
            if items.is_empty() {
                ctx.report_once(
                    "empty-list".to_string(),
                    DiagnosticKind::Normalized,
                    "empty <ul>/<ol> without <li> items was skipped".to_string(),
                );
                return;
            }
            blocks.push(BlockNode::new(
                Block::List {
                    ordered: name == "ol",
                    items,
                },
                ctx.prov(),
            ));
        }
        "blockquote" => {
            let children = walk_blocks(el, ctx);
            blocks.push(BlockNode::new(Block::BlockQuote { children }, ctx.prov()));
        }
        "hr" => blocks.push(BlockNode::new(Block::ThematicBreak, ctx.prov())),
        _ if is_transparent_block(&name) => {
            blocks.extend(walk_blocks(el, ctx));
        }
        _ => handle_unknown_block(el, &name, blocks, ctx),
    }
}

/// Unknown block-level element: recurse transparently when it contains known
/// block structure, otherwise degrade to a `GenericBlock` with a hint.
fn handle_unknown_block(
    el: ElementRef<'_>,
    name: &str,
    blocks: &mut Vec<BlockNode>,
    ctx: &mut Ctx,
) {
    let has_block_children = el
        .children()
        .filter_map(ElementRef::wrap)
        .any(|c| BLOCK_TAGS.contains(&c.value().name()) || is_transparent_block(c.value().name()));

    if has_block_children {
        ctx.report_once(
            format!("flattened:{name}"),
            DiagnosticKind::Unsupported,
            format!("unclassified element <{name}> was flattened into its children"),
        );
        blocks.extend(walk_blocks(el, ctx));
        return;
    }

    let content = collect_inlines(el, ctx);
    if content.is_empty() {
        return;
    }
    let classes: Vec<&str> = el.value().classes().collect();
    let hint = if classes.is_empty() {
        name.to_string()
    } else {
        classes.join(" ")
    };
    ctx.report_once(
        format!("generic:{name}"),
        DiagnosticKind::Unsupported,
        format!("unclassified element <{name}> degraded to GenericBlock (hint: {hint})"),
    );
    blocks.push(BlockNode::new(
        Block::GenericBlock {
            content,
            hint: Some(hint),
            confidence: GENERIC_BLOCK_DEFAULT_CONFIDENCE,
        },
        ctx.prov(),
    ));
}

// --- lists ---------------------------------------------------------------

fn list_items(el: ElementRef<'_>, ctx: &mut Ctx) -> Vec<ListItem> {
    let mut items = Vec::new();
    for child in el.children() {
        let Some(child_el) = ElementRef::wrap(child) else {
            continue;
        };
        let name = child_el.value().name();
        if name == "li" {
            let content = walk_blocks(child_el, ctx);
            items.push(ListItem { content });
        } else if !SKIP_TAGS.contains(&name) {
            ctx.report_once(
                "list-non-li".to_string(),
                DiagnosticKind::Lossy,
                format!("non-<li> element <{name}> inside a list was ignored"),
            );
        }
    }
    items
}

// --- tables ----------------------------------------------------------------

fn table_block(el: ElementRef<'_>, ctx: &mut Ctx) -> Option<Block> {
    let mut header_rows: Vec<Vec<Vec<Inline>>> = Vec::new();
    let mut body_rows: Vec<Vec<Vec<Inline>>> = Vec::new();
    let mut saw_span_attr = false;

    collect_table_rows(
        el,
        &mut header_rows,
        &mut body_rows,
        &mut saw_span_attr,
        ctx,
    );

    if saw_span_attr {
        ctx.report_once(
            "table-span".to_string(),
            DiagnosticKind::Lossy,
            "table colspan/rowspan attributes are not represented; cells were flattened"
                .to_string(),
        );
    }

    // Header selection: thead rows (first one) or a leading all-<th> row.
    let mut headers = if !header_rows.is_empty() {
        if header_rows.len() > 1 {
            ctx.report_once(
                "table-multi-head".to_string(),
                DiagnosticKind::Lossy,
                "table had multiple header rows; only the first was kept as the header".to_string(),
            );
            let extra: Vec<_> = header_rows.drain(1..).collect();
            body_rows.splice(0..0, extra);
        }
        header_rows.remove(0)
    } else if !body_rows.is_empty() {
        ctx.report_once(
            "table-headerless".to_string(),
            DiagnosticKind::Normalized,
            "table without a header row; first row was promoted to header".to_string(),
        );
        body_rows.remove(0)
    } else {
        return None;
    };

    // Shape normalization: the IR requires uniform column counts.
    let width = headers
        .len()
        .max(body_rows.iter().map(Vec::len).max().unwrap_or(0));
    if width == 0 {
        return None;
    }
    let mut padded = false;
    if headers.len() < width {
        headers.resize_with(width, Vec::new);
        padded = true;
    }
    for row in &mut body_rows {
        if row.len() < width {
            row.resize_with(width, Vec::new);
            padded = true;
        }
    }
    if padded {
        ctx.report_once(
            "table-ragged".to_string(),
            DiagnosticKind::Normalized,
            "ragged table rows were padded with empty cells to a uniform width".to_string(),
        );
    }

    Some(Block::Table {
        headers,
        rows: body_rows,
    })
}

fn collect_table_rows(
    el: ElementRef<'_>,
    header_rows: &mut Vec<Vec<Vec<Inline>>>,
    body_rows: &mut Vec<Vec<Vec<Inline>>>,
    saw_span_attr: &mut bool,
    ctx: &mut Ctx,
) {
    for child in el.children() {
        let Some(child_el) = ElementRef::wrap(child) else {
            continue;
        };
        match child_el.value().name() {
            "thead" => {
                header_rows.extend(table_rows_of(child_el, saw_span_attr, ctx));
            }
            "tbody" | "tfoot" => {
                body_rows.extend(table_rows_of(child_el, saw_span_attr, ctx));
            }
            "tr" => {
                if let Some((row, all_th)) = table_row(child_el, saw_span_attr, ctx) {
                    if all_th && body_rows.is_empty() && header_rows.is_empty() {
                        header_rows.push(row);
                    } else {
                        body_rows.push(row);
                    }
                }
            }
            _ => {}
        }
    }
}

fn table_rows_of(
    section: ElementRef<'_>,
    saw_span_attr: &mut bool,
    ctx: &mut Ctx,
) -> Vec<Vec<Vec<Inline>>> {
    section
        .children()
        .filter_map(ElementRef::wrap)
        .filter(|el| el.value().name() == "tr")
        .filter_map(|tr| table_row(tr, saw_span_attr, ctx).map(|(row, _)| row))
        .collect()
}

/// Returns the row's cells and whether every cell was a `<th>`.
fn table_row(
    tr: ElementRef<'_>,
    saw_span_attr: &mut bool,
    ctx: &mut Ctx,
) -> Option<(Vec<Vec<Inline>>, bool)> {
    let mut cells = Vec::new();
    let mut all_th = true;
    for child in tr.children() {
        let Some(cell) = ElementRef::wrap(child) else {
            continue;
        };
        let name = cell.value().name();
        if name != "td" && name != "th" {
            continue;
        }
        if name != "th" {
            all_th = false;
        }
        if cell.value().attr("colspan").is_some() || cell.value().attr("rowspan").is_some() {
            *saw_span_attr = true;
        }
        cells.push(collect_inlines(cell, ctx));
    }
    if cells.is_empty() {
        None
    } else {
        Some((cells, all_th))
    }
}

// --- inline walk -----------------------------------------------------------

/// Collect the inline content of an element, treating nested block wrappers
/// (e.g. `<p>` inside a table cell) as transparent with space separators.
fn collect_inlines(el: ElementRef<'_>, ctx: &mut Ctx) -> Vec<Inline> {
    let mut out = Vec::new();
    walk_inline_children(el, &mut out, ctx);
    finalize_inlines(out)
}

fn walk_inline_children(el: ElementRef<'_>, out: &mut Vec<Inline>, ctx: &mut Ctx) {
    for child in el.children() {
        match child.value() {
            Node::Text(text) => push_collapsed_text(out, text),
            Node::Element(element) => {
                let name = element.name();
                if SKIP_TAGS.contains(&name) {
                    continue;
                }
                let Some(child_el) = ElementRef::wrap(child) else {
                    continue;
                };
                if is_known_inline(name) {
                    inline_for_element(child_el, out, ctx);
                } else {
                    // Block content in an inline context (e.g. <p> inside a
                    // table cell): flatten with a space separator.
                    push_collapsed_text(out, " ");
                    walk_inline_children(child_el, out, ctx);
                    push_collapsed_text(out, " ");
                }
            }
            _ => {}
        }
    }
}

fn inline_for_element(el: ElementRef<'_>, out: &mut Vec<Inline>, ctx: &mut Ctx) {
    let name = el.value().name();
    match name {
        "strong" | "b" => push_styled(el, Style::Strong, out, ctx),
        "em" | "i" => push_styled(el, Style::Emphasis, out, ctx),
        "del" | "s" | "strike" => push_styled(el, Style::Strikethrough, out, ctx),
        "code" | "kbd" | "samp" => {
            let text = collapse_whitespace(&el.text().collect::<String>());
            let text = text.trim().to_string();
            if !text.is_empty() {
                out.push(Inline::Code(text));
            }
        }
        "a" => {
            let href = el.value().attr("href").unwrap_or("");
            let mut children = Vec::new();
            walk_inline_children(el, &mut children, ctx);
            let children = finalize_inlines(children);
            if href.is_empty() {
                ctx.report_once(
                    "link-no-href".to_string(),
                    DiagnosticKind::Lossy,
                    "anchor without href was flattened to its text".to_string(),
                );
                out.extend(children);
            } else if !children.is_empty() {
                out.push(Inline::Link {
                    url: href.to_string(),
                    children,
                });
            }
        }
        "img" => {
            let src = el.value().attr("src").unwrap_or("");
            if src.is_empty() {
                ctx.report_once(
                    "img-no-src".to_string(),
                    DiagnosticKind::Lossy,
                    "image without src was dropped".to_string(),
                );
                return;
            }
            let alt = el
                .value()
                .attr("alt")
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            out.push(Inline::Image {
                url: src.to_string(),
                alt,
            });
        }
        "br" => out.push(Inline::LineBreak),
        "wbr" => {}
        // Presentational / semantic wrappers flatten to their children.
        "span" | "abbr" | "small" | "sub" | "sup" | "u" | "mark" | "time" | "cite" | "q" => {
            walk_inline_children(el, out, ctx);
        }
        _ => {
            ctx.report_once(
                format!("inline:{name}"),
                DiagnosticKind::Unsupported,
                format!("unclassified inline element <{name}> was flattened to its text"),
            );
            walk_inline_children(el, out, ctx);
        }
    }
}

fn push_styled(el: ElementRef<'_>, style: Style, out: &mut Vec<Inline>, ctx: &mut Ctx) {
    let mut children = Vec::new();
    walk_inline_children(el, &mut children, ctx);
    let children = finalize_inlines(children);
    if !children.is_empty() {
        out.push(Inline::Styled { style, children });
    }
}

fn code_language_of(pre: ElementRef<'_>) -> Option<String> {
    let code_sel = Selector::parse("code").expect("static selector");
    let candidates = std::iter::once(pre).chain(pre.select(&code_sel));
    for el in candidates {
        for class in el.value().classes() {
            if let Some(lang) = class.strip_prefix("language-")
                && !lang.is_empty()
            {
                return Some(lang.to_string());
            }
        }
    }
    None
}

// --- text helpers ------------------------------------------------------------
//
// These implement the HTML side of "stripping": collapsing the significant
// whitespace of source HTML down to the normalized text the IR stores. This
// is intentionally *not* shared with `ir::normalize` (which stays
// whitespace-agnostic for cross-format comparison — see its doc comment).

/// Collapse HTML whitespace runs to single spaces, preserving whether the
/// node started/ended with whitespace (word boundaries between elements).
fn push_collapsed_text(out: &mut Vec<Inline>, raw: &str) {
    if raw.is_empty() {
        return;
    }
    let collapsed = collapse_whitespace(raw);
    if collapsed.is_empty() {
        return;
    }
    match out.last_mut() {
        Some(Inline::Text(prev)) => prev.push_str(&collapsed),
        _ => out.push(Inline::Text(collapsed)),
    }
}

fn collapse_whitespace(raw: &str) -> String {
    if raw.trim().is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(raw.len());
    let mut in_ws = false;
    for ch in raw.chars() {
        if ch.is_whitespace() {
            if !in_ws {
                out.push(' ');
            }
            in_ws = true;
        } else {
            out.push(ch);
            in_ws = false;
        }
    }
    out
}

/// Merge adjacent text nodes, collapse whitespace runs created at merge
/// boundaries, trim the ends of the inline sequence, and drop empties.
fn finalize_inlines(inlines: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(inlines.len());
    for inline in inlines {
        match inline {
            Inline::Text(s) => {
                if s.is_empty() {
                    continue;
                }
                match out.last_mut() {
                    // Re-collapse only at the join: each node is already
                    // single-space-collapsed, so merging can create at most a
                    // double space at the boundary.
                    Some(Inline::Text(prev)) => {
                        prev.push_str(&s);
                        *prev = collapse_whitespace(prev);
                    }
                    _ => out.push(Inline::Text(s)),
                }
            }
            other => out.push(other),
        }
    }

    if let Some(Inline::Text(s)) = out.first_mut() {
        *s = s.trim_start().to_string();
    }
    if let Some(Inline::Text(s)) = out.last_mut() {
        *s = s.trim_end().to_string();
    }
    out.retain(|inline| !matches!(inline, Inline::Text(s) if s.is_empty()));
    out
}
