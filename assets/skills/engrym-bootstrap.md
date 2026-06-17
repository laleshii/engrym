---
name: engrym-bootstrap
description: >
  Analyze a code repository and build a high-value initial engrym knowledge
  base — a graph of Markdown docs with typed relations, topic hierarchies, and
  altitude levels. Use when the user wants to populate engrym for a repo: "build
  an engrym KB", "document this repo with engrym", or after `engrym init` hands
  off. (engrym is already initialized by then — this skill authors the content.)
---

# Build an engrym knowledge base for this repository

You are creating the *initial* knowledge base for a repo: a set of Markdown
documents that capture how the project actually works, wired into a graph an
agent can query with `engrym search`, `engrym topic`, and `engrym related`.

engrym is **not** an LLM — it indexes what you write. The value of the KB is
entirely in the judgment you apply here. Your goal is a small, accurate,
well-connected KB that captures **non-obvious knowledge** — architecture,
design decisions, cross-cutting flows, gotchas — not a restatement of the code.

## First: confirm engrym is set up — don't re-initialize

This skill is usually launched *by* `engrym init`, which has **already
initialized** the repo. Run one quick query — e.g. `engrym lint` — to confirm:

- If it succeeds, engrym is ready. **Do not run `engrym init`.** In *local mode*
  the KB lives outside the repo, so the working tree has **no `engrym.toml`** —
  that's expected, not a reason to initialize. Just author with `engrym new`,
  which writes to the right place automatically.
- Only if it errors that this isn't an engrym repo (no config found anywhere)
  should you run `engrym init` once, then proceed.

## The model you are authoring into

Every document has frontmatter (created for you by `engrym new`):

- **`id`** — stable kebab-case identifier (e.g. `auth-architecture`).
- **`title`** — human-readable.
- **`altitude`** — the abstract→specific axis: `0` overview, `1` subsystem,
  `2` component, `3` implementation detail.
- **`topics`** — slash-delimited paths; the hierarchy is implicit
  (`backend/auth/oauth` is under `backend/auth` and `backend`).
- **`relations`** — typed edges to other doc ids:
  - `refines` — this doc elaborates a more abstract one (specific → general)
  - `part_of` — this doc is a component of a larger whole
  - `depends_on` — this doc's subject requires another's
  - `references` — loose "see also"
  - `supersedes` — replaces a deprecated doc
- Inline `[[doc-id]]` wikilinks in the body auto-create `references` edges.

## Method

Work in phases. Do not start writing docs until you've surveyed and planned.

### 1. Survey the repository

Read, don't guess:
- The README and any existing docs.
- Manifests (`Cargo.toml`, `package.json`, `pyproject.toml`, `go.mod`, …) for
  dependencies, entry points, and what the project *is*.
- Top-level directory layout and the main entry points / modules.
- Build/test/CI config for how it's run and verified.
- Lint/format config, contribution notes, and tests for repository-specific
  coding practices.
- If the repo has VCS history, recent commits and recently touched files. Use
  them to bias toward current conventions when older and newer code disagree.

Write down: the project's purpose in one sentence, its major subsystems, the
key data/control flows, and the handful of design decisions that would surprise
a new contributor. Also note the code practices a contributor should follow:
module boundaries, error handling, testing style, naming, data access patterns,
and other conventions that are specific to this repo.

Treat "best practices" as **repo-specific current practice**, not generic
style-guide advice. Prefer patterns reinforced by current docs, config, tests,
and recently changed production code. Do not encode one-off churn, stale legacy
patterns, or speculative preferences.

### 2. Design the topic taxonomy

Choose 4–8 top-level topics that mirror the real subsystems (e.g. `cli`,
`indexing`, `search`, `storage`, `embedding`). Nest where it clarifies
(`search/hybrid`). Keep topic paths stable and lowercase. Don't invent
structure the codebase doesn't have.

### 3. Plan documents by altitude

- **Altitude 0 (1–2 docs):** what the project is + the high-level architecture.
- **Altitude 1 (per subsystem):** an overview of each major subsystem.
- **Altitude 2 (components):** the notable components within a subsystem.
- **Altitude 3 (details):** specific implementation details, algorithms, or
  gotchas genuinely worth capturing.
- **Practice docs:** if the repo has non-obvious conventions that prevent bad
  changes, capture them as altitude-2 or altitude-3 docs under a topic like
  `engineering/practices`, linked to the relevant subsystem docs.

Aim for **coverage, not volume**: every major subsystem should have at least an
altitude-1 overview. A good initial KB is often 8–20 docs. Skip anything that's
just paraphrasing obvious code.

### 4. Create the documents

For each planned doc, create it with `engrym new`, piping the body via stdin so
the prose is real content, not a stub:

```sh
engrym new auth-architecture \
  --title "Authentication architecture" \
  --altitude 0 \
  --topic backend/auth \
  --summary "How the service authenticates users and services end to end." \
  --stdin <<'EOF'
# Authentication architecture

Short, self-contained prose. Each section under a heading becomes a retrieval
passage, so make sections stand on their own. Cite real paths like
`src/auth/mod.rs`. Link related docs inline: see [[oauth-token-refresh]].
EOF
```

Writing guidance:
- **Passage-friendly:** one idea per section; a reader landing on just that
  section should understand it. This is what makes `engrym search` good.
- **Grounded:** reference real files, functions, and decisions. No invented APIs.
- **Concise and non-obvious:** capture the *why* and the *gotchas*. If a section
  just narrates code anyone can read, cut it.

### 5. Wire the graph

Add typed relations as you create docs (`--relation refines:auth-architecture`)
or afterward with `engrym set <id> --add-relation depends_on:token-store`. Use
`refines`/`part_of` to connect each specific doc up to its overview, and
`depends_on` between subsystems that genuinely depend on each other. Use
`[[wikilinks]]` in bodies for see-also links.

### 6. Validate and build

```sh
engrym lint --strict     # fix every error and warning
engrym index             # build the searchable index
```

Resolve dangling relation targets (a typo or a doc you haven't written yet) and
topic typos that lint flags.

### 7. Verify retrieval

Run a few real queries and confirm the right passages come back:

```sh
engrym search "how does X work"
engrym topic <a-top-level-topic>
engrym related <an-overview-doc-id>
```

If a query you'd expect to land somewhere returns nothing useful, the KB has a
gap — add or sharpen a doc.

### 8. Summarize

Tell the user what you built: the topic taxonomy, the document count by
altitude, and any gaps you intentionally left for them to fill. The KB is plain
Markdown under the configured docs root — committed in the repo (reviewable in a
diff), or, for a local KB, under `~/.engrym/projects/…` outside the repo. Either
way you author it through `engrym new`/`set`, never by hand-placing files.
