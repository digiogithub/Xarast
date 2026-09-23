---
id: XARA-T-0156
type: task
title: Reconcile xarast-doc BitmapResource hash (SHA-256) with xarast-image content hash (BLAKE3)
status: backlog
priority: low
parent: XARA-US-0051
author: mcp
labels: [phase-10, image, document-model]
created: 2026-09-23T17:48:11Z
updated: 2026-09-23T17:48:11Z
---

## Description
T10.1.2. `xarast_doc::BitmapResource::content_hash` is SHA-256 over info, pixels, palette and the encoded original; `xarast_image::BitmapData::content_hash` is BLAKE3 over the premultiplied, orientation-normalised pixels. Two keys for the same concept. It did not block XARA-T-0129 (the walker decodes lazily and never stores pixels in the document), so it was left.

## Acceptance Criteria
- One documented dedup key, or a documented reason why the document keeps its own (it must still distinguish two encodings of the same pixels, because the writer emits `original` verbatim).

## Notes
`docs/memory/image.md` "Open TODOs".
