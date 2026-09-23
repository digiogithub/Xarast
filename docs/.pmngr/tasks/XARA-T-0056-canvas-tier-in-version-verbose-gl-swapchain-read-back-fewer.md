---
id: XARA-T-0056
type: task
title: Canvas tier in --version --verbose, GL swapchain read-back, fewer partial-tile restarts
status: backlog
priority: low
parent: XARA-US-0061
author: mcp
labels: [gpu, ui]
created: 2026-09-23T13:24:41Z
updated: 2026-09-23T13:26:50Z
---

## Description
Loose ends left by XARA-T-0050 (see docs/memory/ui.md decisions 27–31):
1. Phase-05 criterion 15 asks for the selected tier in `--version --verbose` too; today only the status bar and the log show it (the tier needs a device).
2. `--screenshot` under `WGPU_BACKEND=gl` logs "this swapchain cannot be read back" (GL surface lacks COPY_SRC); capture from the canvas texture plus a UI pass into an offscreen target instead.
3. A pan restarts partial tiles at the trailing edge (valid area must stay a rectangle), which is most of the ~320 KB uploaded per pan step beyond the strips. Shrinking the valid area instead of restarting would cut it.

## Acceptance Criteria
- `xarast --version --verbose` prints the tier that would be selected.
- `WGPU_BACKEND=gl xarast --screenshot` writes a PNG.
- Per-step pan upload measured before/after with `examples/canvas_probe`.
