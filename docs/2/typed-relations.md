---
id: typed-relations
title: Typed relations
altitude: 2
topics:
- schema/relations
relations:
- type: part_of
  target: frontmatter-schema
summary: The five edge types and how wikilinks become references.
---

# Typed relations

Documents connect through typed edges declared in `relations`. The five kinds
(`EDGE_TYPES` in `src/model.rs`) are: `refines` (specific elaborates general),
`part_of` (component of a whole), `depends_on` (subject requires another),
`references` (loose see-also), and `supersedes` (replaces a deprecated doc).

## Wikilinks

Inline `[[doc-id]]` links in a body auto-generate `references` edges at index
time (extracted in `src/parse.rs`), so authors get graph connectivity for free.
Edges are stored in the `edges` table and queried by `engrym related`.
