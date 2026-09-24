---
id: XARA-T-0257
type: task
title: "Model: keep the third point of a three-point linear fill"
status: backlog
priority: low
parent: XARA-US-0043
author: mcp
labels: [phase-8, xar, doc-model]
created: 2026-09-24T00:01:45Z
updated: 2026-09-24T00:01:45Z
---

## Description
`TAG_LINEARFILL3POINT` (4121, 17 records) and `TAG_LINEARTRANSPARENTFILL3POINT` (4123, 10) carry a third control point (the second axis of the gradient frame). `FillGeometry::Linear` has no such field, so the importer drops it with an `Info` diagnostic (`docs/memory/xar-import.md` finding 10). Measured in XARA-US-0043: all 27 corpus records have the point within 1.2° of perpendicular (26 within 0.4°), so the render does not change; what is lost is the handle and a possible skew.

## Acceptance Criteria
- `FillGeometry::Linear` (or a parallelogram `persp`) carries the third point without making the fill "perspective" in the tools.
- The importer maps 4121/4123 losslessly; `.xarast` writes and reads it (a skewed linear gradient is `gradientTransform` in SVG).
- The fill census in `crates/xarast-cli/tests/fills.rs` still matches.
