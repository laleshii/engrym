---
id: rrf-fusion
title: Reciprocal rank fusion
altitude: 3
topics:
- search/hybrid
relations:
- type: refines
  target: hybrid-search
summary: score(d) = sum over rankers of 1/(k + rank).
---

# Reciprocal rank fusion

engrym fuses the BM25 and cosine rankings with reciprocal rank fusion: each
document's score is the sum over rankers of `1 / (k + rank)`, where `k` is the
`rrf_k` smoothing constant from `engrym.toml` (default 60). Higher is better.

## Why RRF

RRF needs no score calibration between the two very different rankers (BM25
magnitudes vs. cosine in [-1, 1]) — only their rank orders. A single ranker
still produces a valid monotonic ordering, which is why hybrid can collapse to
keyword-only cleanly. Ties break deterministically by chunk id.
