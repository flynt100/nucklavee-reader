//! Phase-3 cross-format integrity gate: markdown → IR → html → IR.
//!
//! Emitting IR as HTML and reparsing it must preserve document structure
//! (spec §5.2 / build sequence Phase 2 exit criteria). Complements the
//! html → IR → markdown → IR direction in `tests/html_ingest.rs`.

use std::path::{Path, PathBuf};

use nucklavee::emitters::html::emit_html;
use nucklavee::emitters::markdown::emit_markdown;
use nucklavee::ir::Source;
use nucklavee::parsers::html::{HtmlParseOptions, parse_html};
use nucklavee::parsers::markdown::{ParseOptions, parse_markdown};
use nucklavee::storage::memory::InMemoryDocumentStore;
use nucklavee::test_support::{NoopEmbedder, NoopVectorIndex};
use nucklavee::{Format, Library, structural_diff_bodies, validate};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn load(name: &str) -> String {
    std::fs::read_to_string(fixtures_dir().join(name))
        .unwrap_or_else(|e| panic!("failed to read fixture {name}: {e}"))
}

/// markdown → IR₁ → html → IR₂; bodies must be structurally equivalent and
/// both IRs valid.
fn assert_md_to_html_roundtrip(name: &str) {
    let source = load(name);
    let md_doc = parse_markdown(&source, ParseOptions::default());
    validate(&md_doc, Some(source.len()))
        .unwrap_or_else(|e| panic!("{name}: markdown IR invalid: {e}"));

    let html = emit_html(&md_doc);
    let html_doc = parse_html(&html, HtmlParseOptions::default());
    validate(&html_doc, None).unwrap_or_else(|e| panic!("{name}: html IR invalid: {e}"));

    if let Some(diff) = structural_diff_bodies(&md_doc, &html_doc) {
        panic!(
            "{name}: markdown→IR→html→IR structural mismatch: {diff}\n\
             --- emitted html ---\n{html}\n\
             --- markdown IR ---\n{md_doc:#?}\n\
             --- html IR ---\n{html_doc:#?}"
        );
    }
}

#[test]
fn cross_format_fixture_markdown_to_html() {
    assert_md_to_html_roundtrip("24_cross_format.md");
}

#[test]
fn cross_format_basic_fixture_markdown_to_html() {
    assert_md_to_html_roundtrip("01_basic.md");
}

#[test]
fn cross_format_headings_fixture_markdown_to_html() {
    assert_md_to_html_roundtrip("06_headings.md");
}

#[test]
fn cross_format_tables_fixture_markdown_to_html() {
    assert_md_to_html_roundtrip("05_tables.md");
}

#[test]
fn cross_format_links_and_images_fixture_markdown_to_html() {
    assert_md_to_html_roundtrip("04_links_and_images.md");
}

#[test]
fn html_emit_escapes_and_maps_styles() {
    let md = "A paragraph with **strong**, *em*, ~~strike~~ and 1 < 2 & 3.\n";
    let doc = parse_markdown(md, ParseOptions::default());
    let html = emit_html(&doc);
    assert!(html.contains("<strong>strong</strong>"));
    assert!(html.contains("<em>em</em>"));
    assert!(html.contains("<del>strike</del>"));
    assert!(html.contains("1 &lt; 2 &amp; 3"));
}

#[test]
fn library_emits_html_format() {
    let mut lib = Library::new(
        InMemoryDocumentStore::default(),
        NoopVectorIndex,
        NoopEmbedder,
    )
    .expect("build library");
    let id = lib
        .ingest(Source::RawMarkdown("# Title\n\nbody **b**\n".into()))
        .expect("ingest");
    let html = lib.emit(id, Format::Html).expect("html emit");
    assert_eq!(html, "<h1>Title</h1>\n<p>body <strong>b</strong></p>");
}

#[test]
fn round_trips_html_to_markdown_to_html_is_stable() {
    // A page ingested as HTML, emitted as markdown, then re-emitted as HTML
    // via a fresh markdown parse should stay structurally stable.
    let html_src = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/html/docs_site.html"),
    )
    .expect("read html fixture");

    let html_doc = parse_html(&html_src, HtmlParseOptions::default());
    let markdown = emit_markdown(&html_doc);
    let md_doc = parse_markdown(&markdown, ParseOptions::default());
    let back_to_html = emit_html(&md_doc);
    let html_doc2 = parse_html(&back_to_html, HtmlParseOptions::default());

    if let Some(diff) = structural_diff_bodies(&md_doc, &html_doc2) {
        panic!(
            "html→md→html structural drift: {diff}\n--- markdown ---\n{markdown}\n--- html2 ---\n{back_to_html}"
        );
    }
}
