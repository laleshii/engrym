---
id: design-decisions
title: Design decisions
altitude: 0
topics:
- architecture
relations:
- type: references
  target: engrym-overview
- type: references
  target: data-model
summary: Why engrym is shaped the way it is — the rationale behind the core choices.
---

# Design decisions

The rationale behind engrym's core choices, distilled from the original design
discussion. These are the *why*s that the code alone doesn't explain.

## Markdown is the source of truth; the index is derived

Documents are plain Markdown with YAML frontmatter, committed to the repo; the
SQLite index under `.engrym/` is a disposable cache, gitignored and rebuildable
from the docs at any time. This keeps the knowledge diffable, human-reviewable,
and AI-writable, while queries stay fast. Nothing important is ever trapped in a
binary format. See [[engrym-overview]].

## Local-first embeddings, by privacy

engrym is meant to index arbitrary — often proprietary — repositories, so the
default embedder is local and offline (`bge-small-en-v1.5` via fastembed): code
and docs never leave the machine. An API embedder is opt-in only. The storage
cost of vectors is negligible, so privacy and zero network dependence drove the
choice, not size.

## Three complementary hierarchies

"Abstract → specific" is captured three independent ways on purpose (see
[[data-model]]): a topic taxonomy (what a doc is about), typed relations (how
docs connect), and altitude (how deep a doc goes). They are orthogonal so an
agent can enter by subject, by graph edge, or by depth — whichever fits the
question — rather than being forced through a single tree.

## Hybrid retrieval over embeddings alone

Search fuses BM25 and vector cosine rather than relying on either. Keyword
search nails exact identifiers like `OAuth2RefreshToken` that embeddings blur;
vectors catch paraphrases that keywords miss. Reciprocal rank fusion combines
them without needing to calibrate their incomparable score scales. Passages are
chunked by heading so retrieval returns the relevant section, not a whole file.

## A single Rust binary

The tool must feel instant on the command line and be trivially droppable into
any repo or agent. A single static Rust binary gives near-zero startup, no
runtime dependencies, and an embedded SQLite — the right fit for "blazing fast,
CLI-accessible, point at any repo."

## The id is the identity, not the path

A document is identified by its frontmatter `id`; the index, relations, and
lookups never depend on its file path. This decoupling is what lets file layout
be a pure human-review concern — files can be flat or mirror the topic tree, and
`engrym relocate` can move them freely without touching the graph.

## A warm daemon, not MCP, for agent speed

The cost of semantic search is loading the embedding model (~120ms), not the
search itself. Rather than a heavyweight protocol layer, the first semantic
query auto-spawns a tiny background daemon that holds the model resident and
self-reaps when idle, dropping repeated queries to ~13ms. A thin MCP wrapper over
the CLI remains possible, but the latency win lives in the daemon.

## The name

*engrym* is a coined spelling: an **engram** is a stored memory trace — exactly
what each document is — blended with **n-gram**, the unit of language. It was
chosen to be free on crates.io and GitHub while staying on-theme for a
language-oriented knowledge base.
