//! Markdown roundtrip harness.
//!
//! For every fixture:
//! 1. parse the source into IR
//! 2. validate IR
//! 3. emit the IR back to markdown
//! 4. re-parse the emitted markdown
//! 5. compare the two IR trees by structural equivalence
//!
//! Provenance ranges on the first parse are additionally checked for
//! in-bounds correctness against the source length.
//!
//! On failure, the test prints:
//! - the original fixture
//! - the first IR (debug)
//! - the emitted markdown
//! - the second IR (debug)
//! - the structural diff
//! - a diagnostic summary

use std::path::{Path, PathBuf};

use nucklavee::emitters::markdown::emit_markdown;
use nucklavee::parsers::markdown::{ParseOptions, parse_markdown};
use nucklavee::{Document, structural_diff, validate};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn load_fixture(name: &str) -> (String, String) {
    let path = fixtures_dir().join(name);
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()));
    (name.to_string(), content)
}

fn opts() -> ParseOptions {
    ParseOptions::default()
}

fn roundtrip(name: &str) {
    let (label, source) = load_fixture(name);
    let doc1 = parse_markdown(&source, opts());
    if let Err(e) = validate(&doc1, Some(source.len())) {
        panic!(
            "fixture {label}: initial IR failed validation: {e}\n--- source ---\n{source}\n--- ir1 ---\n{:#?}",
            doc1
        );
    }

    let emitted = emit_markdown(&doc1);
    let doc2 = parse_markdown(&emitted, opts());
    if let Err(e) = validate(&doc2, Some(emitted.len())) {
        panic!(
            "fixture {label}: re-emitted IR failed validation: {e}\n--- source ---\n{source}\n--- ir1 ---\n{:#?}\n--- emitted ---\n{emitted}\n--- ir2 ---\n{:#?}",
            doc1, doc2
        );
    }

    if let Some(diff) = structural_diff(&doc1, &doc2) {
        panic!(
            "fixture {label}: roundtrip structural mismatch: {diff}\n\
             --- source ---\n{source}\n\
             --- ir1 ---\n{:#?}\n\
             --- emitted ---\n{emitted}\n\
             --- ir2 ---\n{:#?}\n\
             --- diagnostics (ir1) ---\n{:?}\n\
             --- diagnostics (ir2) ---\n{:?}\n",
            doc1, doc2, doc1.diagnostics, doc2.diagnostics
        );
    }
}

#[test]
fn roundtrip_basic() {
    roundtrip("01_basic.md");
}

#[test]
fn roundtrip_lists() {
    roundtrip("02_lists.md");
}

#[test]
fn roundtrip_code_and_quotes() {
    roundtrip("03_code_and_quotes.md");
}

#[test]
fn roundtrip_links_and_images() {
    roundtrip("04_links_and_images.md");
}

#[test]
fn roundtrip_tables() {
    roundtrip("05_tables.md");
}

#[test]
fn roundtrip_headings() {
    roundtrip("06_headings.md");
}

#[test]
fn roundtrip_edges() {
    roundtrip("07_edge.md");
}

#[test]
fn roundtrip_hard() {
    roundtrip("08_hard.md");
}

#[test]
fn roundtrip_frontmatter() {
    roundtrip("10_frontmatter.md");
}

#[test]
fn title_extraction_uses_first_h1() {
    let (_, source) = load_fixture("06_headings.md");
    let doc = parse_markdown(&source, opts());
    assert_eq!(doc.meta.title.as_deref(), Some("Root H1"));
}

#[test]
fn title_is_none_without_h1() {
    let src = "## Only an h2\n\ntext";
    let doc = parse_markdown(src, opts());
    assert!(doc.meta.title.is_none(), "title was {:?}", doc.meta.title);
}

#[test]
fn provenance_ranges_are_in_bounds() {
    let (_, source) = load_fixture("01_basic.md");
    let doc = parse_markdown(&source, opts());
    walk_ranges(&doc, source.len());
}

fn assert_deterministic_parse_emit(label: &str, source: &str, iterations: usize) {
    assert!(iterations > 0, "iterations must be > 0");

    let mut current_doc = parse_markdown(source, opts());
    let mut baseline_emitted: Option<String> = None;
    let mut baseline_diff: Option<Option<String>> = None;

    for i in 0..iterations {
        let emitted = emit_markdown(&current_doc);
        let reparsed = parse_markdown(&emitted, opts());
        let diff = structural_diff(&current_doc, &reparsed);

        if let Some(expected) = &baseline_emitted {
            assert_eq!(
                &emitted, expected,
                "{label}: emitted markdown diverged at iteration {i}"
            );
        } else {
            baseline_emitted = Some(emitted.clone());
        }

        if let Some(expected) = &baseline_diff {
            assert_eq!(
                &diff, expected,
                "{label}: structural_diff diverged at iteration {i}"
            );
        } else {
            baseline_diff = Some(diff.clone());
        }

        assert!(
            validate(&reparsed, Some(emitted.len())).is_ok(),
            "{label}: reparsed document failed validation at iteration {i}"
        );

        current_doc = reparsed;
    }
}

#[test]
fn parse_emit_is_deterministic_for_nested_lists_and_tables_fixture() {
    let (label, source) = load_fixture("08_hard.md");
    assert_deterministic_parse_emit(&label, &source, 5);
}

#[test]
fn parse_emit_is_deterministic_for_diagnostics_fixture() {
    let (label, source) = load_fixture("09_diagnostics.md");
    let initial = parse_markdown(&source, opts());
    assert!(
        !initial.diagnostics.is_empty(),
        "expected diagnostics fixture to produce diagnostics"
    );
    assert_deterministic_parse_emit(&label, &source, 5);
}

#[test]
fn frontmatter_is_captured_and_not_treated_as_body() {
    let (_, source) = load_fixture("10_frontmatter.md");
    let doc = parse_markdown(&source, opts());
    let frontmatter = doc
        .meta
        .frontmatter
        .as_ref()
        .expect("expected frontmatter");
    assert!(frontmatter.raw.starts_with("---\n"));
    assert!(
        matches!(doc.body.first().map(|n| &n.block), Some(nucklavee::Block::Heading { .. })),
        "expected first body block to be heading, got {:?}",
        doc.body.first().map(|n| &n.block)
    );

    assert!(
        frontmatter.raw.contains("tags:"),
        "expected tags key in raw frontmatter"
    );
    assert!(
        frontmatter.raw.contains("published: true"),
        "expected boolean key in raw frontmatter"
    );
    assert!(
        frontmatter.raw.contains("date: 2026-04-24"),
        "expected date key in raw frontmatter"
    );
    assert!(
        frontmatter.raw.contains("\"[[Nucklavee Reader]]\""),
        "expected Obsidian wikilink-style string in frontmatter"
    );
}

fn walk_ranges(doc: &Document, source_len: usize) {
    use nucklavee::{Block, BlockNode};

    fn walk(node: &BlockNode, source_len: usize) {
        if let Some(r) = node.prov.byte_range {
            assert!(
                r.is_valid_within(source_len),
                "byte range {:?} not in bounds [0, {source_len}]",
                r
            );
        }
        match &node.block {
            Block::BlockQuote { children } => {
                for c in children {
                    walk(c, source_len);
                }
            }
            Block::List { items, .. } => {
                for item in items {
                    for c in &item.content {
                        walk(c, source_len);
                    }
                }
            }
            _ => {}
        }
    }

    for node in &doc.body {
        walk(node, source_len);
    }
}
