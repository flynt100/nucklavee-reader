//! Plain-text emitter (spec §5.3). Strips all formatting to a readable,
//! deterministic rendering used for embedding input and context-window
//! assembly. There is no plain-text *parser*, so this is a terminal output —
//! no roundtrip contract, only golden stability.
//!
//! Rules:
//! - Headings: UPPERCASED.
//! - Paragraphs: inline text with styling stripped; links render `text (url)`.
//! - Code blocks: every line indented 4 spaces.
//! - Lists: `- ` (unordered) / `N. ` (ordered) markers; nested content indented.
//! - Block quotes: every line prefixed with `| `.
//! - Tables: pipe-delimited rows (header first), no alignment row.
//! - Thematic breaks: a short dashed rule.
//!
//! Blocks are separated by a single blank line.

use crate::Result;
use crate::emitters::Emitter;
use crate::ir::{Block, BlockNode, Document, Inline, ListItem};

#[derive(Debug, Default, Clone)]
pub struct PlainTextEmitter;

impl Emitter for PlainTextEmitter {
    fn emit(&self, document: &Document) -> Result<String> {
        Ok(emit_text(document))
    }
}

/// Emit a document's body as plain text.
pub fn emit_text(document: &Document) -> String {
    blocks_to_text(&document.body)
}

