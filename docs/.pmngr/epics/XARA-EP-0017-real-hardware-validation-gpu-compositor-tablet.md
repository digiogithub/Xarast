---
id: XARA-EP-0017
type: epic
title: Real-hardware validation (GPU, compositor, tablet)
status: backlog
priority: critical
milestone: XARA-M-0003
author: mcp
labels: [cross-cutting, perf, hardware]
created: 2026-09-23T09:39:43Z
updated: 2026-09-23T09:39:43Z
---

## Description
Everything that defines the product but cannot be proven in the dev container (4 slow cores, no GPU, no compositor, no session bus): interactive performance, GPU backend, compositor behaviour, tablet input.

## Acceptance Criteria
- A reference machine is chosen and the budget table in `docs/memory/perf.md` is pinned to it.
- Pan/zoom ≤ 16 ms at 100k objects on an integrated GPU, measured.

## Notes
Cross-cutting; blocks closing M3 honestly.
