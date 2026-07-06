//! Pre-parse input repair and frontmatter handling for markdown sources.
//!
//! Everything in this module runs *before* the pulldown-cmark event walk:
//! escaped-newline stream decoding, fenced/unfenced frontmatter extraction,
//! and duplicated-leading-segment detection. These are markdown-input-repair
//! heuristics — they are not part of the generic parse contract and must not
//! be replicated by other format parsers.

use crate::ir::{Diagnostic, DiagnosticKind, Frontmatter};

pub(super) struct NormalizedInput {
    pub(super) input: String,
    pub(super) diagnostics: Vec<Diagnostic>,
}

pub(super) fn normalize_preparse_input(input: &str) -> NormalizedInput {
    let mut diagnostics = Vec::new();
    if appears_single_line_escaped_markdown(input) {
        let decoded = decode_escaped_newlines(input);
        if decoded != input {
            diagnostics.push(Diagnostic::new(
                DiagnosticKind::Normalized,
                "decoded escaped newline stream before markdown parse",
            ));
            return NormalizedInput {
                input: decoded,
                diagnostics,
            };
        }
    }

    NormalizedInput {
        input: input.to_string(),
        diagnostics,
    }
}

fn appears_single_line_escaped_markdown(input: &str) -> bool {
    let trimmed = input.trim_end_matches(['\n', '\r']);
    !trimmed.contains('\n') && trimmed.contains("\\n")
}

fn decode_escaped_newlines(input: &str) -> String {
    input.replace("\\r\\n", "\n").replace("\\n", "\n")
}

pub(super) fn extract_frontmatter(input: &str) -> (Option<Frontmatter>, usize) {
    if !input.starts_with("---") {
        return (None, 0);
    }
    let Some((first_line, mut offset)) = read_line(input, 0) else {
        return (None, 0);
    };
    if !is_frontmatter_delimiter(first_line) {
        return (None, 0);
    }

    let yaml_start = offset;
    while let Some((line, next_offset)) = read_line(input, offset) {
        if is_frontmatter_delimiter(line) {
            let yaml_slice = &input[yaml_start..offset];
            let yaml = yaml_slice.replace("\r\n", "\n");
            return (Some(Frontmatter { yaml }), next_offset);
        }
        offset = next_offset;
    }

    (None, 0)
}

pub(super) fn infer_probable_frontmatter(input: &str) -> Option<(Frontmatter, usize)> {
    if input.starts_with("---") {
        return None;
    }

    let mut offset = 0usize;
    let mut yaml_end = 0usize;
    let mut key_value_lines = 0usize;
    let mut saw_nonempty = false;

    while let Some((line, next_offset)) = read_line(input, offset) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        saw_nonempty = true;
        if is_probable_yaml_key_value_line(trimmed) {
            key_value_lines += 1;
            yaml_end = next_offset;
            offset = next_offset;
            continue;
        }
        break;
    }

    if !saw_nonempty || key_value_lines < 2 {
        return None;
    }

    Some((
        Frontmatter {
            yaml: input[..yaml_end].replace("\r\n", "\n"),
        },
        yaml_end,
    ))
}

fn is_probable_yaml_key_value_line(line: &str) -> bool {
    let Some((key, _value)) = line.split_once(':') else {
        return false;
    };
    if key.is_empty() {
        return false;
    }
    key.chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn is_frontmatter_delimiter(line: &str) -> bool {
    line.trim_end_matches('\r') == "---"
}

pub(super) fn read_line(input: &str, offset: usize) -> Option<(&str, usize)> {
    if offset > input.len() {
        return None;
    }
    if offset == input.len() {
        return None;
    }
    let bytes = input.as_bytes();
    let mut i = offset;
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }

    let (line_end, next_offset) = if i < bytes.len() && bytes[i] == b'\n' {
        let end = if i > offset && bytes[i - 1] == b'\r' {
            i - 1
        } else {
            i
        };
        (end, i + 1)
    } else {
        (i, i)
    };
    Some((&input[offset..line_end], next_offset))
}

#[derive(Debug, Clone)]
pub(super) struct DuplicateSegmentBoundary {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) signature: String,
}

pub(super) fn detect_repeated_leading_segment(
    input: &str,
    frontmatter_len: usize,
    frontmatter: Option<&Frontmatter>,
) -> Option<DuplicateSegmentBoundary> {
    const MIN_GAP_BYTES: usize = 128;

    let mut heading_line: Option<&str> = None;
    let mut heading_offset = frontmatter_len;
    let mut offset = frontmatter_len;
    while let Some((line, next_offset)) = read_line(input, offset) {
        let trimmed = line.trim();
        if !trimmed.is_empty() && trimmed.starts_with('#') {
            heading_line = Some(trimmed);
            heading_offset = offset;
            break;
        }
        if next_offset <= offset {
            break;
        }
        offset = next_offset;
    }

    let heading_line = heading_line?;
    let search_from = heading_offset.saturating_add(MIN_GAP_BYTES);
    if search_from >= input.len() {
        return None;
    }

    let needle = format!("\n{heading_line}\n");
    let rel = input[search_from..].find(&needle)?;
    let dup_start = search_from + rel + 1;

    if dup_start < (input.len() / 3) {
        return None;
    }

    let mut signature_parts: Vec<String> = Vec::new();
    if let Some(frontmatter) = frontmatter {
        for line in frontmatter.yaml.lines() {
            let trimmed = line.trim();
            if let Some((key, _)) = trimmed.split_once(':') {
                let key = key.trim();
                if !key.is_empty() {
                    signature_parts.push(format!("fm:{key}"));
                }
            }
            if signature_parts.len() >= 4 {
                break;
            }
        }
    }
    signature_parts.push(format!("h:{}", heading_line.trim_start_matches('#').trim()));

    Some(DuplicateSegmentBoundary {
        start: dup_start,
        end: input.len(),
        signature: signature_parts.join(","),
    })
}
