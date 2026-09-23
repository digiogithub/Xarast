---
id: XARA-T-0099
type: task
title: F3.8 — Images referencing resources/ with href and xlink:href
status: done
priority: high
parent: XARA-US-0023
author: mcp
labels: [phase-6, xarast-format, svg]
created: 2026-09-23T15:40:30Z
updated: 2026-09-23T15:40:30Z
---

## Description
Bitmap nodes → `<image>`; bitmap fills → `<pattern>` with an `<image>` in its unit square. The original encoded bytes go into the `ResourceIndex` once per bitmap (BLAKE3 dedup, hash-derived names), one reference per href written. PNG/JPEG/GIF → `resources/images/`.

## Notes
Done in eb285b2. BMP originals (and the `.xar` compressed BMP) are kept byte for byte in `resources/blobs/*.bin`: referenced but not renderable by browsers — a PNG rendition needs the Phase 10 decoder (filed).
