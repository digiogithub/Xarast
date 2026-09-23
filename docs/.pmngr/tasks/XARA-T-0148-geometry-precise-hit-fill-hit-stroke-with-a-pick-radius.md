---
id: XARA-T-0148
type: task
title: "Geometry: precise hit_fill / hit_stroke with a pick radius, transforms, caps, joins, dashes and hairlines"
status: done
parent: XARA-US-0031
author: mcp
labels: [phase-7, geometry]
created: 2026-09-23T17:39:17Z
updated: 2026-09-23T17:39:17Z
---

## Description

`xarast_geom::hit`: `hit_fill(path, rule, p, tol)`, `hit_stroke(path, style, p, tol)`, their `_transformed` forms taking the object's `Matrix`, `HitTolerance` (radius and minimum stroke width in document millipoints, `from_device(px, mp_per_px)`), and `HitShape { path, transform, fill, stroke }.hit(p, tol) -> Option<ShapeHit>` (stroke first).

A pick is a disc: a hit when it meets the fill region under any fill rule (open subpaths closed implicitly), or the stroke outline the renderer draws (caps, joins, mitre limit, dashes; gaps do not hit). Strokes thinner than `min_stroke_width` (hairlines always) are picked as a band of that width around the dashed centreline. Everything is computed in document space in f64 after applying the matrix to control points.

## Acceptance Criteria

- [x] Correct under NonZero/EvenOdd/Positive/Negative, including edges that are not boundaries and mirroring transforms
- [x] Caps, joins, mitre limit, dashes (gaps miss), hairlines
- [x] Property tests against a dense disc sampling of `stroke_to_path`'s outline (`crates/xarast-geom/tests/hit.rs`)
- [x] Bounded work on hostile input (band-clipped flattening, edge cap, work limit)

## Notes

Commits: 7151c9e (stroke_to_path mismatched caps were unioned at both ends — fixed), c5e6093, 91f43b1, c6e7293.
Also fixed: `fill_contains` (the old exact `hit_fill`) did not close open subpaths, because kurbo's winding number does not. The per-path edge index is now `PathHitIndex`.
