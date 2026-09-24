---
id: XARA-T-0317
type: task
title: C10 — Bake feathers into SVG and .xarast so external viewers draw them
status: done
priority: medium
parent: XARA-US-0069
author: mcp
labels: [phase-13, live-effects, format, export]
created: 2026-09-24T18:06:47Z
updated: 2026-09-24T21:41:24Z
started: 2026-09-24T19:59:53Z
closed: 2026-09-24T21:41:24Z
---

## Description
Since XARA-US-0068 Xarast draws feathers (75 records in the corpus: Groucho2, feathers, Watch4), but the SVG writer still emits only `xarast:feather` (`xarast-format/src/svg/emit.rs`), so resvg, browsers and Inkscape draw those objects unfeathered and the SVG drifts from our PNG. `research/06 §6.8.6` specifies the bake: a filter that isolates alpha, pulls it in and blurs it, applies the profile with `feComponentTransfer`/`feFuncA`, and composites `in` the source, with `color-interpolation-filters="sRGB"`.

## Acceptance Criteria
- Feathered objects export with a filter matching ours: erosion by size/2 (`feMorphology operator="erode"`), `feGaussianBlur` with σ = radius/2 (the interchange convention, `blur::sigma_for_disc_radius`), the profile table.
- `cargo xtask export-check` on the corpus: feathers.svg and Groucho2.svg within their limits against our PNG (record the numbers in `export.md`).
- `.xarast` round trip stays byte-identical (59/59).

## Notes
Renderer side and the feather's geometry: `docs/memory/render.md`, "Live effects: the offscreen pipeline".
