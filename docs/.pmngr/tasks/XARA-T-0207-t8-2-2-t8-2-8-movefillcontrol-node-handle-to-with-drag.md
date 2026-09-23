---
id: XARA-T-0207
type: task
title: T8.2.2 / T8.2.8 — MoveFillControl { node, handle, to } with drag coalescing
status: done
parent: XARA-US-0038
author: mcp
labels: [phase-8, doc]
created: 2026-09-23T19:53:36Z
updated: 2026-09-23T19:53:36Z
---

## Result
- Commit `48807d8`.
- `FillHandle` mapping per shape (documented on the type): linear Start/End; radial Centre (moves the fill)/Major/Minor (aspect-locked keeps 90° and equal length); conical Centre/End; diamond Centre/Corner1/Corner2; three/four colour Start/End/End2/End3; bitmap Start/End/End2; `Stop(i)` projects onto the arm and re-sorts. Anything else → `EditError::FillEdit`, document untouched (tested for every shape × handle).
- Coalescing: `drag: Option<u64>` (the tool's bus gesture) → `CoalesceKey` over (drag, node, slot, channel, handle). 60 moves = 1 undo step; undo restores the pre-drag geometry exactly; a second drag of another handle is a second step. Same for `MoveStop` keyed on the stop index.
- Esc mid-drag is the tool's (no command emitted before DragEnd, per tools.md); W8.4 wires it.
