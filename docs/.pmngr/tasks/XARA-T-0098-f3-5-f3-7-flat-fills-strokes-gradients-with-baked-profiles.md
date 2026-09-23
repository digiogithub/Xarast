---
id: XARA-T-0098
type: task
title: F3.5–F3.7 — Flat fills, strokes, gradients with baked profiles, transparency and blend modes
status: done
priority: high
parent: XARA-US-0023
author: mcp
labels: [phase-6, xarast-format, svg]
created: 2026-09-23T15:40:30Z
updated: 2026-09-23T15:40:30Z
---

## Description
`svg/paint.rs`. Resolved paint per ink element (attribute stack as the renderer's walker); SVG defaults elided. Flat fills with palette refs (`xarast:fill-ref`), strokes (width, hairline as `vector-effect`, caps, joins, mitre limit, dashes, stroke opacity), fill-rule. Linear/circular/elliptical gradients exact (`gradientUnits="userSpaceOnUse"`, spread only for the "extra" repeat); profile/sin-easing/rainbow ramps baked adaptively (≤ 2/255, 9–33 stops per eighth) with `xarast:profile`/`xarast:stops`/`xarast:fill-effect`. Perspective drawn affine plus `xarast:persp`. Graduated transparency → `<mask>` with a greyscale gradient, `color-interpolation="sRGB"`; flat → fill/stroke opacity; modes → `style="mix-blend-mode:…"` + `xarast:blend`.

## Notes
Done in eb285b2. Leftovers filed separately: arrowheads (markers), conical/3/4-colour/procedural baking.
