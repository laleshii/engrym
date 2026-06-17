---
id: agent-integration
title: Agent integration
altitude: 1
topics:
- cli/agent
relations:
- type: refines
  target: engrym-overview
- type: references
  target: init-and-skill
- type: references
  target: design-decisions
summary: 'How engrym plugs into coding agents: a pull-based skill, not a blocking hook.'
---

# Agent integration

engrym becomes useful to a coding agent (Claude Code, Codex, …) through a
**working skill** — a standing playbook with two reflexes: *retrieve* what's
known (to orient a task or to comprehend the codebase) and *capture* durable
knowledge after. The skill lives in `assets/skills/engrym.md` and is installed
by [[init-and-skill]].

## Pull, not push

Retrieval is pull-based and model-judged: the agent queries engrym when it
judges the task relevant, never on every prompt. A hook that injected search
results each turn would overwhelm context with mostly-irrelevant passages and
add latency — so the model is the relevance filter, and the KB is a resource,
not a gate. See [[design-decisions]].

## Encoding is deliberate

Retrieval is self-serving (it helps the current task, so it happens reliably);
encoding is altruistic (it benefits future sessions), so the agent will skip it
unless prompted. The skill compensates with explicit capture triggers (a
decision made, a non-obvious root cause, a new subsystem, a gotcha) and a hard
quality bar — encode only durable, non-obvious knowledge, or the KB rots.

## Per-agent install

The `engrym` CLI is agent-agnostic — anything that runs a shell can use it. Only
the *delivery* of the skill differs, and `engrym init` installs it into the
**chosen** agent's native skill directory and no other: Claude Code's
project-level `.claude/skills/` (committed, travels with the repo) or Codex's
user-global `~/.codex/skills/`. Agents without a skill mechanism still use the
CLI directly. `engrym install skills` re-runs just this step on demand — e.g. to
refresh the skill text after a CLI upgrade. See [[init-and-skill]].
