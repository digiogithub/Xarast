---
id: XARA-T-0301
type: task
title: Photo panel UI with live proxy-resolution preview (T10.6.5 preview, T10.6.8)
status: in_review
parent: XARA-US-0054
author: mcp
labels: [phase-10, image, ui]
created: 2026-09-24T13:19:22Z
updated: 2026-09-24T15:28:54Z
started: 2026-09-24T14:36:13Z
---

## Description
XARA-US-0054 built the model (`xarast_doc::photo`), the pixel pipeline, the walker's cached evaluation and `Session::set_photo_ops` (one undo step, "Adjust Photo"), but no UI reaches it. Build the photo panel of phase 10 T10.6.8: list the selected bitmap object's chain, per-op controls (crop, rotate/flip, levels per channel, gamma, brightness, contrast, saturation, greyscale), reset, and a "non-editable: unknown operation" state when `PhotoOps::is_editable()` is false.

Dragging a slider must be one undo step (coalesce through a gesture or commit on release) and preview live. Today the walker evaluates a chain on the walk thread at full resolution (≈ 66 ms of LUT for 24 Mpx plus the copy); the preview should evaluate at proxy resolution (a pyramid level of the master) and/or off the main thread, keeping the ≤ 33 ms slider-latency budget of phase 10.

## Acceptance Criteria
- Panel lists the chain and edits it through `Session::set_photo_ops`; unknown ops show the chain read-only.
- A slider drag is one undo step; preview latency ≤ 33 ms on a 24 Mpx photo (trace).
- The walk thread never evaluates a full-resolution chain during a drag.

## Notes
See `docs/memory/image.md`, "Photo adjustments (XARA-US-0054)".
