use std::path::Path;

use nucklavee::emitters::Emitter;
use nucklavee::emitters::markdown::MarkdownEmitter;
use nucklavee::ir::Source;
use nucklavee::parsers::Parser;
use nucklavee::parsers::markdown::MarkdownParser;

fn roundtrip(fixture: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/markdown")
        .join(fixture);
    let input = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    let first_source = Source::File(path.clone());
    let first = MarkdownParser
        .parse(&input, &first_source)
        .expect("first parse");
    first
        .validate_strict()
        .expect("first parse produced invalid IR");

    let emitted = MarkdownEmitter.emit(&first).expect("emit");

    let second = MarkdownParser
        .parse(&emitted, &Source::RawMarkdown(emitted.clone()))
        .expect("second parse");
    second
        .validate_strict()
        .expect("second parse produced invalid IR");

    assert!(
        first.structural_eq(&second),
        "roundtrip structural mismatch for {fixture}\n--- first IR ---\n{:#?}\n--- emitted ---\n{emitted}\n--- second IR ---\n{:#?}",
        first.body,
        second.body,
    );
}

#[test]
fn roundtrip_headings() {
    roundtrip("headings.md");
}

#[test]
fn roundtrip_inline_styles() {
    roundtrip("inline_styles.md");
}

#[test]
fn roundtrip_links_images() {
    roundtrip("links_images.md");
}

#[test]
fn roundtrip_code_blocks() {
    roundtrip("code_blocks.md");
}

#[test]
fn roundtrip_lists_nested() {
    roundtrip("lists_nested.md");
}

#[test]
fn roundtrip_tables() {
    roundtrip("tables.md");
}

#[test]
fn roundtrip_blockquote() {
    roundtrip("blockquote.md");
}

#[test]
fn roundtrip_thematic_breaks() {
    roundtrip("thematic_breaks.md");
}
