use serde::{Deserialize, Serialize};

use super::DocumentId;

/// Byte-range in the original source that produced a block.
///
/// End is exclusive. Phase 1 guarantees block-level granularity only;
/// inline byte ranges are not recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByteRange {
    pub start: usize,
    pub end: usize,
}

impl ByteRange {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn is_valid_within(&self, source_len: usize) -> bool {
        self.start <= self.end && self.end <= source_len
    }
}

/// Provenance recorded for each parsed block.
///
/// Phase 1 provenance is not inline-granular: it answers
/// "which document, which section, which byte range" at block level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// Document this block came from.
    pub document_id: DocumentId,
    /// Heading breadcrumb up to (and not including) this block.
    pub section_path: Vec<String>,
    /// Byte range into the original source string, if known.
    pub byte_range: Option<ByteRange>,
}

impl Provenance {
    pub fn new(document_id: DocumentId) -> Self {
        Self {
            document_id,
            section_path: Vec::new(),
            byte_range: None,
        }
    }

    pub fn with_range(mut self, range: ByteRange) -> Self {
        self.byte_range = Some(range);
        self
    }

    pub fn with_section_path(mut self, path: Vec<String>) -> Self {
        self.section_path = path;
        self
    }
}
