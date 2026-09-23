---
id: XARA-T-0149
type: task
title: "Geometry: HitIndex, a spatial index over 100k+ objects' bounds for picking and marquee"
status: done
parent: XARA-US-0031
author: mcp
labels: [phase-7, geometry]
created: 2026-09-23T17:39:33Z
updated: 2026-09-23T17:39:33Z
---

## Description

`xarast_geom::HitIndex<K>` (hashed uniform grid, large-object list). `insert`/`set_bounds`/`set_z`/`remove`/`retain`/`rebuild`/`from_entries`; `candidates_at(p, radius)` yields candidates nearest the viewer first (by caller-supplied `u64` z) so the precise test stops at the first hit; `topmost(p, radius, accept)`; `query_rect(rect, RectMode::{Touch, Enclose}, out)`.

## Acceptance Criteria

- [x] Point and rect queries, touch vs enclose
- [x] Cheap incremental edits: insert/remove 23–30 ns, moves 70–680 ns at 100k objects (reference machine)
- [x] Property tests against a brute-force list through arbitrary edit sequences, including a crowded 1 000-key variant
- [x] Fuzzed (`fuzz_hit_test`)

## Notes

Commits: c36a24d, a212db0, c11713d. Integration contract in `docs/memory/geometry.md`.
