---
id: XARA-T-0277
type: task
title: T10.5.1 Global pixel budget, accounting and instrumentation
status: done
parent: XARA-US-0053
author: mcp
labels: [phase-10, image, perf]
created: 2026-09-24T09:36:58Z
updated: 2026-09-24T10:39:00Z
started: 2026-09-24T09:36:58Z
closed: 2026-09-24T10:39:00Z
---

## Description
A process-wide pixel budget over the renderer's decoded bitmap levels: accounting of resident evictable and proxy bytes, LRU eviction under a limit, and counters (evictions, re-materialisations, spill traffic).

## Acceptance Criteria
- Budget limit configurable (env + API), default documented in perf.md.
- Stats exposed for tests and instrumentation.
- Rendering under a tiny budget is byte-identical to an unlimited budget.
