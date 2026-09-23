---
id: XARA-T-0054
type: task
title: Bitmap fills ignore the fill-mapping attribute (they should tile by default)
status: backlog
priority: low
parent: XARA-US-0052
author: mcp
labels: [app-core, xar-import, bitmap]
created: 2026-09-23T12:28:41Z
updated: 2026-09-23T12:32:44Z
---

## Description
The importer builds `FillGeometry::Bitmap { tiling: Tiling::default(), .. }` (`crates/xarast-xar/src/import.rs`, colour and transparency arms), and `xarast-app/src/paint.rs` renders a bitmap fill with that per-fill tiling. The fill-mapping attribute in force is never consulted.

In the original, a bitmap fill's tiling **is** the mapping attribute, passed straight through to the renderer (`wxOil/grndrgn.cpp:3538-3539` for colour, `:4302-4304` for transparency). The factory default mapping is 2, "repeat" (`Kernel/fillval.cpp:7942-7945`). So a bitmap fill with no mapping record tiles in the original, and ours draws a single tile. See `docs/research/01-xar-format.md` §8.3, "How the mapping renders".

Invisible today, because bitmap fills are not decoded yet (Phase 10). `WalkStats::images_pending` now counts them: `Designs/Fill Types simple.xar` (5, including "bitmap repeating" and "Repeating inverted"), `Designs/leafgirl.xar` (2) and `testfiles/TestBitmapFill.xar` (1).

## Acceptance Criteria
- A bitmap fill takes its repeat from the fill-mapping attribute. `Tiling::None` (the model default) maps to the original's default, repeat. Either drop the per-fill `tiling` or have the walker prefer the attribute.
- `Fill Types simple.xar`'s bitmap row shows single, repeating and mirrored tiles once bitmaps decode.

## Notes
Belongs with Phase 10 bitmap decoding. `paint.rs::repeat_of` is the bitmap mapping (`RepeatExtra` → `Repeat`); graduated fills and meshes have their own rules (`gradient_repeat`, `mesh_repeat`).
