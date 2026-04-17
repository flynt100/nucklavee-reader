use serde::{Deserialize, Serialize};

use super::provenance::ByteRange;

/// A diagnostic emitted by the parser (or other IR producers) to surface
/// unsupported or lossy handling without silently degrading content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub message: String,
    /// Source byte range associated with the diagnostic, if known.
    pub byte_range: Option<ByteRange>,
}

impl Diagnostic {
    pub fn new(kind: DiagnosticKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            byte_range: None,
        }
    }

    pub fn with_range(mut self, range: ByteRange) -> Self {
        self.byte_range = Some(range);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticKind {
    /// Feature was encountered and ignored / skipped.
    Unsupported,
    /// Feature was partially represented; some detail was dropped.
    Lossy,
    /// Feature was intentionally normalized (not a loss, but a reformatting).
    Normalized,
}
