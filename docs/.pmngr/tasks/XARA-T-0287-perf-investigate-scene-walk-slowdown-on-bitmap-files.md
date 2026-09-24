---
id: XARA-T-0287
type: task
title: "perf: investigate scene walk slowdown on bitmap files (Groucho2 35 ms vs 5.6 ms in perf.md)"
status: in_review
priority: high
author: mcp
labels: [perf, render, phase-10]
created: 2026-09-24T10:12:47Z
updated: 2026-09-24T11:06:18Z
started: 2026-09-24T10:39:44Z
---

## Description
While measuring US-0053, scene walks of bitmap-heavy corpus files were several times slower than the table in `docs/memory/perf.md` (Groucho2 ~35 ms vs 5.6 ms), even without `ImageRef::prepare`. The machine was heavily loaded (load 30–98), so this may be noise, or a real regression from recent merges (US-0052 resampler, US-0053 image store, T-0221 two-scene reuse, T-0218 font embedding).

## Acceptance Criteria
- Measure on an idle machine with the existing benches; bisect across the recent merges if the slowdown is real.
- Either fix the regression or update perf.md with the explained new baseline.

## Notes
Also: `build_scene` re-decodes every bitmap on each call (see XARA-T-0281).
