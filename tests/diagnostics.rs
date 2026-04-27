//! Tests for diagnostic surfacing of unsupported / lossy Markdown handling.

use nucklavee::parsers::markdown::{ParseOptions, parse_markdown};
use nucklavee::{Block, DiagnosticKind};

fn opts() -> ParseOptions {
    ParseOptions::default()
}

#[test]
fn raw_html_block_emits_unsupported_diagnostic() {
    let src = "<div>hello</div>\n";
    let doc = parse_markdown(src, opts());
    assert!(
        doc.diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::Unsupported),
        "expected Unsupported diagnostic for raw HTML, got {:?}",
        doc.diagnostics
    );
}

#[test]
fn image_with_inline_formatting_flags_lossy() {
    // Alt text with inline code: pulldown-cmark emits Code inside Image.
    let src = "![alt with `code`](https://e.com/x.png)\n";
    let doc = parse_markdown(src, opts());
    assert!(
        doc.diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::Lossy),
        "expected Lossy diagnostic for image alt with formatting, got {:?}",
        doc.diagnostics
    );
}

#[test]
fn plain_markdown_produces_no_diagnostics() {
    let src = "# hello\n\nparagraph\n";
    let doc = parse_markdown(src, opts());
    assert!(
        doc.diagnostics.is_empty(),
        "expected no diagnostics, got {:?}",
        doc.diagnostics
    );
}

#[test]
fn block_provenance_is_populated() {
    let src = "# A\n\ntext\n";
    let doc = parse_markdown(src, opts());
    assert!(!doc.body.is_empty());
    for node in &doc.body {
        assert!(
            node.prov.byte_range.is_some(),
            "every top-level block must carry a byte range in Phase 1"
        );
    }
}

#[test]
fn heading_section_path_is_parent_only() {
    let src = "# A\n\n## B\n\n### C\n\ntext\n";
    let doc = parse_markdown(src, opts());
    // H1 A → []
    // H2 B → ["A"]
    // H3 C → ["A", "B"]
    let headings: Vec<_> = doc
        .body
        .iter()
        .filter_map(|n| match &n.block {
            Block::Heading { level, .. } => Some((*level, n.prov.section_path.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(headings.len(), 3);
    assert_eq!(headings[0], (1, vec![]));
    assert_eq!(headings[1], (2, vec!["A".to_string()]));
    assert_eq!(headings[2], (3, vec!["A".to_string(), "B".to_string()]));
}

#[test]
fn text_after_heading_inherits_section_path() {
    let src = "# A\n\n## B\n\nbody\n";
    let doc = parse_markdown(src, opts());
    let body = doc
        .body
        .iter()
        .rev()
        .find_map(|n| match &n.block {
            Block::Paragraph { .. } => Some(n.prov.section_path.clone()),
            _ => None,
        })
        .expect("paragraph not found");
    assert_eq!(body, vec!["A".to_string(), "B".to_string()]);
}

#[test]
fn bare_callout_without_blockquote_prefix_emits_diagnostic() {
    let src = "[!note] Release Notes\nMore details\n";
    let doc = parse_markdown(src, opts());
    assert!(
        doc.diagnostics.iter().any(|d| {
            d.kind == DiagnosticKind::Normalized && d.message.contains("expected `> [!type]`")
        }),
        "expected bare callout diagnostic, got {:?}",
        doc.diagnostics
    );
}
