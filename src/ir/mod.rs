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
    use super::{Block, Document, DocumentMeta, Inline, IrValidationError, SourceFormat, Style};
    use chrono::Utc;
    use uuid::Uuid;

    fn sample_meta() -> DocumentMeta {
        DocumentMeta {
            id: Uuid::new_v4(),
            source: super::SourceInfo {
                raw_source: "raw:markdown".to_string(),
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
}
