---
id: XARA-T-0009
type: task
title: Measure open-to-first-paint for a 5 MB .xar on the reference machine
status: backlog
priority: high
parent: XARA-US-0010
author: mcp
labels: [perf, hardware]
created: 2026-09-23T10:00:29Z
updated: 2026-09-23T10:00:29Z
---

## Description
Budget: open a 5 MB `.xar` ≤ 500 ms to first paint (roadmap performance table, Phase 5). Needs the wired viewer, so it was deferred from round 1 of XARA-US-0010.

## Acceptance Criteria
- Wall time from the open request (CLI argument and file dialog) to the first presented frame, for `ProbeX16.xar` (7.4 MB, the only corpus file above 5 MB) and for a ~5 MB file if one can be synthesised; median of ≥ 5 warm runs plus one cold-cache run.
- Breakdown: read, import (`xarast_xar::import`), scene build, display list, first raster, present.
- Recorded in `docs/memory/perf.md`.

## Notes
Round 1 measured the import alone in release: `ProbeX16.xar` ≈ 0.64 s (parse ≈ 0.14 s) under load — the import by itself already exceeds the 500 ms first-paint budget and the Phase 3 ≤ 350 ms full-import row. See the `.xar` import section in `perf.md`.
