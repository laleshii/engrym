---
id: indexing-pipeline
title: Indexing pipeline
altitude: 1
topics:
- indexing
relations:
- type: refines
  target: engrym-overview
- type: depends_on
  target: frontmatter-schema
summary: How engrym index turns Markdown into the searchable SQLite index.
---

# Indexing pipeline

`engrym index` (`src/commands/index.rs`) does a full *structural* rebuild on
every run: it wipes the content tables, walks the docs root, parses and
validates each file, and inserts docs, topics, edges, and chunks.

## Stages

Files without frontmatter are skipped (they are plain Markdown, not KB docs).
Each doc is chunked by heading (see [[chunking]]) into passages stored in the
[[sqlite-index]]. A separate embedding pass then fills in vectors, reusing
cached ones where the passage text is unchanged (see [[incremental-reembed]]).
