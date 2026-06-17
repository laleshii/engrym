---
id: engrym-overview
title: engrym overview
altitude: 0
topics:
- architecture
summary: 'What engrym is: a fast, AI-first knowledge base layered over Markdown.'
---

# engrym overview

engrym is a fast, AI-first knowledge base layered over a repository's Markdown.
Authored `.md` files with YAML frontmatter are the single source of truth; a
derived SQLite index under `.engrym/` (gitignored, rebuildable) makes queries
sub-millisecond. The binary is global and points at any repo containing an
`engrym.toml`.

## Three layers

Markdown + frontmatter (git-tracked) → SQLite index (derived) → CLI / agent.
The index is never hand-edited and always rebuildable from the docs, so the KB
stays diffable, human-reviewable, and AI-writable. See [[data-model]] for how
documents relate, and [[cli-surface]] for the query commands.
