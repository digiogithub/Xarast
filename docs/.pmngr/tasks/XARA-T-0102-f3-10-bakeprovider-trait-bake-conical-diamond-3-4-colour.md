---
id: XARA-T-0102
type: task
title: F3.10 — BakeProvider trait; bake conical/diamond/3-4-colour/procedural fills, feathers and live effects
status: todo
priority: medium
parent: XARA-US-0023
author: mcp
labels: [phase-6, xarast-format, svg]
created: 2026-09-23T15:40:55Z
updated: 2026-09-23T15:40:55Z
---

## Description
Today (eb285b2) SVG-inexpressible paints are written as an approximation plus their `xarast:` twin: conical, 3- and 4-colour and fractal/noise fills as one flat mean colour, diamond as a radial gradient through its frame, feathering and variable-width/brush strokes recorded but not drawn, generated live-effect children written as-is with `xarast:base-authoritative="true"` (nothing regenerates them before Phase 13). research/06 §5.4.1 wants graded baking: geometry (≤ 96 conical wedges), SVG filters (feTurbulence for fractals, blur for feathers/shadows), and 2× rasterisation into `resources/baked/` with `xarast:baked-dpi` and `xarast:generated="fill-bake"`.

Corpus impact: `Fill Types simple.xar` resvg-vs-Xarast SSIM 0.85 is almost entirely the flat conical/3-/4-colour rows.

## Acceptance Criteria
- `BakeProvider` trait in xarast-format (implementation over the renderer lives in xarast-app, like `ThumbnailProvider`).
- Baked subtrees marked `xarast:generated*`; the reader discards and regenerates them.
