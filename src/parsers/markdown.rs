//! Markdown parser built on `pulldown-cmark`.
//!
//! The implementation is a stack-of-frames walk over the event stream. Each
//! frame knows what it is collecting (blocks vs. inlines) and how to finalize
//! itself when the matching end event arrives.
//!
//! Phase 1 supported constructs: headings, paragraphs, emphasis, strong,
//! strikethrough, links, images, inline code, fenced code blocks with optional
//! language, unordered/ordered/nested lists, blockquotes, tables, thematic
//! breaks, hard line breaks.
//!
//! Everything else (raw HTML, task list markers, footnotes, metadata blocks,
//! indented code blocks) is surfaced as a typed [`Diagnostic`] rather than
//! silently dropped or flattened.

use std::ops::Range;

use chrono::Utc;
use pulldown_cmark::{
    CodeBlockKind, Event, HeadingLevel, Options, Parser as CmarkParser, Tag, TagEnd,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::Result;
use crate::ir::{
    Block, BlockNode, ByteRange, Diagnostic, DiagnosticKind, Document, DocumentId, DocumentMeta,
    Frontmatter, Inline, ListItem, Provenance, SourceFormat, SourceInfo, Style,
};
use crate::parsers::Parser;

#[derive(Debug, Default, Clone)]
pub struct MarkdownParser;

impl Parser for MarkdownParser {
    fn parse(&self, input: &str) -> Result<Document> {
        Ok(parse_markdown(input, ParseOptions::default()))
    }
}

/// Parser options. Phase 1 exposes only the source descriptor; defaults are
/// sufficient for pure-string parsing.
#[derive(Debug, Clone, Default)]
pub struct ParseOptions {
    /// Optional human-readable source descriptor (path, url, etc.) to attach
    /// to `DocumentMeta.source.raw_source`.
    pub source_descriptor: Option<String>,
    /// If enabled, drops a suspected duplicate segment when the opening
    /// signature appears again deep in the body. Disabled by default so parse
    /// behavior is conservative and content remains unchanged.
    pub normalize_repeated_leading_segment: bool,
}

/// Parse a markdown string into a [`Document`]. Never fails — unsupported
/// constructs produce diagnostics.
pub fn parse_markdown(input: &str, opts: ParseOptions) -> Document {
    let (frontmatter, frontmatter_len) = extract_frontmatter(input);
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    let mut body_end = input.len();
    if let Some(dup) = detect_repeated_leading_segment(input, frontmatter_len, frontmatter.as_ref())
    {
        let range = ByteRange::new(dup.start, dup.end);
        if opts.normalize_repeated_leading_segment {
            diagnostics.push(
                Diagnostic::new(
                    DiagnosticKind::Lossy,
                    format!(
                        "suspected duplicated leading segment removed at byte {} using signature [{}]",
                        dup.start, dup.signature
                    ),
                )
                .with_range(range),
            );
            body_end = dup.start;
        } else {
            diagnostics.push(
                Diagnostic::new(
                    DiagnosticKind::Normalized,
                    format!(
                        "suspected duplicated leading segment starts at byte {} (signature [{}]); content kept unchanged",
                        dup.start, dup.signature
                    ),
                )
                .with_range(range),
            );
        }
    }

    let body_input = &input[frontmatter_len..body_end];

    let id: DocumentId = Uuid::new_v4();
    let meta = DocumentMeta {
        id,
        source: SourceInfo {
            raw_source: opts
                .source_descriptor
                .unwrap_or_else(|| "raw:markdown".to_string()),
        },
        format: SourceFormat::Markdown,
        title: None,
        frontmatter,
        ingested_at: Utc::now(),
        content_hash: sha256_hex(input),
    };
    let mut doc = Document::new(meta);

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = CmarkParser::new_ext(body_input, options);

    let mut stack: Vec<Frame> = vec![Frame::Document];
    let mut top_body: Vec<BlockNode> = Vec::new();
    let mut section_path: Vec<String> = Vec::new();
    // Previous heading levels in source order; used to pop section path.
    let mut heading_levels: Vec<u8> = Vec::new();

    for (event, range) in parser.into_offset_iter() {
        let range = (range.start + frontmatter_len)..(range.end + frontmatter_len);
        match event {
            Event::Start(tag) => start_tag(tag, range, &mut stack),
            Event::End(end) => end_tag(
                end,
                range,
                &mut stack,
                &mut top_body,
                &mut section_path,
                &mut heading_levels,
                &mut diagnostics,
                id,
            ),
            Event::Text(s) => push_inline(
                &mut stack,
                &mut top_body,
                &mut diagnostics,
                range.clone(),
                id,
                Inline::Text(restore_escaped_math_brackets(
                    s.into_string(),
                    &input[range.clone()],
                )),
            ),
            Event::Code(s) => push_inline(
                &mut stack,
                &mut top_body,
                &mut diagnostics,
                range,
                id,
                Inline::Code(s.into_string()),
            ),
            Event::SoftBreak => {
                let soft_break = if in_blockquote(&stack) {
                    // Preserve logical line boundaries in quoted content so
                    // callout-like first-line markers and body lines survive
                    // parser/emitter roundtrips.
                    Inline::Text("\n".to_string())
                } else {
                    Inline::Text(" ".to_string())
                };
                push_inline(
                    &mut stack,
                    &mut top_body,
                    &mut diagnostics,
                    range,
                    id,
                    soft_break,
                )
            }
            Event::HardBreak => push_inline(
                &mut stack,
                &mut top_body,
                &mut diagnostics,
                range,
                id,
                Inline::LineBreak,
            ),
            Event::Rule => {
                let prov = prov_for(id, &section_path, range.clone());
                append_block(
                    &mut stack,
                    &mut top_body,
                    BlockNode::new(Block::ThematicBreak, prov),
                );
            }
            Event::Html(_) | Event::InlineHtml(_) => {
                diagnostics.push(
                    Diagnostic::new(
                        DiagnosticKind::Unsupported,
                        "raw HTML is not supported in Phase 1 and was skipped",
                    )
                    .with_range(ByteRange::new(range.start, range.end)),
                );
            }
            Event::FootnoteReference(_) => {
                diagnostics.push(
                    Diagnostic::new(
                        DiagnosticKind::Unsupported,
                        "footnote references are not supported in Phase 1",
                    )
                    .with_range(ByteRange::new(range.start, range.end)),
                );
            }
            Event::TaskListMarker(_) => {
                diagnostics.push(
                    Diagnostic::new(
                        DiagnosticKind::Unsupported,
                        "task list markers are not supported in Phase 1",
                    )
                    .with_range(ByteRange::new(range.start, range.end)),
                );
            }
        }
    }

    doc.body = top_body;
    doc.meta.title = extract_title(&doc.body);
    doc.diagnostics = diagnostics;
    doc
}

fn extract_frontmatter(input: &str) -> (Option<Frontmatter>, usize) {
    if !input.starts_with("---") {
        return (None, 0);
    }
    let Some((first_line, mut offset)) = read_line(input, 0) else {
        return (None, 0);
    };
    if !is_frontmatter_delimiter(first_line) {
        return (None, 0);
    }

    let yaml_start = offset;
    while let Some((line, next_offset)) = read_line(input, offset) {
        if is_frontmatter_delimiter(line) {
            let yaml_slice = &input[yaml_start..offset];
            let yaml = yaml_slice.replace("\r\n", "\n");
            return (Some(Frontmatter { yaml }), next_offset);
        }
        offset = next_offset;
    }

    (None, 0)
}

fn is_frontmatter_delimiter(line: &str) -> bool {
    line.trim_end_matches('\r') == "---"
}

fn read_line(input: &str, offset: usize) -> Option<(&str, usize)> {
    if offset > input.len() {
        return None;
    }
    if offset == input.len() {
        return None;
    }
    let bytes = input.as_bytes();
    let mut i = offset;
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }

    let (line_end, next_offset) = if i < bytes.len() && bytes[i] == b'\n' {
        let end = if i > offset && bytes[i - 1] == b'\r' {
            i - 1
        } else {
            i
        };
        (end, i + 1)
    } else {
        (i, i)
    };
    Some((&input[offset..line_end], next_offset))
}

