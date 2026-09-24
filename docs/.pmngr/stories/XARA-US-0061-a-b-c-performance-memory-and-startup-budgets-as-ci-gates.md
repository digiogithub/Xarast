---
id: XARA-US-0061
type: story
title: A/B/C — Performance, memory and startup budgets as CI gates
status: done
parent: XARA-EP-0013
author: mcp
labels: [phase-12, perf, ci]
created: 2026-09-23T09:43:25Z
updated: 2026-09-24T13:53:16Z
started: 2026-09-24T12:43:30Z
closed: 2026-09-24T13:53:16Z
---

## Tasks (full tables: phase-12 §A, §B, §C)
- A1–A4 `bench` subcommand, scenario set (1k/10k/100k), criterion baselines, `cargo xtask bench --check-budgets`.
- B1–B4 `dhat` profiling, peak RSS, render/bitmap cache ceiling, undo budget in bytes.
- C1–C4 `XARAST_TRACE_STARTUP`, ≤ 400 ms cold-start gate, defer font enumeration and gallery population.
- f64 cross-architecture determinism promoted to a gate (geometry TODO).
