---
id: XARA-T-0296
type: task
title: "perf: leak gate — explain the ~5 MiB warm-up and the ±2 MiB drift, then tighten to the phase's 5 %"
status: backlog
priority: medium
parent: XARA-US-0061
author: mcp
labels: [perf, phase-12]
created: 2026-09-24T13:11:09Z
updated: 2026-09-24T13:11:09Z
---

## Description
`xarast-cli bench leak --nodes 10000 --iterations 200` (gates `leak`, `leak-first` in `xtask/perf-budgets.txt`): resident set after close grows ~5 MiB over cycles 1–3, then wanders ±2 MiB. Against the first close that is 15–21 % (phase 12 B5 asks ≤ 5 %); after warm-up 2–9 %, so the gate limits are 35 % / 20 % today. See docs/memory/perf.md "CI gates".

## Acceptance Criteria
- The warm-up attributed (thread arenas? pools? per-process caches) with heaptrack or dhat.
- Drift explained (glibc fragmentation vs. a real per-document retention), with `M_ARENA_MAX`/`malloc_trim` tried as a measurement aid.
- Gate limits brought down towards the 5 % budget.
