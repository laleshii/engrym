---
id: embedding-layer
title: Embedding layer
altitude: 1
topics:
- embedding
relations:
- type: part_of
  target: hybrid-search
summary: Local, offline embeddings via fastembed (bge-small-en-v1.5).
---

# Embedding layer

`src/embed.rs` wraps `fastembed` to produce local, offline embeddings with
`bge-small-en-v1.5` (384-dim ONNX) — code never leaves the machine. The model
is lazy-loaded, so only commands that need vectors pay the cost, and it is
cached in one global location so target repos are never polluted.

## Retrieval recipe

Passages are embedded verbatim; queries get a BGE instruction prefix. All
vectors are L2-normalized so cosine similarity is a dot product (`src/vector.rs`).
Loading the model costs ~120ms, which the [[warm-daemon]] amortizes.
