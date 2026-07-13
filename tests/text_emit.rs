//! Plain-text emitter integration tests (spec §5.3, audit Task 5).
//!
//! Plain text is a terminal output (no parser), so these assert readable,
//! deterministic rendering and format-stripping rather than a roundtrip.

use std::path::{Path, PathBuf};

use nucklavee::emitters::text::emit_text;
use nucklavee::parsers::html::{HtmlParseOptions, parse_html};
use nucklavee::parsers::markdown::{ParseOptions, parse_markdown};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn load(name: &str) -> String {
    std::fs::read_to_string(fixtures_dir().join(name))
        .unwrap_or_else(|e| panic!("failed to read fixture {name}: {e}"))
}

#[test]
fn strips_all_markdown_formatting_markers() {
    let source = load("24_cross_format.md");
    let doc = parse_markdown(&source, ParseOptions::default());
    let text = emit_text(&doc);

    // No markdown syntax noise should survive.
    for marker in ["**", "~~", "```", "|---", "###"] {
        assert!(
            !text.contains(marker),
            "plain text should not contain markdown marker {marker:?}:\n{text}"
        );
    }
    // Heading text is uppercased.
    assert!(
        text.contains("CROSS-FORMAT FIXTURE"),
        "missing uppercased H1:\n{text}"
    );
    // A link renders as `text (url)`.
    assert!(
        text.contains("link (https://example.com/path)"),
        "link not rendered as text (url):\n{text}"
    );
    // Determinism.
    assert_eq!(text, emit_text(&doc));
}

#[test]
fn code_blocks_are_indented_and_lists_marked() {
    let source = load("24_cross_format.md");
    let doc = parse_markdown(&source, ParseOptions::default());
    let text = emit_text(&doc);

    assert!(
        text.lines().any(|l| l.starts_with("    fn main")),
        "expected 4-space-indented code line:\n{text}"
    );
    assert!(
        text.lines().any(|l| l.starts_with("- first item")),
        "expected unordered list marker:\n{text}"
    );
    assert!(
        text.lines().any(|l| l.starts_with("1. ordered one")),
        "expected ordered list marker:\n{text}"
    );
    assert!(
        text.lines().any(|l| l.starts_with("  - nested child")),
        "expected indented nested list item:\n{text}"
    );
}

#[test]
fn html_source_also_renders_to_plain_text() {
    let html = load("html/docs_site.html");
    let doc = parse_html(&html, HtmlParseOptions::default());
    let text = emit_text(&doc);

    assert!(text.contains("CONFIGURING THE WIDGET ENGINE"));
    assert!(!text.contains("<"), "no html tags should survive:\n{text}");
    assert!(
        !text.contains("NAVLINK"),
        "chrome must stay stripped:\n{text}"
    );
    assert!(
        text.lines().any(|l| l.starts_with("    widgetctl install")),
        "expected indented code from html source:\n{text}"
    );
}

#[test]
fn blockquote_lines_are_gutter_prefixed() {
    let doc = parse_markdown(
        "> first quoted line\n> second quoted line\n",
        ParseOptions::default(),
    );
    let text = emit_text(&doc);
    for line in text.lines() {
        assert!(
            line.starts_with("| ") || line == "|",
            "every quote line should be gutter-prefixed: {line:?}"
        );
    }
}
