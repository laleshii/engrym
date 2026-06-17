---
id: sqlite-index
title: SQLite index schema
altitude: 2
topics:
- indexing/storage
relations:
- type: part_of
  target: indexing-pipeline
summary: 'The derived tables: docs, topics, edges, chunks, fts, embed_cache, meta.'
---

# SQLite index schema

The index is a single SQLite file at `.engrym/index.db`, created from
`spec/index-schema.sql` (embedded into the binary via `include_str!` in
`src/db.rs`). It is disposable and rebuildable.

## Tables

`docs` (one row per file), `topics` (assignments with depth), `edges` (typed
relations), `chunks` (passages with an `embedding` BLOB), a contentless FTS5
`fts` table for BM25, `embed_cache` (vectors by passage hash; survives rebuilds)
and `meta` (schema version, embed model and dim). A schema-version mismatch
forces a clean recreate.
