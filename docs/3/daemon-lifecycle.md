---
id: daemon-lifecycle
title: Daemon lifecycle
altitude: 3
topics:
- daemon
relations:
- type: part_of
  target: warm-daemon
summary: Auto-spawn, stale-socket handling, idle self-shutdown.
---

# Daemon lifecycle

The daemon is detached with `setsid` and writes its socket into `.engrym/`. A
client connects; on failure it spawns the daemon, polls for the socket, then
queries — falling back to in-process embedding if anything goes wrong, so a
search never fails on the daemon's account.

## Reaping and safety

A watchdog thread exits the process after `idle_secs` (default 300) of
inactivity, unlinking the socket. Stale sockets from a crash are detected
(connect refused → unlink and rebind) and double-spawn races resolve via
`bind()` atomicity. `ENGRYM_NO_DAEMON=1` opts out entirely.
