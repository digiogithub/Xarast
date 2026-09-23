---
id: XARA-T-0125
type: task
title: T10.2.1/T10.2.2 Decoder façade and per-format decoders (PNG, JPEG, WebP, GIF, TIFF, BMP/DIB, PNM)
status: done
parent: XARA-US-0051
author: mcp
labels: [phase-10, image, xarast-image]
created: 2026-09-23T17:06:12Z
updated: 2026-09-23T17:06:12Z
---

## Description
`probe`/`probe_as`, `decode`/`decode_as`, `decode_on_worker` over `image` 0.25.10 (default features off; png, jpeg via zune-jpeg 0.5, webp, gif, tiff, bmp, pnm). Own JPEG probe fast path (67 ns vs 487 µs through `image`). `.xar` wrappings in `xar::decode_xar_bitmap` (headerless DIB for tag 65, BMPZIP inflate for 69, palette snap for 71).

## Acceptance Criteria
- Round trip per format against synthetic fixtures (15+ tests); every bitmap record in the 59-file corpus decodes (102 records: 58 GIF previews, 4 JPEG, 32 PNG, 8 JPEG8BPP; 0 failures).

## Notes
Commits c5605ee, 6f16ebc (fuzz finding fix). New facts in `research/01 §4.5` (tag 65 is a headerless DIB, tag 69 framing, tag 71 conditions).
