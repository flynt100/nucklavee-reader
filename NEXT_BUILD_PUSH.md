# Nucklavee Repo Analysis and Recommended Next Major Build Push

_Date analyzed: 2026-03-27_

## 1) Current implementation snapshot

The repository is in a deliberate scaffold phase:

- Public `Library` API exists, but all core methods still return `Error::NotImplemented`.
- IR types are present and now include a first pass of `SourceInfo` (`DocumentMeta.source: SourceInfo`) with a simple conversion from `Source`.
- Parser/emitter/chunker/storage/vector/embedder boundaries are in place as traits/modules.
- Concrete parser implementations (`markdown`, `html`, `pdf`) are still stubs returning not-implemented errors.
- CLI exists but only prints a scaffold message.

This indicates architecture-first progress with implementation still pending.

## 2) Planned roadmap signals

Three planning signals point to the immediate next push:

1. README "Near-Term Implementation Order" starts with IR test/validation harness, then Markdown parser+emitter roundtrip.
2. Week-1 TODO explicitly tracks seven IR finalization items, with item #1 done and item #2 marked as the next focus.
3. The spec is IR-first and emphasizes provenance metadata, dedupe (`content_hash`), and strict, trustworthy structure before pipeline features.

## 3) Gap analysis vs plan

### Already aligned

- Core module boundaries and trait seams are correctly laid out.
- `SourceInfo` has landed in the IR model, partially addressing Week-1 item #2.

### Highest-impact gaps still open

- Week-1 item #2 is not complete (current `SourceInfo` is a single `raw_source: String`, not yet normalized enough for robust storage/query/filter use).
- Week-1 item #3 (UUID/content-hash dedupe flow contract) is not wired at API boundaries.
- Week-1 items #4-#7 are all prerequisites for reliable parser roundtrip testing and stricter contracts.
- No parser implementation exists yet, so starting parser work now would force rework if IR invariants/errors/canonicalization are still moving.

## 4) Recommended next major build push

## **Push Name: "Week-1 IR Finalization Completion"**

Complete the remaining Week-1 IR reliability contract before parser implementation.

### Why this should be next

- It directly follows current TODO ownership and avoids context switching.
- It de-risks Week-2 parser/emitter work by freezing invariants and error surfaces first.
- It establishes stable test semantics (normalized equality + inline canonicalization) required for markdown roundtrip acceptance criteria in the spec.

## 5) Scope for this push (concrete deliverables)

1. **Finish SourceInfo normalization**
   - Replace/extend `raw_source` with structured, storage-ready fields (e.g., `kind`, `locator`, `display`, optional `origin_host` for URLs).
   - Preserve enough raw context for debugging while making metadata queryable.

2. **Define ingest identity contract**
   - Explicitly codify when `DocumentMeta.id` is generated (UUID v4).
   - Add deterministic content-hash function and document dedupe decision contract.

3. **Add semantic IR comparison utilities for tests**
   - Introduce normalized-equality helper(s) tolerant of insignificant whitespace.
   - Keep structural differences strict failures.

4. **Add inline canonicalization pass**
   - Merge adjacent `Inline::Text` nodes.
   - Enforce canonical order/shape where appropriate before emit/compare.

5. **Strengthen table and GenericBlock invariants**
   - Validation checks for row/cell consistency and legal confidence ranges.
   - Unit tests around malformed IR cases.

6. **Expand typed error model across traits**
   - Move `Result<_, String>` trait signatures toward typed domain errors.
   - Keep conversion points into top-level `Error` explicit.

7. **Backfill tests as hard gate**
   - IR validation unit tests.
   - Canonicalization tests.
   - Semantic equality tests.
   - Metadata normalization tests.

## 6) Definition of done

This push should be considered complete only when:

- Week-1 TODO items 2-7 are checked off.
- `cargo test` includes a dedicated IR contract suite with positive+negative cases.
- Public docs (`README` or TODO) are updated to mark the handoff to "Markdown parser + markdown emitter roundtrip" as the next implementation phase.

## 7) What to do immediately after this push

Start the next major push: **Markdown parser + markdown emitter + IR roundtrip tests** (README order #2), using the now-stable IR contract as the foundation.

---

## One-line decision

The next major build push should be **finishing Week-1 IR finalization (items 2-7) as a single reliability milestone**, then moving to Markdown roundtrip implementation.
