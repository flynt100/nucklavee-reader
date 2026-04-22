use thiserror::Error;

use super::provenance::Provenance;
use super::types::{Block, BlockNode, Document, Inline};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("heading level {0} is out of range (expected 1..=6)")]
    HeadingLevelOutOfRange(u8),
    #[error("generic block confidence {0} is out of range (expected 0.0..=1.0)")]
    ConfidenceOutOfRange(String),
    #[error("table row {row} has {got} cells, expected {want} (header count)")]
    TableRowShape { row: usize, got: usize, want: usize },
    #[error("table has rows but no header cells")]
    TableMissingHeader,
    #[error("list has zero items")]
    EmptyList,
    #[error("block provenance byte range is out of bounds for source of length {source_len}")]
    ProvenanceRangeOutOfBounds { source_len: usize },
    #[error("block provenance byte range is inverted (start {start} > end {end})")]
    ProvenanceRangeInverted { start: usize, end: usize },
    #[error("link url is empty")]
    LinkEmptyUrl,
    #[error("image url is empty")]
    ImageEmptyUrl,
}

/// Validate an entire document. Checks block structure and, if a
/// `source_len` is supplied, verifies provenance ranges are in bounds.
pub fn validate(doc: &Document, source_len: Option<usize>) -> Result<(), ValidationError> {
    for node in &doc.body {
        validate_block_node(node, source_len)?;
    }
    Ok(())
}

fn validate_block_node(node: &BlockNode, source_len: Option<usize>) -> Result<(), ValidationError> {
    validate_provenance(&node.prov, source_len)?;
    validate_block(&node.block, source_len)
}

fn validate_block(block: &Block, source_len: Option<usize>) -> Result<(), ValidationError> {
    match block {
        Block::Heading { level, content } => {
            if !(1..=6).contains(level) {
                return Err(ValidationError::HeadingLevelOutOfRange(*level));
            }
            for inline in content {
                validate_inline(inline)?;
            }
        }
        Block::Paragraph { content } => {
            for inline in content {
                validate_inline(inline)?;
            }
        }
        Block::CodeBlock { .. } => {}
        Block::Table { headers, rows } => {
            if headers.is_empty() && !rows.is_empty() {
                return Err(ValidationError::TableMissingHeader);
            }
            let want = headers.len();
            for (idx, row) in rows.iter().enumerate() {
                if row.len() != want {
                    return Err(ValidationError::TableRowShape {
                        row: idx,
                        got: row.len(),
                        want,
                    });
                }
                for cell in row {
                    for inline in cell {
                        validate_inline(inline)?;
                    }
                }
            }
            for cell in headers {
                for inline in cell {
                    validate_inline(inline)?;
                }
            }
        }
        Block::List { items, .. } => {
            if items.is_empty() {
                return Err(ValidationError::EmptyList);
            }
            for item in items {
                for child in &item.content {
                    validate_block_node(child, source_len)?;
                }
            }
        }
        Block::BlockQuote { children } => {
            for child in children {
                validate_block_node(child, source_len)?;
            }
        }
        Block::GenericBlock {
            confidence,
            content,
            ..
        } => {
            if !(0.0..=1.0).contains(confidence) || confidence.is_nan() {
                return Err(ValidationError::ConfidenceOutOfRange(format!(
                    "{confidence}"
                )));
            }
            for inline in content {
                validate_inline(inline)?;
            }
        }
        Block::ThematicBreak => {}
    }
    Ok(())
}

fn validate_inline(inline: &Inline) -> Result<(), ValidationError> {
    match inline {
        Inline::Text(_) | Inline::Code(_) | Inline::LineBreak => {}
        Inline::Styled { children, .. } => {
            for c in children {
                validate_inline(c)?;
            }
        }
        Inline::Link { url, children } => {
            if url.is_empty() {
                return Err(ValidationError::LinkEmptyUrl);
            }
            for c in children {
                validate_inline(c)?;
            }
        }
        Inline::Image { url, .. } => {
            if url.is_empty() {
                return Err(ValidationError::ImageEmptyUrl);
            }
        }
    }
    Ok(())
}

fn validate_provenance(
    prov: &Provenance,
    source_len: Option<usize>,
) -> Result<(), ValidationError> {
    if let Some(range) = prov.byte_range {
        if range.start > range.end {
            return Err(ValidationError::ProvenanceRangeInverted {
                start: range.start,
                end: range.end,
            });
        }
        if let Some(len) = source_len
            && range.end > len
        {
            return Err(ValidationError::ProvenanceRangeOutOfBounds { source_len: len });
        }
    }
    Ok(())
}
