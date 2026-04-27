# Nucklavee Next Build Push

_As of 2026-04-27_

## Snapshot (current truth)

- **Phase 1 is complete** and its quality gate is satisfied:
  - IR validation invariants complete and tested.
  - Normalization + structural equivalence stable.
  - Markdown parse->emit->parse fixtures passing.
  - Diagnostics behavior for unsupported/lossy cases defined.
- **Phase 2A is complete**:
  - `MarkdownParser` implemented (`pulldown-cmark`) with tree construction.
  - Markdown emitter implemented for semantic IR output.
  - IR roundtrip harness in place (parse -> emit -> parse -> semantic compare).
- **Active implementation target: Phase 2B hardening**.

This supersedes older scaffold-era notes that described parser/emitter components as stubs.

## Active Push: Phase 2B Hardening

### Goal

Tighten behavior contracts and handoff boundaries around the now-complete Markdown core loop.

### Source of truth alignment

This push is derived directly from:

- `TODO.md` (Phase 2B checklist)
- `docs/phase-gates.md` (Phase 1 exit criteria and blocker/warning semantics)

### Scope (Phase 2B)

1. **Define and document acceptable normalization deltas**
   - Explicitly codify what whitespace-only or formatting-only markdown changes are acceptable when structural equivalence is preserved.

2. **Expand fixture coverage for difficult Markdown edge patterns**
   - Add targeted deltas/cases that stress parser/emitter normalization boundaries while maintaining gate semantics.

3. **Define and lock CLI integration boundary**
   - Establish the exact Phase-2 CLI contract for parser/emitter + roundtrip workflow handoff.

## Definition of done for this push

- Phase 2B checklist items in `TODO.md` are checked.
- Any docs/tests touched by the above changes remain consistent with Phase-1 gate definitions in `docs/phase-gates.md`.
- Planning docs continue to state the current reality: **Phase 1 + Phase 2A done; Phase 2B hardening active**.

## Why this is the correct next push

- It matches the current roadmap state and avoids regression to outdated scaffold assumptions.
- It preserves the Phase-1 quality bar while hardening contracts needed before broader phase expansion.
- It reduces drift between planning documents by anchoring status to an explicit dated snapshot.
