# Markdown Normalization Delta Policy

This document defines which parse/emit differences are acceptable for the Markdown
roundtrip harness, and which differences are treated as semantic regressions.

Scope: `parse_markdown -> emit_markdown -> parse_markdown`, validated by
`structural_diff` over IR plus targeted string/golden checks.

## Policy model

### Allowed normalization deltas (non-regressions)

These changes are allowed when the reparsed IR remains structurally equivalent.

1. **Whitespace folding / paragraph wrapping**
   - Internal spacing and line wrapping may be normalized by the emitter.
   - Blank-line placement may be canonicalized where block boundaries remain the same.

2. **List marker canonicalization**
   - Equivalent bullet/ordered marker styles may be rewritten to canonical output.
   - List nesting is authoritative in IR; marker glyph choice is not.

3. **Delimiter canonicalization for escaped markdown syntax**
   - Equivalent escaped forms may be rewritten while preserving parsed inline/block meaning.

4. **Quote and callout line formatting normalization**
   - Physical line wrapping inside blockquotes/callouts may change.
   - Logical paragraph semantics and preserved line-break markers must remain stable.

5. **Emitter stabilization effects**
   - First-pass emission may canonicalize formatting; repeated parse/emit passes must converge
     to a deterministic fixed point.

## Semantic regressions (not allowed)

Any change below is a failure even if output still parses:

1. **Structural IR mismatch**
   - Any non-empty `structural_diff` between first parse IR and reparsed IR.

2. **Block type or nesting changes**
   - Paragraph turning into heading/list/table, list nesting drift, quote boundaries moving, etc.

3. **Syntax-preservation contract breaks (where explicitly required)**
   - Loss of literal syntaxes that are intentionally preserved by contract (e.g., wikilinks,
     callout markers, protected math forms).

4. **Frontmatter/body boundary regressions**
   - Frontmatter leaking into body blocks, or body content absorbed into frontmatter.

5. **Validation/provenance failures**
   - Invalid IR after parse/reparse.
   - Out-of-bounds provenance ranges.

## Test evidence map (`tests/markdown_roundtrip.rs`)

### A) Core structural equivalence gate
- `roundtrip_*` fixture tests: broad policy gate that allowed deltas are only accepted when
  reparsed IR remains structurally equivalent.

### B) Whitespace/list canonicalization and hard fixture normalization
- `roundtrip_lists`, `roundtrip_edges`, `roundtrip_hard`
- `parse_emit_is_deterministic_for_nested_lists_and_tables_fixture`

### C) Syntax preservation contracts (no regression)
- `emitted_text_preserves_wikilink_callout_and_math_syntax`
- `callout_blockquote_fixture_has_stable_golden_output_and_structure`
- `math_delimiters_fixture_has_stable_golden_output_and_structure`

### D) Frontmatter boundary integrity
- `roundtrip_frontmatter`
- `roundtrip_frontmatter_nested_yaml`
- `frontmatter_is_captured_and_not_treated_as_body`

### E) Deterministic convergence/stability
- `parse_emit_is_deterministic_for_nested_lists_and_tables_fixture`
- `parse_emit_is_deterministic_for_diagnostics_fixture`

### F) Validation/provenance invariants
- Validation checks embedded in `roundtrip(...)` and determinism helper assertions.
- `provenance_ranges_are_in_bounds`

## Practical review rule

When a Markdown output diff appears:

1. Reparse both versions and compare IR via `structural_diff`.
2. If structurally equivalent, classify under allowed normalization deltas above.
3. If structurally different, or if one of the explicit syntax/frontmatter/provenance contracts
   fails, classify as semantic regression and block.
