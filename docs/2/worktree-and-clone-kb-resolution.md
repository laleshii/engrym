---
id: worktree-and-clone-kb-resolution
title: Worktree & multi-clone KB resolution
altitude: 2
topics:
- cli
relations:
- type: refines
  target: local-mode
- type: references
  target: agent-integration
summary: How local-mode KBs stay unified across git worktrees and separate clones of the same repo, and how discovery works as a query (engrym where/list) rather than a hand-maintained list.
---

# Worktree & multi-clone KB resolution

Local mode keys a KB by the repo's *checkout path* ([[local-mode]]). That breaks
down in two workflows: git **worktrees** (each linked worktree has its own path)
and separate **clones** of the same remote (e.g. a standalone dev clone plus a
worktree-hub's parent clone). Both are "the same repo" but would otherwise get
distinct, empty KBs — and captured findings would be stranded when an ephemeral
worktree is torn down. Three coordinated mechanisms fix this.

## (a) Worktree-aware anchor

`config::repo_anchor` walks up to the first `.git`. A linked worktree's `.git` is
a *file* (`gitdir: …`), not a directory. `worktree_main_root` reads that pointer
and the worktree gitdir's `commondir` file to resolve the **main worktree root**
— with no `git` subprocess (consistent with engrym bundling everything). So every
worktree of a repo folds to one anchor, hence one KB. Bare repos / malformed
metadata fall back to the worktree path (no regression).

## (b) Identity + registry

- **Identity** = the normalized `origin` URL, read from `<anchor>/.git/config`
  (`registry::repo_identity` + `normalize_git_url`, which collapses scp-vs-https
  forms). It's the signal that two *clones* are the same repo. No `origin` →
  fall back to path-based keying.
- **Registry** = `~/.engrym/registry.json`: entries of `{ key, identity,
  anchors[] }` mapping many anchors to one store `key`. `config::local_key`
  resolves an anchor to its store via the registry, else the default
  `project_key`. Pure lookup — it never links silently.
- **Establishing a link** is deliberate: `init --local` detects a same-identity
  store and *prompts* to reuse it (`reuse_existing_kb`); `engrym link`/`unlink`
  do it explicitly. Worktrees need no prompt — (a) already unifies them.
- **Lazy learn**: `Config::discover` records the anchor↔store mapping (and
  identity) on load, writing only on change, so a store created before the
  registry becomes linkable by identity.
- **Self-heal**: when the registry file is *missing* (first use after upgrade, or
  deleted), `Registry::reconcile` backfills entries for every on-disk store by
  reading its `# Bound to repo:` header → anchor + identity. Gated on absence, so
  it's a one-time migration, not a per-command scan (`load_migrated`).

Forks stay separate (different origin). Root-commit hash is deliberately *not*
used as the identity — forks and template-derived repos share it, so it's too
loose.

## (c) Discovery is a query, not a list

There is no hand-maintained list of "engrym repos" to keep updated. Instead:

- `engrym where` — a fast gate (exit code + `--json` `{kb, mode, shared,
  identity, link_candidate}`). Reports a KB whether in-repo or local, resolving
  worktrees/links; on a miss it surfaces a `link_candidate` (a same-repo KB under
  another clone).
- `engrym list` — stores on disk under `projects/`, enriched with registry
  identity/anchors, self-healing by pruning anchors whose paths are gone.
- The working skill self-gates on `engrym where` and installs user-globally, so
  it triggers correctly in any checkout and no-ops where there's no KB.

`engrym init` runs the dedupe check *first* — before scaffolding, skill install,
or the bootstrap handoff. On a same-identity match it offers to link and then
skips scaffolding + bootstrap (the KB already exists), but still installs the
skills so the agent in this checkout can query it.

## Keeping the skill current

Skill text is `include_str!`'d into the binary, so upgrading the CLI never
touches already-installed copies. Each installed `SKILL.md` carries an
`<!-- engrym-skill-version: X.Y.Z -->` trailer. `engrym where` reports
`skill_outdated` when an installed stamp differs from the running binary (the
skill surfaces this to the user), and `engrym install skills --refresh` rewrites
every already-installed location — project *and* user-global — in one shot.

Implementation: `src/config.rs` (`repo_anchor`, `worktree_main_root`,
`local_key`), `src/registry.rs`, `src/commands/kb.rs`, `src/commands/init.rs`,
`src/commands/agents.rs` (skill stamp / refresh).

## In-repo mode sidesteps all of this

If a repo commits its `docs/` (in-repo mode), git already shares the KB across
worktrees and merges it via PR — branch-scoped, reviewed, no shared-store write
contention. Prefer in-repo for worktree-heavy, parallel-ticket workflows; use
local mode + linking where the repo must stay untouched.
