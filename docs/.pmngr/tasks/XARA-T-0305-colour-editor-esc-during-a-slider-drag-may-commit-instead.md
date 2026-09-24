---
id: XARA-T-0305
type: task
title: "Colour editor: Esc during a slider drag may commit instead of cancel"
status: done
author: mcp
labels: [phase-8, ui, bug]
created: 2026-09-24T14:59:17Z
updated: 2026-09-24T17:18:35Z
started: 2026-09-24T16:17:19Z
closed: 2026-09-24T17:18:35Z
---

## Description
XARA-T-0301 measured this in egui 0.33 under kittest: when `Esc` is pressed while a `Slider` is being dragged, egui ends the drag itself and reports `drag_stopped()` in that same frame, with `dragged()` false. `panels::colour::ColourPanel::classify` only looks for `Esc` while `r.dragged()` is true, and maps `drag_stopped` to `Use::Commit`. So an `Esc` during a drag of a colour editor number or derivation slider probably commits the drag instead of cancelling it. This has not been checked for the colour panel; it is inferred from the identical code path. The photo panel fixes it in its own classifier by checking `key_pressed(Escape)` on the `drag_stopped` frame (`docs/memory/ui.md`, dead ends, "egui's own drag state").

## Acceptance Criteria
- A kittest test drags a colour editor slider (for example Tint), presses `Esc` mid-drag, and sees `ColourEditorOp::Cancel` and no `Commit`.
- The fix is applied if the test fails.

## Notes
The 2D field and strip use `PressState`, not `classify`, and are probably unaffected.
