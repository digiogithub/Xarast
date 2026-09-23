# tools

Memory note for **editing and tools** (phase 7): the command bus seen from
the application, the tool machine, the tools, the palette and the infobar.
Phase spec: [`../phases/phase-07-tools-and-editing.md`](../phases/phase-07-tools-and-editing.md)
(§W1, §W2). The document-side substrate (actions, `Tx`, `History`,
revisions) is in [`document-model.md`](document-model.md) decisions 7, 19,
34, 35; the shell/UI contract in [`app-core.md`](app-core.md).

## Current state (2026-09-23, round 3: XARA-US-0034)

| Piece | Where | State |
|---|---|---|
| `EditCommand` (`TransformNodes`, `DeleteNodes`, `CreateShape`, `SetShapeParams`), `CommandSink` | `xarast-app/src/ops.rs` | done; labels Move/Scale/Rotate/Skew/Transform, Create Rectangle/Ellipse, Edit Shape |
| Layer-lock refusal in every command | `ops.rs` `on_locked_layer` | done |
| Line-width scaling (`scale_line_widths`) | `ops.rs` | done: ×√|det| on every `LineWidth` in the subtree, inherited width pinned as a first child |
| `Session::apply_edit`, gestures, labels; selects a created shape; `ToolRequests::tool` | `session.rs` | done |
| `ToolMachine`, `ToolCtx` (+ `picker`, `grabs`, `enclosed`), `Preview`, `Infobar` (`Measure`/`Angle`/`Toggle`/`Anchor`), `InfobarValue`, `Anchor` | `tool.rs` | done |
| Precise picking: `Picker` over `xarast_geom::HitIndex` + `HitShape` | `picking.rs` | done (lazy rebuild; T-0152) |
| Selector: click, Adjust-toggle, dual state, scale/rotate/skew/centre drags, move, marquee, infobar X/Y/W/H/anchor/lock/angle/scale-lines, double click → creating tool | `selector.rs` | done |
| Rectangle, Ellipse (draw, Ctrl square/circle, Shift from centre, radius handle, W/H/Radius infobar) | `shapes.rs` | done |
| Push, Zoom | `tools.rs` | done |
| Shape editor (F4): node/handle select, marquee, move, reshape, add/delete point, line/curve, smooth/cusp, close, break, join, X/Y, even-odd | `node_edit.rs` | done (US-0034, T6.1–T6.7) |
| Pen (Shift+F5): click corner, drag smooth, continue an end, close, Enter/Esc | `pen.rs` | done (T6.8) |
| Freehand (F3): every sample, chunked incremental fit, smoothing slider, Shift rub-out, closed = filled | `freehand.rs` | done (T6.9, T6.10) |
| `SetPath`/`CreatePath`/`ConvertToPaths`/`SetWindingRule`, `PathEdit` labels | `ops.rs` | done |
| `Tool::action` + `ToolAction`; `AppCommand::Action` (L C S Z B J Enter), `ConvertToShapes` (Ctrl+Shift+S), Backspace = Delete | `tool.rs`, `command.rs` | done |
| Current attributes | `edit.rs` `CurrentAttributes`, `Intent::SetCurrentAttribute` | done (no UI to set them yet) |
| Keys | `AppCommand` + shell | as before, plus `3` = Zoom to selection; momentary Space / Alt+S (selector), Alt+Z (zoom), Alt+X (push) |
| `--probe drag|scale|rotate|rect|ellipse|nodes|pen|freehand` | `xarast-shell` | done |

Tests: `crates/xarast-app/tests/transforms.rs` (18: dual state, scale
corner/aspect/centre, live Ctrl mid-scale, line widths, rotate with
constrain + angle field, centre drag + snap, skew, the 72-case anchor
table, rectangle/ellipse creation, square/circle, from-centre, current
attributes, Esc mid-draw, radius handle + W/H/Radius typing, parametric
survives transforms, double click, locked layer), `tests/picking.rs` (4),
unit tests in `selector.rs`, `shapes.rs`, `ops.rs`; shell:
`input/momentary.rs` (3), viewer `held_switch_keys_change_the_tool_until_released`.