#[derive(Debug, Clone)]
struct DuplicateSegmentBoundary {
    start: usize,
    end: usize,
    signature: String,
}

fn detect_repeated_leading_segment(
    input: &str,
    frontmatter_len: usize,
    frontmatter: Option<&Frontmatter>,
) -> Option<DuplicateSegmentBoundary> {
    const MIN_GAP_BYTES: usize = 128;

    let mut heading_line: Option<&str> = None;
    let mut heading_offset = frontmatter_len;
    let mut offset = frontmatter_len;
    while let Some((line, next_offset)) = read_line(input, offset) {
        let trimmed = line.trim();
        if !trimmed.is_empty() && trimmed.starts_with('#') {
            heading_line = Some(trimmed);
            heading_offset = offset;
            break;
        }
        if next_offset <= offset {
            break;
        }
        offset = next_offset;
    }

    let heading_line = heading_line?;
    let search_from = heading_offset.saturating_add(MIN_GAP_BYTES);
    if search_from >= input.len() {
        return None;
    }

    let needle = format!("\n{heading_line}\n");
    let rel = input[search_from..].find(&needle)?;
    let dup_start = search_from + rel + 1;

    if dup_start < (input.len() / 3) {
        return None;
    }

    let mut signature_parts: Vec<String> = Vec::new();
    if let Some(frontmatter) = frontmatter {
        for line in frontmatter.yaml.lines() {
            let trimmed = line.trim();
            if let Some((key, _)) = trimmed.split_once(':') {
                let key = key.trim();
                if !key.is_empty() {
                    signature_parts.push(format!("fm:{key}"));
                }
            }
            if signature_parts.len() >= 4 {
                break;
            }
        }
    }
    signature_parts.push(format!("h:{}", heading_line.trim_start_matches('#').trim()));

    Some(DuplicateSegmentBoundary {
        start: dup_start,
        end: input.len(),
        signature: signature_parts.join(","),
    })
}

