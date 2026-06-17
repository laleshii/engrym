---
id: hybrid-search
title: Hybrid search
altitude: 1
topics:
- search
relations:
- type: refines
  target: engrym-overview
- type: depends_on
  target: indexing-pipeline
summary: Keyword BM25 and vector cosine fused via reciprocal rank fusion.
---

# Hybrid search

`engrym search` (`src/commands/search.rs`) fuses two rankers: keyword BM25 over
the FTS5 table and semantic vector cosine over chunk embeddings. Keyword nails
exact identifiers like `OAuth2RefreshToken`; vectors catch paraphrases like
"how do we keep sessions alive".

## Modes and fusion

The two ranked lists are combined with reciprocal rank fusion (see
[[rrf-fusion]]). `--keyword` skips the model entirely; `--semantic` uses vectors
alone; hybrid (default) degrades gracefully to keyword when the index has no
embeddings. The vector half is produced by the [[embedding-layer]].
