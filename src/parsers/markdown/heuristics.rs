//! Post-parse block-level repair heuristics for markdown paragraphs.
//!
//! Phase-2B additions: promoting tab-delimited paragraph text to `Block::Table`
//! and rewriting bare Obsidian-style callout paragraphs into blockquotes.
//! Private to the markdown parser; other format parsers must not use these.

use crate::ir::{Block, BlockNode, Inline, Provenance};

pub(super) enum Phase2bParagraphBlock {
    Table {
        headers: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    GenericTableHint {
        content: Vec<Inline>,
        message: String,
    },
}

pub(super) fn detect_phase2b_tabular_fallback(source_slice: &str) -> Option<Phase2bParagraphBlock> {
    let normalized = source_slice
        .trim()
        .trim_matches('\n')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let lines: Vec<&str> = normalized.lines().collect();
    if lines.len() < 2 {
        return None;
    }

    let mut widths: Vec<usize> = Vec::with_capacity(lines.len());
    let mut tabbed_lines = 0usize;
    let mut max_width = 0usize;
    for line in &lines {
        if line.contains('\t') {
            tabbed_lines += 1;
            let width = line.split('\t').count();
            max_width = max_width.max(width);
            widths.push(width);
        } else {
            widths.push(1);
        }
    }

    if tabbed_lines < 2 || max_width < 2 {
        return None;
    }

    let first_width = widths[0];
    let all_rows_have_tabs = lines.iter().all(|line| line.contains('\t'));
    let shape_consistent = first_width >= 2 && widths.iter().all(|w| *w == first_width);
    if all_rows_have_tabs && shape_consistent {
        let mut parsed_rows: Vec<Vec<Vec<Inline>>> = lines
            .iter()
            .map(|line| {
                line.split('\t')
                    .map(|cell| vec![Inline::Text(cell.trim().to_string())])
                    .collect()
            })
            .collect();
        let headers = parsed_rows.remove(0);
        return Some(Phase2bParagraphBlock::Table {
            headers,
            rows: parsed_rows,
        });
    }

    let mismatch_count = widths
        .iter()
        .filter(|w| **w != first_width && **w > 1)
        .count();
    if mismatch_count > 0 || !all_rows_have_tabs {
        return Some(Phase2bParagraphBlock::GenericTableHint {
            content: vec![Inline::Text(normalized)],
            message: "tab-delimited table-like paragraph had inconsistent row widths; preserved as GenericBlock".to_string(),
        });
    }

    None
}

pub(super) fn normalize_bare_callout_paragraph(
    source_slice: &str,
    prov: Provenance,
) -> Option<BlockNode> {
    let normalized = source_slice
        .trim()
        .trim_matches('\n')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let mut lines = normalized.lines();
    let first = lines.next()?.trim_start();
    if !first.starts_with("[!") {
        return None;
    }
    let end_bracket = first.find(']')?;
    if end_bracket < 3 {
        return None;
    }
    let label = &first[2..end_bracket];
    if label.is_empty()
        || !label
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    {
        return None;
    }

    let mut paragraph_text = String::new();
    for (idx, line) in normalized.lines().enumerate() {
        if idx > 0 {
            paragraph_text.push('\n');
        }
        paragraph_text.push_str(line.trim_end());
    }

    Some(BlockNode::new(
        Block::BlockQuote {
            children: vec![BlockNode::new(
                Block::Paragraph {
                    content: vec![Inline::Text(paragraph_text)],
                },
                prov.clone(),
            )],
        },
        prov,
    ))
}
