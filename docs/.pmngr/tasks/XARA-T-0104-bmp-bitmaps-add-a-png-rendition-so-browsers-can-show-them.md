---
id: XARA-T-0104
type: task
title: "BMP bitmaps: add a PNG rendition so browsers can show them"
status: todo
priority: low
parent: XARA-US-0023
author: mcp
labels: [phase-6, xarast-format, svg, phase-10]
created: 2026-09-23T15:40:55Z
updated: 2026-09-23T15:40:55Z
---

## Description
`.xar` BMP (and compressed-BMP) originals are written byte for byte to `resources/blobs/b3-….bin` and referenced from `<image>`, but no browser-grade renderer decodes them (`SvgWriteStats::images_unrenderable`). Once Phase 10 decodes bitmaps, write a PNG rendition under `resources/derived/` with `mf:derived-from` the master (F5.5) and point the `href` at it, keeping the master for fidelity.
