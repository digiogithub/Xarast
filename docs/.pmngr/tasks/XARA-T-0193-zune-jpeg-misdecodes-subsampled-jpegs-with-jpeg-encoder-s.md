---
id: XARA-T-0193
type: task
title: zune-jpeg misdecodes subsampled JPEGs with jpeg-encoder's optimised Huffman tables
status: backlog
priority: low
author: mcp
labels: [image, jpeg, import]
created: 2026-09-23T19:49:04Z
updated: 2026-09-23T19:49:04Z
---

## Description
Found while building JPEG export (XARA-US-0057). A JPEG written by `jpeg-encoder` 0.7.1 with `set_optimized_huffman_tables(true)` and 4:2:2 or 4:2:0 subsampling decodes wrongly through `image` → `zune-jpeg` 0.5 (a flat (200,40,90) patch came back as (201,84,0) at 4:2:2 and (0,135,0) at 4:2:0), while libjpeg (Pillow 12.3) and ImageMagick decode the same bytes correctly. 4:4:4 decodes fine, and so does the same image with the standard Annex K tables.

Export works around it by writing the standard tables (`crates/xarast-io/src/jpeg.rs`), so our own exports re-import correctly. The underlying decoder issue may affect real-world JPEGs from other encoders that emit optimised tables, which is what `xarast-image` imports.

## Work
- Reproduce with a minimal file (encode a flat 64x48 image with jpeg-encoder, optimised tables, 4:2:0) and check against the latest zune-jpeg.
- If still present, report upstream and decide whether `xarast-image` needs a fallback.

## Acceptance Criteria
- Either a zune-jpeg version that decodes the reproduction correctly is pinned, or the limitation is documented in `docs/memory/image.md` with the upstream issue.
