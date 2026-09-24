---
id: XARA-T-0293
type: task
title: Merge the bitmap gallery's thumbnail decode into the walker's shared decode path and DecodedImages
status: done
parent: XARA-US-0053
author: mcp
labels: [phase-10, image, perf]
created: 2026-09-24T12:06:55Z
updated: 2026-09-24T12:37:57Z
started: 2026-09-24T12:09:27Z
closed: 2026-09-24T12:37:57Z
---

## Description
US-0055 added `crates/xarast-app/src/bitmap_gallery.rs`, whose thumbnail decoder copies the walker's decode dispatch (`Encoded::decode` in `walker.rs`: tag 71 with the palette, 65, 69, the façade). XARA-T-0281 added a per-document decoded-image cache (`xarast-app/src/decoded.rs`, `Session::decoded_images`, `SceneWalker::with_decoded_images`). The T-0281 branch could not merge the working branch (the merge was refused by the permission system), so the two were not joined.

## Acceptance Criteria
- One decode dispatch for `.xar`/placed bitmaps, used by the walker and the gallery (no copy).
- Gallery thumbnails reuse the session's `DecodedImages` when the bitmap is already decoded (and file what they decode, or document why not: a thumbnail may want a smaller decode).
- A test counts decodes: opening the gallery on a walked document decodes nothing again.

## Notes
See docs/memory/image.md "Walker integration" item 7 and app-core.md decision 42.
