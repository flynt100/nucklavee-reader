# Phase 2B QC Report

_Generated: 2026-04-29_
_Branch: `claude/phase-2b-qa-check-e9YR8`_

---

## Executive Summary

**Phase 2B is green.** The formal gate command passes cleanly, all 75 tests pass across the full suite, and the codebase is structurally consistent with the roadmap. There are no blockers and no regressions. Several non-blocking findings are documented below, ranging from one untested opt-in parser feature to stale planning docs and minor clippy noise.

---

## 1. Gate Status

### Phase 2B gate command

```
cargo test --test markdown_roundtrip --test cli_smoke_contract
```

**Result: PASS** — 43 roundtrip + 6 CLI smoke = 49 tests, 0 failures.

### Full suite

```
cargo test
```

| Suite | Tests | Result |
|---|---|---|
| `lib` unit tests (emitters::markdown) | 6 | ✅ Pass |
| `cli_smoke_contract` | 6 | ✅ Pass |
| `diagnostics` | 8 | ✅ Pass |
| `markdown_roundtrip` | 43 | ✅ Pass |
| `phase2_contract` | 3 | ✅ Pass |
| `validation` | 9 | ✅ Pass |
| **Total** | **75** | **✅ All pass** |

---

## 2. Roadmap Alignment

### Phase 1 — IR Finalization
All four exit criteria confirmed satisfied: IR validation (9 rules), normalization + structural equivalence (deterministic, stable), markdown parse→emit→parse roundtrip (all fixtures), and diagnostics taxonomy (Unsupported/Lossy/Normalized, tested by diagnostics.rs).

### Phase 2A — Markdown Core Loop
`MarkdownParser`, `MarkdownEmitter`, and the roundtrip harness are fully implemented. The library `ingest → store → emit` loop is functional end-to-end over markdown.

### Phase 2B — Hardening
All three checklist items delivered:

1. **Normalization delta policy documented** — `docs/normalization-deltas.md` exists and is complete, with an A–F test evidence map.
2. **Fixture expansion classification enforced** — `FIXTURE_EXPANSION_EXPECTED_STABLE` and `FIXTURE_EXPANSION_EXPECTED_DIAGNOSTIC` classification tests are present and passing. 29 fixtures total.
3. **CLI smoke-contract boundaries locked** — `docs/cli-phase2-boundary.md` exists; `tests/cli_smoke_contract.rs` asserts exact error strings for all 6 boundary cases; `phase2_contract.rs` centralizes the canonical strings, which are shared by library and CLI.

### Phase 3+ (HTML, PDF, storage, chunking, embeddings)
All correctly scaffolded as stubs returning explicit `Error::NotImplemented` or `Error::InvalidInput`. Dependency gate policy honored — no runtime/storage/vector deps added.

### Verified live CLI behavior

| Command | Expected | Actual |
|---|---|---|
| `ingest-emit <path> --format markdown` | Emits markdown | ✅ Correct |
| `ingest-emit <path> --format html` | Clap parse error, exit 2 | ✅ Correct |
| `emit --id <uuid> --format markdown` | Disabled error, exit 1 | ✅ Correct |
| `query` | NotImplemented, exit 1 | ✅ Correct |
| `context-window` | NotImplemented, exit 1 | ✅ Correct |
| `html` | NotImplemented, exit 1 | ✅ Correct |
| `pdf` | NotImplemented, exit 1 | ✅ Correct |

---

## 3. Findings

### 3.1 Untested opt-in parser feature — `normalize_repeated_leading_segment`

**Severity: Low / Hanging issue**

`ParseOptions::normalize_repeated_leading_segment` exists in the parser and has a corresponding fixture (`tests/fixtures/18_duplicated_trailing_segment.md`) that exercises the exact scenario it targets (frontmatter + heading repeated in the trailing body). However, the fixture is **not referenced by any test**. The `detect_repeated_leading_segment` code path runs on every parse and emits a `Diagnostic::Normalized`, but the actual deduplication branch (when the option is `true`) has zero test coverage.

This is the only fixture in the directory with no test entry point.

**Recommended action (Phase 3 or standalone):** Add a test using `ParseOptions { normalize_repeated_leading_segment: true, .. }` against `18_duplicated_trailing_segment.md` asserting the truncation behavior and the emitted diagnostic.

---

### 3.2 `NoopVectorIndex` / `NoopEmbedder` duplicated

**Severity: Low / Redundancy**

Both `src/cli/main.rs` and `tests/phase2_contract.rs` define identical `NoopVectorIndex` and `NoopEmbedder` structs with identical trait impls. The duplication is a natural consequence of the Phase 2 dependency gate (no new trait shims in the library until Phase 3+), but it will accumulate maintenance surface as Phase 3 approaches.

**Recommended action (pre-Phase 3 cleanup):** Expose a `nucklavee::testing` or `nucklavee::stubs` module behind a `cfg(test)` or `[dev-dependencies]` path, or consolidate into a shared test fixture helper crate, when Phase 3 wiring work begins.