/// Render a block sequence, one blank line between blocks.
fn blocks_to_text(nodes: &[BlockNode]) -> String {
    nodes
        .iter()
        .map(|node| block_to_text(&node.block))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn block_to_text(block: &Block) -> String {
    match block {
        Block::Heading { content, .. } => render_inlines(content, " ").to_uppercase(),
        Block::Paragraph { content } => render_inlines(content, "\n"),
        Block::CodeBlock { content, .. } => content
            .lines()
            .map(|line| format!("    {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
        Block::Table { headers, rows } => table_to_text(headers, rows),
        Block::List { ordered, items } => list_to_text(*ordered, items),
        Block::BlockQuote { children } => {
            let inner = blocks_to_text(children);
            prefix_lines(&inner, "| ", "|")
        }
        Block::GenericBlock { content, .. } => render_inlines(content, "\n"),
        Block::ThematicBreak => "----".to_string(),
    }
}

fn table_to_text(headers: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>]) -> String {
    let mut lines: Vec<String> = Vec::new();
    if !headers.is_empty() {
        lines.push(row_to_text(headers));
    }
    for row in rows {
        lines.push(row_to_text(row));
    }
    lines.join("\n")
}

fn row_to_text(cells: &[Vec<Inline>]) -> String {
    cells
        .iter()
        .map(|cell| render_inlines(cell, " "))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn list_to_text(ordered: bool, items: &[ListItem]) -> String {
    let mut out: Vec<String> = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        let marker = if ordered {
            format!("{}. ", idx + 1)
        } else {
            "- ".to_string()
        };
        let indent = " ".repeat(marker.len());
        let body = blocks_to_text(&item.content);

        let mut lines = body.lines();
        let first = lines.next().unwrap_or("");
        let mut rendered = format!("{marker}{first}");
        for line in lines {
            rendered.push('\n');
            if line.is_empty() {
                // keep blank lines blank (no trailing indent whitespace)
            } else {
                rendered.push_str(&indent);
                rendered.push_str(line);
            }
        }
        out.push(rendered);
    }
    out.join("\n")
}

/// Prefix every line: `nonempty_prefix` for content lines, `empty_prefix` for
/// blank lines (so quote gutters stay clean without trailing spaces).
fn prefix_lines(text: &str, nonempty_prefix: &str, empty_prefix: &str) -> String {
    text.lines()
        .map(|line| {
            if line.is_empty() {
                empty_prefix.to_string()
            } else {
                format!("{nonempty_prefix}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Flatten inlines to plain text with all styling removed. `line_break` is the
/// substitution for `Inline::LineBreak` (`"\n"` inside paragraphs, `" "` for
/// headings/cells where a single logical line is wanted).
fn render_inlines(inlines: &[Inline], line_break: &str) -> String {
    let mut out = String::new();
    for inline in inlines {
        render_inline(inline, line_break, &mut out);
    }
    out
}

fn render_inline(inline: &Inline, line_break: &str, out: &mut String) {
    match inline {
        Inline::Text(s) => out.push_str(s),
        Inline::Code(s) => out.push_str(s),
        Inline::Styled { children, .. } => {
            for child in children {
                render_inline(child, line_break, out);
            }
        }
        Inline::Link { url, children } => {
            let text = render_inlines(children, " ");
            if text.is_empty() {
                out.push_str(&format!("({url})"));
            } else {
                out.push_str(&format!("{text} ({url})"));
            }
        }
        Inline::Image { url, alt } => {
            let alt = alt.as_deref().unwrap_or("");
            if alt.is_empty() {
                out.push_str(&format!("(image: {url})"));
            } else {
                out.push_str(&format!("{alt} (image: {url})"));
            }
        }
        Inline::LineBreak => out.push_str(line_break),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{DocumentMeta, Provenance, SourceFormat, SourceInfo, Style};
    use chrono::Utc;
    use uuid::Uuid;

    fn doc(body: Vec<BlockNode>) -> Document {
        Document {
            meta: DocumentMeta {
                id: Uuid::nil(),
                source: SourceInfo {
                    raw_source: "test".into(),
                },
                format: SourceFormat::Markdown,
                title: None,
                frontmatter: None,
                ingested_at: Utc::now(),
                content_hash: String::new(),
            },
            body,
            diagnostics: Vec::new(),
        }
    }

    fn p() -> Provenance {
        Provenance::new(Uuid::nil())
    }

    fn n(block: Block) -> BlockNode {
        BlockNode::new(block, p())
    }

    #[test]
    fn heading_uppercased_and_paragraph_plain() {
        let d = doc(vec![
            n(Block::Heading {
                level: 2,
                content: vec![Inline::Text("Hello World".into())],
            }),
            n(Block::Paragraph {
                content: vec![
                    Inline::Text("a ".into()),
                    Inline::Styled {
                        style: Style::Strong,
                        children: vec![Inline::Text("bold".into())],
                    },
                    Inline::Text(" word".into()),
                ],
            }),
        ]);
        assert_eq!(emit_text(&d), "HELLO WORLD\n\na bold word");
    }

    #[test]
    fn link_renders_text_and_url() {
        let d = doc(vec![n(Block::Paragraph {
            content: vec![
                Inline::Text("see ".into()),
                Inline::Link {
                    url: "https://e.com/x".into(),
                    children: vec![Inline::Text("here".into())],
                },
            ],
        })]);
        assert_eq!(emit_text(&d), "see here (https://e.com/x)");
    }

    #[test]
    fn code_block_indented_four_spaces() {
        let d = doc(vec![n(Block::CodeBlock {
            language: Some("rust".into()),
            content: "fn main() {\n    ok();\n}".into(),
        })]);
        assert_eq!(emit_text(&d), "    fn main() {\n        ok();\n    }");
    }

    #[test]
    fn blockquote_prefixed_with_bar() {
        let d = doc(vec![n(Block::BlockQuote {
            children: vec![n(Block::Paragraph {
                content: vec![Inline::Text("quoted line".into())],
            })],
        })]);
        assert_eq!(emit_text(&d), "| quoted line");
    }

    #[test]
    fn table_pipe_delimited_no_alignment_row() {
        let d = doc(vec![n(Block::Table {
            headers: vec![
                vec![Inline::Text("Name".into())],
                vec![Inline::Text("Value".into())],
            ],
            rows: vec![vec![
                vec![Inline::Text("a".into())],
                vec![Inline::Text("1".into())],
            ]],
        })]);
        assert_eq!(emit_text(&d), "Name | Value\na | 1");
    }

    #[test]
    fn nested_list_indented() {
        let d = doc(vec![n(Block::List {
            ordered: false,
            items: vec![ListItem {
                content: vec![
                    n(Block::Paragraph {
                        content: vec![Inline::Text("outer".into())],
                    }),
                    n(Block::List {
                        ordered: false,
                        items: vec![ListItem {
                            content: vec![n(Block::Paragraph {
                                content: vec![Inline::Text("inner".into())],
                            })],
                        }],
                    }),
                ],
            }],
        })]);
        // A nested list is a distinct block, so it is separated by a blank
        // line (which carries no indent) and its items are indented.
        assert_eq!(emit_text(&d), "- outer\n\n  - inner");
    }
}
