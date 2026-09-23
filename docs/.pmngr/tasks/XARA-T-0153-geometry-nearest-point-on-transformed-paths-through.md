---
id: XARA-T-0153
type: task
title: "Geometry: nearest point on transformed paths through HitIndex, for snapping to paths (W8)"
status: done
author: mcp
labels: [phase-7, geometry]
created: 2026-09-23T17:41:32Z
updated: 2026-09-23T19:28:37Z
started: 2026-09-23T19:28:20Z
closed: 2026-09-23T19:28:37Z
---

## Description

Phase 7 W8 snapping to paths needs the nearest point on any path within a snap radius. `xarast_geom::nearest_point` already exists and is exact per segment, but it is untransformed and works on one path. Add `nearest_point_transformed(path, matrix, p, max_distance)`. It should:

- skip segments by control-hull distance, as `hit_stroke` now does;
- work in f64 document space;
- return the segment and parameter.

The snapper then walks `HitIndex::candidates_at(p, snap_radius)` and keeps the closest result.

## Acceptance Criteria

- [ ] Property test against dense sampling of the transformed curve
- [ ] 100k-object document: snap query under 1 ms on the reference machine

## Notes

Filed from XARA-US-0031 (geometry half), which deliberately did not do this.
