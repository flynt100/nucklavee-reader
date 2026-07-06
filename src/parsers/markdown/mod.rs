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
//!
//! Markdown-specific input-repair heuristics live in the private submodules:
//! [`preparse`] (escaped-newline decode, frontmatter, duplicate-segment
//! detection), [`math`] (math-span shielding), and [`heuristics`]
//! (TSV-paragraph promotion, bare-callout normalization).

mod heuristics;
pub(crate) mod math;
mod offsets;
mod preparse;

use std::ops::Range;

use chrono::Utc;
use pulldown_cmark::{
    CodeBlockKind, Event, HeadingLevel, Options, Parser as CmarkParser, Tag, TagEnd,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::ir::{
    Block, BlockNode, ByteRange, Diagnostic, DiagnosticKind, Document, DocumentId, DocumentMeta,
    Inline, ListItem, Provenance, SourceFormat, SourceInfo, Style,
};

use heuristics::{Phase2bParagraphBlock, detect_phase2b_tabular_fallback};
use math::{
    MathRestoreState, restore_escaped_math_brackets, restore_math_placeholders,
    shield_math_segments,
};
use offsets::OffsetMap;
use preparse::{
    detect_repeated_leading_segment, extract_frontmatter, infer_probable_frontmatter,
    normalize_preparse_input,
};

#[derive(Debug, Default, Clone)]
pub struct MarkdownParser;

impl MarkdownParser {
    /// Convenience wrapper over [`parse_markdown`] with default options.
    pub fn parse(&self, input: &str) -> Document {
        parse_markdown(input, ParseOptions::default())
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
    /// If enabled, rewrites bare callout paragraphs (e.g. `[!tip] Title`)
    /// into canonical blockquote callouts during paragraph finalization.
    /// Disabled by default so parse behavior remains conservative.
    pub normalize_bare_callouts: bool,
}

/// Parse a markdown string into a [`Document`]. Never fails — unsupported
/// constructs produce diagnostics.
pub fn parse_markdown(input: &str, opts: ParseOptions) -> Document {
    let preparse = normalize_preparse_input(input);
    let parse_input = preparse.input;
    // Maps decoded (`parse_input`) positions back to `input` positions.
    let decode_map = preparse.offset_map;
    let mut diagnostics: Vec<Diagnostic> = preparse.diagnostics;
    let (frontmatter, frontmatter_len) = match extract_frontmatter(&parse_input) {
        (Some(frontmatter), len) => (Some(frontmatter), len),
        (None, _) => {
            if let Some((frontmatter, len)) = infer_probable_frontmatter(&parse_input) {
                diagnostics.push(Diagnostic::new(
                    DiagnosticKind::Normalized,
                    "inferred unfenced YAML-like frontmatter block at document start",
                ));
                (Some(frontmatter), len)
            } else {
                (None, 0)
            }
        }
    };

    let mut body_end = parse_input.len();
    if let Some(dup) =
        detect_repeated_leading_segment(&parse_input, frontmatter_len, frontmatter.as_ref())
    {
        // Report the duplicate boundary in original coordinates directly;
        // this diagnostic is created before math shielding, so the final
        // remap pass (which also undoes shielding) must not touch it.
        let range = decode_map.map_range(ByteRange::new(dup.start, dup.end));
        if opts.normalize_repeated_leading_segment {
            diagnostics.push(
                Diagnostic::new(
                    DiagnosticKind::Lossy,
                    format!(
                        "suspected duplicated leading segment removed at byte {} using signature [{}]",
                        range.start, dup.signature
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
                        range.start, dup.signature
                    ),
                )
                .with_range(range),
            );
        }
    }

    // Every diagnostic from this point on (shielding + event walk) carries
    // ranges in shielded-stream coordinates and is remapped to original
    // coordinates at the end of the parse.
    let stream_coord_diags_from = diagnostics.len();

    let body_input = &parse_input[frontmatter_len..body_end];
    let math_shield = shield_math_segments(body_input, frontmatter_len);
    diagnostics.extend(math_shield.diagnostics);
    // Maps shielded-stream positions back to decoded (`parse_input`) positions.
    let shield_map = math_shield.offset_map;
    let parse_body_input = math_shield.shielded_input;
    let parse_stream_input = format!("{}{}", &parse_input[..frontmatter_len], parse_body_input);
    let mut math_restore =
        (!math_shield.disabled).then(|| MathRestoreState::new(math_shield.payloads));

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
    let parser = CmarkParser::new_ext(&parse_stream_input[frontmatter_len..], options);

    let mut stack: Vec<Frame> = vec![Frame::Document];
    let mut top_body: Vec<BlockNode> = Vec::new();
    let mut section_path: Vec<String> = Vec::new();
    // Previous heading levels in source order; used to pop section path.
    let mut heading_levels: Vec<u8> = Vec::new();

    for (event, range) in parser.into_offset_iter() {
        // `into_offset_iter()` ranges are relative to `body_input` (the
        // frontmatter-stripped body we parse, after math shielding). Translate
        // them back into `parse_stream_input`/`parse_input` coordinates by
        // adding `frontmatter_len`.
        let range = (range.start + frontmatter_len)..(range.end + frontmatter_len);
        match event {
            Event::Start(tag) => start_tag(tag, range, &mut stack, &section_path, id),
            Event::End(end) => end_tag(
                end,
                range,
                &parse_stream_input,
                &mut stack,
                &mut top_body,
                &mut section_path,
                &mut heading_levels,
                &mut diagnostics,
                id,
                opts.normalize_bare_callouts,
            ),
            Event::Text(s) => {
                let mut restored = restore_escaped_math_brackets(
                    s.into_string(),
                    &parse_stream_input[range.start..range.end],
                );
                if let Some(restore) = math_restore.as_mut() {
                    restored = restore_math_placeholders(
                        restored,
                        restore,
                        &mut diagnostics,
                        range.clone(),
                    );
                }
                push_inline(
                    &mut stack,
                    &mut top_body,
                    &mut diagnostics,
                    range,
                    id,
                    Inline::Text(restored),
                )
            }
            Event::Code(s) => {
                // Shielding runs on raw text before cmark, so placeholders can
                // land inside inline-code spans; restore them here too or the
                // code content is silently replaced by the sentinel character.
                let mut code = s.into_string();
                if let Some(restore) = math_restore.as_mut() {
                    code =
                        restore_math_placeholders(code, restore, &mut diagnostics, range.clone());
                }
                push_inline(
                    &mut stack,
                    &mut top_body,
                    &mut diagnostics,
                    range,
                    id,
                    Inline::Code(code),
                )
            }
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
                flush_item_pending_inlines_if_block_boundary(
                    &mut stack,
                    &Tag::Paragraph,
                    &section_path,
                    id,
                    range.start,
                );
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

    // Translate provenance and stream-coordinate diagnostics back to
    // original-source byte offsets (undo math shielding, then newline
    // decoding). Ranges recorded before shielding are already original.
    if !(shield_map.is_identity() && decode_map.is_identity()) {
        for node in &mut doc.body {
            remap_block_node_ranges(node, &shield_map, &decode_map);
        }
        for diag in &mut diagnostics[stream_coord_diags_from..] {
            if let Some(range) = diag.byte_range {
                diag.byte_range = Some(decode_map.map_range(shield_map.map_range(range)));
            }
        }
    }

    doc.meta.title = extract_title(&doc.body);
    doc.diagnostics = diagnostics;
    doc
}

/// Remap a block node's provenance range (and its children's, recursively)
/// from shielded-stream coordinates back to original-source coordinates.
fn remap_block_node_ranges(node: &mut BlockNode, shield_map: &OffsetMap, decode_map: &OffsetMap) {
    if let Some(range) = node.prov.byte_range {
        node.prov.byte_range = Some(decode_map.map_range(shield_map.map_range(range)));
    }
    match &mut node.block {
        Block::BlockQuote { children } => {
            for child in children {
                remap_block_node_ranges(child, shield_map, decode_map);
            }
        }
        Block::List { items, .. } => {
            for item in items {
                for child in &mut item.content {
                    remap_block_node_ranges(child, shield_map, decode_map);
                }
            }
        }
        _ => {}
    }
}

// --- frames ------------------------------------------------------------

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
        pending_inlines: Vec<Inline>,
        pending_start: Option<usize>,
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

fn start_tag(
    tag: Tag<'_>,
    range: Range<usize>,
    stack: &mut Vec<Frame>,
    section_path: &[String],
    doc_id: DocumentId,
) {
    flush_item_pending_inlines_if_block_boundary(stack, &tag, section_path, doc_id, range.start);

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
        Tag::Item => stack.push(Frame::Item {
            blocks: Vec::new(),
            pending_inlines: Vec::new(),
            pending_start: None,
        }),
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

fn flush_item_pending_inlines_if_block_boundary(
    stack: &mut [Frame],
    tag: &Tag<'_>,
    section_path: &[String],
    doc_id: DocumentId,
    boundary_end: usize,
) {
    let is_block_boundary = matches!(
        tag,
        Tag::Paragraph
            | Tag::Heading { .. }
            | Tag::BlockQuote
            | Tag::CodeBlock(_)
            | Tag::List(_)
            | Tag::Table(_)
            | Tag::HtmlBlock
            | Tag::MetadataBlock(_)
            | Tag::FootnoteDefinition(_)
    );
    if !is_block_boundary {
        return;
    }

    let Some(Frame::Item {
        blocks,
        pending_inlines,
        pending_start,
    }) = stack.last_mut()
    else {
        return;
    };

    if pending_inlines.is_empty() {
        return;
    }

    let start = pending_start.unwrap_or(boundary_end);
    let prov = Provenance::new(doc_id)
        .with_range(ByteRange::new(start, boundary_end.max(start)))
        .with_section_path(section_path.to_vec());
    let content = std::mem::take(pending_inlines);
    *pending_start = None;
    blocks.push(BlockNode::new(Block::Paragraph { content }, prov));
}

#[allow(clippy::too_many_arguments)]
fn end_tag(
    end: TagEnd,
    end_range: Range<usize>,
    source: &str,
    stack: &mut Vec<Frame>,
    top_body: &mut Vec<BlockNode>,
    section_path: &mut Vec<String>,
    heading_levels: &mut Vec<u8>,
    diagnostics: &mut Vec<Diagnostic>,
    doc_id: DocumentId,
    normalize_bare_callouts: bool,
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
            let prov = standard_prov(
                doc_id,
                section_path,
                start..end_range.end,
                ProvSectionPathBranch::Current,
            );
            let source_slice = &source[start..end_range.end];
            if normalize_bare_callouts
                && !in_blockquote(stack)
                && let Some(blockquote) =
                    heuristics::normalize_bare_callout_paragraph(source_slice, prov.clone())
            {
                append_block(stack, top_body, blockquote);
                return;
            }
            match detect_phase2b_tabular_fallback(source_slice) {
                Some(Phase2bParagraphBlock::Table { headers, rows }) => {
                    append_block(
                        stack,
                        top_body,
                        BlockNode::new(Block::Table { headers, rows }, prov),
                    );
                }
                Some(Phase2bParagraphBlock::GenericTableHint { content, message }) => {
                    diagnostics.push(
                        Diagnostic::new(DiagnosticKind::Lossy, message)
                            .with_range(ByteRange::new(start, end_range.end)),
                    );
                    append_block(
                        stack,
                        top_body,
                        BlockNode::new(
                            Block::GenericBlock {
                                content,
                                hint: Some("table-like:tab-delimited".to_string()),
                                confidence: 0.5,
                            },
                            prov,
                        ),
                    );
                }
                None => append_block(
                    stack,
                    top_body,
                    BlockNode::new(Block::Paragraph { content: inlines }, prov),
                ),
            }
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

            // Attribute this heading with the section path *above* it so the
            // first heading under "root" shows an empty path.
            let prov = standard_prov(
                doc_id,
                section_path,
                start..end_range.end,
                ProvSectionPathBranch::HeadingParent,
            );
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
            let prov = standard_prov(
                doc_id,
                section_path,
                start..end_range.end,
                ProvSectionPathBranch::Current,
            );
            append_block(
                stack,
                top_body,
                BlockNode::new(Block::CodeBlock { language, content }, prov),
            );
        }
        (Frame::BlockQuote { blocks, start }, TagEnd::BlockQuote) => {
            let prov = standard_prov(
                doc_id,
                section_path,
                start..end_range.end,
                ProvSectionPathBranch::Current,
            );
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
            let prov = standard_prov(
                doc_id,
                section_path,
                start..end_range.end,
                ProvSectionPathBranch::Current,
            );
            append_block(
                stack,
                top_body,
                BlockNode::new(Block::List { ordered, items }, prov),
            );
        }
        (
            Frame::Item {
                mut blocks,
                pending_inlines,
                pending_start,
            },
            TagEnd::Item,
        ) => {
            if !pending_inlines.is_empty() {
                let start = pending_start.unwrap_or(end_range.end);
                let prov = standard_prov(
                    doc_id,
                    section_path,
                    start..end_range.end.max(start),
                    ProvSectionPathBranch::Current,
                );
                blocks.push(BlockNode::new(
                    Block::Paragraph {
                        content: pending_inlines,
                    },
                    prov,
                ));
            }
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
            let prov = standard_prov(
                doc_id,
                section_path,
                start..end_range.end,
                ProvSectionPathBranch::Current,
            );
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
    if !attach_inline_checked(stack, inline.clone(), Some(range.start)) {
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
fn attach_inline_checked(stack: &mut [Frame], inline: Inline, range_start: Option<usize>) -> bool {
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
            Frame::Item {
                pending_inlines,
                pending_start,
                ..
            } => {
                if pending_start.is_none() {
                    *pending_start = range_start;
                }
                pending_inlines.push(inline);
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
    attach_inline_checked(stack, inline, None);
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
            Frame::BlockQuote { blocks, .. } | Frame::Item { blocks, .. } => {
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

enum ProvSectionPathBranch {
    Current,
    HeadingParent,
}

fn standard_prov(
    doc_id: DocumentId,
    section_path: &[String],
    range: Range<usize>,
    branch: ProvSectionPathBranch,
) -> Provenance {
    let mut path = section_path.to_vec();
    if matches!(branch, ProvSectionPathBranch::HeadingParent) {
        path.pop();
    }
    prov_for(doc_id, &path, range)
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
