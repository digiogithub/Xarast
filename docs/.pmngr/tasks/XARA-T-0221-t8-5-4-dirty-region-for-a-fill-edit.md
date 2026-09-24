---
id: XARA-T-0221
type: task
title: T8.5.4 — dirty region for a fill edit
status: done
parent: XARA-US-0040
author: mcp
labels: [phase-8, render]
created: 2026-09-23T21:18:03Z
updated: 2026-09-24T02:28:34Z
started: 2026-09-24T01:21:39Z
closed: 2026-09-24T02:28:34Z
---

## Description
A fill edit should mark only (old fill extent ∪ new fill extent) ∩ the object's bounds as stale. Today `Session::after_mutation` invalidates the whole viewport after every mutation, and the render thread reuses tiles by content hash. This needs the session's dirty tracking to accept regions per command.

## Acceptance Criteria
- After a fill edit the dirty region is the object's device bounds, not the whole viewport. A test checks this.
