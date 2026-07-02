# Changelog

All notable changes to engrym are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/laleshii/engrym/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/laleshii/engrym/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/laleshii/engrym/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/laleshii/engrym/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/laleshii/engrym/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/laleshii/engrym/releases/tag/v0.1.0
