# tools

Memory note for **editing and tools** (phase 7): the command bus seen from
the application, the tool machine, the tools, the palette and the infobar.
Phase spec: [`../phases/phase-07-tools-and-editing.md`](../phases/phase-07-tools-and-editing.md)
(§W1, §W2). The document-side substrate (actions, `Tx`, `History`,
revisions) is in [`document-model.md`](document-model.md) decisions 7, 19,
34, 35; the shell/UI contract in [`app-core.md`](app-core.md).

## Current state (2026-09-23, XARA-US-0029 / XARA-US-0030 / palette)

| Piece | Where | State |
|---|---|---|
| `EditCommand` (`TransformNodes`, `DeleteNodes`), `CommandSink` | `xarast-app/src/ops.rs` | done |
| `Session::apply_edit`, `begin_gesture`/`end_gesture`, `undo_label`/`redo_label` | `session.rs` | done |
| `ToolMachine`, `Tool`, `ToolCtx`, `GestureEvent`, `Preview`, `pick` | `tool.rs` | done (bbox pick) |
| Selector (click, Adjust-toggle, move-drag, Constrain 45°, marquee enclose, X/Y infobar) | `tools.rs` | done |
| Push, Zoom (click in, Adjust-click out, drag = zoom to area) | `tools.rs` | done |
| Shape editor, Rectangle, Ellipse, Pen, Freehand | `tools.rs` `PendingTool` | choosable, "coming soon" |
| Fill, Transparency, Text | — | not choosable (greyed, phase 8/9) |
| Preview rendering | `walker.rs` `rebuild_previewed` | done: transformed scene group |
| Per-node `ContentHash` | `walker.rs` `content_hash` | done |
| Edit menu, tool palette, infobar row | `xarast-ui` `menus.rs`, `toolbar.rs` | done |
| Keys | `AppCommand` table | Ctrl+Z, Ctrl+Shift+Z, Ctrl+Y, Del, Ctrl+A, Esc (works in drag), F2, F3, F4, Shift+F3/F4/F5/F7/F8 |
| `--probe drag` | `xarast-shell` | done |

Measured: `cargo bench -p xarast-app --bench undo` — undo+redo of a move of
one object at 250 000 nodes **339 µs the pair** through `Session` (≈0.17 ms
each; budget 1 ms), 314 µs through the bare bus. Live window
(`--probe drag`, GardenPlan.xar, 2056×1286, GPU tiles): input → presented
p50 2.1 ms, p99 7.8 ms over 100 drag frames, each a preview scene rebuild.

## Decisions taken (and why)

1. **No second transaction builder.** `xarast_doc::Tx` already is T1.2
   (applies as it goes, records inverses first, drops without commit =
   rollback) and `CommandBus` already had gestures; the app adds the typed
   command enum on top, not a parallel bus.
2. **The enum is `EditCommand`, not `Command`**: `xarast_doc::Command` is
   the trait every command implements. Same for the machine's state enum:
   `InteractionState` (the spec's `ToolState` is taken by
   `edit::ToolState`, which tool is chosen).
3. **Coalescing = gesture ∧ `coalesces_with`.** Inside an open gesture a
   command merges with the previous step only if it is the same kind of
   transform on the same nodes; otherwise `apply_edit` opens a new gesture,
   so the step splits. A drag needs none of this: it previews and commits
   exactly one command at `DragEnd`.
4. **The infobar is data.** `xarast-app` has no toolkit, so a tool returns
   an `Infobar` (fields + notes) and gets `infobar_edit(field, Mp)`;
   `xarast-ui` formats/parses units (`units.rs`) and raises
   `UiCommand::InfobarEdit` → `Intent::InfobarEdit`. The spec's
   `fn infobar(&mut self, ui: &mut egui::Ui)` would have broken app-core
   invariant 1.
