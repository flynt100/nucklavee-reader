use std::process::Command;

use nucklavee::chunking::ChunkId;
use nucklavee::embedder::Embedder;
use nucklavee::ir::Source;
use nucklavee::phase2_contract;
use nucklavee::storage::memory::InMemoryDocumentStore;
use nucklavee::vector::VectorIndex;
use nucklavee::{Error, Format, Library, Result};

#[derive(Debug, Default)]
struct NoopVectorIndex;

impl VectorIndex for NoopVectorIndex {
    fn add(&mut self, _id: ChunkId, _vector: Vec<f32>) -> Result<()> {
        Ok(())
    }

    fn search(&self, _query: &[f32], _limit: usize) -> Result<Vec<(ChunkId, f32)>> {
        Ok(Vec::new())
    }
}

#[derive(Debug, Default)]
struct NoopEmbedder;

impl Embedder for NoopEmbedder {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(vec![vec![0.0; self.dimension()]; texts.len()])
    }

    fn dimension(&self) -> usize {
        8
    }
}

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
fn library_emit_unsupported_format_uses_canonical_contract_string() {
    let mut lib = Library::new(
        InMemoryDocumentStore::default(),
        NoopVectorIndex,
        NoopEmbedder,
    );
    let id = lib
        .ingest(Source::RawMarkdown("# title".into()))
        .expect("raw markdown ingest should succeed");

    let err = lib
        .emit(id, Format::PlainText)
        .expect_err("text emit should fail in phase 2");
    assert!(matches!(
        err,
        Error::NotImplemented(phase2_contract::UNSUPPORTED_FORMAT_TEXT)
    ));
}

#[test]
fn cli_and_library_share_canonical_boundary_strings() {
    let mut lib = Library::new(
        InMemoryDocumentStore::default(),
        NoopVectorIndex,
        NoopEmbedder,
    );

    let query_err = lib
        .query("anything", 3)
        .expect_err("query should fail in phase 2");
    let query_output = cli().arg("query").output().expect("run query command");
    let query_stderr = String::from_utf8_lossy(&query_output.stderr);
    assert!(
        query_stderr.contains(&query_err.to_string()),
        "query stderr should include library contract string: {query_stderr}"
    );

    let context_err = lib
        .context_window("anything", 1000)
        .expect_err("context window should fail in phase 2");
    let context_output = cli()
        .arg("context-window")
        .output()
        .expect("run context-window command");
    let context_stderr = String::from_utf8_lossy(&context_output.stderr);
    assert!(
        context_stderr.contains(&context_err.to_string()),
        "context-window stderr should include library contract string: {context_stderr}"
    );

    let id = lib
        .ingest(Source::RawMarkdown("# title".into()))
        .expect("raw markdown ingest should succeed");
    let emit_text_err = lib
        .emit(id, Format::PlainText)
        .expect_err("text emit should fail in phase 2");

    let format_output = cli()
        .args([
            "ingest-emit",
            "tests/fixtures/01_basic.md",
            "--format",
            "text",
        ])
        .output()
        .expect("run ingest-emit unsupported format command");
    let format_stderr = String::from_utf8_lossy(&format_output.stderr);
    assert!(
        format_stderr.contains(phase2_contract::SUPPORTED_FORMATS)
            && emit_text_err
                .to_string()
                .contains(phase2_contract::SUPPORTED_FORMATS),
        "cli and library unsupported-format boundaries should agree on supported formats"
    );
}
