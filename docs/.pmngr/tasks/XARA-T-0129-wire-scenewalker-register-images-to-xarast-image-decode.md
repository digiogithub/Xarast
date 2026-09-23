---
id: XARA-T-0129
type: task
title: Wire SceneWalker::register_images to xarast-image (decode original bytes, register straight RGBA)
status: todo
parent: XARA-US-0051
author: mcp
labels: [phase-10, image, xarast-app]
created: 2026-09-23T17:06:23Z
updated: 2026-09-23T17:06:23Z
---

## Description
Bitmaps imported from `.xar` keep their encoded bytes in `BitmapResource::original` with empty `pixels`, so `register_images` skips them and `WalkStats::images_pending` > 0 on every bitmap file. `xarast-image` can now decode all of them (every corpus record decodes). Full contract: `docs/memory/image.md`, "Walker integration contract".

1. `xarast-app` depends on `xarast-image`.
2. For a resource with empty `pixels` and `Some(original)`, decode once with `DecodeLimits::default()`:
   - `ImageFormat::Png | Jpeg | Gif` → `xarast_image::decode(bytes, &limits)`;
   - `ImageFormat::Bmp` → `xarast_image::xar::decode_xar_bitmap(65, bytes, &[], &limits)` (sniffs `BM`, else headerless DIB);
   - `ImageFormat::Unknown` (the importer's BMPZIP) → `decode_xar_bitmap(69, bytes, &[], &limits)`.
3. Register `ImageRef::new(d.data.width, d.data.height, d.data.to_straight_rgba8())` — `ImageRef` takes **straight** alpha; `BitmapData` is premultiplied. Use decoded dimensions (the importer leaves `res.info` zeroed).
4. Negative cache of failed `BitmapId`s so a bad bitmap is not re-decoded each frame; count them in a new `WalkStats::images_failed`, not `images_pending`.

## Acceptance Criteria
- `images_pending == 0` on every corpus file in `xarast-app/tests/corpus.rs` (tighten the tolerance there).
- `xarast-cli render` of `testfiles/TestBitmapFill.xar` and the bitmap-bearing `Designs/` files draws the bitmaps.
- A failed decode never panics and is not retried every frame.

## Notes
- Whole-corpus decode ≈ 120 ms (test profile); synchronous decode on the first frame is acceptable for now; off-frame decoding is T10.5.5.
- Known gap: the importer maps tag 71 (JPEG8BPP) to `Jpeg` and drops its palette, so those render as the 24 bpp JPEG until T10.2.6 (`xarast-xar`) keeps the palette or decodes at import with `decode_xar_bitmap(71, bytes, palette, …)`.
