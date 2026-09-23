---
id: XARA-T-0109
type: task
title: Writer drops the fill mapping (tiling) of bitmap, fractal and noise fills
status: in_progress
priority: medium
parent: XARA-US-0028
author: mcp
labels: [phase-6, xarast-format, svg]
created: 2026-09-23T16:46:26Z
updated: 2026-09-23T17:10:21Z
started: 2026-09-23T17:10:21Z
---

## Description
`svg/paint.rs::colour_paint` writes the `FillMapping` tiling (`xarast:fill-repeat` / `xarast:repeat`) for linear, radial, diamond, conical, three- and four-colour fills, but not for bitmap fills (only the bitmap's own `tile-mode`) nor for fractal and noise twins. The renderer honours the mapping for those families ("a bitmap or procedural fill takes the value at face value", `fill.rs::Tiling`), so a reloaded document renders differently: Watch4 (noise, 0.5 % of pixels, up to 15 levels), scope3 simple (fractal), Spitfire (bitmap fills, 14 pixels).

## Acceptance Criteria
- The writer records a non-`None` `FillMapping` on bitmap patterns and on fractal/noise twins (e.g. `xarast:fill-repeat` on the `<pattern>`, `xarast:repeat` on the `<xarast:fill>` twin).
- The reader already reads `xarast:repeat` on every twin; add `xarast:fill-repeat` on `<pattern>` (one line in `svg/read/build/paint.rs::pattern`).
- `crates/xarast-app/tests/xarast_roundtrip.rs`: Watch4 and Spitfire leave `KNOWN_RENDER_GAPS`.
