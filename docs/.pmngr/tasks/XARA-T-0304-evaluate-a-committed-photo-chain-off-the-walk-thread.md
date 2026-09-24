---
id: XARA-T-0304
type: task
title: Evaluate a committed photo chain off the walk thread (release hitch on large photos)
status: in_review
parent: XARA-US-0054
author: mcp
labels: [phase-10, image, perf]
created: 2026-09-24T14:59:05Z
updated: 2026-09-24T16:52:56Z
started: 2026-09-24T16:08:22Z
---

## Description
XARA-T-0301 moved slider drags onto a proxy: a drag frame evaluates the chain on a reduced pyramid level, with a median of about 7 ms of walk on a 24 Mpx photograph. The release still commits through `SetPhotoOps`, and the next walk evaluates the whole chain at full resolution on the walk thread (`walker::derive`). On 6000 × 4000 that takes about 60 ms of evaluation plus about 200 ms of `ImageRef::prepare` (the pyramid), roughly 264 ms once, so the release frame hitches.

Evaluate the committed chain on a worker (the pattern of the XARA-T-0281 helper that brings an evicted base back). Keep drawing the last proxy for that object until the full image lands, then repaint only the object's damage. Keep the result byte-identical to the synchronous evaluation, because export and thumbnails must still produce it in place.

## Acceptance Criteria
- The frame after a slider release on a 24 Mpx photo stays within the 33 ms budget (trace).
- Once the worker finishes, the view equals the full-resolution evaluation pixel for pixel (the existing `the_committed_render_is_the_full_resolution_evaluation` still passes).
- Export and headless renders never show a proxy.

## Notes
Numbers and design: `docs/memory/image.md`, "Photo adjustments", under "Live proxy preview" and "Measured".
