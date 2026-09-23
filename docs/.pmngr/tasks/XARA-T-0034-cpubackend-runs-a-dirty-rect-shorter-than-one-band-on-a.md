---
id: XARA-T-0034
type: task
title: CpuBackend runs a dirty rect shorter than one band on a single core
status: done
priority: medium
parent: XARA-US-0016
author: mcp
labels: [render, perf, phase-5]
created: 2026-09-23T11:20:26Z
updated: 2026-09-23T11:57:37Z
closed: 2026-09-23T11:57:37Z
---

## Description
`CpuBackend::render` parallelises over horizontal bands (`plan_bands`, ~136 rows at 1080p with a 1 MiB band budget). A display list whose bounds are a short, wide rectangle (the horizontal strip a pan exposes, the top and bottom of a zoom-out border) falls in one or two bands and is rasterised on one core, while a tall narrow strip of the same area uses all of them.

Measured with the app viewport bench (105 852 objects, 224 218 primitives, 1920x1080, zoomed 3x): rasterising the border a Draft zoom-out uncovers (36 % of the frame) cost ~270 ms, more than a whole full-frame Draft (~230 ms). The app now paints the backdrop there and leaves it to the Final (commit "core: leave a Draft zoom-out's uncovered border to the Final"). Row-slab Finals had the same problem: 192-row slabs doubled a 1080p Final, which is why `xarast-app` cuts Finals into full-height columns instead.

## Acceptance Criteria
- A dirty rect shorter than a band is split across cores (tile columns within a band, or bands sized to the dirty rect's height) with output byte-identical to the serial path.
- A 1920x9 strip and a 9x1080 strip of the same scene cost about the same.

## Notes
When this lands, the Draft zoom-out border can be rasterised again (`render_thread.rs`, `Plan::Rescale`).
