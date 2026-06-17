---
id: warm-daemon
title: Warm embedding daemon
altitude: 1
topics:
- daemon
relations:
- type: refines
  target: engrym-overview
- type: depends_on
  target: embedding-layer
summary: Auto-spawned, self-reaping daemon that keeps the model resident.
---

# Warm embedding daemon

Loading the model costs ~120ms per process but the search math is microseconds,
so the first semantic query transparently spawns a background daemon
(`src/daemon.rs`) that holds the model resident. Warm semantic queries then drop
from ~130ms to ~13ms.

## Scope

The daemon only embeds query strings — it never touches the index — so `index`
rebuilds never invalidate it. It listens on a per-repo unix socket in
`.engrym/`, and search falls back to in-process embedding if it is unavailable.
Its start/stop and idle behavior is covered in [[daemon-lifecycle]].
