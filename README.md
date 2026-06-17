# engrym

A fast, AI-first knowledge base over Markdown. Your `.md` files (with YAML
frontmatter) are the source of truth; a disposable SQLite index gives instant
keyword, semantic, topic, and graph queries. Built for coding agents to *query
and encode* what a repo knows — and plain enough for humans to review in a diff.

> *engrym* — a play on **engram**, a stored memory trace.

## Install

```sh
cargo build --release                  # binary at target/release/engrym
./target/release/engrym install bin    # symlink onto PATH
```

## Use

```sh
engrym init                 # scaffold a repo + hand off to an agent to build the KB
engrym index                # (re)build the index from your docs
engrym search "how does hybrid search work"   # hybrid keyword + semantic retrieval
engrym topic indexing                         # everything under a topic
engrym related hybrid-search                  # typed graph neighborhood of a doc
engrym show engrym-overview                   # print a document
engrym deinit               # remove engrym from the repo entirely (inverse of init)
```

Every command takes `--json` (for agents) and `--repo <dir>` (target another
repo). This repo dogfoods itself — its [`docs/`](docs/) is an engrym KB about
engrym, so the examples above work right here.

## Commands

Notation: `<required>`, `[optional]`, `a|b` = choose one. Anything not bracketed
is typed literally.

| Command | What it does |
|---|---|
| `engrym init [--local] [--docs <dir>]` | Scaffold a repo and hand off to an agent |
| `engrym index [--no-embed]` | (Re)build the index |
| `engrym search <query> [--keyword\|--semantic] [--altitude <n>]` | Retrieve passages |
| `engrym topic <path>` | List documents under a topic |
| `engrym related <id>` | Show a document's graph neighborhood |
| `engrym show <id>` | Print a document |
| `engrym new <id> …` | Create a document (also `set`, `rm`, `relocate`) |
| `engrym lint [--strict]` | Validate the frontmatter contract |
| `engrym serve [--stop]` | Warm embedding daemon (usually automatic) |
| `engrym install <skills\|bin\|memory>` | Install agent skills, link the binary, or record the repo in agent memory |
| `engrym uninstall <skills\|bin\|memory>` | Inverse of `install` |
| `engrym reset` | Delete the KB's documents + index (keeps config) |
| `engrym deinit` | Remove engrym from the repo entirely (inverse of `init`) |

## Data model

Each document is Markdown with a small frontmatter contract (full spec:
[`spec/document-schema.md`](spec/document-schema.md)):

```yaml
---
id: oauth-token-refresh          # required · stable, unique — the identity
title: OAuth token refresh flow  # required
altitude: 3                      # required · 0 = overview … 3 = impl detail
topics: [backend/auth/oauth]     # required · slash-paths, hierarchy implicit
relations:                       # optional · typed edges to other ids
  - { type: refines,    target: auth-architecture }
  - { type: depends_on, target: token-store }
---
Body prose. Inline [[wikilinks]] become `references` edges for free.
```

Three hierarchies make "abstract → specific" navigable: the **topic** taxonomy
(`engrym topic`), typed **relations** (`refines`/`part_of`/`depends_on`/…), and
**altitude** (0–3). A document's `id` is its identity — links never reference
file paths — so the on-disk `layout` (`flat` / `topic` / `altitude`) is purely
for human review, and `relocate` rearranges files safely.

## How it works

- **Hybrid search** — BM25 (exact terms, identifiers) and vector cosine
  (meaning) fused with reciprocal rank fusion. `--keyword` / `--semantic` force
  one ranker; an unembedded index falls back to keyword.
- **Local embeddings** — offline by default
  ([fastembed](https://github.com/Anush008/fastembed-rs), `bge-small-en-v1.5`).
  Your code never leaves the machine; only changed passages re-embed.
- **Warm daemon** — the first semantic query spawns a tiny background daemon
  that keeps the model resident (~130ms → ~13ms), self-terminating when idle.
  Auto-managed; `ENGRYM_NO_DAEMON=1` opts out.
- **Authoring** — `new`/`set`/`rm` *generate* frontmatter (never hand-write it)
  and edit source files by `id`, so they work regardless of index freshness.

## Agents

`init` installs two skills into your chosen agent (Claude Code, Codex, …): a
**bootstrap** skill that builds the initial KB, and a **working** skill that
retrieves before a task and captures durable findings after — pull-based and
model-judged, never a hook on every prompt.

**Local mode** (`engrym init --local`) keeps the entire KB outside the repo
(`~/.engrym/projects/<repo>-<hash>/`, keyed by git root) so the repo is never
touched. Because there's then no in-repo cue, `init --local` also records the
repo in the agent's global memory (`~/.claude/CLAUDE.md`, `~/.codex/AGENTS.md`);
`install`/`uninstall memory` manage it on demand.

## Configuration (`engrym.toml`)

```toml
[docs]
root   = "docs"                # where the Markdown KB lives
layout = "altitude"            # flat | topic | altitude

[embedding]
provider = "local"             # offline by default
model    = "bge-small-en-v1.5"

[search]
rrf_k = 60

[lint]
strict = false                 # CI passes --strict

[daemon]
enabled   = true
idle_secs = 300
```

## Architecture

```
Markdown + frontmatter   →   SQLite index (.engrym/)   →   CLI / agent
(authored, git-tracked)      (derived, gitignored)         (query surface)
```

The index is never hand-edited and always rebuildable from the docs. Schema:
[`spec/index-schema.sql`](spec/index-schema.sql). Deeper design notes live in
the KB itself — try `engrym search "…"` or read [`docs/`](docs/).
