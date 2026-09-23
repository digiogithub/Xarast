---
id: XARA-T-0231
type: task
title: "render: Mix over a transparent destination darkens; cap_end and dash offset ignored by the CPU stroker"
status: backlog
priority: medium
author: mcp
labels: [render, bug, phase-11]
created: 2026-09-23T21:35:10Z
updated: 2026-09-23T21:35:10Z
---

## Description
Found while building PDF export (XARA-US-0059).

1. **Transparent destination.** `blend::composite` blends the source against the destination's *straight* colour; over a transparent pixel that colour is (0,0,0), so a partly transparent Mix (flat or graduated transparency, or antialiased coverage) mixes towards black and the result is then premultiplied again: effectively `colour × a²`. Visible in PNG/WebP exports with `Background::Transparent` as dark fringes and darkened translucent objects. Correct source-over: `out_a = a_s + a_d(1−a_s)`, `out_c = (c_s a_s + c_d a_d (1−a_s)) / out_a`. The non-Mix families need a decision on what they read over nothing (the PDF exporter renders their backdrop over the paper, as the editor shows it). Goldens are mostly on opaque backdrops; check `golden` and the corpus digests before and after.
2. **Stroke caps.** `backend/cpu.rs` strokes with `with_caps(cap_start)`, ignoring `StrokeStyle::cap_end`; `xarast_geom::stroke_to_path` honours both.
3. **Dash offset.** The CPU stroker dashes with offset 0 (`with_dashes(0.0, …)`), ignoring `DashPattern::offset`.

The PDF exporter follows the document model (both caps, the offset), so PDF and raster exports differ on 2 and 3 until this is fixed.

## Acceptance Criteria
- A 50 % white square over a transparent background exports to PNG as (255,255,255,128), not grey.
- Mismatched caps and a dash offset render as `stroke_to_path` defines them.

## Notes
Workaround in `crates/xarast-io/src/pdf/rasterise.rs`: object-only rasters are rendered over opaque black and opaque white and unmixed.
