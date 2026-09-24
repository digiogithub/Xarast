---
id: XARA-T-0278
type: task
title: T10.5.2 Resident screen-resolution proxy per resource (pyramid off the render thread)
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
Keep one pyramid level per image resident (the proxy), chosen from its on-canvas size and capped at 2048 on the long edge; build the pyramid next to the decode instead of on the render thread.

## Acceptance Criteria
- The proxy is never evicted.
- The rule is recorded in docs/memory/perf.md.