// --- frames ------------------------------------------------------------

fn restore_escaped_math_brackets(parsed_text: String, source_slice: &str) -> String {
    if source_slice == "\\[" && parsed_text == "[" {
        return "\\[".to_string();
    }
    if source_slice == "\\]" && parsed_text == "]" {
        return "\\]".to_string();
    }
    parsed_text
}

enum Frame {
    Document,
    Paragraph {
        inlines: Vec<Inline>,
        start: usize,
    },
    Heading {
        level: u8,
        inlines: Vec<Inline>,
        start: usize,
    },
    CodeBlock {
        language: Option<String>,
        content: String,
        start: usize,
    },
    BlockQuote {
        blocks: Vec<BlockNode>,
        start: usize,
    },
    List {
        ordered: bool,
        items: Vec<ListItem>,
        start: usize,
    },
    Item {
        blocks: Vec<BlockNode>,
    },
    Table {
        headers: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
        current_row: Vec<Vec<Inline>>,
        in_head: bool,
        start: usize,
    },
    TableCell {
        inlines: Vec<Inline>,
    },
    Styled {
        style: Style,
        inlines: Vec<Inline>,
    },
    Link {
        url: String,
        inlines: Vec<Inline>,
    },
    /// Images collect their alt text via child Text events. Nested formatting
    /// inside alt text is flattened to a plain string; if that happens a Lossy
    /// diagnostic is produced.
    Image {
        url: String,
        alt: String,
        saw_nested_formatting: bool,
    },
    /// Swallow events for an unsupported block (e.g. HtmlBlock) without
    /// treating them as markup.
    Skip,
}

