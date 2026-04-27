//! Phase 2B CLI smoke-contract tests.
//!
//! These tests lock boundary behavior for argument parsing and expected
//! user-facing error strings documented in docs/cli-phase2-boundary.md.

use std::process::Command;

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
        stderr.contains("unsupported format 'html'. supported: markdown"),
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
        stderr.contains("error: invalid input: emit --id is disabled in Phase 2 because document IDs are process-local. use `ingest-emit <path> --format markdown`"),
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
        stderr.contains("error: not implemented: query is not implemented in Phase 2 (markdown ingest/emit only)"),
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
        stderr.contains("error: not implemented: context_window is not implemented in Phase 2 (markdown ingest/emit only)"),
        "unexpected context-window boundary message: {stderr}"
    );
}
