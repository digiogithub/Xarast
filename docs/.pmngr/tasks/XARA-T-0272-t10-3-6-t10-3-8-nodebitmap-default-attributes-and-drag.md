---
id: XARA-T-0272
type: task
title: T10.3.6/T10.3.8 — NodeBitmap default attributes and drag/paste-to-place at natural size
status: backlog
priority: low
parent: XARA-US-0052
author: mcp
labels: [phase-10, bitmap, app-core]
created: 2026-09-24T07:56:41Z
updated: 2026-09-24T07:56:41Z
---

## Description
Gap found while closing XARA-US-0052's renderer half. W10.3 also lists `NodeBitmap` = rectangle + bitmap fill with an `ApplyDefaultBitmapAttrs` equivalent (T10.3.6, `xarast-doc`) and drag-to-place / paste-to-place with the natural size from DPI (T10.3.8, `xarast-app`). Neither exists; placing a bitmap today only happens through `.xar`/`.xarast` import. T10.3.8 overlaps the import UX of XARA-US-0055: do it wherever that story lands first and close the other.

## Acceptance Criteria
- A new placed bitmap gets the original's default attributes (no line, the bitmap as fill) and its natural size (`width × 72 000 / dpi` mp; `xarast-image` `recommended_width`).
- Drop and paste place it centred on the pointer / view; one undo step.
