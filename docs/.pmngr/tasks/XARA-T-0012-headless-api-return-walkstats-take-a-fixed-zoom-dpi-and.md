---
id: XARA-T-0012
type: task
title: "Headless API: return WalkStats, take a fixed zoom/dpi, and make drawing_rect exclude pages"
status: done
priority: medium
parent: XARA-US-0081
author: mcp
labels: [app, headless, api]
created: 2026-09-23T10:04:32Z
updated: 2026-09-23T11:22:29Z
started: 2026-09-23T11:22:25Z
closed: 2026-09-23T11:22:29Z
---

## Description
Found while wiring `xarast-cli render` (XARA-T-0004). The CLI works around three gaps in `xarast-app`'s headless API. It does not change `xarast-app`.

1. **`HeadlessResult` has no `WalkStats`.** `headless::render` keeps its `SceneWalker` private, so the CLI walks a second time (`Session::rebuild_scene`) to find out what the render skipped (text, quick shapes, images, live effects, clips).
2. **`HeadlessOptions` has no zoom, dpi or centre.** Its only framing choice is `fit_drawing`. To render at 100 %, the CLI sets `session.viewport` itself (`set_dpi`, `resize`, `set_zoom`, `set_centre`) and passes `fit_drawing: false`.
3. **`viewport::drawing_rect` includes the page nodes.** It returns the root's bounds, and `compute_bounds_with` gives `NodeKind::Page` its page rectangle. So "the bounding box of everything drawn" is really pages ∪ drawing. A one-inch square on an A4 page frames the whole A4 page, and `HeadlessOptions::fit_drawing` has the same bias. The CLI has its own `ink_rect`: the union of `nodes_rect` over the children of the active spread's visible, non-guide layers.

## Acceptance Criteria
- `HeadlessResult` carries the walk's `WalkStats`, and `SceneStats` too if possible.
- `HeadlessOptions` can express "zoom Z at D dpi, centred on rect R", as well as fit.
- `drawing_rect` excludes page nodes, or a separate `ink_rect` is added and documented. `fit_drawing` uses it.
- `xarast-cli` drops its workarounds (`render.rs`: `ink_rect` and the second walk).
