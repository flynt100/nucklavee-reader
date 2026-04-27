_Expected diagnostic: malformed-but-common authoring (raw HTML and rich image alt text) should produce diagnostics while still roundtripping structurally._
# Common Malformed Variants

<div class="admonition">This raw HTML block should be flagged as unsupported.</div>

![alt with **bold** and `code`](https://example.com/image.png)

Paragraph with an unclosed emphasis marker *that often appears in rough notes.
