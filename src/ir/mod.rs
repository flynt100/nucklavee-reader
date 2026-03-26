use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Source;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub meta: DocumentMeta,
    pub body: Vec<Block>,
}

impl Document {
    /// Validates strict IR invariants intended for parser outputs.
    pub fn validate_strict(&self) -> Result<(), IrValidationError> {
        for block in &self.body {
            block.validate_strict()?;
        }

        Ok(())
    }
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SourceKind {
    File,
    Url,
    RawMarkdown,
    RawHtml,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceInfo {
    /// Normalized source kind suitable for compact storage and filtering.
    pub kind: SourceKind,
    /// User-visible locator when available (e.g., file path, URL).
    pub locator: Option<String>,
    /// Stable canonical source string for persistence and display.
    pub canonical: String,
    /// Raw payload size when source is inline content.
    pub payload_bytes: Option<usize>,
}

impl SourceInfo {
    pub fn from_source(source: &Source) -> Self {
        match source {
            Source::File(path) => {
                let locator = path.display().to_string();
                Self {
                    kind: SourceKind::File,
                    canonical: format!("file://{locator}"),
                    locator: Some(locator),
                    payload_bytes: None,
                }
            }
            Source::Url(url) => Self {
                kind: SourceKind::Url,
                canonical: url.clone(),
                locator: Some(url.clone()),
                payload_bytes: None,
            },
            Source::RawMarkdown(text) => Self {
                kind: SourceKind::RawMarkdown,
                locator: None,
                canonical: "raw:markdown".to_string(),
                payload_bytes: Some(text.len()),
            },
            Source::RawHtml(text) => Self {
                kind: SourceKind::RawHtml,
                locator: None,
                canonical: "raw:html".to_string(),
                payload_bytes: Some(text.len()),
            },
        }
    }
}

impl From<&Source> for SourceInfo {
    fn from(value: &Source) -> Self {
        Self::from_source(value)
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

impl Block {
    /// Explicit constructor for uncertain structure.
    pub fn generic(content: Vec<Inline>, hint: Option<String>, confidence: f32) -> Self {
        let clamped_confidence = confidence.clamp(0.0, 1.0);

        Self::GenericBlock {
            content,
            hint,
            confidence: clamped_confidence,
        }
    }

    pub fn validate_strict(&self) -> Result<(), IrValidationError> {
        match self {
            Self::Heading { level, .. } if !(1..=6).contains(level) => {
                Err(IrValidationError::InvalidHeadingLevel(*level))
            }
            Self::List { items, .. } => {
                for item in items {
                    for child in &item.content {
                        child.validate_strict()?;
                    }
                }
                Ok(())
            }
            Self::BlockQuote { children } => {
                for child in children {
                    child.validate_strict()?;
                }
                Ok(())
            }
            Self::GenericBlock { confidence, .. } if !(0.0..=1.0).contains(confidence) => {
                Err(IrValidationError::InvalidConfidence(*confidence))
            }
            _ => Ok(()),
        }
    }
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

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum IrValidationError {
    #[error("invalid heading level: {0}; expected 1..=6")]
    InvalidHeadingLevel(u8),

    #[error("invalid generic block confidence: {0}; expected 0.0..=1.0")]
    InvalidConfidence(f32),
}

#[cfg(test)]
mod tests {
    use super::{
        Block, Document, DocumentMeta, Inline, IrValidationError, SourceFormat, SourceInfo,
        SourceKind, Style,
    };
    use crate::Source;
    use chrono::Utc;
    use uuid::Uuid;

    fn sample_meta() -> DocumentMeta {
        DocumentMeta {
            id: Uuid::new_v4(),
            source: SourceInfo {
                kind: SourceKind::RawMarkdown,
                locator: None,
                canonical: "raw:markdown".to_string(),
                payload_bytes: Some(0),
            },
            format: SourceFormat::Markdown,
            title: None,
            ingested_at: Utc::now(),
            content_hash: "abc123".to_string(),
        }
    }

    #[test]
    fn document_validate_strict_accepts_valid_blocks() {
        let doc = Document {
            meta: sample_meta(),
            body: vec![
                Block::Heading {
                    level: 1,
                    content: vec![Inline::Text("Title".to_string())],
                },
                Block::Paragraph {
                    content: vec![Inline::Styled {
                        style: Style::Emphasis,
                        children: vec![Inline::Text("body".to_string())],
                    }],
                },
                Block::generic(vec![Inline::Text("uncertain".to_string())], None, 0.4),
            ],
        };

        assert!(doc.validate_strict().is_ok());
    }

    #[test]
    fn document_validate_strict_rejects_invalid_heading_level() {
        let doc = Document {
            meta: sample_meta(),
            body: vec![Block::Heading {
                level: 0,
                content: vec![Inline::Text("invalid".to_string())],
            }],
        };

        let err = doc
            .validate_strict()
            .expect_err("expected invalid heading error");
        assert!(matches!(err, IrValidationError::InvalidHeadingLevel(0)));
    }

    #[test]
    fn generic_constructor_clamps_confidence_to_valid_range() {
        let block = Block::generic(vec![Inline::Text("uncertain".to_string())], None, 2.4);

        let Block::GenericBlock { confidence, .. } = block else {
            panic!("expected generic block");
        };

        assert_eq!(confidence, 1.0);
    }

    #[test]
    fn source_info_normalizes_file_source() {
        let source = Source::File("./docs/spec.md".into());
        let info = SourceInfo::from_source(&source);

        assert_eq!(info.kind, SourceKind::File);
        assert_eq!(info.locator.as_deref(), Some("./docs/spec.md"));
        assert_eq!(info.canonical, "file://./docs/spec.md");
        assert_eq!(info.payload_bytes, None);
    }

    #[test]
    fn source_info_normalizes_raw_markdown_source() {
        let source = Source::RawMarkdown("# Title".to_string());
        let info = SourceInfo::from_source(&source);

        assert_eq!(info.kind, SourceKind::RawMarkdown);
        assert_eq!(info.locator, None);
        assert_eq!(info.canonical, "raw:markdown");
        assert_eq!(info.payload_bytes, Some(7));
    }
}
