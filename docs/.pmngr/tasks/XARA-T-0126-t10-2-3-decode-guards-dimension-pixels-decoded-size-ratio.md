---
id: XARA-T-0126
type: task
title: "T10.2.3 Decode guards: dimension, pixels, decoded size, ratio, JPEG scans, allocation, wall clock"
status: done
parent: XARA-US-0051
author: mcp
labels: [phase-10, image, xarast-image, security]
created: 2026-09-23T17:06:12Z
updated: 2026-09-23T17:06:12Z
---

## Description
`DecodeLimits` (phase-10 W10.2 table defaults plus `ratio_floor_bytes` 16 MiB and `max_jpeg_scans` 128) enforced from the header before any pixel allocation; typed `DecodeError`, only `TooLarge` overridable. Allocation ceiling through `image::Limits::max_alloc` (1.25 × native + 1 MiB); cooperative deadline reader plus hard `decode_on_worker`.

## Acceptance Criteria
- Bomb suite `tests/bombs.rs`: 11 synthetic fixtures (PNG 65535², zlib ratio bomb, 5000-scan JPEG, GIF huge screen, 2 WebP header bombs, TIFF, BMP, PNM, truncated PNG, BMPZIP inflate bomb) each refused in µs; peak RSS growth over the suite 32 MiB (< 64 MiB).

## Notes
Commit c5605ee.
