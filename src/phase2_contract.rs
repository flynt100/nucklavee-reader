//! Canonical Phase 2 boundary contract strings and error helpers.

use crate::Error;

pub const QUERY_NOT_IMPLEMENTED: &str =
    "query is not implemented in Phase 2 (markdown ingest/emit only)";
pub const CONTEXT_WINDOW_NOT_IMPLEMENTED: &str =
    "context_window is not implemented in Phase 2 (markdown ingest/emit only)";
pub const HTML_PIPELINE_NOT_IMPLEMENTED: &str =
    "html pipeline is not implemented in Phase 2; markdown only";
pub const PDF_PIPELINE_NOT_IMPLEMENTED: &str =
    "pdf pipeline is not implemented in Phase 2; markdown only";
pub const EMIT_BY_ID_DISABLED: &str = "emit --id is disabled in Phase 2 because document IDs are process-local. use `ingest-emit <path> --format markdown`";

pub const SUPPORTED_FORMATS: &str = "markdown";

pub fn unsupported_format_message(value: &str) -> String {
    format!("unsupported format '{value}'. supported: {SUPPORTED_FORMATS}")
}

pub fn not_implemented(message: &'static str) -> Error {
    Error::NotImplemented(message)
}

pub fn invalid_input(message: impl Into<String>) -> Error {
    Error::InvalidInput(message.into())
}
