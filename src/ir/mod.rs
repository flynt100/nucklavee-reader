//! Intermediate representation shared by every parser and emitter.
//!
//! Phase 1 provenance is recorded at block granularity via [`BlockNode`],
//! which wraps a [`Block`] with a [`Provenance`] value. Unsupported or lossy
//! parser behavior is surfaced through [`Diagnostic`] entries on the
//! produced [`Document`].

pub mod diagnostic;
pub mod equiv;
pub mod normalize;
pub mod provenance;
pub mod types;
pub mod validate;

pub use diagnostic::{Diagnostic, DiagnosticKind};
pub use equiv::{structural_diff, structurally_equivalent};
pub use normalize::normalize_document;
pub use provenance::{ByteRange, Provenance};
pub use types::{
    Block, BlockNode, Document, DocumentId, DocumentMeta, Inline, ListItem, Source, SourceFormat,
    SourceInfo, Style,
};
pub use validate::{ValidationError, validate};
