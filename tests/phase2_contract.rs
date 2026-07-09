use std::process::Command;

use nucklavee::ir::Source;
use nucklavee::phase2_contract;
use nucklavee::storage::memory::InMemoryDocumentStore;
use nucklavee::test_support::{NoopEmbedder, NoopVectorIndex};
use nucklavee::{Error, Format, Library};

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_nucklavee"))
}

#[test]
fn library_query_and_context_window_use_canonical_contract_strings() {
    let lib = Library::new(
        InMemoryDocumentStore::default(),
        NoopVectorIndex,
        NoopEmbedder,
    );

    let query_err = lib
        .query("anything", 3)
        .expect_err("query should fail in phase 2");
    assert!(matches!(
        query_err,
        Error::NotImplemented(phase2_contract::QUERY_NOT_IMPLEMENTED)
    ));

    let context_err = lib
        .context_window("anything", 1000)
        .expect_err("context_window should fail in phase 2");
    assert!(matches!(
        context_err,
        Error::NotImplemented(phase2_contract::CONTEXT_WINDOW_NOT_IMPLEMENTED)
    ));
}

#[test]
fn library_emits_all_three_supported_formats() {
    let mut lib = Library::new(
        InMemoryDocumentStore::default(),
        NoopVectorIndex,
        NoopEmbedder,
    );
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
fn cli_and_library_share_canonical_boundary_strings() {
    let lib = Library::new(
        InMemoryDocumentStore::default(),
        NoopVectorIndex,
        NoopEmbedder,
    );

    let query_err = lib
        .query("anything", 3)
        .expect_err("query should fail before Phase 4");
    let query_output = cli().arg("query").output().expect("run query command");
    let query_stderr = String::from_utf8_lossy(&query_output.stderr);
    assert!(
        query_stderr.contains(&query_err.to_string()),
        "query stderr should include library contract string: {query_stderr}"
    );

    let context_err = lib
        .context_window("anything", 1000)
        .expect_err("context window should fail before Phase 4");
    let context_output = cli()
        .arg("context-window")
        .output()
        .expect("run context-window command");
    let context_stderr = String::from_utf8_lossy(&context_output.stderr);
    assert!(
        context_stderr.contains(&context_err.to_string()),
        "context-window stderr should include library contract string: {context_stderr}"
    );

    // The remaining unsupported-format boundary lives at the CLI string parse
    // (the library `Format` enum is now fully supported). `rtf` is not a
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
