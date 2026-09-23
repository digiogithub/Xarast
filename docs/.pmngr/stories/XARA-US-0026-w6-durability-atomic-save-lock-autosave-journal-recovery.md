---
id: XARA-US-0026
type: story
title: "W6 — Durability: atomic save, lock, autosave, journal, recovery"
status: in_progress
priority: high
parent: XARA-EP-0007
author: mcp
labels: [phase-6, xarast-format, durability]
created: 2026-09-23T09:41:28Z
updated: 2026-09-23T15:01:24Z
started: 2026-09-23T14:22:07Z
---

## Description
As a user, I never lose work to a crash, a full disk or a second instance.

## Tasks (full table: phase-06 §W6)
- F6.1 `save_atomic` (temp, fsync, rename, fsync dir).
- F6.2 Optional `.bak` rotation.
- F6.3 `DocumentLock` (`O_EXCL` + `flock`, stale detection).
- F6.4 Lock UX: read-only / open a copy / force.
- Autosave timer, journal and recovery; incremental history checkpoints and persistent history across save/reload (doc-model TODOs).
