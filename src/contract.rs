//! Canonical error strings and helpers for library/CLI boundaries.

use crate::Error;

/// Supported *emit* formats. Ingest additionally accepts HTML and (via URL)
/// content-type sniffing.
pub const SUPPORTED_FORMATS: &str = "markdown, html, text";

/// Supported ingest file extensions.
pub const SUPPORTED_EXTENSIONS: &str = ".md, .html, .htm";

/// Message for a `.pdf` ingest attempt (Phase 6).
pub const PDF_PIPELINE_NOT_IMPLEMENTED: &str =
    "pdf ingest is not implemented yet (Phase 6); supported: .md, .html, .htm";

pub fn unsupported_format_message(value: &str) -> String {
    format!("unsupported format '{value}'. supported: {SUPPORTED_FORMATS}")
}

pub fn unsupported_extension_message(ext: &str) -> String {
    format!("unsupported file extension '{ext}'. supported: {SUPPORTED_EXTENSIONS}")
}

pub fn not_implemented(message: &'static str) -> Error {
    Error::NotImplemented(message)
}

pub fn invalid_input(message: impl Into<String>) -> Error {
    Error::InvalidInput(message.into())
}
