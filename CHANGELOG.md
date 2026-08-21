# Changelog

All notable changes to engrym are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.1] - 2026-08-21

### Added

- **Dark mode in `engrym browse`.** The web UI now has a light and a dark
  palette, and follows the operating system's appearance setting out of the box.
  Code blocks, tables, the relation panels and the altitude badges all get their
  own dark tints rather than reusing the light pastels, which glared against a
  dark page.
- **A theme toggle next to the title**, cycling auto → light → dark. The choice
  is remembered per browser in `localStorage` and applied in `<head>` before the
  page paints, so a stored preference never flashes the other theme. "Auto" is
  stored as the *absence* of a preference, so it keeps tracking the OS rather
  than freezing whatever it was when you picked it.

## [0.3.0] - 2026-08-20

### Added

- **Workspace search across sibling repos.** Standing in a folder that holds
  several clones (`~/Projects/{api,web,jobs}`), `engrym search` now finds every
  child repo's KB and searches them all, grouping hits under the repo that
  produced them. Discovery is automatic only where there's nothing to conflict
  with — a KB reachable by walking *up* from the cwd still wins outright — and
  the new global `--all` flag asks for the same fan-out from inside a repo, so
  its siblings join in.
- `index`, `topic`, `show` and `related` span the workspace too. A document in
  another repo is addressed with a repo qualifier (`engrym show api:auth-overview`);
  a bare id still works while it's unambiguous. `--json` hits carry `repo` and a
  ready-made `ref`.
- **`--depth <N>`** (global, 1–5) widens the scan for nested layouts like
  `~/Projects/<org>/<repo>`, and implies `--all`. Members are then named by their
  path relative to the workspace root (`org-a/api`), so two orgs' `api` stay
  distinct. The scan stops *descending* at every git checkout — a nested checkout
  with a KB is still a member, we just never look inside one — so it keeps clear
  of `node_modules` and `target`, and nested repos are reachable from the
  directory containing them rather than from further above. Depth widens downward
  only: the root is the cwd, or inside a repo the one folder holding it, never
  inferred further up (`--repo <dir>` sets another).
- **Sibling fallback from a repo that doesn't use engrym.** Standing in a plain
  checkout (or anywhere inside it) with no KB of its own, engrym now looks at the
  folder holding it and searches the sibling repos that *do* have KBs, instead of
  reporting nothing. A different clone of the *same* repo is deliberately
  excluded — that stays `engrym link`'s job, so `engrym where` still surfaces it
  as a `link_candidate`.
- `engrym where` reports `{"mode": "workspace"}` with the repos in scope (and
  exits zero, so an agent's gate passes in a folder of clones); `engrym list`
  shows what a workspace search would reach. `where` honours `--all` / `--depth`,
  so the gate always answers for the scope the other commands resolve.

### Notes

- Single-repo behavior and `--json` output are unchanged. Commands that write to
  a KB — authoring, `reset`, `lint`, `browse`, `serve` — never fan out; they
  error with the member list unless exactly one KB is in scope, and `--repo <dir>`
  picks one.

## [0.2.2] - 2026-08-10

### Added

- **opencode skill support.** `engrym init` / `install skills --agent opencode`
  now install the engrym skills into opencode's native locations: project-level
  `.opencode/skills/` (committed), or user-global `~/.config/opencode/skills/`
  in local mode.

## [0.2.1] - 2026-07-02

### Fixed

- The local-KB registry now **self-heals**: on the first use after upgrading (or
  any time the registry file is missing), engrym backfills it from the stores
  already on disk — recovering each store's repo binding and `origin` identity —
  so existing local KBs are recognized and dedupe/linking work with no manual
  migration steps. The backfill runs only when the registry is absent, so
  there's no per-command scan.

## [0.2.0] - 2026-07-02

### Added

- **Worktree-aware anchoring.** A local KB is now keyed to the repo, not the
  checkout: git worktrees resolve to the main worktree's root, so every worktree
  shares one KB.
- **Cross-clone linking.** Separate clones of the same repo (matched by
  normalized `origin` URL) can share one local KB. `engrym init` offers to link
  a same-repo clone; `engrym link <key|path>` / `engrym unlink` do it explicitly.
  Mappings live in `~/.engrym/registry.json`.
- **`engrym where`** — a fast gate reporting whether a KB is reachable here
  (resolving worktrees and links), with an exit code and `--json` output.
- **`engrym list`** — enumerate local KB stores and how they're shared across
  checkouts (self-healing: prunes dead worktree paths).
- **`engrym install skills --refresh`** — update every already-installed skill
  location (project and user-global) to the running binary's version. Installed
  skills now carry a version stamp; `engrym where` reports `skill_outdated`.

### Changed

- **`engrym init` dedupes first.** Before scaffolding, skill install, or the
  bootstrap handoff, it checks for an existing KB for the same repo and offers to
  link it (reusing its knowledge, skipping bootstrap) — while still installing
  the skills.
- **The working skill self-gates on `engrym where`** and is meant to be installed
  once, globally; it no-ops where there's no KB, so there's no per-repo list to
  maintain.

## [0.1.2] - 2026-06-22

### Changed

- Reworked the README to lead with what engrym is, why you'd want it, and a
  one-command quick start; documented `engrym init --local`.

## [0.1.1] - 2026-06-22

### Added

- Distribution on crates.io — `cargo install engrym`.
- An MIT `LICENSE` file and author metadata.

### Changed

- Leaner published package (excludes `docs/`, `examples/`, `tests/`).

### Removed

- The `install bin` / `uninstall bin` linking command — `cargo install` handles
  PATH now.

### Fixed

- Corrected the `repository` URL in the crate metadata.

## [0.1.0] - 2026-06-22

### Added

- Initial release: a fast, AI-first knowledge base over Markdown with a
  disposable SQLite index, hybrid search (BM25 + local vector embeddings fused
  via RRF), topic/relation/altitude navigation, authoring commands, and
  `engrym browse` (a local web UI).

[Unreleased]: https://github.com/laleshii/engrym/compare/v0.3.1...HEAD
[0.3.1]: https://github.com/laleshii/engrym/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/laleshii/engrym/compare/v0.2.2...v0.3.0
[0.2.2]: https://github.com/laleshii/engrym/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/laleshii/engrym/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/laleshii/engrym/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/laleshii/engrym/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/laleshii/engrym/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/laleshii/engrym/releases/tag/v0.1.0
