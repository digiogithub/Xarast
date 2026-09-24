---
id: XARA-T-0280
type: task
title: "T10.5.4 Spill directory management: cleanup on exit and crash recovery"
status: done
parent: XARA-US-0053
author: mcp
labels: [phase-10, image, perf]
created: 2026-09-24T09:36:59Z
updated: 2026-09-24T10:39:00Z
started: 2026-09-24T09:36:59Z
closed: 2026-09-24T10:39:00Z
---

## Description
A per-session spill directory, locked while alive, deleted on drop; stale directories of dead sessions are swept on the next start.

## Acceptance Criteria
- Tests cover cleanup and the stale-directory sweep.
