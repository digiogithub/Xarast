---
id: XARA-T-0171
type: task
title: CPU render of a bitmap transparency is misplaced (scope3/JagSS100 shadows tiled or flipped vs the bitmap-fill convention)
status: done
priority: medium
parent: XARA-US-0051
author: mcp
labels: [render, bitmap, phase-10]
created: 2026-09-23T18:24:03Z
updated: 2026-09-23T19:03:36Z
started: 2026-09-23T19:03:23Z
closed: 2026-09-23T19:03:36Z
---

## Description
Found during the XARA-US-0028 resvg conformance run (2026-09-23). `.xarast` now draws a bitmap transparency as a luminance `<mask>` whose pattern uses exactly the bitmap-fill mapping (origin = image bottom-left, axis_y = top-left; verified for fills in resvg/Inkscape/Chrome). In resvg the drop shadows of `Designs/scope3 simple.xar` (the "Xtreme" and "Xara" logo shadows) and `Designs/JagSS100 simple.xar` (the car's shadow) land where the design intends: directly under the objects. Xarast's own CPU render of the same `.xar` puts them elsewhere: the "Xtreme" shadow appears twice (a second, apparently mirrored copy below), the "Xara" shadow is cut by a hard edge, the Jaguar's shadow sits above the wheel line.

Fill and transparency share `affine(origin, axis_x, axis_y)` in `xarast-app/src/paint.rs` and the same `v * height` texel lookup in `xarast-render` (`LevelSampler::sample` vs `Paint::Image`), so the cause is somewhere between the transparency's mapping and its sampling (sample point space, repeat, or the frame used by `LevelSampler`).

## Acceptance Criteria
- scope3 simple and JagSS100 simple shadows render in the CPU backend where resvg draws them from the `.xarast` (SSIM of the region ≥ 0.98).
- A unit test with an asymmetric bitmap transparency pins the orientation.

## Notes
Repro: `xarast-cli convert F.xar -o F.xarast`, unzip, `cargo xtask svg-render document.svg svg.png W` vs `xarast-cli render F.xar -o ref.png --frame page`.
