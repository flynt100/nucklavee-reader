# Week 1 IR Finalization TODO

Tracking the seven Week 1 design items from planning.

1. [x] **Strict typed IR with controlled `GenericBlock` fallback**
   - Added IR-side guardrails so `GenericBlock` is explicit and confidence-bounded.
2. [ ] **Normalized source metadata (`SourceInfo`) suitable for storage**
3. [ ] **UUID v4 IDs + content-hash dedupe flow contract**
4. [ ] **Normalized semantic equality for roundtrip tests**
5. [ ] **Inline canonicalization pass (merge adjacent text nodes)**
6. [ ] **Table cell fidelity (`Vec<Inline>`) + invariants/tests**
7. [ ] **Typed error model expansion across module traits**

## Next focus
- Implement Item 2 without crossing into Week 2 parser behavior.
