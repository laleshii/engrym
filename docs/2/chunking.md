---
id: chunking
title: Heading chunker
altitude: 2
topics:
- indexing
relations:
- type: part_of
  target: indexing-pipeline
summary: Passage-level chunking by Markdown heading.
---

# Heading chunker

`src/parse.rs` chunks a document body into passages, one per heading section;
the lead text before the first heading becomes chunk 0. Chunks are what engrym
embeds and full-text indexes, so retrieval returns the *relevant section*
rather than a whole file.

## Text handling

Markdown events are concatenated verbatim (preserving inline runs like
`[[wikilinks]]`), with whitespace collapsed and block boundaries separated, so
the stored passage text is clean for both BM25 and embeddings.