---

### 3.3 Fixture numbering collisions

**Severity: Low / Cosmetic**

Multiple fixtures share a numeric prefix. Collisions exist at `13_`, `18_`, `19_`, `20_`, and `21_`. These do not cause any functional problem (all files have distinct names and are individually loaded by string), but they will continue to accumulate as new fixtures are added and make the sequence harder to scan.

| Prefix | Files |
|---|---|
| `13_` | `13_frontmatter_nested_yaml.md`, `13_syntax_preservation.md` |
| `18_` | `18_duplicated_trailing_segment.md`, `18_escaped_newline_frontmatter.md`, `18_normalized_bare_callout.md`, `18_phase2b_tsv_like.md` |
| `19_` | `19_phase2b_tsv_malformed.md`, `19_unfenced_frontmatter_block.md` |
| `20_` | `20_real_world_obsidian.md`, `20_styled_leading_list.md`, `20_styled_leading_list_items.md` |
| `21_` | `21_math_inline_underscore_asterisk.md`, `21_math_underscore.md` |

**Recommended action:** No urgent action needed. Accept the current naming, or re-sequence during a future batch fixture commit.

---

### 3.4 Clippy warnings (7)

**Severity: Low / Style**

```
cargo clippy
```

Produces 7 warnings, none of which affect correctness:

- **6× "this `if` statement can be collapsed"** — all in `src/emitters/markdown.rs` in the syntax-preservation scanner (`scan_for_literal_syntax_run`). These are nested `if let ... { if let ... }` guards that could be merged with `&&`. They are cosmetically verbose but functionally correct.
- **1× "this `let...else` may be rewritten with the `?` operator"** — `src/parsers/markdown.rs:1401` inside `normalize_bare_callout_paragraph`. The `?` form is cleaner but equivalent.

The existing `#[allow(clippy::too_many_arguments)]` on `end_tag` (`src/parsers/markdown.rs:871`) is already in place and appropriate for the 10-arg function.

**Recommended action:** Run `cargo clippy --fix --lib` when convenient, or address during the Phase 3 parser refactor.

---

### 3.5 `NEXT_BUILD_PUSH.md` is stale

**Severity: Negligible / Doc drift**

`NEXT_BUILD_PUSH.md` still describes Phase 2B hardening as the _active_ push, with status saying "Phase 2A complete; Phase 2B hardening active." The document is dated 2026-04-27 but the phase has since been completed.

**Recommended action:** Either update to reflect Phase 3 as the next focus, or retire the file in favor of `TODO.md` as the single status snapshot (which is already accurate).

---

### 3.6 Scaffold modules with no trait implementations

**Severity: Expected / By design**

The following modules are empty stubs with no trait implementations, as intended by the Phase 2 dependency gate policy:

| Module | Struct | Trait impl missing |
|---|---|---|
| `src/storage/sqlite.rs` | `SqliteDocumentStore` | `DocumentStore` |
| `src/vector/usearch.rs` | `UsearchIndex` | `VectorIndex` |
| `src/embedder/api.rs` | `ApiEmbedder` | `Embedder` |
| `src/context/mod.rs` | `ContextAssembler` | _(no trait yet)_ |
| `src/pipeline/mod.rs` | `IngestionPipeline` | _(no trait yet)_ |

Additionally, `DocumentStore::insert_chunks` and `get_chunks_by_document` are fully implemented on `InMemoryDocumentStore` but are never called from the `Library` API in Phase 2 (no chunking pipeline exists yet). This is intentional forward scaffolding.

**No action needed in Phase 2.** These become the primary implementation targets for Phase 3 and 4.

---

## 4. Cross-Phase Quality Gate Status

From `TODO.md` "Cross-Phase Quality Gates":

| Gate | Status |
|---|---|
| Unit tests for each module boundary | ⚠️ Partial — markdown parser/emitter thoroughly covered; storage, chunking, embedder, vector, context have no unit tests (Phase 3+ work) |
| Golden fixtures for parser/emitter stability | ✅ Covered — roundtrip harness + golden/determinism tests in `markdown_roundtrip.rs` |
| Property/invariant tests for IR validation and canonicalization | ✅ Covered — `tests/validation.rs` (9 tests) |
| Error taxonomy consistency (typed errors, no opaque String leaks in stable APIs) | ✅ Covered for Phase 2 surface — `phase2_contract.rs` centralizes boundary strings |
| Performance baselines for ingest/query on representative corpora | ❌ Not started (Phase 4+ appropriate) |
| Documentation updates per phase (README + examples + migration notes) | ✅ README accurate; TODO.md accurate; `NEXT_BUILD_PUSH.md` stale (see 3.5) |

---

## 5. Verdict

Phase 2B is complete and ready for handoff. The gate command is green, all 75 tests pass, the CLI behaves exactly per spec, and all Phase 2B roadmap items are delivered. The five findings above are all non-blocking; none represent contract breakage or regressions. The codebase is ready to begin Phase 3 (HTML parser/emitter + cross-format integrity).
