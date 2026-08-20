---
id: workspace-search
title: Workspace search across sibling repos
altitude: 2
topics:
- cli
relations:
- type: refines
  target: cli-surface
- type: references
  target: hybrid-search
- type: references
  target: worktree-and-clone-kb-resolution
summary: Standing in a folder of clones, engrym searches every child repo's KB and groups the hits by repo; --all opts into the same fan-out from inside a repo, and --depth widens the scan to nested <org>/<repo> layouts.
---

# Workspace search across sibling repos

engrym is anchored to a repo, but a working machine is usually a *folder of
clones*: `~/Projects/{api,web,jobs}`, each its own repo with its own KB. Standing
in that parent folder, "search" should mean "search all of them" — cross-service
answers are exactly what you're there to find. `src/workspace.rs` turns that
folder into a **workspace**: a set of KBs a command operates on, replacing the
single `Config` that used to be threaded through every command.

## Resolution order

Ordered so it never surprises an existing user:

1. A KB reachable by walking **up** from the cwd wins, exactly as before
   ([[local-mode]] and worktree/clone resolution included). Single-KB output is
   byte-for-byte what it was.
2. Only when there is none do we look **down**, one level, for directories that
   carry their own KB. That's the "master folder of clones" case, and it's
   automatic because there is nothing to conflict with.
3. If *that* is empty and we're standing in a git checkout that simply doesn't
   use engrym, we look at the folder holding it — the other common place to be
   standing, where the repos you meant are the ones beside you. This is the same
   one-level-up root `--all` uses, so the rule "never infer a root further than
   one level up" still holds.

   The exception is a different **clone of this same repo** sitting alongside:
   that's not a sibling service, it's this repo's KB under another path, and
   adopting it is `engrym link`'s deliberate job (see
   [[worktree-and-clone-kb-resolution]]) — never a scan's side effect. Members
   matching the current checkout's `origin` identity are dropped from the
   fallback, so `engrym where` still answers with a `link_candidate` there.
4. `--all` (a global flag) asks for the fan-out explicitly from *inside* a repo.
   The workspace root is then the folder holding that repo, so its siblings join
   in.
5. `--depth N` widens the scan to `N` levels and implies `--all` — asking for
   reach *is* asking to search wider, so it would be a trap for `--depth 2`
   inside a repo to silently do nothing.

`Config::discover_here` is what keeps this honest: unlike `discover` it never
climbs. A directory qualifies as a member only if it holds `engrym.toml` itself,
or is a git checkout with a local store bound to it. Without that rule every
subdirectory of a repo would "have" that repo's KB, and a folder of clones would
match itself recursively.

Three more constraints fall out of the same instinct:

- **The walk stops at every git checkout.** Pruning *descent*, not membership: a
  nested checkout with a KB is still added, we just never look inside it. So
  `/work/repoA` (no KB) containing `repoB` and `repoC` resolves to those two when
  you stand in `repoA` — but from `/work`, `repoA` is a checkout and gets pruned,
  so the nested pair is invisible at *any* depth. Nested repos are reachable only
  from the directory immediately containing them. Not a perf hack — sibling repos
  sit *beside* each other, never inside one another, so a vendored checkout or
  submodule buried in `api/` is part of `api`, not a peer of it. It's also what
  makes depth affordable: `--depth 3` over a folder of JS repos costs one `stat`
  per candidate and never stats its way through a `node_modules`. The walk only
  spans the thin scaffolding *between* repos.
- **Depth widens downward only.** The root is the directory you're standing in,
  or — from inside a repo — the one folder holding it. It is never inferred
  further up, because "go up 3 and scan 3 back down" from `~/Projects/org/api`
  roots at `/`. A different root is asked for explicitly with `--repo <dir>`.
  Depth is capped at 5, which covers any real layout.
- **Members are de-duplicated by store.** Worktrees and linked clones share one
  KB ([[worktree-and-clone-kb-resolution]]); keeping both checkouts would make
  the same KB answer twice under two names.

## Grouped, not interleaved

Each member is searched independently by the existing pipeline
([[hybrid-search]]) and hits are **grouped under the repo that produced them**.
That's a deliberate limit: RRF scores are reciprocal *ranks*, so rank 1 in an
irrelevant repo scores exactly what rank 1 in the perfect repo does. Fusing
across indexes would manufacture a global ranking the numbers can't support.
Groups are ordered by their best hit and never interleaved, and `--limit` applies
per repo — each group is its own ranking.

The one thing genuinely shared across members is the **query embedding**: it's
cached by model name, so a semantic fan-out over ten repos pays for one
embedding, not ten (a daemon round-trip or model load each — see [[warm-daemon]]).

## Addressing a hit in another repo

Members are named by their **path relative to the workspace root** — `api` when
clones sit side by side, `org-a/api` when they're grouped a level deeper — and
that name qualifies a document: `engrym show org-a/api:auth-overview`. Relative
paths are unique by construction, which plain basenames stop being the moment two
orgs both have an `api`. A bare id still works while it's
unambiguous; when several repos define it, engrym lists them and asks for the
qualifier rather than guessing. Search and topic output carry a ready-made `ref`
field in `--json` for exactly this, and the human-readable follow-up hint repeats
the `--depth` the workspace was found at — a qualifier is only resolvable at the
reach that discovered it.

`engrym where` answers for whatever scope the other commands would resolve,
`--all` and `--depth` included — a gate that reports one in-repo KB while
`search --all` spans the siblings is worse than no gate.

## What does *not* fan out

`search`, `topic`, `show`, `related` and `index` span the workspace. Everything
that mutates a KB or is otherwise repo-specific — authoring, `reset`, `lint`,
`browse`, `serve`, `deinit` — goes through `Workspace::only`, which errors with
the member list unless exactly one KB is in scope. Fanning a *write* across
repos because of where the shell happened to be is a footgun, not a convenience;
`--repo <dir>` picks one explicitly.
