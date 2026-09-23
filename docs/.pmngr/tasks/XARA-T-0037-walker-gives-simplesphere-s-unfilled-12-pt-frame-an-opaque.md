---
id: XARA-T-0037
type: task
title: Walker gives SimpleSphere's unfilled 12 pt frame an opaque black fill, hiding the whole drawing
status: in_progress
priority: medium
parent: XARA-US-0081
author: mcp
labels: [app-core, render]
created: 2026-09-23T11:41:06Z
updated: 2026-09-23T12:02:55Z
started: 2026-09-23T12:02:55Z
---

## Description
This is the diagnosis behind XARA-T-0025 (render agent, 2026-09-23). The fault is in the arena→scene walker or the attribute defaults, not in `xarast-render`.

`Designs/SimpleSphere.xar` ends its only layer with a frame: record 9458 (index in the decompressed record stream) is `TAG_PATH_RELATIVE_FILLED_STROKED`, and its only attributes are `TAG_LINEWIDTH` 12 000 mp, `TAG_LINECOLOUR` (black) and `TAG_JOIN`/`176`. It has **no fill attribute**. The walker emits it as display-list command #1841 of 1949: `Fill { paint: Solid(0,0,0,255), transparency: Mix Flat(0) }` with bounds (65,24)-(1178,805) px at 100 %. It is drawn after the ~1 800 sphere shapes, so it covers them all and only the caption (drawn later) survives.

I verified this by skipping that one command in the CPU backend with a temporary debug hack: the full design appears, including the sphere, the rings and the binary text. The frame's 12 pt black stroke (a separate `Stroke` command) stays correct.

So an unset fill resolves to opaque black. Per the original's behaviour, the default fill must leave an unfilled frame's interior undrawn (or at least not opaque black). Otherwise this design could never have shown anything. Check the default fill in the research notes (`docs/research/02`) and fix the attribute default or the walker's fallback.

A second, lower-confidence suspicion: every gradient and graduated transparency in this file reaches the renderer with `repeat: Repeat`. Yet the file has no per-object `TAG_FILL_REPEATING`/`NONREPEATING` records: the only two `163`/`180` records sit in `TAG_CURRENTATTRIBUTES`. The render shows hard bars where ramps wrap. So the default mapping (`Tiling::None`) may be mapped to `Repeat` in `xarast-app/src/paint.rs` instead of `Simple`. Please verify it against the research doc.

Related fix already in `xarast-render`: `d392269` moves graduated transparency into device space. Before it, every transparency gradient in this file evaluated as flat.

## Acceptance Criteria
- An unfilled `FILLED_STROKED` path takes the original's default fill; SimpleSphere shows its sphere headless and in the window.
- The gradient/transparency repeat default is checked against the research doc.
- A pixel-probe or golden test pins the sphere's presence (XARA-T-0025's criterion).
