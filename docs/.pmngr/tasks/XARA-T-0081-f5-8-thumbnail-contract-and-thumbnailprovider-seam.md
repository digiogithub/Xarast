---
id: XARA-T-0081
type: task
title: F5.8 — Thumbnail contract and ThumbnailProvider seam
status: done
parent: XARA-US-0025
author: mcp
labels: [phase-6, xarast-format]
created: 2026-09-23T14:23:15Z
updated: 2026-09-23T14:23:15Z
---

## Description
`thumbnail::ThumbnailProvider` (takes `&xarast_doc::Document`), `check_png` (PNG, RGBA8, ≤ 512 px thumbnail / ≤ 1024 px preview) enforced by `set_thumbnail`/`set_preview`; `XarastReader::thumbnail()` never touches the document.

## Acceptance Criteria
- `thumbnail::tests::contract`, `writer_refuses_bad_input`.

## Notes
Done in commit 0529f6d. The provider implementation over the renderer is a separate task (app/render side).