fn start_tag(tag: Tag<'_>, range: Range<usize>, stack: &mut Vec<Frame>) {
    match tag {
        Tag::Paragraph => stack.push(Frame::Paragraph {
            inlines: Vec::new(),
            start: range.start,
        }),
        Tag::Heading { level, .. } => stack.push(Frame::Heading {
            level: heading_level_to_u8(level),
            inlines: Vec::new(),
            start: range.start,
        }),
        Tag::BlockQuote => stack.push(Frame::BlockQuote {
            blocks: Vec::new(),
            start: range.start,
        }),
        Tag::CodeBlock(kind) => {
            let language = match kind {
                CodeBlockKind::Indented => None,
                CodeBlockKind::Fenced(lang) => {
                    if lang.is_empty() {
                        None
                    } else {
                        Some(lang.into_string())
                    }
                }
            };
            stack.push(Frame::CodeBlock {
                language,
                content: String::new(),
                start: range.start,
            });
        }
        Tag::List(first) => stack.push(Frame::List {
            ordered: first.is_some(),
            items: Vec::new(),
            start: range.start,
        }),
        Tag::Item => stack.push(Frame::Item { blocks: Vec::new() }),
        Tag::Table(_) => stack.push(Frame::Table {
            headers: Vec::new(),
            rows: Vec::new(),
            current_row: Vec::new(),
            in_head: false,
            start: range.start,
        }),
        Tag::TableHead => {
            if let Some(Frame::Table { in_head, .. }) = stack.last_mut() {
                *in_head = true;
            }
        }
        Tag::TableRow => {
            if let Some(Frame::Table {
                in_head,
                current_row,
                ..
            }) = stack.last_mut()
            {
                *in_head = false;
                current_row.clear();
            }
        }
        Tag::TableCell => stack.push(Frame::TableCell {
            inlines: Vec::new(),
        }),
        Tag::Emphasis => stack.push(Frame::Styled {
            style: Style::Emphasis,
            inlines: Vec::new(),
        }),
        Tag::Strong => stack.push(Frame::Styled {
            style: Style::Strong,
            inlines: Vec::new(),
        }),
        Tag::Strikethrough => stack.push(Frame::Styled {
            style: Style::Strikethrough,
            inlines: Vec::new(),
        }),
        Tag::Link { dest_url, .. } => stack.push(Frame::Link {
            url: dest_url.into_string(),
            inlines: Vec::new(),
        }),
        Tag::Image { dest_url, .. } => stack.push(Frame::Image {
            url: dest_url.into_string(),
            alt: String::new(),
            saw_nested_formatting: false,
        }),
        Tag::HtmlBlock | Tag::MetadataBlock(_) | Tag::FootnoteDefinition(_) => {
            // Ignore the body but keep balance.
            stack.push(Frame::Skip);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn end_tag(
    end: TagEnd,
    end_range: Range<usize>,
    stack: &mut Vec<Frame>,
    top_body: &mut Vec<BlockNode>,
    section_path: &mut Vec<String>,
    heading_levels: &mut Vec<u8>,
    diagnostics: &mut Vec<Diagnostic>,
    doc_id: DocumentId,
) {
    match end {
        TagEnd::TableHead => {
            if let Some(Frame::Table { in_head, .. }) = stack.last_mut() {
                *in_head = false;
            }
            return;
        }
        TagEnd::TableRow => {
            if let Some(Frame::Table {
                in_head,
                current_row,
                headers,
                rows,
                ..
            }) = stack.last_mut()
            {
                let cells = std::mem::take(current_row);
                if *in_head {
                    *headers = cells;
                } else {
                    rows.push(cells);
                }
            }
            return;
        }
        _ => {}
    }

    let Some(frame) = stack.pop() else {
        return;
    };

    match (frame, end) {
        (Frame::Paragraph { inlines, start }, TagEnd::Paragraph) => {
            let range = ByteRange::new(start, end_range.end);
            let prov = Provenance::new(doc_id)
                .with_range(range)
                .with_section_path(section_path.clone());
            append_block(
                stack,
                top_body,
                BlockNode::new(Block::Paragraph { content: inlines }, prov),
            );
        }
        (
            Frame::Heading {
                level,
                inlines,
                start,
            },
            TagEnd::Heading(_),
        ) => {
            // Update section path state before attaching the heading.
            while heading_levels.last().map(|l| *l >= level).unwrap_or(false) {
                heading_levels.pop();
                section_path.pop();
            }
            let title_text = inlines_to_plain(&inlines);
            heading_levels.push(level);
            section_path.push(title_text);

            let range = ByteRange::new(start, end_range.end);
            // Attribute this heading with the section path *above* it so the
            // first heading under "root" shows an empty path.
            let mut parent_path = section_path.clone();
            parent_path.pop();
            let prov = Provenance::new(doc_id)
                .with_range(range)
                .with_section_path(parent_path);
            append_block(
                stack,
                top_body,
                BlockNode::new(
                    Block::Heading {
                        level,
                        content: inlines,
                    },
                    prov,
                ),
            );
        }
        (
            Frame::CodeBlock {
                language,
                content,
                start,
            },
            TagEnd::CodeBlock,
        ) => {
            // Fenced code blocks in source always end with a newline before
            // the closing fence. Strip every trailing newline so the IR
            // stores only the logical content and the emitter can add a
            // single newline before the closing fence unconditionally.
            let content = content.trim_end_matches('\n').to_string();
            let range = ByteRange::new(start, end_range.end);
            let prov = Provenance::new(doc_id)
                .with_range(range)
                .with_section_path(section_path.clone());
            append_block(
                stack,
                top_body,
                BlockNode::new(Block::CodeBlock { language, content }, prov),
            );
        }
        (Frame::BlockQuote { blocks, start }, TagEnd::BlockQuote) => {
            let range = ByteRange::new(start, end_range.end);
            let prov = Provenance::new(doc_id)
                .with_range(range)
                .with_section_path(section_path.clone());
            append_block(
                stack,
                top_body,
                BlockNode::new(Block::BlockQuote { children: blocks }, prov),
            );
        }
        (
            Frame::List {
                ordered,
                items,
                start,
            },
            TagEnd::List(_),
        ) => {
            let range = ByteRange::new(start, end_range.end);
            let prov = Provenance::new(doc_id)
                .with_range(range)
                .with_section_path(section_path.clone());
            append_block(
                stack,
                top_body,
                BlockNode::new(Block::List { ordered, items }, prov),
            );
        }
        (Frame::Item { blocks }, TagEnd::Item) => {
            if let Some(Frame::List { items, .. }) = stack.last_mut() {
                items.push(ListItem { content: blocks });
            }
        }
        (
            Frame::Table {
                headers,
                rows,
                start,
                ..
            },
            TagEnd::Table,
        ) => {
            let range = ByteRange::new(start, end_range.end);
            let prov = Provenance::new(doc_id)
                .with_range(range)
                .with_section_path(section_path.clone());
            append_block(
                stack,
                top_body,
                BlockNode::new(Block::Table { headers, rows }, prov),
            );
        }
        (Frame::TableCell { inlines }, TagEnd::TableCell) => {
            if let Some(Frame::Table {
                in_head,
                current_row,
                headers,
                ..
            }) = stack.last_mut()
            {
                if *in_head {
                    headers.push(inlines);
                } else {
                    current_row.push(inlines);
                }
            }
        }
        (Frame::Styled { style, inlines }, _) => {
            let produced = Inline::Styled {
                style,
                children: inlines,
            };
            attach_inline(stack, produced);
        }
        (Frame::Link { url, inlines }, TagEnd::Link) => {
            attach_inline(
                stack,
                Inline::Link {
                    url,
                    children: inlines,
                },
            );
        }
        (
            Frame::Image {
                url,
                alt,
                saw_nested_formatting,
            },
            TagEnd::Image,
        ) => {
            if saw_nested_formatting {
                diagnostics.push(Diagnostic::new(
                    DiagnosticKind::Lossy,
                    format!(
                        "image alt text for {url:?} contained inline formatting; only plain text is preserved"
                    ),
                ));
            }
            let alt = if alt.is_empty() { None } else { Some(alt) };
            attach_inline(stack, Inline::Image { url, alt });
        }
        (Frame::Skip, _) => {
            diagnostics.push(Diagnostic::new(
                DiagnosticKind::Unsupported,
                "raw HTML block / metadata block / footnote definition was skipped",
            ));
        }
        // Defensive: mismatched frame/end — shouldn't happen given balanced events.
        (_, _) => {}
    }
}

fn push_inline(
    stack: &mut [Frame],
    top_body: &mut Vec<BlockNode>,
    diagnostics: &mut Vec<Diagnostic>,
    range: Range<usize>,
    doc_id: DocumentId,
    inline: Inline,
) {
    // Special case: text events inside an image frame become alt text.
    if let Some(Frame::Image {
        alt,
        saw_nested_formatting,
        ..
    }) = stack.last_mut()
    {
        match inline {
            Inline::Text(s) => alt.push_str(&s),
            Inline::LineBreak => alt.push('\n'),
            _ => {
                *saw_nested_formatting = true;
            }
        }
        return;
    }

    // Inline must land in some inline-collecting frame. If there is none,
    // promote it to a paragraph and record a diagnostic.
    if !attach_inline_checked(stack, inline.clone()) {
        let prov = Provenance::new(doc_id).with_range(ByteRange::new(range.start, range.end));
        diagnostics.push(Diagnostic::new(
            DiagnosticKind::Lossy,
            "inline content appeared outside a recognized block frame; wrapped in a paragraph",
        ));
        append_block(
            stack,
            top_body,
            BlockNode::new(
                Block::Paragraph {
                    content: vec![inline],
                },
                prov,
            ),
        );
    }
}

/// Attach an inline to the innermost inline-collecting frame. Returns `true`
/// if a frame was found.
fn attach_inline_checked(stack: &mut [Frame], inline: Inline) -> bool {
    for frame in stack.iter_mut().rev() {
        match frame {
            Frame::Paragraph { inlines, .. }
            | Frame::Heading { inlines, .. }
            | Frame::Styled { inlines, .. }
            | Frame::Link { inlines, .. }
            | Frame::TableCell { inlines } => {
                inlines.push(inline);
                return true;
            }
            Frame::CodeBlock { content, .. } => {
                if let Inline::Text(s) = inline {
                    content.push_str(&s);
                } else if let Inline::LineBreak = inline {
                    content.push('\n');
                }
                return true;
            }
            _ => continue,
        }
    }
    false
}

fn attach_inline(stack: &mut [Frame], inline: Inline) {
    attach_inline_checked(stack, inline);
}

fn in_blockquote(stack: &[Frame]) -> bool {
    stack
        .iter()
        .rev()
        .any(|frame| matches!(frame, Frame::BlockQuote { .. }))
}

fn append_block(stack: &mut [Frame], top_body: &mut Vec<BlockNode>, node: BlockNode) {
    for frame in stack.iter_mut().rev() {
        match frame {
            Frame::BlockQuote { blocks, .. } | Frame::Item { blocks } => {
                blocks.push(node);
                return;
            }
            Frame::Document => {
                top_body.push(node);
                return;
            }
            _ => continue,
        }
    }
    top_body.push(node);
}

fn prov_for(doc_id: DocumentId, section_path: &[String], range: Range<usize>) -> Provenance {
    Provenance::new(doc_id)
        .with_range(ByteRange::new(range.start, range.end))
        .with_section_path(section_path.to_vec())
}

fn heading_level_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Flatten inlines into a plain string (used for heading text in section paths
/// and document title extraction).
fn inlines_to_plain(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        push_plain(inline, &mut out);
    }
    out
}

fn push_plain(inline: &Inline, out: &mut String) {
    match inline {
        Inline::Text(s) => out.push_str(s),
        Inline::Code(s) => out.push_str(s),
        Inline::LineBreak => out.push(' '),
        Inline::Styled { children, .. } | Inline::Link { children, .. } => {
            for c in children {
                push_plain(c, out);
            }
        }
        Inline::Image { alt, .. } => {
            if let Some(alt) = alt {
                out.push_str(alt);
            }
        }
    }
}

fn extract_title(body: &[BlockNode]) -> Option<String> {
    for node in body {
        if let Block::Heading { level: 1, content } = &node.block {
            return Some(inlines_to_plain(content));
        }
    }
    None
}

fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(s, "{:02x}", b);
    }
    s
}
