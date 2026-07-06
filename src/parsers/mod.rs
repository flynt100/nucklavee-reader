//! Format parsers.
//!
//! There is deliberately **no cross-format `Parser` trait** (frozen
//! 2026-07-06; see `docs/ir-deltas-from-spec.md`): parse options are
//! format-specific, so each format exposes free functions and/or a struct
//! with inherent methods. Re-introduce a trait only when a real cross-format
//! abstraction is needed.

use sha2::{Digest, Sha256};

use crate::ir::Inline;

pub mod html;
pub mod markdown;
pub mod pdf;

/// SHA-256 of the raw input, hex-encoded. Used for `DocumentMeta.content_hash`.
pub(crate) fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

/// Flatten inlines into a plain string (used for heading text in section
/// paths and document title extraction).
pub(crate) fn inlines_to_plain(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        push_plain(inline, &mut out);
    }
    out
}

fn push_plain(inline: &Inline, out: &mut String) {
    match inline {
        Inline::Text(s) => out.push_str(s),
        Inline::Code(s) => out.push_str(s),
        Inline::LineBreak => out.push(' '),
        Inline::Styled { children, .. } | Inline::Link { children, .. } => {
            for c in children {
                push_plain(c, out);
            }
        }
        Inline::Image { alt, .. } => {
            if let Some(alt) = alt {
                out.push_str(alt);
            }
        }
    }
}