Live window (GardenPlan.xar, 2056×1286, GPU tiles, 100 frames each,
input → presented p50/p99): drag 2.5/4.9 ms, scale 2.5/6.7 ms, rotate
1.8/4.4 ms, rect 0.8/3.2 ms, ellipse 1.0/1.5 ms. `cargo bench -p
xarast-app --bench pick` at 100 000 objects: warm precise pick 2.3 µs;
first pick after a change (index rebuild) 62 ms.

Measured: `cargo bench -p xarast-app --bench undo` — undo+redo of a move of
one object at 250 000 nodes **339 µs the pair** through `Session` (≈0.17 ms
each; budget 1 ms), 314 µs through the bare bus. Live window
(`--probe drag`, GardenPlan.xar, 2056×1286, GPU tiles): input → presented
p50 2.1 ms, p99 7.8 ms over 100 drag frames, each a preview scene rebuild.

Round 3 (XARA-US-0034): `tests/node_edit.rs` (21: node selection incl.
Adjust/Adjust+Constrain/marquee/select all/Esc, `Arc::ptr_eq` on point
selection, node drag one step + undo exact, 45° constrain, Esc at 10 cut
points, smooth-handle sync + cusp independence, segment reshape, add/delete
exact on a line, continuity on curves, delete-all deletes the object, every
path operation and full undo, double click, typed X, winding rule,
rectangle never converted silently + Convert, pen lines/drag/close/Esc/
continue, selector double click on a path), `tests/freehand.rs` (4: the
4000-sample 200 Hz stroke, no sample dropped, rub-out, Esc + closed
stroke); geometry proptests in `xarast-geom/tests/path_edit.rs`.
Live window (GardenPlan.xar, 2056×1286, GPU tiles, 100 frames, input →
presented p50/p99): nodes 2.9/14.7 ms, pen 3.4/8.9 ms, freehand
3.2/9.8 ms.

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
    suspension/resume is not implemented. Keys: decision 26.

15. **Ctrl+Shift+Z stays Redo** (open decision, not ours to change);
    **Zoom to selection is `3`** (beside `1` = 100 %, where Inkscape keeps
    it). The original's chord is recorded in `research/04 §4.4`.
16. **The dual state lives in the selector, keyed by the selection.** The
    tool remembers the selection its mode, rotation centre and angle belong
    to; any other selection reads as scale mode, centre in the middle,
    angle 0. No session state, nothing undone. A click on the rotation
    centre is not a click on the object under it.
17. **Handle grabbing is in device space**, `HANDLE_TOLERANCE_PX` = 6, and
    handles beat objects: a press on a blob starts a transform even over
    another object.
18. **Transform facts** are `research/04 §4.10`: scale/skew fix the
    opposite blob, Adjust fixes the box centre, rotation turns about the
    centre, Constrain = aspect for scale and 45° steps for rotate and
    skew (the default constrain angle). The rotation centre snaps to the
    box's nine anchor points within the grab distance, and is carried by a
    move or a scale.
19. **Labels come from the matrix** (`ops.rs transform_label`): translation
    → Move, orthonormal (a half turn included) → Rotate, diagonal →
    Scale, unit diagonal with one shear → Skew, else Transform. No new
    command variants for each gesture; coalescing still keys on the label.
20. **Line widths scale by √|det|** (the geometric mean of the axis
    scales), only for scale gestures and W/H typing with "Scale lines" on
    (the default). An inherited width is pinned as a new first attribute
    child so the object keeps it after scaling. Rotate and skew never
    scale lines.
21. **The infobar is typed**: `InfobarValue::{Length, Angle, Toggle,
    Anchor}` travels in `Intent::InfobarEdit`. X/Y are the *anchor point's*
    coordinates (default anchor SW, so X/Y = left/bottom as before); W/H
    scale about the anchor, the padlock makes the other axis follow. The
    Angle field is "rotation applied through the selector since this
    selection was made"; typing sets it, rotating about the centre by the
    difference (`research/04` has no fact about what the original shows).
