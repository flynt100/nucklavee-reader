# Real World Mixed Fixture

## Context

This fixture approximates real-world mixed authoring with links, lists, and equations.

- **Status:** Draft notes from [[Project Sync]].
- Owner: [Docs Team](https://example.com/docs).
- Equation reference: $P_t = P_{t-1} + \Delta_t$.

### Steps

1. Review callout style and preserve markdown semantics.
2. Verify mixed inline math like $f_i(x) = x_i^2$ remains unchanged.
3. Keep fenced code stable.

```python
for i in range(3):
    print(f"x_{i}")
```

> [!note]
> This blockquote should remain a single canonical callout paragraph.
