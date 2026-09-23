---
id: XARA-T-0229
type: task
title: "W11.4 T11.4.8 PDF images: DCTDecode passthrough, FlateDecode, SMask, bitmap fills as image patterns"
status: backlog
priority: medium
parent: XARA-US-0059
author: mcp
labels: [phase-11, io, pdf, image]
created: 2026-09-23T21:35:10Z
updated: 2026-09-23T21:35:10Z
---

## Description
Round 1 rasterises placed images and bitmap fills through the CPU backend at `rasterise_dpi` (reported as `Compromise::Rasterised`). Emit them as image XObjects instead: original JPEG bytes under `DCTDecode` (phase 11 criteria 8 and 11), other bitmaps under `FlateDecode` with alpha as an `SMask`, repeating bitmap fills as tiling patterns, contone/adjustments baked when needed; `PdfImagePolicy` (passthrough / recompress / downsample).

## Acceptance Criteria
- A document with an imported JPEG exports to PDF containing the JPEG's original bytes (criterion 11).
- No `Rasterised` entry for plain affine bitmap fills or placed images.

## Notes
Needs the original bytes: `ExportSource`/`SourceScene` must expose the resource store (phase 10 `xarast-image`). `PdfWriter::image` already writes RGB + SMask.
