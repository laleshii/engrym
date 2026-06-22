---
id: releasing-to-crates-io
title: Releasing engrym to crates.io
altitude: 1
topics:
- distribution
relations:
- type: refines
  target: engrym-overview
summary: 'How engrym is published to crates.io: version immutability, the lean-package exclude list, and the include_str! assets that must never be excluded.'
---

# Releasing engrym to crates.io

engrym is distributed as the [`engrym`](https://crates.io/crates/engrym) crate.
Owner: `laleshii` (Rares S). `cargo install engrym` builds it and places the
binary on PATH — which is why the old `engrym install bin` linking command was
removed (cargo owns that job now).

## Publishing a release

```sh
# 1. Bump the version (crates.io versions are IMMUTABLE — 0.1.0 can never be
#    re-uploaded or edited, only superseded).
#    Edit `version` in Cargo.toml, then sync the lockfile:
cargo build --release        # rewrites Cargo.lock to the new version
cargo test                   # all tests green
cargo publish --dry-run      # packages + compiles in isolation
cargo publish                # irreversible; the version is claimed forever
```

Metadata baked into an already-published version (e.g. the `repository` URL)
cannot be changed retroactively — only fixed in the *next* version. `0.1.0`
shipped with a wrong `repository` (`github.com/engrym/engrym`) and the bloated
file set; `0.1.1` is the first corrected release.

## The lean-package `exclude` list

`Cargo.toml` carries an `exclude` list (`docs/`, `examples/`, `tests/`,
`spec/document-schema.md`) so the published crate ships ~34 files instead of
~61 — none of those are needed to build or run the binary.

## CRITICAL: assets pulled in via `include_str!` must never be excluded

These files are embedded into the binary at *compile time*, so excluding them
from the package (or deleting them) breaks `cargo build` / `cargo publish`:

- `spec/index-schema.sql` — `src/db.rs`
- `assets/skills/engrym-bootstrap.md` — `src/commands/agents.rs`
- `assets/skills/engrym.md` — `src/commands/agents.rs`
- `assets/engrym.toml.template` — `src/commands/init.rs`

This is why `spec/index-schema.sql` stays in the package while only
`spec/document-schema.md` is excluded. Before adding anything to `exclude`,
grep for `include_str!`/`include_bytes!` to confirm it isn't compiled in.

## License

MIT, with a `LICENSE` file at the repo root (`Copyright (c) 2026 Rares S`). The
copyright line is the binding attribution — anyone redistributing engrym must
keep it. See [[design-decisions]] for the rationale on staying MIT.
