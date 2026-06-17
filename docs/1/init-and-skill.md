---
id: init-and-skill
title: init and the agent skill
altitude: 1
topics:
- cli/authoring
relations:
- type: part_of
  target: cli-surface
- type: references
  target: authoring-commands
- type: references
  target: agent-integration
summary: engrym init scaffolds the repo and delegates KB-building to an agent skill.
---

# init and the agent skill

`engrym init` (`src/commands/init.rs`) does the deterministic scaffolding —
writes `engrym.toml`, creates the docs directory (interactive in-repo init asks
where, defaulting to `docs/`; `--docs` sets it non-interactively), ignores
`.engrym/`, and installs the `engrym-bootstrap` agent skill — then asks which
agent to launch. `--local` keeps all of that out of the repo entirely, and
additionally records the repo in the agent's global memory so it's discoverable
despite the absent in-repo cue (see [[local-mode]]).

## Delegation, not generation

engrym is not an LLM, so it does not analyze the repo itself. The installed
skill (`assets/skills/engrym-bootstrap.md`) gives the agent a structured methodology:
survey the repo, design a topic taxonomy, plan documents by altitude, create
them with [[authoring-commands]], wire relations, then `lint --strict` and
`index`. The survey also captures repo-specific coding practices, using recent
commits and recently touched files as stronger evidence when older and newer
patterns conflict. If no agent or terminal is available, `init` prints the next
steps.

## `install`: the same machinery, decoupled

The agent catalogue and skill-writing logic live in `src/commands/agents.rs`,
shared by `init` and `install`. `init` bundles skill installation into a
one-time scaffold; `engrym install skills [--agent <bin>]` does *only* that step,
so you can refresh the skill text after upgrading the CLI, or add it to a repo
you scaffolded before picking an agent. A separate `engrym install bin` symlinks
the running binary onto PATH — a symlink by default, so a local
`cargo build --release` takes effect immediately. It is the deliberate
replacement for `cargo install --path .` during development.
