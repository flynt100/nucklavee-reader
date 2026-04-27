//! Tests for diagnostic surfacing of unsupported / lossy Markdown handling.

use std::path::Path;

use nucklavee::parsers::markdown::{ParseOptions, parse_markdown};
use nucklavee::{Block, DiagnosticKind};

fn opts() -> ParseOptions {
    ParseOptions::default()
}

fn load_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()))
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
fn duplicated_trailing_segment_emits_deterministic_diagnostic_ranges() {
    let src = load_fixture("18_duplicated_trailing_segment.md");

    let doc1 = parse_markdown(&src, opts());
    let doc2 = parse_markdown(&src, opts());

    let d1: Vec<_> = doc1
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("duplicated leading segment"))
        .collect();
    let d2: Vec<_> = doc2
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("duplicated leading segment"))
        .collect();

    assert_eq!(
        d1.len(),
        1,
        "expected one duplicate-segment diagnostic: {:?}",
        doc1.diagnostics
    );
    assert_eq!(
        d2.len(),
        1,
        "expected one duplicate-segment diagnostic: {:?}",
        doc2.diagnostics
    );

    assert_eq!(d1[0].kind, DiagnosticKind::Normalized);
    assert_eq!(d1[0].byte_range, d2[0].byte_range);
    assert!(
        d1[0].byte_range.is_some(),
        "expected duplicate-segment diagnostic byte range"
    );

    let mut normalize_opts = opts();
    normalize_opts.normalize_repeated_leading_segment = true;
    let normalized = parse_markdown(&src, normalize_opts);
    let dropped = normalized
        .diagnostics
        .iter()
        .find(|d| d.message.contains("duplicated leading segment removed"))
        .expect("expected lossy duplicate-segment removal diagnostic");
    assert_eq!(dropped.kind, DiagnosticKind::Lossy);
    assert_eq!(dropped.byte_range, d1[0].byte_range);
}
