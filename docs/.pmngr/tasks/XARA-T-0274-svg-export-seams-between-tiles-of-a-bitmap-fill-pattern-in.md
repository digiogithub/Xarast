---
id: XARA-T-0274
type: task
title: "SVG export: seams between tiles of a bitmap-fill pattern in resvg (leafgirl 11.4/255)"
status: backlog
priority: low
parent: XARA-US-0052
author: mcp
labels: [phase-10, bitmap, xarast-format, export]
created: 2026-09-24T07:56:41Z
updated: 2026-09-24T07:56:41Z
---

## Description
Follow-up split from XARA-US-0052 (renderer side done there). resvg draws visible seams between the tiles of a bitmap fill's `<pattern>` in exported SVG / `.xarast` (leafgirl, 11.4/255, noted in XARA-T-0236). Our CPU renderer has no seams: tiling is a coordinate wrap in the sampler, and filtering reads across the tile edge through the same wrap (`crates/xarast-render/src/resample.rs`).

## Acceptance Criteria
- Find whether the seam comes from the pattern tile's size/rounding, `patternTransform`, or resvg's per-tile resampling, and fix it in `xarast-format`'s writer (e.g. pad the tile image by a mirrored/wrapped border and clip, or snap the tile to whole device pixels) without breaking Inkscape/browser rendering.
- `cargo xtask export-check` on leafgirl drops below the current 11.4/255.
