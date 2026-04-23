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
