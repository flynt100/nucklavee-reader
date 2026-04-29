//! Deterministic Markdown emitter. Walks the IR and produces normalized
//! Markdown text. Output is not byte-for-byte identical to arbitrary parser
//! input; it is a stable canonical form.
//!
//! Normalization rules used by the emitter:
//! - blocks separated by exactly one blank line
//! - unordered list marker `- `
//! - ordered list marker `N. ` (starting from 1)
//! - nested list indent: 2 spaces for unordered, 3 spaces for ordered children
//! - blockquote prefix `> ` on every emitted line
//! - fenced code blocks use ``` fences
//! - emphasis `*`, strong `**`, strikethrough `~~`
//! - hard line break: two trailing spaces + `\n`

use crate::Result;
use crate::emitters::Emitter;
use crate::ir::{Block, BlockNode, Document, Inline, Style};

#[derive(Debug, Default, Clone)]
pub struct MarkdownEmitter;

impl Emitter for MarkdownEmitter {
    fn emit(&self, document: &Document) -> Result<String> {
        Ok(emit_markdown(document))
    }
}

pub fn emit_markdown(document: &Document) -> String {
    let mut out = String::new();
    if let Some(frontmatter) = &document.meta.frontmatter {
        out.push_str(&format_frontmatter(frontmatter));
        if !document.body.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
    }
    emit_blocks(&document.body, &mut out);
    out
}

fn format_frontmatter(frontmatter: &crate::ir::Frontmatter) -> String {
    let mut yaml = frontmatter
        .yaml
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    format!("---\n{yaml}---\n")
}

fn emit_blocks(nodes: &[BlockNode], out: &mut String) {
    for (i, node) in nodes.iter().enumerate() {
        emit_block(&node.block, out);
        if i + 1 < nodes.len() {
            // Ensure blank line between blocks.
            if !out.ends_with("\n\n") {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push('\n');
            }
        }
    }
}

