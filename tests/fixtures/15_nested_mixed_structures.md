_Expected stable: normalize marker/spacing choices only; preserve list -> blockquote -> table nesting and paragraph boundaries without diagnostics._
# Nested Mixed Structures

- Parent item with an embedded quote:

  > Quoted intro line.
  >
  > | Key | Value |
  > | --- | --- |
  > | alpha | `code` |
  > | beta | **bold** |
  >
  > Closing quote line after table.

  Continuation paragraph after quote.

- Second parent item
  1. Ordered child
  2. Ordered child with quote

     > inner quote in ordered item
     >
     > - bullet one
     > - bullet two
