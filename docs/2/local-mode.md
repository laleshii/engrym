---
id: local-mode
title: 'Local mode: external KB storage'
altitude: 2
topics:
- cli
relations:
- type: part_of
  target: init-and-skill
- type: references
  target: cli-surface
summary: init --local stores the KB under ~/.engrym/projects/ keyed by the repo, so engrym never writes into the repository.
---

# Local mode: external KB storage

By default an engrym KB lives in the repo: `engrym.toml`, the `docs/` tree, and
the gitignored `.engrym/` index all sit at the repo root. `engrym init --local`
instead stores everything **outside** the repo, so the repository is never
modified — useful for repos you can't or don't want to commit a KB into.

## Where it lives

State goes under `~/.engrym/projects/<key>/` (override the root with
`$ENGRYM_HOME`). The `<key>` is `<repo-basename>-<8 hex of sha256(path)>`,
computed by `config::project_key` from a stable *anchor*: the repo's git
top-level (`config::repo_anchor`), so queries resolve from any subdirectory. The
external folder holds the same `engrym.toml`, `docs/`, and `.engrym/` index a
normal repo would — no `.gitignore` is written, because nothing lands in the
repo to ignore.

## Discovery

`config::discover` resolves a KB in two steps: an in-repo `engrym.toml` (found by
walking up) is canonical and wins; otherwise engrym computes the anchor's key and
looks for `~/.engrym/projects/<key>/engrym.toml`. When that external config
loads, `Config::source_repo` records the bound repo (surfaced by `engrym index`
as a "local KB" note). Every command — `search`, `new`, `index`, … — then works
identically; only `repo_root` points at the external folder.

## Skill placement

Local mode must not write into the repo, so skill install adapts: Claude Code's
normally project-level `.claude/skills/` becomes user-global `~/.claude/skills/`
(`KnownAgent::skills_for`). Codex is already user-global, so it's unchanged. See
[[init-and-skill]].

## The invisible-KB gotcha, and `install memory`

Local mode has a real downside: with **nothing in the repo**, the agent has no
cue that engrym is available there. The working skill is pull-based and
model-judged, and its trigger is "a repo set up with engrym" — but the only
in-repo signal (`engrym.toml`) is exactly what local mode removes. A user-global
skill is loaded everywhere yet can't tell *which* repos are engrym repos, so it
rarely fires on its own. Local mode (zero repo footprint) and reflexive agent
use (needs a repo cue) pull against each other.

The memory note bridges this without touching the repo: it records the repo's
anchor path in the agent's **user-global memory file** (`~/.claude/CLAUDE.md`, or
`~/.codex/AGENTS.md` for Codex — `CODEX_HOME` defaults to `~/.codex`), inside a
marker-delimited block engrym owns (`agents::add_memory_entry` /
`render_memory_block`). That memory is loaded in every session, so the agent
learns "this repo has an engrym KB" wherever you work.

`engrym init --local` writes this note automatically for the chosen agent (plain
in-repo `init` does *not* — its committed `engrym.toml` + project skill are
already the cue). `engrym install memory [--agent <bin>]` does it on demand, and
`uninstall memory` removes just that repo's line, deleting the whole block once
it's empty. If reflexive use matters more than a clean repo, in-repo mode is the
better fit.