22. **Rectangles and ellipses are `QuickShape`s**, as the importer makes
    them (`shapes.rs` module docs; `research/04 §4.10`). The corner radius
    is `primary_curvature × |major|`, so a *scale* keeps the ratio (along
    the major axis the radius scales, across it not); typing W/H keeps the
    absolute radius. Answer to K8 by representation, pending VM
    observation.
23. **Creation preview is an overlay outline**, not a phantom node: the
    walker (owned elsewhere) has no phantom support. Radius drags hide the
    node (`Preview::hidden`) and draw the new outline.
24. **Current attributes** are session state (`EditState::current`),
    applied as the new object's own attribute children; empty = document
    defaults (black 0.25 pt outline, no fill).
25. **Picking is precise and lazy** (`picking.rs`, contract in
    `geometry.md`): leaves of visible unlocked layers, z = render rank <<
    16, fill tested only if painted, stroke only if the line colour is not
    none. `Session::after_mutation` invalidates; the next pick rebuilds
    (62 ms at 100k, off the undo path). Constrain = leaf, Alternative =
    under the selected object. Top objects with no testable leaf (text,
    unrendered live effects) fall back to their box. **A transparent
    interior no longer selects** — fixtures that click inside shapes must
    fill them.
26. **Momentary switching** (`xarast-shell/src/input/momentary.rs`): press
    raises `MomentaryTool(Some)`, release of the *same key* (whatever the
    modifiers by then) or focus loss raises `MomentaryTool(None)`; repeats
    are swallowed; Space needs no text field focused.
27. **Double click** on a rectangle or ellipse (quick shape or
    `ShapeNode`) with the selector selects it and chooses its tool
    (`ToolRequests::tool`); on a path it opens the shape editor.
28. **Locked layers** refuse `TransformNodes`, `DeleteNodes`,
    `SetShapeParams` (any node on one) and `CreateShape` (the target
    layer) with `EditError::NotPermitted`, document untouched.

29. **Behaviour source for the path tools is `research/04 §4.11`.** The
    shape editor does not draw (the original's Bézier tool does; here the
    pen draws). Handles show for a path's one selected node only, as in
    the original.
30. **Parametric shapes are never converted silently.** The shape editor
    edits paths only (the original's Bézier tool ignores rectangles,
    ellipses and quick shapes too); with one selected it draws its bounds
    dashed and its infobar offers *Convert to editable shapes*
    (`Ctrl+Shift+S`, `EditCommand::ConvertToPaths`: same `NodeId`, same
    attribute children, `NodeKind::Path` of `picking::geometry_of`).
31. **Point selection** stays `EditState` point indices; tools ask for a
    new one through `ToolRequests::points` (applied after the commands,
    so indices refer to the new geometry) and `created_points` (the pen's
    new end on a created path). `node_edit::Nodes` maps indices ⇄ nodes.
32. **Keys go to the tool first**: Delete/Backspace, Esc (when no gesture
    is in flight) and Ctrl+A become `ToolAction`s the tool may take
    (delete points, deselect points, select all points); otherwise the
    object-level command runs. L/C/S/Z/B/J/Enter are `AppCommand::Action`
    and do nothing in tools that ignore them.
