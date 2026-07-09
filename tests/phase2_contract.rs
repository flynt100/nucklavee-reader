use std::process::Command;

use nucklavee::ir::Source;
use nucklavee::phase2_contract;
use nucklavee::storage::memory::InMemoryDocumentStore;
use nucklavee::test_support::{NoopEmbedder, NoopVectorIndex};
use nucklavee::{Format, Library};

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_nucklavee"))
}

#[test]
fn library_query_and_context_window_are_implemented_and_empty_without_data() {
    let lib = Library::new(
        InMemoryDocumentStore::default(),
        NoopVectorIndex,
        NoopEmbedder,
    )
    .expect("build library");

    // Implemented as of Phase 4: with a no-op index and nothing ingested,
    // both return empty rather than erroring.
    let results = lib.query("anything", 3).expect("query is implemented");
    assert!(results.is_empty());

    let ctx = lib
        .context_window("anything", 1000)
        .expect("context_window is implemented");
    assert!(ctx.is_empty());
}

#[test]
fn library_emits_all_three_supported_formats() {
    let mut lib = Library::new(
        InMemoryDocumentStore::default(),
        NoopVectorIndex,
        NoopEmbedder,
    )
    .expect("build library");
    let id = lib
        .ingest(Source::RawMarkdown("# title\n\nbody\n".into()))
        .expect("raw markdown ingest should succeed");

    let md = lib.emit(id, Format::Markdown).expect("markdown emit");
    assert!(md.contains("# title"), "unexpected markdown: {md}");

    let html = lib.emit(id, Format::Html).expect("html emit");
    assert!(html.contains("<h1>title</h1>"), "unexpected html: {html}");

    let text = lib.emit(id, Format::PlainText).expect("text emit");
    assert!(text.contains("TITLE"), "unexpected text: {text}");
}

#[test]
fn cli_unsupported_format_boundary_is_canonical() {
    // The library `Format` enum is fully supported; the only remaining
    // unsupported-format boundary is the CLI string parse. `rtf` is not a
    // known format token.
    let expected_message = phase2_contract::unsupported_format_message("rtf");
    let format_output = cli()
        .args(["ingest-emit", "tests/fixtures/01_basic.md", "--format", "rtf"])
        .output()
        .expect("run ingest-emit unsupported format command");
    let format_stderr = String::from_utf8_lossy(&format_output.stderr);
    assert!(
        format_stderr.contains(&expected_message),
        "cli unsupported-format parse boundary should include canonical message: {format_stderr}"
    );
}
