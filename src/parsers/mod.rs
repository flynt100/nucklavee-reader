//! Format parsers.
//!
//! There is deliberately **no cross-format `Parser` trait** (frozen
//! 2026-07-06; see `docs/ir-deltas-from-spec.md`): parse options are
//! format-specific, so each format exposes free functions and/or a struct
//! with inherent methods. Re-introduce a trait only when a real cross-format
//! abstraction is needed.

pub mod html;
pub mod markdown;
pub mod pdf;
