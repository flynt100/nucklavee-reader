use crate::Result;
use crate::emitters::Emitter;
use crate::ir::{Block, Document, Inline, ListItem, Style};

#[derive(Debug, Default)]
pub struct MarkdownEmitter;

impl Emitter for MarkdownEmitter {
    fn emit(&self, document: &Document) -> Result<String> {
        let mut out = String::new();
        let mut first = true;
        for block in &document.body {
            if !first {
                out.push_str("\n\n");
            }
            first = false;
            emit_block(block, &mut out);
        }
        if !out.ends_with('\n') {
            out.push('\n');
        }
        Ok(out)
    }
}

fn emit_block(block: &Block, out: &mut String) {
    match block {
        Block::Heading { level, content } => {
            for _ in 0..*level {
                out.push('#');
            }
            out.push(' ');
            emit_inlines(content, out);
        }
        Block::Paragraph { content } => emit_inlines(content, out),
        Block::CodeBlock { language, content } => {
            out.push_str("```");
            if let Some(lang) = language {
                out.push_str(lang);
            }
            out.push('\n');
            out.push_str(content);
            if !content.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```");
        }
        Block::Table { headers, rows } => emit_table(headers, rows, out),
        Block::List { ordered, items } => emit_list(*ordered, items, out),
        Block::BlockQuote { children } => {
            let mut inner = String::new();
            let mut first = true;
            for child in children {
                if !first {
                    inner.push_str("\n\n");
                }
                first = false;
                emit_block(child, &mut inner);
            }
            for (i, line) in inner.lines().enumerate() {
                if i > 0 {
                    out.push('\n');
                }
                if line.is_empty() {
                    out.push('>');
                } else {
                    out.push_str("> ");
                    out.push_str(line);
                }
            }
        }
        Block::GenericBlock { content, .. } => emit_inlines(content, out),
        Block::ThematicBreak => out.push_str("---"),
    }
}

fn emit_inlines(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        emit_inline(inline, out);
    }
}

fn emit_inline(inline: &Inline, out: &mut String) {
    match inline {
        Inline::Text(s) => out.push_str(s),
        Inline::Code(s) => {
            out.push('`');
            out.push_str(s);
            out.push('`');
        }
        Inline::Styled { style, children } => {
            let delim = match style {
                Style::Strong => "**",
                Style::Emphasis => "*",
                Style::Strikethrough => "~~",
            };
            out.push_str(delim);
            emit_inlines(children, out);
            out.push_str(delim);
        }
        Inline::Link { url, children } => {
            out.push('[');
            emit_inlines(children, out);
            out.push_str("](");
            out.push_str(url);
            out.push(')');
        }
        Inline::Image { url, alt } => {
            out.push_str("![");
            if let Some(alt) = alt {
                out.push_str(alt);
            }
            out.push_str("](");
            out.push_str(url);
            out.push(')');
        }
        Inline::LineBreak => out.push_str("  \n"),
    }
}

fn emit_table(headers: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>], out: &mut String) {
    let col_count = headers.len().max(rows.iter().map(Vec::len).max().unwrap_or(0));
    emit_table_row(headers, col_count, out);
    out.push('\n');
    out.push('|');
    for _ in 0..col_count {
        out.push_str("----|");
    }
    for row in rows {
        out.push('\n');
        emit_table_row(row, col_count, out);
    }
}

fn emit_table_row(cells: &[Vec<Inline>], col_count: usize, out: &mut String) {
    out.push('|');
    for i in 0..col_count {
        out.push(' ');
        if let Some(cell) = cells.get(i) {
            emit_inlines(cell, out);
        }
        out.push_str(" |");
    }
}

fn emit_list(ordered: bool, items: &[ListItem], out: &mut String) {
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let marker = if ordered {
            format!("{}. ", i + 1)
        } else {
            "- ".to_string()
        };
        emit_list_item(item, &marker, out);
    }
}

fn emit_list_item(item: &ListItem, marker: &str, out: &mut String) {
    let indent = " ".repeat(marker.len());
    let mut inner = String::new();
    let mut first = true;
    for block in &item.content {
        if !first {
            inner.push_str("\n\n");
        }
        first = false;
        emit_block(block, &mut inner);
    }
    for (i, line) in inner.lines().enumerate() {
        if i == 0 {
            out.push_str(marker);
        } else {
            out.push('\n');
            if !line.is_empty() {
                out.push_str(&indent);
            }
        }
        out.push_str(line);
    }
}
