---
id: XARA-T-0273
type: task
title: Wire the document bitmap-smoothing flag (TAG_DOCUMENTBITMAPSMOOTHING 4116) to the image filter
status: backlog
priority: low
parent: XARA-US-0052
author: mcp
labels: [phase-10, bitmap, xar-import, app-core]
created: 2026-09-24T07:56:41Z
updated: 2026-09-24T07:56:41Z
---

## Description
Gap found while closing XARA-US-0052. The renderer has the three filters (`Nearest`, `Bilinear`, `HighQuality`) and the walker picks `RenderQuality::image_filter` (Draft → Nearest, Final → HighQuality). The document's smoothing flag — `TAG_DOCUMENTBITMAPSMOOTHING` (4116, `u8 flags` + 4 reserved, `research/01`), and per-bitmap `TAG_BITMAP_PROPERTIES` (4115) flags — is decoded by `xarast-xar` but stored nowhere, so a document that asks for unsmoothed ("pixelated") bitmaps is always smoothed at Final. The `.xarast` mapping is `image-rendering` / `xarast:bitmap-smoothing` (`research/06`).

## Acceptance Criteria
- `xarast-doc` stores the document flag (and the per-bitmap one), the importer fills it, `.xarast` round-trips it.
- The walker maps "smoothing off" to `Filter::Nearest` at Final (keep the aligned fast path's rule: at 1 texel per pixel every filter is a point anyway).
- Export keeps its own override (HighQuality) only if the original did when printing (`grndrgn.cpp:3400-3433`, `research/03 §2.8`); check before deciding.
