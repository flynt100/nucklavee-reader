use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::diagnostic::Diagnostic;
use super::provenance::Provenance;

/// Input source for ingestion.
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
    pub body: Vec<BlockNode>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Document {
    pub fn new(meta: DocumentMeta) -> Self {
        Self {
            meta,
            body: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentMeta {
    pub id: DocumentId,
    pub source: SourceInfo,
    pub format: SourceFormat,
    pub title: Option<String>,
    pub frontmatter: Option<Frontmatter>,
    pub ingested_at: DateTime<Utc>,
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frontmatter {
    /// YAML payload from a leading markdown frontmatter block, excluding
    /// opening/closing `---` delimiters and normalized to `\n` line endings.
    pub yaml: String,
}

pub type DocumentId = Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// A block paired with its provenance. `Document.body` is a sequence of these.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockNode {
    pub block: Block,
    pub prov: Provenance,
}

impl BlockNode {
    pub fn new(block: Block, prov: Provenance) -> Self {
        Self { block, prov }
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
        children: Vec<BlockNode>,
    },
    /// Catch-all for content that cannot be cleanly classified.
    GenericBlock {
        content: Vec<Inline>,
        hint: Option<String>,
        confidence: f32,
    },
    ThematicBreak,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListItem {
    pub content: Vec<BlockNode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Inline {
    Text(String),
    Styled { style: Style, children: Vec<Inline> },
    Code(String),
    Link { url: String, children: Vec<Inline> },
    Image { url: String, alt: Option<String> },
    LineBreak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Style {
    Strong,
    Emphasis,
    Strikethrough,
}
