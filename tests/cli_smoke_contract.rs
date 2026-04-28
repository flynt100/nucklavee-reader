//! Phase 2B CLI smoke-contract tests.
//!
//! These tests lock boundary behavior for argument parsing and expected
//! user-facing error strings documented in docs/cli-phase2-boundary.md.

use std::process::Command;

use nucklavee::phase2_contract;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_nucklavee"))
}

#[test]
fn ingest_emit_rejects_unsupported_format_with_expected_parse_error() {
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
        Some(2),
        "clap parse errors should exit with code 2"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid value 'html' for '--format <FORMAT>'"),
        "missing clap invalid-format boundary message: {stderr}"
    );
    assert!(
        stderr.contains(phase2_contract::UNSUPPORTED_FORMAT_HTML),
        "missing domain-specific invalid-format reason: {stderr}"
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
