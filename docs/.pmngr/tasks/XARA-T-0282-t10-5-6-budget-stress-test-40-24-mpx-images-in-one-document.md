---
id: XARA-T-0282
type: task
title: "T10.5.6 Budget stress test: 40 × 24 Mpx images in one document"
status: backlog
parent: XARA-US-0053
author: mcp
labels: [phase-10, image, perf]
created: 2026-09-24T09:54:30Z
updated: 2026-09-24T09:54:30Z
---

## Description
Gap left by XARA-US-0053: the phase's stress test (`xarast-cli`). Generate 40 procedural 6000 × 4000 photos (no committed fixtures), place them in one document, pan/zoom through them under the default budget, and record peak RSS, spill traffic and frame times in docs/memory/perf.md. Watch disk space; the spill root is `$XDG_CACHE_HOME/xarast/spill`.

## Acceptance Criteria
- Peak RSS stays within budget + proxies + one frame of pins.
- No frame loses a bitmap (`BudgetStats::lost == 0`).
