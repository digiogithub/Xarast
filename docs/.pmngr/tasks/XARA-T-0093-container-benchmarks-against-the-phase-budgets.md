---
id: XARA-T-0093
type: task
title: Container benchmarks against the phase budgets
status: done
parent: XARA-US-0025
author: mcp
labels: [phase-6, xarast-format, perf]
created: 2026-09-23T14:58:50Z
updated: 2026-09-23T14:58:50Z
---

## Description
`crates/xarast-format/benches/container.rs` (criterion): save 20 MB (in memory and through write_atomic), re-save 300 MB with raw copies, open + thumbnail, BLAKE3.

## Acceptance Criteria
- Reference machine, miniz_oxide: save 273 ms / 285 ms atomic (≤ 1 s), re-save 300 MB 104 ms (≤ 1 s), open 46 µs (≤ 15 ms), BLAKE3 5.4–5.9 GiB/s (≥ 1 GB/s). Recorded in docs/memory/perf.md.

## Notes
Commits cdf768c, 541d461. The SVG serialiser is not in these numbers yet.
