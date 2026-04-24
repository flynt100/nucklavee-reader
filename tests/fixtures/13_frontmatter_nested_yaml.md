---
title: "Nested YAML frontmatter"
aliases:
  - "[[Daily/Note]]"
  - "[[Project::Nucklavee]]"
matrix:
  - - "a"
    - "[[b]]"
  - - "c:d"
    - "value with # hash and : colon"
metadata:
  links:
    - path: "[[One]]"
      tags:
        - "a/b"
        - "[[tag::x]]"
  flags:
    published: true
    reviewed: false
---

# Nested Frontmatter Fixture

Body text should remain outside frontmatter.
