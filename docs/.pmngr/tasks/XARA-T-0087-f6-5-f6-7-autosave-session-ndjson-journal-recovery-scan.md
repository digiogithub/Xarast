---
id: XARA-T-0087
type: task
title: F6.5–F6.7 — Autosave session, NDJSON journal, recovery scan
status: done
parent: XARA-US-0026
author: mcp
labels: [phase-6, xarast-format, durability]
created: 2026-09-23T14:23:38Z
updated: 2026-09-23T19:53:37Z
closed: 2026-09-23T19:53:37Z
---

## Description
`AutosaveSession` writing full `.xarast` snapshots to `$XDG_STATE_HOME/xarast/autosave/<doc-id>/snapshot.xarast` via `write_atomic`, off the UI thread from a model snapshot, every 5 min or 50 undo ops; `state.json`; append-only `journal.ndjson` with fsync ≤ 250 ms batching, truncated on autosave, deleted on real save; startup recovery scan (dead pid / other host), seq-continuity-checked replay.

## Notes
Left over from round 1: needs the SVG layer (a snapshot is a whole package) and a serialisable operation form from the doc model.
