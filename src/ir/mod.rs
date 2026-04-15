use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Input source for ingestion. Re-exported at the crate root as `nucklavee::Source`.
///
/// Defined here (not in `lib.rs`) so leaf modules can depend on it without
/// creating a cycle through the crate root.
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
