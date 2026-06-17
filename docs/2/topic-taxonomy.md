---
id: topic-taxonomy
title: Topic taxonomy
altitude: 2
topics:
- schema/topics
relations:
- type: part_of
  target: frontmatter-schema
summary: Slash-delimited topic paths with an implicit hierarchy.
---

# Topic taxonomy

Topics are slash-delimited paths whose tree is implicit: a document tagged
`backend/auth/oauth` is also under `backend/auth` and `backend`. No separate
taxonomy file is needed — the hierarchy lives in the paths.

## Subtree queries

`engrym topic backend/auth` returns every document at or below that prefix,
implemented as a `LIKE` prefix match on the `topics` table. Topics answer *what*
a document is about, an axis orthogonal to [[altitude]] and to the
[[typed-relations]] graph.
