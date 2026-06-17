---
id: incremental-reembed
title: Incremental re-embedding
altitude: 3
topics:
- embedding
relations:
- type: refines
  target: embedding-layer
- type: references
  target: sqlite-index
summary: Only passages whose text changed are re-embedded, via embed_cache.
---

# Incremental re-embedding

Embedding is the one slow step, so `index` never repeats it needlessly. Each
chunk's vector is cached in the `embed_cache` table keyed by the SHA-256 of its
passage text. The cache survives the structural rebuild that every `index`
performs.

## Behavior

On re-index, unchanged passages hit the cache (0 new) and only edited passages
re-embed. The cache is pruned to live passages each run, and cleared wholesale
when the configured model changes, since vectors across models are
incomparable.