33. **A node edit previews by hiding the path** and drawing its new
    outline and nodes over it (decision 23's pattern); one `SetPath` step
    at `DragEnd`, labelled by `PathEdit` (Move Points, Reshape Curve, Add
    Point, Delete Points, Make Line/Curve, Smooth/Cusp Points, Close Path,
    Break Path, Join Ends, Add Segment). Deleting every node deletes the
    object (label "Delete").
34. **Segment reshape** is ours: both handles move along the remaining
    offset, weighted `(1−t)` and `t`, scaled so the grabbed point lands
    exactly under the pointer (`node_edit::reshape`). The original uses a
    constant 0.656875 factor over `t`; not copied.
35. **Pen**: one undo step per segment (Create Path, Add Segment, Close
    Path), as the original. Its state is the selection (the one selected
    open end) plus a start point held by the tool; Enter/Esc finish by
    deselecting the end; Esc drops an unused start. Clicking an open end
    of a selected path picks it up (ours; the original needs the shape
    editor for that). A closed pen path is filled.
36. **Freehand tolerance** = `(64 + 160 × smoothing) / zoom` mp, the
    original's (`research/04 §4.11`); default 50. The machine replays the
    pointer positions seen below the drag threshold as `DragUpdate`s once
    a drag starts, so the fitter sees every sample (other tools ignore
    the extra updates). The preview fits in chunks of 96 samples (frozen
    prefix + bounded tail); the release refits the whole stroke.

## Provisional values (observe in the VM before trusting)

- Segment grab = 4 + 3 device px; freehand chunk 96 samples; freehand
  closes when its ends are within the 6 px grab distance.
- Drag threshold 4 device px; double click 500 ms / 6 px; pick tolerance
  3 device px; handle grab 6 device px; auto-scroll band 16 px, step ≤ 24
  px/frame; constrain angle 45°.

## Invariants that must not be broken

1. `ToolCtx` has no `&mut Document` (compile-fail doctest).
2. Nothing is emitted before `DragEnd`; `Cancel` leaves the canonical
   digest and the history length unchanged (`tests/tools.rs`, 10 cut
   points).
3. A drag is one undo step, labelled ("Move", "Scale", "Rotate", "Skew",
   "Create Rectangle", "Create Ellipse", "Edit Shape").
4. The machine, not the tool, clears the preview on commit and cancel.
5. `edit.tool.drag_from` mirrors `ToolMachine::is_pressed`; the shell's
   shortcut gate reads it.
6. A rectangle or ellipse stays a `QuickShape` through every transform and
   edit (`parametric_shapes_stay_parametric_through_transforms`).
7. The pick index never sees a drag frame: it is invalidated only by
   committed mutations.
8. Selecting path points never changes the path (`Arc::ptr_eq`,
   `tests/node_edit.rs`); a node drag, a pen segment and a freehand
   stroke are each one undo step.
9. The shape editor never changes a quick shape's kind except through
   `ConvertToPaths`.

## Dead ends (do not retry)

- **An anchor grid built from nested egui `horizontal` rows**: each row
  takes a full interaction height and pushed the whole infobar out of its
  28 pt row. Allocate the grid once and `interact` per cell.
- **Computing a probe's handle from the object nearest the centre**: the
  click selects whatever is on top at that point, so the blob belonged to
  another object and the "scale" probe moved instead. Pick first.

- **Fitting scroll bounds after every mutation** — see decision 10.
- **Benchmarking "one edit" on the first selectable object of the synthetic
  document**: it is a layer-sized group of thousands of nodes (1.5 ms per
  undo pair through the bus). The bench now moves the smallest object.

## Open TODOs

- [ ] **Incremental pick index** (`set_bounds`/`insert`/`remove` per
      committed transaction) to remove the 62 ms rebuild on the first
      click after an edit at 100k objects. Image alpha picking.
- [ ] Phantom-node preview (a filled shape while drawing) needs walker
      support (`Preview::phantom`).
- [ ] Constrain+Adjust while drawing is "square from the centre" here; the
      original's radius mode (rotates with the pointer) is not done.
- [ ] Observe in the VM: the radius under a non-uniform scale (K8), what
      the original's Angle field shows, the default of "scale lines".
- [ ] Undo of a large group move (thousands of nodes) costs ~1.5 ms: the
      per-node `Transform` inverse; a subtree-level action would fix it.
- [ ] Nudges, snapping (W4 T4.10, W8), flip and copy-and-transform (T4.6,
      T4.7), bump buttons, UI to set the current attributes (phase 8).
- [x] Double click on a path → shape editor.
- [ ] Path-point nudges (T6.11, needs the W4 nudge family), Tab/Home/End
      point cycling, mid-drag snapping of nodes (W8).
- [ ] Freehand: Alt straight segments, joining to a selected path's end,
      refitting the last stroke when the smoothing changes (retro fit),
      pressure. Pen: re-editing the start point's handle.
- [ ] Shape editor: editable paths inside blends/moulds, the neighbouring
      handles' X/Y in the infobar, reverse path, arrowheads; join across
      two path objects.
- [ ] The menu bar has no entries for the path operations yet (keys and
      infobar buttons only).
