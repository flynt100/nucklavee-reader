_Expected stable: preserve escaped markdown syntax at emphasis/link/code boundaries and avoid introducing diagnostics._
# Escape Boundaries

Literal punctuation: \*not emphasis\*, \_not italic\_, \[not a link\], and \\ backslash.

Escapes adjacent to formatting: **bold \* inside** and *italic with escaped \_ underscore*.

Inline code should remain literal: `\* \_ \[ \]`.

A tricky link label: [escaped \[label\] text](https://example.com/path?q=a\&b=c).
