# engrym — document schema

Every document in the knowledge base is a Markdown file with a YAML frontmatter
block. The frontmatter is the **machine-readable contract**; the body is prose
for humans and for semantic search. Files live under the configured docs root
(`docs/` by default) and are the single source of truth — the index in
`.engrym/` is always rebuildable from them.

## Frontmatter

```yaml
---
id: oauth-token-refresh          # required · stable, kebab-case, unique
title: OAuth token refresh flow  # required · human-readable
altitude: 3                      # required · 0=overview … 3=implementation detail
topics:                          # required · ≥1 path; hierarchy is implicit
  - backend/auth/oauth
  - security/tokens
relations:                       # optional · typed edges to other doc ids
  - { type: refines,    target: auth-architecture }
  - { type: depends_on, target: token-store }
  - { type: references, target: rfc-6749-notes }
summary: >                       # optional · one-line gist for fast listing
  How expired access tokens are exchanged for new ones.
---
```

### Fields

| Field       | Required | Type            | Notes |
|-------------|----------|-----------------|-------|
| `id`        | yes      | string          | Stable identity. Renaming the file is fine; changing `id` breaks inbound relations. |
| `title`     | yes      | string          | |
| `altitude`  | yes      | int 0–3         | The abstract→specific axis. `0` overview, `1` subsystem, `2` component, `3` impl detail. |
| `topics`    | yes      | list<path>      | Slash-delimited. A doc tagged `a/b/c` is implicitly under `a` and `a/b`. |
| `relations` | no       | list<edge>      | See edge types below. `target` is another doc's `id`. |
| `summary`   | no       | string          | Shown in list/search output; not a substitute for the body. |

### Edge types (`relations[].type`)

| Type         | Meaning                                            | Direction |
|--------------|----------------------------------------------------|-----------|
| `refines`    | This doc is a more specific elaboration of target  | child → parent (abstract) |
| `part_of`    | This doc is a component of a larger whole          | part → whole |
| `depends_on` | This doc's subject requires target's subject       | directed |
| `references` | Loose citation / "see also"                        | directed |
| `supersedes` | This doc replaces target (deprecation)             | new → old |

Inline `[[wiki-links]]` in the body auto-generate `references` edges at index
time, so authors get graph connectivity for free.

## The three hierarchies

1. **Topic taxonomy** — implicit in slash-delimited topic paths. Query a prefix
   (`backend/auth`) to get the whole subtree.
2. **Document refinement** — `refines` / `part_of` edges connect high-level docs
   to the specific docs that elaborate them.
3. **Altitude** — explicit 0–3 level so an agent can enter at the overview and
   drill down regardless of topic.

## Validation (`engrym lint`)

Default is **warn**; CI runs `engrym lint --strict` which turns warnings into a
non-zero exit. Checks:

| Rule                                   | Local | `--strict` |
|----------------------------------------|-------|------------|
| Missing required field                 | error | error      |
| `altitude` out of 0–3                  | error | error      |
| Duplicate `id`                         | error | error      |
| `relations[].target` is a dangling id  | warn  | error      |
| Topic not seen elsewhere (likely typo) | warn  | error      |
| Body empty / frontmatter-only          | warn  | error      |
