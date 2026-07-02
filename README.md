# engrym

**A knowledge base for your codebase — stored as Markdown your coding agent can
read and write.**

engrym captures what a repo *knows* (architecture, decisions, the non-obvious
gotchas) as a graph of plain `.md` files, then builds a disposable SQLite index
over them for instant keyword, semantic, topic, and graph search. It's built so
coding agents **retrieve** that knowledge before a task and **record** durable
findings after — so the same things stop getting re-explained every session.
The source of truth stays plain Markdown with a little YAML frontmatter, so a
human reviews it in a normal diff.

> *engrym* — a play on **engram**, a stored memory trace.

## Why you'd want it

- **Onboard in minutes, not days.** `engrym search "how does auth work"` returns
  the exact passage — no spelunking through the codebase.
- **Your agent stops re-deriving the obvious.** Knowledge compounds in the repo
  instead of evaporating when the chat window closes.
- **No lock-in, no cloud.** Just Markdown plus a rebuildable index. Embeddings
  run locally and offline by default — your code never leaves the machine.
- **Reviewable like code.** Every fact is a line in a `.md` file; changes show
  up in pull requests.

## Install

Needs a Rust toolchain. If you don't have one:

```sh
brew install rust                                                # macOS (Homebrew)
# or, any platform, via rustup:
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # then: source "$HOME/.cargo/env"
```

Then install from crates.io (cargo builds it and places `engrym` on your PATH;
all deps, incl. bundled SQLite, are pulled in — no system libraries needed):

```sh
cargo install engrym
```

Or build from a clone of this repo:

```sh
cargo install --path .     # builds + installs onto PATH
# or just: cargo build --release   → binary at target/release/engrym
```

## Quick start

In any repo, one command sets everything up:

```sh
engrym init      # scaffold engrym + hand off to your agent to build the initial KB
```

`init` writes `engrym.toml`, installs the agent skills, and hands a prompt to
your coding agent (Claude Code, Codex, …) to author the first docs *from your
codebase*. From then on the agent retrieves and records knowledge on its own.

**Just want to try it without touching the repo?** Add `--local`:

```sh
engrym init --local   # KB lives outside the repo, in ~/.engrym/ — zero files added
```

Local mode keeps the entire KB (docs + index) under `~/.engrym/`, keyed to the
git root, so nothing is committed and the working tree stays clean. It's the
low-commitment way to start; everything below works identically. (See
[Local mode](#agents) for how the agent still finds it.)

Once a KB exists, query it:

```sh
engrym search "how does hybrid search work"   # hybrid keyword + semantic retrieval
engrym topic indexing                         # everything under a topic
engrym related hybrid-search                  # a document's typed graph neighborhood
engrym show engrym-overview                   # print a document
engrym browse                                 # read & navigate the KB in your browser
engrym index                                  # (re)build the index after editing docs
```

Every command takes `--json` (for agents) and `--repo <dir>` (target another
repo). **This repo dogfoods itself** — its [`docs/`](docs/) is an engrym KB
about engrym, so the queries above all work right here, right now.

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
| `engrym browse [--port <n>] [--open]` | Local web UI to read/navigate the KB |
| `engrym serve [--stop]` | Warm embedding daemon (usually automatic) |
| `engrym where` | Report whether a KB is reachable here (a fast gate for agents) |
| `engrym list` | List local KB stores and how they're shared across checkouts |
| `engrym link <key\|path>` / `unlink` | Share (or detach) this checkout's KB with another clone |
| `engrym install <skills [--refresh]\|memory>` | Install/refresh agent skills, or record the repo in agent memory |
| `engrym uninstall <skills\|memory>` | Inverse of `install` |
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

**Worktrees & multiple clones.** A local KB is keyed to the repo, not the
checkout: git worktrees resolve back to one shared store automatically, and
separate clones of the same repo (matched by `origin` URL) can share one too —
`engrym init` offers to link a same-repo clone, or use `engrym link` explicitly.
Discovery is a query, not a list: agents run `engrym where` (which resolves
worktrees and links) to decide whether a KB applies, so there's nothing to keep
updated by hand. The self-gating **working** skill installs once, globally, and
no-ops where there's no KB; after upgrading the CLI, `engrym install skills
--refresh` brings installed copies up to date.

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
