//! Format parsers.
//!
//! There is deliberately **no cross-format `Parser` trait** (frozen
//! 2026-07-06; see `docs/ir-deltas-from-spec.md`): parse options are
//! format-specific, so each format exposes a free `parse_*` function. The PDF
//! parser (`parsers::pdf`) arrives in Phase 6.

use sha2::{Digest, Sha256};

use crate::ir::Inline;

pub mod html;
pub mod markdown;

/// Default confidence for a `GenericBlock` produced when a parser cannot
/// classify content with certainty (HTML unknown elements, markdown TSV
/// fallback). Kept out of the strong-signal range so downstream thresholds
/// (spec default 0.7) treat it as low-confidence.
pub(crate) const GENERIC_BLOCK_DEFAULT_CONFIDENCE: f32 = 0.5;

/// Tracks the heading hierarchy during a linear document walk and produces
/// each heading's ancestor breadcrumb (`section_path`).
///
/// Shared by the markdown and HTML parsers so the "pop to this level, push
/// the new heading" algorithm has a single implementation. The path a block
/// sees is the hierarchy *above* it: the first `# H1` reports `[]`, a `## H2`
/// beneath it reports `["H1"]`, and so on.
#[derive(Debug, Default)]
pub(crate) struct SectionPathTracker {
    levels: Vec<u8>,
    path: Vec<String>,
}

impl SectionPathTracker {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Record a heading of `level` (1–6) with plain-text `title`, returning
    /// the section path *above* it (the path a consumer should attribute to
    /// the heading block itself).
    pub(crate) fn enter_heading(&mut self, level: u8, title: String) -> Vec<String> {
        while self.levels.last().map(|l| *l >= level).unwrap_or(false) {
            self.levels.pop();
            self.path.pop();
        }
        let parent = self.path.clone();
        self.levels.push(level);
        self.path.push(title);
        parent
    }

    /// The current section path (the hierarchy that non-heading blocks sit
    /// under).
    pub(crate) fn current(&self) -> Vec<String> {
        self.path.clone()
    }
}

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
