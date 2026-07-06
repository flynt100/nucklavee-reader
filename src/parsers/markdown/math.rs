//! Math-span shielding for markdown parsing.
//!
//! `$...$` and `\[...\]` spans are replaced by a placeholder character before
//! the pulldown-cmark walk so their contents are not interpreted as markdown,
//! then restored verbatim into the produced `Inline::Text` values. This is a
//! markdown-input-repair heuristic private to the markdown parser.

use std::collections::VecDeque;
use std::ops::Range;

use crate::ir::{ByteRange, Diagnostic, DiagnosticKind};

pub(super) const MATH_PLACEHOLDER: char = '\u{00A4}';

/// Shared heuristic: does a flattened text span look like math content?
///
/// Used by the markdown emitter to decide whether bracketed spans should be
/// (re-)wrapped in `\[ ... \]` display delimiters. Requires a *strong* math
/// signal; a bare `_` is deliberately not sufficient because prose like
/// `[see chapter_3]` must stay literal text.
pub(crate) fn looks_like_math_inline(s: &str) -> bool {
    s.contains('\\') || s.contains('=') || s.contains('^') || s.contains('{') || s.contains('}')
}

#[derive(Debug, Default)]
pub(super) struct ShieldedMathInput {
    pub(super) shielded_input: String,
    pub(super) payloads: Vec<String>,
    pub(super) diagnostics: Vec<Diagnostic>,
}

#[derive(Debug)]
pub(super) struct MathRestoreState {
    pending: VecDeque<String>,
}

impl MathRestoreState {
    pub(super) fn new(payloads: Vec<String>) -> Self {
        Self {
            pending: VecDeque::from(payloads),
        }
    }
}

pub(super) fn shield_math_segments(input: &str, range_offset: usize) -> ShieldedMathInput {
    let mut out = String::with_capacity(input.len());
    let mut payloads = Vec::new();
    let mut diagnostics = Vec::new();
    let mut i = 0usize;

    while i < input.len() {
        if let Some((end, payload, diagnostic)) = try_match_math_span(input, i) {
            if let Some(message) = diagnostic {
                diagnostics.push(
                    Diagnostic::new(DiagnosticKind::Normalized, message)
                        .with_range(ByteRange::new(range_offset + i, range_offset + end)),
                );
                out.push_str(&input[i..end]);
            } else {
                payloads.push(payload);
                out.push(MATH_PLACEHOLDER);
            }
            i = end;
            continue;
        }

        let mut iter = input[i..].char_indices();
        let (_, ch) = iter
            .next()
            .expect("scanner should always have remaining char");
        out.push(ch);
        i += ch.len_utf8();
    }

    ShieldedMathInput {
        shielded_input: out,
        payloads,
        diagnostics,
    }
}

fn try_match_math_span(input: &str, start: usize) -> Option<(usize, String, Option<String>)> {
    if input[start..].starts_with('$') && !is_escaped_delimiter(input, start) {
        let end = find_unescaped_char(input, start + 1, '$');
        return match end {
            Some(close) => {
                if input[start + 1..close].is_empty() || input[start + 1..close].contains('\n') {
                    Some((
                        close + 1,
                        String::new(),
                        Some("skipped ambiguous inline math span during shielding".to_string()),
                    ))
                } else {
                    Some((close + 1, input[start..close + 1].to_string(), None))
                }
            }
            None => Some((
                start + 1,
                String::new(),
                Some("found unmatched '$' delimiter; leaving text unchanged".to_string()),
            )),
        };
    }

    if input[start..].starts_with("\\[") && !is_escaped_delimiter(input, start) {
        let end = find_unescaped_substring_before_paragraph_break(input, start + 2, "\\]");
        return match end {
            Some(close_start) => Some((
                close_start + 2,
                input[start..close_start + 2].to_string(),
                None,
            )),
            None => Some((
                start + 2,
                String::new(),
                Some("found unmatched '\\\\[' delimiter; leaving text unchanged".to_string()),
            )),
        };
    }

    None
}

fn is_escaped_delimiter(input: &str, idx: usize) -> bool {
    let bytes = input.as_bytes();
    let mut slash_count = 0usize;
    let mut p = idx;
    while p > 0 && bytes[p - 1] == b'\\' {
        slash_count += 1;
        p -= 1;
    }
    slash_count % 2 == 1
}

fn find_unescaped_char(input: &str, mut idx: usize, target: char) -> Option<usize> {
    while idx < input.len() {
        let mut iter = input[idx..].char_indices();
        let (rel, ch) = iter.next()?;
        let at = idx + rel;
        if ch == target && !is_escaped_delimiter(input, at) {
            return Some(at);
        }
        idx = at + ch.len_utf8();
    }
    None
}

fn find_unescaped_substring_before_paragraph_break(
    input: &str,
    mut idx: usize,
    needle: &str,
) -> Option<usize> {
    while idx < input.len() {
        let rel = input[idx..].find(needle)?;
        let at = idx + rel;
        if input[idx..at].contains("\n\n") {
            return None;
        }
        if !is_escaped_delimiter(input, at) {
            return Some(at);
        }
        idx = at + needle.len();
    }
    None
}

pub(super) fn restore_math_placeholders(
    parsed_text: String,
    restore: &mut MathRestoreState,
    diagnostics: &mut Vec<Diagnostic>,
    range: Range<usize>,
) -> String {
    if !parsed_text.contains(MATH_PLACEHOLDER) {
        return parsed_text;
    }
    let mut out = String::with_capacity(parsed_text.len());
    for ch in parsed_text.chars() {
        if ch == MATH_PLACEHOLDER {
            if let Some(payload) = restore.pending.pop_front() {
                out.push_str(&payload);
            } else {
                diagnostics.push(
                    Diagnostic::new(
                        DiagnosticKind::Normalized,
                        "math shielding restore placeholder had no matching payload; leaving placeholder as-is",
                    )
                    .with_range(ByteRange::new(range.start, range.end)),
                );
                out.push(ch);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

pub(super) fn restore_escaped_math_brackets(parsed_text: String, source_slice: &str) -> String {
    if source_slice == "\\[" && parsed_text == "[" {
        return "\\[".to_string();
    }
    if source_slice == "\\]" && parsed_text == "]" {
        return "\\]".to_string();
    }
    parsed_text
}
