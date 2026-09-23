---
id: XARA-T-0033
type: task
title: DisplayList::build scans every scene op even when the dirty rect culls almost all of them
status: backlog
priority: high
parent: XARA-US-0016
author: mcp
labels: [render, perf, phase-5]
created: 2026-09-23T11:20:26Z
updated: 2026-09-23T11:20:26Z
---

## Description
Found by the viewport bench (`cargo bench -p xarast-app -- viewport`, XARA-US-0006). The render thread now builds a display list only for the strips a pan exposes (`xarast_app::render_thread`, pixel reuse). With the synthetic document at 105 852 objects (224 218 primitives), 1920x1080, zoomed so that every strip has ink:

- one `DisplayList::build(scene, view, DirtyRect::of(strip))` for a 9 px strip costs **15-19 ms**, although only ~5-10k commands survive the cull;
- a full-view build costs ~60 ms;
- a Draft pan frame is therefore ~35 ms of list building plus ~15 ms of raster = **~54 ms**, against the 16 ms pan budget.

The cost is the per-op scan: every `SceneOp` is visited and `device_bounds_of` dereferences each `PathRef` (an `Arc`) to read its bounds, which is a cache miss per op, plus a `Vec::with_capacity(scene.ops.len())` per build.

Measured by probe: re-walking the document with a dirty rect (`SceneWalker::rebuild(Some(strip))`) is slower still (30-130 ms), so the fix does not belong in the walker.

## Acceptance Criteria
- A culled build over a strip of a 224k-primitive scene costs well under 2 ms. Options: keep each op's document bounds inline in `SceneOp` (no pointer chase), a spatial index over op bounds that preserves op order, or a `build_many(scene, view, &[DeviceRect])` that scans once for several rects (a diagonal pan has two strips, a Final column pass four).
- Capacity sized to the survivors, not to `scene.ops.len()`.
- `viewport/zoomed/pan_draft` in the app bench drops accordingly (currently 51-57 ms).

## Notes
`xarast-app` is not edited for this; `render_thread.rs` calls `DisplayList::build` per strip and per Final column and will pick up any speed-up unchanged, or switch to `build_many` if that is the API chosen.
