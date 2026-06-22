---
id: cli-surface
title: CLI surface
altitude: 1
topics:
- cli
relations:
- type: refines
  target: engrym-overview
summary: 'The command surface: index, search, topic, related, show, lint, serve, authoring, init, install.'
---

# CLI surface

`src/main.rs` defines the command surface with clap. Every command takes
`--json` for agents and `--repo <dir>` to target a repo other than the cwd.

## Commands

- `index` — (re)build the index (`src/commands/index.rs`).
- `search` — hybrid passage retrieval (see [[hybrid-search]]).
- `topic` / `related` / `show` — navigate the graph.
- `browse` — a local web server (`src/commands/browse.rs`) that renders docs as
  HTML, rewrites `[[wikilinks]]` to links, and shows related / same-altitude /
  same-topic panels. Server-rendered (no JS); only new dep is `tiny_http`.
- `new` / `set` / `rm` — author documents (see [[authoring-commands]]).
- `lint` — validate the frontmatter contract.
- `serve` — the warm embedding daemon (see [[warm-daemon]]).
- `init` — scaffold a repo and hand off to an agent (see [[init-and-skill]]);
  `--local` stores the KB outside the repo (see [[local-mode]]).
- `install skills` / `uninstall skills` — (re)install or remove the agent skills
  on demand (see [[init-and-skill]]).
- `install memory` / `uninstall memory` — record (or remove) this repo in an
  agent's global memory file so it learns the repo has a KB (see [[local-mode]]).
- `reset` — delete every document and the index (keeps `engrym.toml`); confirms
  unless `--yes`.
- `deinit` — the inverse of `init`: remove engrym's whole per-repo footprint (KB,
  in-repo skills, `.gitignore` entry, memory entry; or the local store). Leaves
  shared user-global skills and the binary (those are `uninstall`'s job).

`init`, `install`, `uninstall`, and `deinit` are dispatched in `run()` *before*
`Config::discover` — none requires a resolvable config (`init` has none yet;
`deinit` must work even when it's already gone, so it does its own *optional*
discovery). `reset` runs *after* discovery — it operates on the KB that resolves
for the current repo, in-repo or local.
