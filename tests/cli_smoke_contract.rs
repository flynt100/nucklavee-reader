//! CLI smoke-contract tests.
//!
//! These tests lock boundary behavior for argument parsing and expected
//! user-facing error strings documented in docs/cli-phase2-boundary.md
//! (Phase-2 boundary, extended by the Phase-3 HTML ingest path).

use std::process::Command;

use nucklavee::phase2_contract;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_nucklavee"))
}

#[test]
fn ingest_emit_rejects_unsupported_format_with_expected_parse_error() {
    // `rtf` is not a known format token (supported: markdown, html, text).
    let output = cli()
        .args(["ingest-emit", "tests/fixtures/01_basic.md", "--format", "rtf"])
        .output()
        .expect("failed to run nucklavee binary");

    assert_eq!(
        output.status.code(),
        Some(2),
        "clap parse errors should exit with code 2"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid value 'rtf' for '--format <FORMAT>'"),
        "missing clap invalid-format boundary message: {stderr}"
    );
    assert!(
        stderr.contains(&phase2_contract::unsupported_format_message("rtf")),
        "missing domain-specific invalid-format reason: {stderr}"
    );
}

#[test]
fn ingest_emit_supports_text_output_format() {
    let output = cli()
        .args([
            "ingest-emit",
            "tests/fixtures/06_headings.md",
            "--format",
            "text",
        ])
        .output()
        .expect("failed to run nucklavee binary");

    assert_eq!(
        output.status.code(),
        Some(0),
        "text emit should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("ROOT H1"),
        "expected uppercased heading in plain-text output: {stdout}"
    );
}

#[test]
fn ingest_emit_supports_html_output_format() {
    let output = cli()
        .args([
            "ingest-emit",
            "tests/fixtures/01_basic.md",
            "--format",
            "html",
        ])
        .output()
        .expect("failed to run nucklavee binary");

    assert_eq!(
        output.status.code(),
        Some(0),
        "html emit should succeed in Phase 3: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("<h1>") && stdout.contains("</p>"),
        "expected HTML output: {stdout}"
    );
}

#[test]
fn ingest_emit_accepts_html_files_and_emits_markdown() {
    let output = cli()
        .args([
            "ingest-emit",
            "tests/fixtures/html/blog_post.html",
            "--format",
            "markdown",
        ])
        .output()
        .expect("failed to run nucklavee binary");

    assert_eq!(
        output.status.code(),
        Some(0),
        "html ingest-emit should succeed in Phase 3: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("# Shipping the Reader"),
        "expected markdown emission of html content: {stdout}"
    );
    assert!(
        !stdout.contains("BLOGNAV"),
        "chrome must be stripped from CLI output: {stdout}"
    );
}

#[test]
fn ingest_rejects_unsupported_file_extension_with_contract_message() {
    let output = cli()
        .args(["ingest-emit", "notes.docx", "--format", "markdown"])
        .output()
        .expect("failed to run nucklavee binary");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&phase2_contract::unsupported_extension_message("docx")),
        "unexpected unsupported-extension boundary message: {stderr}"
    );
}

#[test]
fn emit_by_id_is_disabled_in_phase2_with_expected_error() {
    let output = cli()
        .args([
            "emit",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--format",
            "markdown",
        ])
        .output()
        .expect("failed to run nucklavee binary");

    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!(
            "error: invalid input: {}",
            phase2_contract::EMIT_BY_ID_DISABLED
        )),
        "unexpected emit-disabled boundary message: {stderr}"
    );
}

#[test]
fn query_command_reports_phase2_not_implemented_contract() {
    let output = cli()
        .arg("query")
        .output()
        .expect("failed to run nucklavee binary");

    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!(
            "error: not implemented: {}",
            phase2_contract::QUERY_NOT_IMPLEMENTED
        )),
        "unexpected query boundary message: {stderr}"
    );
}

#[test]
fn context_window_command_reports_phase2_not_implemented_contract() {
    let output = cli()
        .arg("context-window")
        .output()
        .expect("failed to run nucklavee binary");

    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!(
            "error: not implemented: {}",
            phase2_contract::CONTEXT_WINDOW_NOT_IMPLEMENTED
        )),
        "unexpected context-window boundary message: {stderr}"
    );
}

#[test]
fn html_command_reports_phase2_not_implemented_contract() {
    let output = cli()
        .arg("html")
        .output()
        .expect("failed to run nucklavee binary");

    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!(
            "error: not implemented: {}",
            phase2_contract::HTML_PIPELINE_NOT_IMPLEMENTED
        )),
        "unexpected html boundary message: {stderr}"
    );
}

#[test]
fn pdf_command_reports_phase2_not_implemented_contract() {
    let output = cli()
        .arg("pdf")
        .output()
        .expect("failed to run nucklavee binary");

    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!(
            "error: not implemented: {}",
            phase2_contract::PDF_PIPELINE_NOT_IMPLEMENTED
        )),
        "unexpected pdf boundary message: {stderr}"
    );
}
