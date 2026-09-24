---
id: XARA-T-0271
type: task
title: T10.3.1/T10.3.2 — Bitmap fill editing commands (origin, axes, tiling, DPI) and fill handles
status: done
priority: medium
parent: XARA-US-0052
author: mcp
labels: [phase-10, bitmap, app-core, tools]
created: 2026-09-24T07:56:41Z
updated: 2026-09-24T10:39:00Z
started: 2026-09-24T09:33:22Z
closed: 2026-09-24T10:39:00Z
---

## Description
Gap found while closing XARA-US-0052's renderer half. `FillGeometry::Bitmap` renders (orientation, tiling, contone, transparency) but cannot be edited: `xarast-app/src/fill_handles.rs` returns no handles for it ("bitmap handles are phase 10's"), and `xarast-doc` has no bitmap-specific fill commands (origin, axes, tiling mode, DPI / natural size).

## Acceptance Criteria
- T10.3.1: `xarast-doc` commands to move the origin and axes of a bitmap fill, set its tiling (`Simple`/`Repeat`/`RepeatInverted`), and reset it to the bitmap's natural size from its DPI; undoable.
- T10.3.2: bitmap fill handles in `fill_handles`, reusing phase 8's overlay (start, end, second end; perspective corners when present); dragging previews at Draft and commits at Final.
- Remember the orientation rule: a fill's start point is the image's **bottom**-left (`docs/memory/render.md`, "Bitmap fill orientation"); only `paint::bitmap_frame` translates.
