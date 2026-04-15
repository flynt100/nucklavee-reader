use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Input source for ingestion. Re-exported at the crate root as `nucklavee::Source`.
// Lives in `ir` (not `lib.rs`) so leaf modules can reference it without
// routing back through the crate root.
#[derive(Debug, Clone)]
pub enum Source {
    File(PathBuf),
    Url(String),
    RawMarkdown(String),
    RawHtml(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub meta: DocumentMeta,
    pub body: Vec<Block>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentMeta {
    pub id: DocumentId,
    pub source: SourceInfo,
    pub format: SourceFormat,
    pub title: Option<String>,
    pub ingested_at: DateTime<Utc>,
    pub content_hash: String,
}

pub type DocumentId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SourceFormat {
    Markdown,
    Html,
    Pdf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceInfo {
    pub raw_source: String,
}

impl From<&Source> for SourceInfo {
    fn from(value: &Source) -> Self {
        let raw_source = match value {
            Source::File(path) => path.display().to_string(),
            Source::Url(url) => url.clone(),
            Source::RawMarkdown(_) => "raw:markdown".to_string(),
            Source::RawHtml(_) => "raw:html".to_string(),
        };

        Self { raw_source }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Block {
    Heading {
        level: u8,
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
        headers: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    List {
        ordered: bool,
        items: Vec<ListItem>,
    },
    BlockQuote {
        children: Vec<Block>,
    },
    GenericBlock {
        content: Vec<Inline>,
        hint: Option<String>,
        confidence: f32,
    },
    ThematicBreak,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListItem {
    pub content: Vec<Block>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Inline {
    Text(String),
    Styled { style: Style, children: Vec<Inline> },
    Code(String),
    Link { url: String, children: Vec<Inline> },
    Image { url: String, alt: Option<String> },
    LineBreak,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Style {
    Strong,
    Emphasis,
    Strikethrough,
}

impl Document {
    /// Structural invariants that every well-formed IR document must satisfy.
    /// Intended to be called on parser output before downstream processing.
    pub fn validate_strict(&self) -> Result<(), String> {
        for block in &self.body {
            validate_block(block)?;
        }
        Ok(())
    }

    /// Compare two documents for structural equivalence, ignoring whitespace
    /// differences within text runs. This is the roundtrip-test oracle:
    /// parse → emit → parse should yield a document `structural_eq` to the
    /// original, even if the emitter's whitespace doesn't match byte-for-byte.
    pub fn structural_eq(&self, other: &Self) -> bool {
        blocks_eq(&self.body, &other.body)
    }
}

fn validate_block(block: &Block) -> Result<(), String> {
    match block {
        Block::Heading { level, content } => {
            if !(1..=6).contains(level) {
                return Err(format!("heading level {level} out of range 1..=6"));
            }
            if content.is_empty() {
                return Err("heading has empty content".to_string());
            }
        }
        Block::Paragraph { content } => {
            if content.is_empty() {
                return Err("paragraph has empty content".to_string());
            }
        }
        Block::GenericBlock { confidence, .. } => {
            if !confidence.is_finite() || !(0.0..=1.0).contains(confidence) {
                return Err(format!("generic block confidence {confidence} out of range 0.0..=1.0"));
            }
        }
        Block::BlockQuote { children } => {
            for child in children {
                validate_block(child)?;
            }
        }
        Block::List { items, .. } => {
            for item in items {
                for child in &item.content {
                    validate_block(child)?;
                }
            }
        }
        Block::CodeBlock { .. } | Block::Table { .. } | Block::ThematicBreak => {}
    }
    Ok(())
}

fn blocks_eq(a: &[Block], b: &[Block]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| block_eq(x, y))
}

fn block_eq(a: &Block, b: &Block) -> bool {
    match (a, b) {
        (Block::Heading { level: la, content: ca }, Block::Heading { level: lb, content: cb }) => {
            la == lb && inlines_eq(ca, cb)
        }
        (Block::Paragraph { content: a }, Block::Paragraph { content: b }) => inlines_eq(a, b),
        (
            Block::CodeBlock { language: la, content: ca },
            Block::CodeBlock { language: lb, content: cb },
        ) => la == lb && ca.trim_end() == cb.trim_end(),
        (
            Block::Table { headers: ha, rows: ra },
            Block::Table { headers: hb, rows: rb },
        ) => {
            ha.len() == hb.len()
                && ha.iter().zip(hb).all(|(x, y)| inlines_eq(x, y))
                && ra.len() == rb.len()
                && ra.iter().zip(rb).all(|(row_a, row_b)| {
                    row_a.len() == row_b.len()
                        && row_a.iter().zip(row_b).all(|(c_a, c_b)| inlines_eq(c_a, c_b))
                })
        }
        (
            Block::List { ordered: oa, items: ia },
            Block::List { ordered: ob, items: ib },
        ) => {
            oa == ob
                && ia.len() == ib.len()
                && ia.iter().zip(ib).all(|(x, y)| blocks_eq(&x.content, &y.content))
        }
        (Block::BlockQuote { children: a }, Block::BlockQuote { children: b }) => blocks_eq(a, b),
        (
            Block::GenericBlock { content: ca, hint: ha, confidence: cfa },
            Block::GenericBlock { content: cb, hint: hb, confidence: cfb },
        ) => ha == hb && (cfa - cfb).abs() < f32::EPSILON && inlines_eq(ca, cb),
        (Block::ThematicBreak, Block::ThematicBreak) => true,
        _ => false,
    }
}

fn inlines_eq(a: &[Inline], b: &[Inline]) -> bool {
    let a = normalize_inlines(a);
    let b = normalize_inlines(b);
    a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| inline_eq(x, y))
}

fn inline_eq(a: &Inline, b: &Inline) -> bool {
    match (a, b) {
        (Inline::Text(a), Inline::Text(b)) => normalize_ws(a) == normalize_ws(b),
        (Inline::Code(a), Inline::Code(b)) => a == b,
        (
            Inline::Styled { style: sa, children: ca },
            Inline::Styled { style: sb, children: cb },
        ) => style_eq(sa, sb) && inlines_eq(ca, cb),
        (
            Inline::Link { url: ua, children: ca },
            Inline::Link { url: ub, children: cb },
        ) => ua == ub && inlines_eq(ca, cb),
        (
            Inline::Image { url: ua, alt: aa },
            Inline::Image { url: ub, alt: ab },
        ) => ua == ub && aa.as_deref().unwrap_or("") == ab.as_deref().unwrap_or(""),
        (Inline::LineBreak, Inline::LineBreak) => true,
        _ => false,
    }
}

fn style_eq(a: &Style, b: &Style) -> bool {
    matches!(
        (a, b),
        (Style::Strong, Style::Strong)
            | (Style::Emphasis, Style::Emphasis)
            | (Style::Strikethrough, Style::Strikethrough)
    )
}

/// Merge adjacent `Text` runs and collapse runs of internal whitespace so that
/// trivial whitespace differences from emitter formatting don't fail roundtrip
/// comparison.
fn normalize_inlines(inlines: &[Inline]) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(inlines.len());
    for inline in inlines {
        match inline {
            Inline::Text(s) => {
                let normalized = normalize_ws(s);
                if normalized.is_empty() {
                    continue;
                }
                if let Some(Inline::Text(prev)) = out.last_mut() {
                    if !prev.ends_with(' ') && !normalized.starts_with(' ') {
                        prev.push(' ');
                    }
                    prev.push_str(&normalized);
                } else {
                    out.push(Inline::Text(normalized));
                }
            }
            other => out.push(other.clone()),
        }
    }
    out
}

fn normalize_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !last_space && !out.is_empty() {
                out.push(' ');
            }
            last_space = true;
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Flatten inlines to plain text, discarding all styling and structure.
/// Used for title extraction and debug display.
pub fn inlines_to_plain_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        inline_to_plain_text(inline, &mut out);
    }
    out
}

fn inline_to_plain_text(inline: &Inline, out: &mut String) {
    match inline {
        Inline::Text(s) | Inline::Code(s) => out.push_str(s),
        Inline::Styled { children, .. } | Inline::Link { children, .. } => {
            for child in children {
                inline_to_plain_text(child, out);
            }
        }
        Inline::Image { alt, .. } => {
            if let Some(alt) = alt {
                out.push_str(alt);
            }
        }
        Inline::LineBreak => out.push(' '),
    }
}
