use nucklavee::ir::Source;
use nucklavee::parsers::Parser;
use nucklavee::parsers::markdown::MarkdownParser;
use nucklavee::storage::in_memory::InMemoryDocumentStore;
use nucklavee::{Format, Library};

const SAMPLE: &str = "# Sample\n\n\
A paragraph with **bold** and *italic*.\n\n\
- one\n- two\n- three\n";

fn new_library() -> Library<InMemoryDocumentStore> {
    Library::new(InMemoryDocumentStore::new())
}

#[test]
fn ingest_then_get_returns_the_document() {
    let mut library = new_library();
    let id = library
        .ingest(Source::RawMarkdown(SAMPLE.to_string()))
        .expect("ingest");

    let document = library.get_document(id).expect("get_document");
    assert_eq!(document.meta.id, id);
    assert_eq!(document.meta.title.as_deref(), Some("Sample"));
}

#[test]
fn ingest_then_emit_roundtrips_structurally() {
    let mut library = new_library();
    let id = library
        .ingest(Source::RawMarkdown(SAMPLE.to_string()))
        .expect("ingest");

    let emitted = library.emit(id, Format::Markdown).expect("emit markdown");

    let reparsed = MarkdownParser
        .parse(&emitted, &Source::RawMarkdown(emitted.clone()))
        .expect("reparse emitted");
    let original = library.get_document(id).expect("get_document");
    assert!(
        original.structural_eq(&reparsed),
        "library roundtrip structural mismatch:\n--- emitted ---\n{emitted}"
    );
}

#[test]
fn ingesting_identical_content_is_deduped() {
    let mut library = new_library();
    let first = library
        .ingest(Source::RawMarkdown(SAMPLE.to_string()))
        .expect("first ingest");
    let second = library
        .ingest(Source::RawMarkdown(SAMPLE.to_string()))
        .expect("second ingest");
    assert_eq!(first, second, "identical content should produce one id");
}

#[test]
fn html_ingest_returns_not_implemented() {
    let mut library = new_library();
    let err = library
        .ingest(Source::RawHtml("<p>hi</p>".to_string()))
        .expect_err("html parser is not wired");
    let message = err.to_string();
    assert!(
        message.contains("not implemented") && message.contains("html parser"),
        "unexpected error: {message}"
    );
}

#[test]
fn emit_html_is_not_implemented() {
    let mut library = new_library();
    let id = library
        .ingest(Source::RawMarkdown(SAMPLE.to_string()))
        .expect("ingest");
    let err = library
        .emit(id, Format::Html)
        .expect_err("html emitter is not wired");
    assert!(
        err.to_string().contains("html emitter"),
        "unexpected error: {err}"
    );
}

#[test]
fn url_source_detects_as_not_implemented() {
    let mut library = new_library();
    let err = library
        .ingest(Source::Url("https://example.com/page".to_string()))
        .expect_err("url ingestion is not wired");
    assert!(
        err.to_string().contains("url ingestion"),
        "unexpected error: {err}"
    );
}

#[test]
fn unsupported_file_extension_is_rejected() {
    let mut library = new_library();
    let err = library
        .ingest(Source::File("foo.xyz".into()))
        .expect_err("xyz is not a known format");
    assert!(
        err.to_string().contains("unsupported file extension"),
        "unexpected error: {err}"
    );
}
