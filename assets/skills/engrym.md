---
name: engrym
description: >
  Work with the repository's engrym knowledge base. Use when fixing a bug,
  building a feature, exploring product direction, or simply understanding how
  the codebase works, in a repo set up with engrym (its KB may live in the repo
  or in a local store outside it): query engrym for relevant knowledge — to
  orient a task, or to answer a comprehension question — and capture durable,
  non-obvious findings back into it.
---

# Working with the engrym knowledge base

This repo is set up with an engrym knowledge base: Markdown docs with typed
relations, topic paths, and altitude levels, queryable via the `engrym` CLI.
Treat it as a resource you *pull from* — it must never block or slow the task.
Every query is milliseconds; add `--json` when you want to parse output.

**Reach the KB through the CLI, never by guessing file paths.** The docs root is
configurable, and in *local mode* the KB lives outside the repo (under
`~/.engrym/projects/…`) so the repo is never touched — but `engrym search` /
`show` / `topic` / `related` resolve it the same way wherever it lives. If a
command reports this isn't an engrym repo, there's no KB here — proceed without
one. Only when you must read raw files, take the docs root from `engrym.toml`
rather than assuming `docs/`.

Two reflexes: **retrieve** what's already known — to *orient* a task or to
*comprehend* the codebase — and **capture** durable knowledge after.

## Orient — before you act

When starting a bug fix, a feature, or a design discussion, run one cheap query
to see what's already known. Read only what's relevant; if nothing useful comes
back, just proceed — you've spent ~10ms.

- **Bug:** `engrym search "<symptom / error / area>"`, then `engrym related <id>`
  / `engrym show <id>` on the hits. Look for documented gotchas and the *why*
  behind the code.
- **Feature:** `engrym topic <area>` and `engrym related <overview-id>` to
  surface constraints and prior decisions before you plan.
- **Direction / brainstorm:** read the altitude-0 docs and `design-decisions`
  (`engrym search "<theme>" --altitude 0`, `engrym show design-decisions`) for
  what's been decided and why, and what's been tried.

Briefly tell the user what the KB surfaced and how it shaped your approach.

## Comprehend — when understanding *is* the task

When there's no task to do — onboarding, "how does X work?", "give me the map of
this codebase" — the KB is your entry point and the retrieval is the deliverable
(no encoding needed). Read **top-down by altitude**, the axis built for exactly
this:

- Start at the map: read the altitude-0 overviews
  (`engrym search "<area>" --altitude 0`, `engrym show <overview-id>`) or
  `engrym topic <area>` for everything under a subject.
- Drill down: follow `engrym related <id>` along `refines`/`part_of`/`depends_on`
  and `engrym show` the deeper docs, narrowing with `--altitude` as you go.
- Synthesize a grounded answer that cites doc ids, and verify against the code
  where it matters — the KB is the index into the code, not a replacement for it.
- If the KB doesn't cover what was asked, say so plainly and read the code
  instead — and treat that miss as a candidate gap to fill (see Capture).

## Capture — after knowledge crystallizes

Encoding benefits *future* work, not the task in front of you, so it won't
happen on its own — you have to do it deliberately. Watch for these moments and
capture *then*, while the insight is crisp:

- a **decision** is firmly made (record why, and what was rejected)
- a **non-obvious root cause** is found (bugs are the richest source)
- a **new subsystem or feature** lands
- a **constraint or gotcha** is discovered
- an existing doc is now **wrong** → record the change and `supersedes` it

### The bar — this matters most

Encode only **durable, non-obvious** knowledge: the *why*, the gotcha, the
decision, the cross-cutting flow. Ask: "would a competent newcomer be surprised,
or have to re-derive this painfully?" If not, **don't encode it.** Never restate
code, transient details, or speculation. Over-encoding poisons retrieval — a
noisy KB is worse than a small one.

### How

Prefer **growing the graph** over rewriting prose:

- New durable knowledge → create a focused doc (often altitude 2–3) wired to its
  parent, so the graph stays connected without disturbing existing docs:
  ```sh
  engrym new <id> --title "…" --altitude 3 --topic <area> \
    --relation refines:<parent-id> --stdin <<'EOF'
  # …
  Self-contained prose — one idea per heading. Cite real paths. Link [[other-id]].
  EOF
  ```
- New connection between existing docs → `engrym set <id> --add-relation depends_on:<other-id>`
- A decision that replaces an old one → `engrym new <new-id> … --relation supersedes:<old-id>`

Then **always** rebuild and validate:

```sh
engrym index
engrym lint --strict     # catches dangling relation targets and topic typos
```

## The loop, per task

- **Bug:** orient on the subsystem → fix → if the cause was non-obvious, capture
  it as a detail doc under the subsystem's topic.
- **Feature:** orient on constraints/decisions → build → encode the design as a
  doc wired with `refines`/`part_of`/`depends_on`, and update anything it changed.
- **Direction:** orient on prior decisions → discuss → encode only the decisions
  that actually crystallize, never raw ideas.
- **Understanding:** enter at altitude 0 → drill via `related` / `--altitude` →
  answer from the KB, grounded in the code; no capture unless you hit a gap.

## Guardrails

- **Pull, never block.** One cheap query to orient; skip it when the task is
  clearly unrelated to documented knowledge.
- **Non-obvious only.** Protect the signal-to-noise of the KB.
- **Human-reviewable.** Docs are plain Markdown under the configured docs root
  (in local mode, outside the repo under `~/.engrym/projects/…`) — author them
  via the CLI and keep them concise, factual, and grounded in real file paths.
- When the user explicitly asks to capture or record something, do it regardless
  of the heuristics above.
