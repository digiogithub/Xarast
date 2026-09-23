---
id: XARA-T-0232
type: task
title: "io: PDF fidelity-ladder cost — one display-list build per rasterised object, backdrop rasters inflate files"
status: backlog
priority: low
parent: XARA-US-0059
author: mcp
labels: [phase-11, io, pdf, perf]
created: 2026-09-23T21:35:10Z
updated: 2026-09-23T21:35:10Z
---

## Description
`pdf/rasterise.rs` builds a full display list for every rasterised object (O(n·k) for n commands and k rasterised objects) and `DisplayList::with_commands` clones the side tables each time. Backdrop rasters at 300 dpi dominate file size on blend-heavy designs (corpus, `--background paper`, Exact: `scope3 simple` 29 MB, `Spitfire` 22 MB, `Watch4` 13 MB; whole corpus 121 MB, 6.5 s render in release).

Ideas: build one raster list per page at the rasterising resolution and select from it; merge overlapping backdrop regions into one image; JPEG-compress opaque backdrop rasters (lossy, report it); `PreferNative` once T11.4.6 clears families.

## Acceptance Criteria
- Phase 11 budget "PDF export with 20 rasterised objects at 300 dpi ≤ 8 s" measured by a criterion bench.
- Corpus total PDF size and time recorded in `docs/memory/perf.md` before and after.
