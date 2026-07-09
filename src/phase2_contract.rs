//! Canonical phase-boundary contract strings and error helpers.
//!
//! Named `phase2_contract` for continuity; it now carries the Phase-3
//! boundary. Retired wholesale by the Phase-5 CLI rework (audit Task 10).

use crate::Error;

pub const QUERY_NOT_IMPLEMENTED: &str =
    "query is not implemented yet (Phase 4); ingest/emit only";
pub const CONTEXT_WINDOW_NOT_IMPLEMENTED: &str =
    "context_window is not implemented yet (Phase 4); ingest/emit only";
pub const HTML_PIPELINE_NOT_IMPLEMENTED: &str = "there is no standalone `html` command; html ingest is supported via `ingest`/`ingest-emit` on .html files (html emit arrives in Phase 3, Task 3)";
pub const PDF_PIPELINE_NOT_IMPLEMENTED: &str =
    "pdf pipeline is not implemented yet (Phase 6); ingest supports .md and .html files";
pub const EMIT_BY_ID_DISABLED: &str = "emit --id is disabled because document IDs are process-local. use `ingest-emit <path> --format markdown`";

/// Supported *emit* formats. Ingest additionally accepts HTML (Phase 3).
pub const SUPPORTED_FORMATS: &str = "markdown, html, text";

/// Supported ingest file extensions.
pub const SUPPORTED_EXTENSIONS: &str = ".md, .html, .htm";

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
