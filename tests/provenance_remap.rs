//! Provenance byte ranges must refer to ORIGINAL source coordinates even when
//! the parser internally rewrites its input (math shielding, escaped-newline
//! decoding), and the math-shielding sentinel must never corrupt content.

use nucklavee::parsers::markdown::{ParseOptions, parse_markdown};
use nucklavee::{Block, Document, Inline, validate};

fn opts() -> ParseOptions {
    ParseOptions::default()
}

fn paragraph_ranges(doc: &Document) -> Vec<(usize, usize)> {
    doc.body
        .iter()
        .filter_map(|node| match &node.block {
            Block::Paragraph { .. } => node.prov.byte_range.map(|r| (r.start, r.end)),
            _ => None,
        })
        .collect()
}

fn paragraph_texts(doc: &Document) -> Vec<String> {
    doc.body
        .iter()
        .filter_map(|node| match &node.block {
            Block::Paragraph { content } => Some(
                content
                    .iter()
                    .map(|inline| match inline {
                        Inline::Text(s) => s.clone(),
                        Inline::Code(s) => s.clone(),
                        _ => String::new(),
                    })
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect()
}

#[test]
fn ranges_after_shielded_math_span_point_at_original_source() {
    let src = "start $x+y$ middle\n\nsecond paragraph here\n";
    let doc = parse_markdown(src, opts());
    validate(&doc, Some(src.len())).expect("valid IR");

    let ranges = paragraph_ranges(&doc);
    assert_eq!(ranges.len(), 2, "expected two paragraphs: {doc:#?}");

    // First paragraph covers the math span in original coordinates.
    let (s0, e0) = ranges[0];
    assert!(
        src[s0..e0].contains("$x+y$"),
        "first paragraph slice {:?} should contain the math span",
        &src[s0..e0]
    );

    // Second paragraph starts exactly at its original position, i.e. the
    // 3-byte shrink from replacing `$x+y$` with the placeholder is undone.
    let (s1, e1) = ranges[1];
    assert!(
        src[s1..e1].starts_with("second paragraph here"),
        "second paragraph slice {:?} should start at 'second paragraph here'",
        &src[s1..e1]
    );
}

#[test]
fn ranges_after_escaped_newline_decode_point_at_original_source() {
    let src = "# Title\\n\\nfirst $a=b$ para\\n\\nlast words\n";
    let doc = parse_markdown(src, opts());
    validate(&doc, Some(src.len())).expect("valid IR");

    assert!(
        doc.diagnostics
            .iter()
            .any(|d| d.message.contains("decoded escaped newline stream")),
        "expected decode diagnostic: {:?}",
        doc.diagnostics
    );

    let ranges = paragraph_ranges(&doc);
    assert_eq!(ranges.len(), 2, "expected two paragraphs: {doc:#?}");

    let (s1, e1) = ranges[1];
    assert!(
        src[s1..e1].starts_with("last words"),
        "last paragraph slice {:?} should start at 'last words' in the ORIGINAL escaped source",
        &src[s1..e1]
    );
}

#[test]
fn shielded_spans_inside_inline_code_are_restored_not_replaced_by_sentinel() {
    // `\[ \]` inside inline code is grabbed by pre-cmark math shielding; the
    // placeholder must be restored in Code events, not leaked into the IR.
    let src = "Inline code stays literal: `\\* \\_ \\[ \\]`.\n";
    let doc = parse_markdown(src, opts());
    validate(&doc, Some(src.len())).expect("valid IR");

    let code = doc
        .body
        .iter()
        .find_map(|node| match &node.block {
            Block::Paragraph { content } => content.iter().find_map(|inline| match inline {
                Inline::Code(s) => Some(s.clone()),
                _ => None,
            }),
            _ => None,
        })
        .expect("expected an inline code span");

    assert!(
        !code.contains('\u{00A4}'),
        "sentinel must not leak into code content: {code:?}"
    );
    assert_eq!(
        code, "\\* \\_ \\[ \\]",
        "code content must be preserved verbatim"
    );

    let emitted = nucklavee::emitters::markdown::emit_markdown(&doc);
    assert!(
        emitted.contains("`\\* \\_ \\[ \\]`"),
        "emitted markdown must contain the original code span: {emitted}"
    );
}

#[test]
fn literal_sentinel_character_disables_shielding_without_corruption() {
    let src = "literal \u{00A4} char then $x+y$ math\n";
    let doc = parse_markdown(src, opts());
    validate(&doc, Some(src.len())).expect("valid IR");

    assert!(
        doc.diagnostics.iter().any(|d| {
            d.message
                .contains("reserved math-shielding sentinel U+00A4")
        }),
        "expected shielding-disabled diagnostic: {:?}",
        doc.diagnostics
    );
    assert!(
        !doc.diagnostics
            .iter()
            .any(|d| d.message.contains("no matching payload")),
        "restore queue must not misfire when shielding is disabled: {:?}",
        doc.diagnostics
    );

    let texts = paragraph_texts(&doc);
    assert_eq!(texts.len(), 1);
    assert!(
        texts[0].contains('\u{00A4}') && texts[0].contains("x+y"),
        "content must be preserved verbatim, got {:?}",
        texts[0]
    );
}
