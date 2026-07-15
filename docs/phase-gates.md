# Phase Gates

This document defines exit criteria between implementation phases and provides
clear release semantics for test outcomes.

## Phase 1 Exit Criteria

Phase 1 is considered complete only when **all blocker criteria** below are
satisfied.

### Required outcomes

1. **IR validation invariants complete and tested**
   - Validation rules are implemented and exercised by
     `tests/validation.rs`.
2. **Normalization + structural equivalence stable**
   - Canonicalization/normalization behavior is deterministic and structural
     equivalence remains stable under expected deltas; coverage anchored by
     `tests/validation.rs` and roundtrip semantics in
     `tests/markdown_roundtrip.rs`.
3. **Markdown parse->emit->parse fixtures passing**
   - All committed Markdown fixture cases pass through the roundtrip harness in
     `tests/markdown_roundtrip.rs`.
4. **Diagnostics behavior defined for unsupported/lossy cases**
   - Expected diagnostics taxonomy and behavior are explicit and covered by
     `tests/diagnostics.rs`.

## Blockers vs Warnings

### Blocker (must be resolved before Phase-1 exit)

A failure is a **blocker** when it indicates contract breakage in core Phase-1
behavior:

- Any failing test in `tests/validation.rs`.
- Any failing test in `tests/markdown_roundtrip.rs` for committed fixtures.
- Any failing test in `tests/diagnostics.rs` where expected diagnostics are
  missing, misclassified, or incorrectly escalated.
- Non-deterministic normalization/equivalence behavior that causes unstable
  outcomes across repeated runs.

### Warning (does not block Phase-1 exit by itself)

A finding is a **warning** when behavior remains within defined contracts and no
Phase-1 core guarantees are violated:

- Additional diagnostics are emitted in supported flows, but expected
  diagnostics are still present and correctly classified.
- Formatting-level Markdown deltas that preserve structural equivalence and keep
  `tests/markdown_roundtrip.rs` green.
- Non-contractual quality observations (documentation clarity, future fixture
  suggestions, naming cleanups) that do not affect test pass/fail status in:
  - `tests/validation.rs`
  - `tests/diagnostics.rs`
  - `tests/markdown_roundtrip.rs`

## Operational gate check

Recommended Phase-1 gate check command:

```bash
cargo test --test validation --test diagnostics --test markdown_roundtrip
```

Phase 1 exit is approved only when this command succeeds without failures.
Phase 1 quality gate is satisfied by tests `validation`/`diagnostics`/`markdown_roundtrip`; Phase 2A denotes post-gate packaging/wiring work.

## Phase-2 dependency gate policy

To preserve implementation focus and avoid premature coupling during Phase-2,
dependency additions are constrained by gate criteria:

> no new runtime/storage/vector deps during Phase-2 unless required by an accepted gate criterion.

Implications:

- CLI command parsing dependencies are allowed when directly required for
  Phase-2 command usability.
- `serde_json` should remain optional and only be added when JSON diagnostics or
  IR printing is explicitly required by an accepted gate criterion.
- Async/runtime, storage backends, and vector/database dependencies are deferred
  until Phase-3 wiring.
## Phase 2B exit checklist

Phase 2B is complete only when all items below pass in the same revision:

1. **Fixture expansion classification is enforced**
   - Stable fixtures and expected-diagnostic fixtures are explicitly classified in `tests/markdown_roundtrip.rs`.
   - Classification tests must pass and agree with expected diagnostic behavior.
2. **CLI smoke-contract boundaries are locked**
   - Dedicated CLI smoke tests verify argument parsing behavior and exact user-facing boundary errors for unsupported/disabled/not-implemented Phase-2 commands.
3. **Targeted gate test command is green**

```bash
cargo test --test markdown_roundtrip --test cli_smoke_contract
```

Phase 2B exit is approved only when this command succeeds without failures.

## Experimental MVP release gate

Every candidate revision must pass the same locked-dependency gate locally and
in CI:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features --locked
cargo build --release --locked
cargo package --locked
```

Release blockers also include a missing license, unbounded URL ingestion,
undocumented external data transfer, or documentation that claims unsupported
PDF/office/OCR behavior.
