---
id: data-model
title: Data model
altitude: 0
topics:
- architecture
- schema
relations:
- type: refines
  target: engrym-overview
summary: 'The three complementary hierarchies: topics, typed relations, altitude.'
---

# Data model

engrym makes "abstract → specific" navigable through three complementary
hierarchies that sit over the document set.

## The three hierarchies

1. Topic taxonomy — slash-delimited topic paths whose tree is implicit (see
   [[topic-taxonomy]]).
2. Document refinement — typed `relations` edges between documents (see
   [[typed-relations]]).
3. Altitude — an explicit 0–3 level per document (see [[altitude]]).

Together they let an agent enter at an overview and drill down by topic, by
graph edge, or by altitude independently.
