---
id: XARA-T-0318
type: task
title: "C1–C5, C9 — Shadows as a live effect: import 4050/4051, render wall/floor/glow, .xarast round trip"
status: in_review
priority: high
parent: XARA-US-0069
author: mcp
labels: [phase-13, live-effects, render, xar, format]
created: 2026-09-24T19:33:15Z
updated: 2026-09-24T21:04:30Z
started: 2026-09-24T19:33:18Z
---

## Description
The shadow part of XARA-US-0069. The corpus has 98 shadow controllers (Groucho2 81, Girard_simple 9, Watch4 4, SoftShadow 3, testimp1 1), all wall shadows, and the importer strips them with their source objects (atomic subtree).

- C1: `ShadowParams` gains what the records carry (glow width, colour).
- C9: decoders for `TAG_SHADOWCONTROLLER` (4050) and `TAG_SHADOW` (4051); the controller maps to `Live(Controller)` → `Live(Generated)` + `Live(Source)`.
- C2–C4: `LayerEffect::Shadow` in the offscreen pipeline: silhouette → (glow: dilate) → shadow transform (wall offset, floor scale+shear) → disc blur of half the penumbra → profile → colour × darkness, composited under the source.
- C10 (shadow part): `.xarast` round trip exact (59/59 normal form, byte-identical re-save). SVG bake is minimal: reported as a Compromise, coordinated with XARA-T-0317.
- PNG through the renderer; PDF rasterises the effect including its growth.

## Acceptance Criteria
- All 98 corpus shadows import, render and round-trip.
- Preview scores (T-0248 method) before/after for every file with shadows.
- All local gates, perf PR tier and corpus export check green.

## Notes
Inner shadows are not in the `.xar` format and are out of scope. Editing tools (C7) are optional.
