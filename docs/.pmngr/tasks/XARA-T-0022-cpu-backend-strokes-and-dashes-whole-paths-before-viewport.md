---
id: XARA-T-0022
type: task
title: "CPU backend strokes and dashes whole paths before viewport clipping: memory exhaustion at deep zoom"
status: done
parent: XARA-US-0016
author: mcp
labels: [fuzz, render, perf]
created: 2026-09-23T10:24:19Z
updated: 2026-09-23T11:57:09Z
started: 2026-09-23T11:08:11Z
closed: 2026-09-23T11:57:09Z
---

## Description
Found by `fuzz_display_list` (two OOMs, 1.8 GB single allocations). `rasterise_coverage` in `crates/xarast-render/src/backend/cpu.rs` expands a stroke with `kurbo::stroke` (dashes included) over the **whole** path in document space at the view's flattening tolerance, and only then clips to the band. Reproducers (synthetic, via the target's input format):
- an extent-sized dashed rectangle (~1.7e9 mp sides, 4935 mp dashes, 0.9 pt width) at 0.35 px/mp: millions of dashes;
- a ~1e7 mp rectangle, 693 pt round-capped stroke, ~1 pt dashes, at 0.35 px/mp (25 600 % zoom): ~28 k dashes x two round caps flattened at 0.29 mp.

## Acceptance Criteria
- Cull/clip stroke geometry (with dash phase preserved) to the band plus the stroke's reach before expansion, or impose a documented work budget with a graceful fallback.
- Remove the `GEOMETRY_LIMIT` and `MAX_DASHES` bounds in `fuzz/fuzz_targets/fuzz_display_list.rs` and let it run clean.

## Notes
Not fixed in XARA-US-0014: it is a render design change, not a targeted bug fix. The fuzz target is bounded meanwhile so the nightly job is not permanently red.