fn emit_block(block: &Block, out: &mut String) {
    match block {
        Block::Heading { level, content } => {
            for _ in 0..(*level as usize).clamp(1, 6) {
                out.push('#');
            }
            out.push(' ');
            emit_inlines(content, out);
        }
        Block::Paragraph { content } => {
            if !emit_display_math_paragraph(content, out) {
                emit_inlines(content, out);
            }
        }
        Block::CodeBlock { language, content } => {
            out.push_str("```");
            if let Some(lang) = language {
                out.push_str(lang);
            }
            out.push('\n');
            if !content.is_empty() {
                out.push_str(content);
                out.push('\n');
            }
            out.push_str("```");
        }
        Block::Table { headers, rows } => {
            emit_table(headers, rows, out);
        }
        Block::List { ordered, items } => {
            emit_list(*ordered, items, out);
        }
        Block::BlockQuote { children } => {
            let mut inner = String::new();
            emit_blocks(children, &mut inner);
            let prefixed: String = inner
                .lines()
                .map(|l| {
                    if l.is_empty() {
                        ">".to_string()
                    } else {
                        format!("> {l}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            out.push_str(&prefixed);
        }
        Block::GenericBlock { content, .. } => {
            emit_inlines(content, out);
        }
        Block::ThematicBreak => {
            out.push_str("---");
        }
    }
}

fn emit_list(ordered: bool, items: &[crate::ir::ListItem], out: &mut String) {
    for (idx, item) in items.iter().enumerate() {
        let marker = if ordered {
            format!("{}. ", idx + 1)
        } else {
            "- ".to_string()
        };
        let indent = " ".repeat(marker.len());

        let mut body = String::new();
        emit_blocks(&item.content, &mut body);

        let mut first_line = true;
        for line in body.split('\n') {
            if first_line {
                out.push_str(&marker);
                out.push_str(line);
                first_line = false;
            } else {
                out.push('\n');
                if line.is_empty() {
                    // keep a pure blank line without trailing indent whitespace
                } else {
                    out.push_str(&indent);
                    out.push_str(line);
                }
            }
        }
        if idx + 1 < items.len() {
            out.push('\n');
        }
    }
}

fn emit_table(headers: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>], out: &mut String) {
    // Header row
    out.push('|');
    for cell in headers {
        out.push(' ');
        let mut buf = String::new();
        emit_inlines(cell, &mut buf);
        out.push_str(&escape_table_cell(&buf));
        out.push_str(" |");
    }
    out.push('\n');

    // Alignment row (default alignment)
    out.push('|');
    for _ in headers {
        out.push_str(" --- |");
    }

    // Body rows
    for row in rows {
        out.push('\n');
        out.push('|');
        for cell in row {
            out.push(' ');
            let mut buf = String::new();
            emit_inlines(cell, &mut buf);
            out.push_str(&escape_table_cell(&buf));
            out.push_str(" |");
        }
    }
}

fn escape_table_cell(s: &str) -> String {
    // Escape pipe characters inside a cell so the table shape is preserved.
    // Newlines are not allowed inside a markdown table cell; convert to space.
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '|' => out.push_str("\\|"),
            '\n' => out.push(' '),
            _ => out.push(ch),
        }
    }
    out
}

fn emit_inlines(inlines: &[Inline], out: &mut String) {
    let mut i = 0usize;
    while i < inlines.len() {
        if let Some((consumed, math_span)) = collect_math_bracket_span(&inlines[i..]) {
            out.push_str(&math_span);
            i += consumed;
            continue;
        }
        emit_inline(&inlines[i], out);
        i += 1;
    }
}

fn collect_math_bracket_span(inlines: &[Inline]) -> Option<(usize, String)> {
    if !matches!(inlines.first(), Some(Inline::Text(s)) if s.trim() == "[") {
        return None;
    }

    let mut inner = String::new();
    let mut idx = 1usize;
    while idx < inlines.len() {
        match &inlines[idx] {
            Inline::Text(s) if s.trim() == "]" => {
                let trimmed = inner.trim();
                let looks_mathy = looks_like_math_inline(trimmed);
                if !looks_mathy || trimmed.is_empty() {
                    return None;
                }
                return Some((idx + 1, format!("\\[ {trimmed} \\]")));
            }
            Inline::Text(s) => inner.push_str(s),
            Inline::LineBreak => inner.push('\n'),
            _ => return None,
        }
        idx += 1;
    }
    None
}

fn emit_display_math_paragraph(content: &[Inline], out: &mut String) -> bool {
    let mut flat = String::new();
    for inline in content {
        match inline {
            Inline::Text(s) => flat.push_str(s),
            Inline::LineBreak => flat.push('\n'),
            _ => return false,
        }
    }

    let trimmed = flat.trim();
    let Some(inner) = parse_display_math_inner(trimmed) else {
        return false;
    };
    if inner.is_empty() {
        return false;
    }

    let looks_mathy = looks_like_math_inline(inner);
    if !looks_mathy {
        return false;
    }

    out.push_str("\\[");
    out.push_str(inner);
    out.push_str("\\]");
    true
}

fn parse_display_math_inner(trimmed: &str) -> Option<&str> {
    let inner = trimmed
        .strip_prefix("\\[")
        .and_then(|s| s.strip_suffix("\\]"))?;
    if inner.is_empty() {
        return None;
    }
    Some(inner)
}

fn looks_like_math_inline(s: &str) -> bool {
    s.contains('\\')
        || s.contains('=')
        || s.contains('^')
        || s.contains('_')
        || s.contains('{')
        || s.contains('}')
}

fn emit_inline(inline: &Inline, out: &mut String) {
    match inline {
        Inline::Text(s) => out.push_str(&escape_text(s)),
        Inline::Code(s) => {
            // Use an adaptive number of backticks to support content with
            // backticks inside. Find the longest run of backticks in the
            // content and use one more than that.
            let max_run = longest_backtick_run(s);
            let fence: String = "`".repeat(max_run + 1);
            out.push_str(&fence);
            if s.starts_with('`') || s.ends_with('`') {
                out.push(' ');
                out.push_str(s);
                out.push(' ');
            } else {
                out.push_str(s);
            }
            out.push_str(&fence);
        }
        Inline::Styled { style, children } => {
            let (open, close) = match style {
                Style::Emphasis => ("*", "*"),
                Style::Strong => ("**", "**"),
                Style::Strikethrough => ("~~", "~~"),
            };
            out.push_str(open);
            emit_inlines(children, out);
            out.push_str(close);
        }
        Inline::Link { url, children } => {
            out.push('[');
            emit_inlines(children, out);
            out.push(']');
            out.push('(');
            out.push_str(url);
            out.push(')');
        }
        Inline::Image { url, alt } => {
            out.push_str("![");
            if let Some(alt) = alt {
                out.push_str(&escape_text(alt));
            }
            out.push(']');
            out.push('(');
            out.push_str(url);
            out.push(')');
        }
        Inline::LineBreak => {
            out.push_str("  \n");
        }
    }
}

fn longest_backtick_run(s: &str) -> usize {
    let mut max = 0usize;
    let mut cur = 0usize;
    for ch in s.chars() {
        if ch == '`' {
            cur += 1;
            if cur > max {
                max = cur;
            }
        } else {
            cur = 0;
        }
    }
    max
}

fn escape_text(s: &str) -> String {
    // Escape inline markdown punctuation that can unintentionally restyle text.
    // Preserve syntax-like spans used in plain text (wikilinks, callout labels,
    // and inline-math-like `$...$` content) so they are not over-escaped.
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;

    while i < s.len() {
        let rest = &s[i..];

        // Preserve Obsidian-style wikilinks as-is.
        if let Some(tail) = rest.strip_prefix("[[") {
            if let Some(close) = tail.find("]]") {
                let end = i + 2 + close + 2;
                out.push_str(&s[i..end]);
                i = end;
                continue;
            }
        }

        // Preserve callout labels like `[!abstract]` as-is.
        if let Some(tail) = rest.strip_prefix("[!") {
            if let Some(close) = tail.find(']') {
                let end = i + 2 + close + 1;
                out.push_str(&s[i..end]);
                i = end;
                continue;
            }
        }

        // Preserve inline-math-like `$...$` spans as-is.
        if let Some(tail) = rest.strip_prefix('$') {
            if let Some(close) = tail.find('$') {
                let end = i + 1 + close + 1;
                out.push_str(&s[i..end]);
                i = end;
                continue;
            }
        }

        // Preserve display-math-like `\\[ ... \\]` spans as-is.
        if let Some(tail) = rest.strip_prefix("\\[") {
            if let Some(close) = tail.find("\\]") {
                let end = i + 2 + close + 2;
                out.push_str(&s[i..end]);
                i = end;
                continue;
            }
        }

        // Promote math-like bracket spans to escaped display delimiters so
        // parse->emit preserves `\\[ ... \\]` intent.
        if let Some(tail) = rest.strip_prefix('[') {
            if let Some(close) = tail.find(']') {
                let inner = &tail[..close];
                let looks_mathy = looks_like_math_inline(inner);
                if looks_mathy {
                    out.push_str("\\[");
                    out.push_str(inner);
                    out.push_str("\\]");
                    i += 1 + close + 1;
                    continue;
                }
            }
        }

        let ch = rest.chars().next().expect("slice is non-empty");
        match ch {
            '\\' | '`' | '*' | '_' | '~' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
        i += ch.len_utf8();
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BlockNode, DocumentMeta, Frontmatter, Provenance, SourceFormat, SourceInfo};
    use chrono::Utc;
    use uuid::Uuid;

    fn doc_from_blocks(body: Vec<BlockNode>) -> Document {
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

    fn p(doc_id: Uuid) -> Provenance {
        Provenance::new(doc_id)
    }

    #[test]
    fn heading_paragraph() {
        let id = Uuid::nil();
        let body = vec![
            BlockNode::new(
                Block::Heading {
                    level: 2,
                    content: vec![Inline::Text("Title".into())],
                },
                p(id),
            ),
            BlockNode::new(
                Block::Paragraph {
                    content: vec![Inline::Text("hello".into())],
                },
                p(id),
            ),
        ];
        let doc = doc_from_blocks(body);
        let md = emit_markdown(&doc);
        assert_eq!(md, "## Title\n\nhello");
    }

    #[test]
    fn thematic_break() {
        let id = Uuid::nil();
        let body = vec![BlockNode::new(Block::ThematicBreak, p(id))];
        let doc = doc_from_blocks(body);
        assert_eq!(emit_markdown(&doc), "---");
    }

    #[test]
    fn preserves_frontmatter_raw_before_body() {
        let id = Uuid::nil();
        let mut doc = doc_from_blocks(vec![BlockNode::new(
            Block::Paragraph {
                content: vec![Inline::Text("body".into())],
            },
            p(id),
        )]);
        doc.meta.frontmatter = Some(Frontmatter {
            yaml: "a: 1\n".to_string(),
        });
        assert_eq!(emit_markdown(&doc), "---\na: 1\n---\nbody");
    }

    #[test]
    fn parse_display_math_inner_preserves_trailing_backslash() {
        assert_eq!(parse_display_math_inner(r"\[x + y\\]"), Some(r"x + y\"));
    }

    #[test]
    fn parse_display_math_inner_preserves_escaped_delimiters_inside_content() {
        assert_eq!(
            parse_display_math_inner(r"\[a \\[ b \\] c\\]"),
            Some(r"a \\[ b \\] c\")
        );
    }

    #[test]
    fn parse_display_math_inner_rejects_malformed_delimiters() {
        assert_eq!(parse_display_math_inner(r"[x\]"), None);
        assert_eq!(parse_display_math_inner(r"\[x]"), None);
        assert_eq!(parse_display_math_inner(r"\\[x\]"), None);
        assert_eq!(parse_display_math_inner(r"\[x\\]"), Some(r"x\"));
    }
}
