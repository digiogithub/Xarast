---
id: XARA-T-0124
type: task
title: T10.1.1 BitmapResource, BitmapInfo (colour space), BitmapData, OriginalEncoded in xarast-image
status: done
parent: XARA-US-0051
author: mcp
labels: [phase-10, image, xarast-image]
created: 2026-09-23T17:06:12Z
updated: 2026-09-23T17:06:12Z
---

## Description
The resource model of phase 10 in the leaf crate `xarast-image`: `BitmapInfo` with `ColourSpace` (AssumedSrgb / Srgb / Icc{hash} / Grey{gamma}), premultiplied RGBA8 `BitmapData`, `OriginalEncoded`, `BitmapResource`, `ImageFormat`; BLAKE3 content hash over decoded, orientation-normalised pixels.

## Acceptance Criteria
- Types public and documented; content hash identical across containers and EXIF orientations (tested).

## Notes
Commit c5605ee. `xarast-doc` still has its own `BitmapResource` (SHA-256); reconciliation is T10.1.2. See `docs/memory/image.md`.
