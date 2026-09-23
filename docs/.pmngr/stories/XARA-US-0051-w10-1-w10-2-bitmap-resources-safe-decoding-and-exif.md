---
id: XARA-US-0051
type: story
title: W10.1/W10.2 — Bitmap resources, safe decoding and EXIF
status: backlog
parent: XARA-EP-0011
author: mcp
labels: [phase-10, image]
created: 2026-09-23T09:42:40Z
updated: 2026-09-23T09:42:40Z
---

## Description
As a user, bitmaps in imported files display (today `images_pending` > 0 on every file with a bitmap).

## Tasks (full tables: phase-10 §W10.1, §W10.2)
- T10.1.1–4 `BitmapResource`, hash dedup over decoded pixels, unused sweep, commands.
- T10.2.1–4 `Decoder` façade, PNG/JPEG/WebP/GIF/TIFF/BMP/PNM, guards (dimension/pixels/alloc/time), EXIF orientation.
- Wire `SceneWalker::register_images`.
