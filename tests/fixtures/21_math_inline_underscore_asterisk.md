# Math Inline Underscore Asterisk Fixture

Inline protected spans should remain literal: $m_\ell$, $a*b$, and escaped delimiters \$not-math\$.

Malformed spans should remain unchanged and produce diagnostics: $unterminated.

- list item with inline math $m_\ell$
- list item with styled marker and math **strong** then $a*b$
- list item mixing inline math markers $x_1 + y_2$

Paragraph with mixed markdown and math: before *italic* then $a*b$ then after.
