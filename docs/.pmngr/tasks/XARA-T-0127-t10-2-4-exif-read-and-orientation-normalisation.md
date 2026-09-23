---
id: XARA-T-0127
type: task
title: T10.2.4 EXIF read and orientation normalisation
status: done
parent: XARA-US-0051
author: mcp
labels: [phase-10, image, xarast-image]
created: 2026-09-23T17:06:12Z
updated: 2026-09-23T17:06:12Z
---

## Description
EXIF (JPEG APP1, PNG eXIf, WebP, TIFF) read through the decoders; the orientation applied once at decode (`Orientation`, all 8 values); the raw EXIF and ICC blocks retained in `DecodedImage` for re-emission. No `kamadak-exif` needed.

## Acceptance Criteria
- All 8 orientations decode to pixel-identical upright output with equal content hashes; JPEG orientation 6 applied; probe agrees with decode.

## Notes
Commit c5605ee.
