---
id: altitude
title: Altitude
altitude: 2
topics:
- schema/altitude
relations:
- type: part_of
  target: frontmatter-schema
summary: The explicit 0–3 abstract-to-specific level.
---

# Altitude

Altitude is an explicit integer 0–3 capturing how abstract a document is,
independent of its topic: `0` overview, `1` subsystem, `2` component, `3`
implementation detail. It is enforced to the 0–3 range (`MAX_ALTITUDE` in
`src/model.rs`) by both `index` and `lint`.

## Why a separate axis

Topics say *what* a doc is about; altitude says *how deep* it goes. `engrym
search --altitude 0` lets an agent ask for the overview of a topic, then drill
down — a different axis from the `refines` graph edge.
