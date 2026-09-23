---
id: XARA-T-0028
type: task
title: Write fuzz_display_list (xarast-render)
status: done
parent: XARA-US-0014
author: mcp
labels: [fuzz, render]
created: 2026-09-23T10:33:14Z
updated: 2026-09-23T10:33:14Z
---

## Description
Arbitrary `SceneBuilder` sequences (fills, strokes with caps/joins/mitre/dashes, images, groups with 16.16 transforms, clips, transparencies, layers; unmatched pops and unclosed pushes included), every paint/transparency source/blend family, view scale up to 25 600 %. Asserts: accepted scenes give a balanced display list inside the viewport, and the deterministic CPU backend renders it twice to identical bytes. Closes render.md TODO 9 (with fuzz_ramp).

## Notes
Found the stroke-before-clip memory hazard (XARA-T-0022); bounded (geometry ±1e7 mp, ≤ 2 000 dashes per path) until that is fixed.
