//! HTML emitter. Walks the IR and produces semantic HTML per spec §5.2.
//!
//! Output is a fragment (a sequence of block elements), not a full
//! `<html>` document — it is designed to be re-ingested by the HTML parser
//! (`Html::parse_document` wraps fragments automatically) and to embed inside
//! a larger page. Content is HTML-escaped; attribute values additionally
//! escape double quotes.
//!
//! Style mapping matches the HTML parser's inverse so that
//! markdown → IR → html → IR round-trips structurally:
//! Strong→`<strong>`, Emphasis→`<em>`, Strikethrough→`<del>`.

use crate::ir::{Block, BlockNode, Document, Inline, ListItem, Style};

/// Emit a document's body as an HTML fragment.
pub fn emit_html(document: &Document) -> String {
    let mut out = String::new();
    emit_blocks(&document.body, &mut out);
    out
}

fn emit_blocks(nodes: &[BlockNode], out: &mut String) {
    for (i, node) in nodes.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        emit_block(&node.block, out);
    }
}

fn emit_block(block: &Block, out: &mut String) {
    match block {
        Block::Heading { level, content } => {
            let level = (*level as usize).clamp(1, 6);
            out.push_str(&format!("<h{level}>"));
            emit_inlines(content, out);
            out.push_str(&format!("</h{level}>"));
        }
        Block::Paragraph { content } => {
            out.push_str("<p>");
            emit_inlines(content, out);
            out.push_str("</p>");
        }
        Block::CodeBlock { language, content } => {
            out.push_str("<pre><code");
            if let Some(lang) = language {
                out.push_str(&format!(" class=\"language-{}\"", escape_attr(lang)));
            }
            out.push('>');
            out.push_str(&escape_text(content));
            out.push_str("</code></pre>");
        }
        Block::Table { headers, rows } => emit_table(headers, rows, out),
        Block::List { ordered, items } => emit_list(*ordered, items, out),
        Block::BlockQuote { children } => {
            out.push_str("<blockquote>");
            emit_blocks(children, out);
            out.push_str("</blockquote>");
        }
        Block::GenericBlock { content, .. } => {
            out.push_str("<div class=\"generic-block\">");
            emit_inlines(content, out);
            out.push_str("</div>");
        }
        Block::ThematicBreak => out.push_str("<hr>"),
    }
}

fn emit_table(headers: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>], out: &mut String) {
    out.push_str("<table>");
    if !headers.is_empty() {
        out.push_str("<thead><tr>");
        for cell in headers {
            out.push_str("<th>");
            emit_inlines(cell, out);
            out.push_str("</th>");
        }
        out.push_str("</tr></thead>");
    }
    out.push_str("<tbody>");
    for row in rows {
        out.push_str("<tr>");
        for cell in row {
            out.push_str("<td>");
            emit_inlines(cell, out);
            out.push_str("</td>");
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table>");
}

fn emit_list(ordered: bool, items: &[ListItem], out: &mut String) {
    let tag = if ordered { "ol" } else { "ul" };
    out.push_str(&format!("<{tag}>"));
    for item in items {
        out.push_str("<li>");
        emit_list_item(&item.content, out);
        out.push_str("</li>");
    }
    out.push_str(&format!("</{tag}>"));
}

/// A list item that is a single paragraph is emitted "tight" (its inlines
/// directly inside `<li>`); anything richer emits its block children in full
/// so nested lists and multiple paragraphs survive.
fn emit_list_item(children: &[BlockNode], out: &mut String) {
    if let [only] = children
        && let Block::Paragraph { content } = &only.block
    {
        emit_inlines(content, out);
        return;
    }
    emit_blocks(children, out);
}

fn emit_inlines(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        emit_inline(inline, out);
    }
}

fn emit_inline(inline: &Inline, out: &mut String) {
    match inline {
        Inline::Text(s) => out.push_str(&escape_text(s)),
        Inline::Code(s) => {
            out.push_str("<code>");
            out.push_str(&escape_text(s));
            out.push_str("</code>");
        }
        Inline::Styled { style, children } => {
            let tag = match style {
                Style::Strong => "strong",
                Style::Emphasis => "em",
                Style::Strikethrough => "del",
            };
            out.push_str(&format!("<{tag}>"));
            emit_inlines(children, out);
            out.push_str(&format!("</{tag}>"));
        }
        Inline::Link { url, children } => {
            out.push_str(&format!("<a href=\"{}\">", escape_attr(url)));
            emit_inlines(children, out);
            out.push_str("</a>");
        }
        Inline::Image { url, alt } => {
            out.push_str(&format!(
                "<img src=\"{}\" alt=\"{}\">",
                escape_attr(url),
                escape_attr(alt.as_deref().unwrap_or(""))
            ));
        }
        Inline::LineBreak => out.push_str("<br>"),
    }
}

/// Escape text content: `&`, `<`, `>`.
fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Escape an attribute value: text escapes plus `"`.
fn escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{DocumentMeta, Provenance, SourceFormat, SourceInfo};
    use chrono::Utc;
    use uuid::Uuid;

    fn doc(body: Vec<BlockNode>) -> Document {
        Document {
            meta: DocumentMeta {
                id: Uuid::nil(),
                source: SourceInfo {
                    raw_source: "test".into(),
                },
                format: SourceFormat::Html,
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

    #[test]
    fn heading_and_paragraph() {
        let d = doc(vec![
            BlockNode::new(
                Block::Heading {
                    level: 2,
                    content: vec![Inline::Text("Hi".into())],
                },
                p(),
            ),
            BlockNode::new(
                Block::Paragraph {
                    content: vec![
                        Inline::Text("a ".into()),
                        Inline::Styled {
                            style: Style::Strong,
                            children: vec![Inline::Text("b".into())],
                        },
                    ],
                },
                p(),
            ),
        ]);
        assert_eq!(emit_html(&d), "<h2>Hi</h2>\n<p>a <strong>b</strong></p>");
    }

    #[test]
    fn escapes_text_and_attributes() {
        let d = doc(vec![BlockNode::new(
            Block::Paragraph {
                content: vec![
                    Inline::Text("1 < 2 & 3".into()),
                    Inline::Link {
                        url: "https://e.com/?a=1&b=2".into(),
                        children: vec![Inline::Text("x".into())],
                    },
                ],
            },
            p(),
        )]);
        assert_eq!(
            emit_html(&d),
            "<p>1 &lt; 2 &amp; 3<a href=\"https://e.com/?a=1&amp;b=2\">x</a></p>"
        );
    }

    #[test]
    fn code_block_with_language() {
        let d = doc(vec![BlockNode::new(
            Block::CodeBlock {
                language: Some("rust".into()),
                content: "fn main() {}".into(),
            },
            p(),
        )]);
        assert_eq!(
            emit_html(&d),
            "<pre><code class=\"language-rust\">fn main() {}</code></pre>"
        );
    }
}