5. **Overlay is data too** (`OverlayShape`), converted by the viewer to
   `xarast_ui::OverlayItem` and painted by egui over the canvas: moving a
   handle or a rubber band dirties no document tile.
6. **Preview = a scene group with the preview matrix** around each
   previewed node (id = tag | 1<<63). The display list already composes
   group transforms, so no path is cloned per frame. A non-empty preview
   ignores the dirty rect, and `rebuild_scene` widens `scene_ink` by the
   moved bounds (the render thread skips strips outside the ink).
7. **Preview changes report `Changed::DOCUMENT`** (the scene must be
   rebuilt); overlay-only changes report `SELECTION`.
8. **Session applies tool requests after the tool returns** (`run_tool`):
   selection changes, view changes, then commands through the bus. The tool
   holds only `&Document`; a compile-fail doctest in `tool.rs` pins that.
9. **Esc**: consumed by the machine when a gesture is in flight (cancel,
   preview dropped, nothing emitted); otherwise it is "select none". It is
   the only `works_in_drag` command. Undo, redo and every tool switch cancel
   a gesture in flight first.
10. **Scroll bounds are refreshed lazily** at the next `rebuild_scene`
    (`scroll_bounds_stale`). Recomputing them in `after_mutation` walked the
    whole cold-bounds document: 34 ms per undo at 250 000 nodes.
11. **Ctrl+Shift+Z is Redo**, as the maintainer asked and as every Linux
    program does. The original uses it for *zoom to selection*
    (`research/04 §4.4`); that command will need another key.
12. **Shifted letters are bound in both cases** in the shell: the layout
    reports Ctrl+Shift+Z as `"Z"`.
13. **Constrained move snaps to multiples of 45°**, projecting the drag on
    that direction (the original's default constrain angle).
14. **Momentary switch cancels the gesture** (rule 5, explicit variant);
    suspension/resume is not implemented. No key is bound to momentary
    switching yet: `Intent::MomentaryTool` works, the shell's
    press/release plumbing for Space/Alt+X/Alt+Z/Alt+S is open.

## Provisional values (observe in the VM before trusting)

- Drag threshold 4 device px; double click 500 ms / 6 px; pick tolerance
  3 device px; auto-scroll band 16 px, step ≤ 24 px/frame.

## Invariants that must not be broken

1. `ToolCtx` has no `&mut Document` (compile-fail doctest).
2. Nothing is emitted before `DragEnd`; `Cancel` leaves the canonical
   digest and the history length unchanged (`tests/tools.rs`, 10 cut
   points).
3. A drag is one undo step, labelled ("Move").
4. The machine, not the tool, clears the preview on commit and cancel.
5. `edit.tool.drag_from` mirrors `ToolMachine::is_pressed`; the shell's
   shortcut gate reads it.

## Dead ends (do not retry)

- **Fitting scroll bounds after every mutation** — see decision 10.
- **Benchmarking "one edit" on the first selectable object of the synthetic
  document**: it is a layer-sized group of thousands of nodes (1.5 ms per
  undo pair through the bus). The bench now moves the smallest object.

## Open TODOs

- [ ] Precise hit testing (W3): `tool::pick` is a bounding-box test over
      top-level objects; swap in `xarast-geom`'s `HitIndex` behind the same
      signature. Leaf (Ctrl) and under (Alt) picking.
- [ ] Momentary tool keys in the shell (Space, Alt+X/Z/S on press, restore on
      release).
- [ ] Double-click opens the creating tool (T2.6); `Click.count` is there.
- [ ] Dual state, scale/rotate handles, nudges, snapping (W4, W8).
- [ ] Undo of a large group move (thousands of nodes) costs ~1.5 ms: the
      per-node `Transform` inverse; a subtree-level action would fix it.
- [ ] Cursor per state is wired (tool cursor over the canvas); leaf/under/
      snapped cursors are W3.
- [ ] Infobar W/H editing (the 9-anchor grid, W4) and bump buttons.
