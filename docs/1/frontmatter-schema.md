---
id: frontmatter-schema
title: Frontmatter schema
altitude: 1
topics:
- schema
relations:
- type: refines
  target: data-model
summary: The machine-readable contract every document carries.
---

# Frontmatter schema

Every document opens with a YAML frontmatter block — the machine-readable
contract — followed by prose. The contract is defined by `Frontmatter` in
`src/model.rs` and documented in `spec/document-schema.md`.

## Fields

`id` (stable, kebab-case, unique), `title`, `altitude` (see [[altitude]]),
`topics` (≥1; see [[topic-taxonomy]]), optional `relations` (see
[[typed-relations]]), and an optional `summary`. Fields are parsed as optional
so `lint` can report *which* required fields are missing; `index` enforces
presence. The same struct is serialized when authoring, so its field order is
the on-disk order.
