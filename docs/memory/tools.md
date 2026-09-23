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
| `Tool::action` + `ToolAction`; `AppCommand::Action` (L C S Z B J Enter), `ConvertToShapes` (Ctrl+Shift+C, see decision 30), Backspace = Delete | `tool.rs`, `command.rs` | done |
| Current attributes | `edit.rs` `CurrentAttributes`, `Intent::SetCurrentAttribute` | done (no UI to set them yet) |
| Keys | `AppCommand` + shell | as before, plus `3` = Zoom to selection; momentary Space / Alt+S (selector), Alt+Z (zoom), Alt+X (push) |
| `--probe drag|scale|rotate|rect|ellipse|nodes|pen|freehand` | `xarast-shell` | done |
| Incremental pick index (journal → re-walk the touched top-level objects) | `picking.rs`, `xarast-doc` `Tree::drain_changes` | done (XARA-T-0168) |
| Group/ungroup (localise + factor-out), z-order ×6, align/distribute, duplicate, cut | `structure.rs` `StructureCommand` | done (XARA-US-0035) |
| Clipboard: fragment document + SVG flavour, internal/system paste, paste in place | `structure.rs`, `app.rs` `InternalClipboard`, shell | done |
| Alignment panel, Arrange menu, Edit/View items | `xarast-ui/src/menus.rs` | done |
| Snapping: `SnapSource`, resolver, grid, guides, objects (corners, then outlines), NumPad toggles, marker | `snap.rs`, `ToolCtx::snap_point`/`snap_move` | done (XARA-US-0036, T-0153) |
| Guides/grid as undoable document edits; ruler guides and grid settings wired | `snap.rs` `GuideCommand`, shell `ui_intent` | done |
| `--probe snap|arrange|paste` | `xarast-shell` | done |

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

Round 3 (2026-09-23): `bench pick` "undo + first pick after it" 2.26 ms
per undo+pick+redo+pick (≈1.1 ms per edit and pick; was 62 ms);
`bench structure`: group 5 000 + undo 10 ms, align+distribute 5 000 +
undo 1.7 ms (budgets 50 ms). Live, GardenPlan, 2056×1286, GPU tiles:
`--probe snap` (grid + guide + object snapping) p50 1.99 / p99 4.22 ms;
`--probe arrange` p50 2.32 / p99 4.32 ms; `--probe paste` (isolated GNOME
46 session, 939×708) p50 2.45 / p99 3.25 ms. Tests: `tests/structure.rs`
(10), `tests/structure_corpus.rs` (41 files), `tests/snapping.rs` (6),
`tests/picking.rs` (+1), `snap.rs` unit (4), `xarast-geom/tests/nearest.rs`
(3), `xarast-doc` journal and move-cost tests.

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
    (`Ctrl+Shift+C`, `EditCommand::ConvertToPaths`: same `NodeId`, same
    attribute children, `NodeKind::Path` of `picking::geometry_of`).
    **Key (XARA-US-0084):** the original binds it to Ctrl+Shift+S
    (`research/04 §4.11`, line 599), but Ctrl+Shift+S is Save As in every
    other desktop program and Xarast keeps that convention; Convert takes
    Ctrl+Shift+C, which is Inkscape's "Object to Path" and was free.
    `command.rs::no_two_commands_share_a_key` and the shell's
    `every_chord_of_the_command_table_reaches_its_command` guard clashes.
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
37. **Incremental pick index** (XARA-T-0168). `Action::apply` journals
    `(node, parent-at-the-time)` into the tree; the session drains it into
    the picker after every mutation (undo/redo included) and the next pick
    removes and re-walks only the touched *top-level objects* (layer
    children). Z is per top: `key + leaf ordinal`, tops `2^24` apart, new
    keys halve the gap to the nearest indexed neighbour. Rebuilds instead
    on: journal overflow (2^16 entries, a fresh tree, a resource change),
    any layer/spread change (visibility and lock included), a loose
    layer-level attribute change, > ¼ of the tops touched, no key left, or
    a layer with no indexed object yet. Undo + first pick at 100k objects
    1.1 ms the pair (was a 62 ms rebuild).
38. **Structure operations move through one primitive** (`structure.rs`):
    inherited-before → move → inherited-after → an attribute child per
    differing slot (`localise`); grouping then `factor_out`s leading
    attributes every member sets identically. No normalisation after other
    edits (`research/02 §10.6`). Multi attributes are not materialised.
    Group goes right above the topmost member; ungroup puts members where
    the group was, so group∘ungroup of a contiguous run restores z-order
    (41 corpus files pixel-identical, `tests/structure_corpus.rs`).
39. **Send to back goes after the parent's leading attributes**; forward/
    backward skip attribute siblings; layer up/down go to the top of the
    next *editable* layer of the same spread. An operation with nothing to
    do returns `structure::NOTHING_TO_DO`, rolled back: no empty undo step.
40. **A move is not charged as a deletion** (`Tx::move_node`): the detach
    used to charge the whole subtree to the 128 MiB budget, so grouping a
    large drawing evicted its own undo step (dead end found on
    `ProbeX16.xar`).
41. **Clipboard.** Copy = `copy_fragment`: a new `Document` (source
    defaults and resources cloned) holding self-contained copies, kept in
    `AppState` as `InternalClipboard { fragment, svg }`; the shell puts the
    `.xarast` SVG profile on the system clipboard **as text** — `arboard`
    has no custom MIME types, so `image/svg+xml` is not offered (task
    filed). Paste asks the shell (`PlatformRequest::ReadClipboard`); the
    answer `Intent::PasteText` pastes our fragment when the text is our SVG
    or there is no clipboard (`None`), else reads the text as SVG; other
    text → a diagnostic. Plain paste centres in the view, paste in place
    keeps coordinates; pasted objects go on the active layer, selected.
    Across documents palette references are resolved to direct colours
    unless the target palette has the identical entry; bitmaps are
    imported (dedup by content, outside undo — unreferenced ones are swept).
    Measured in an isolated GNOME session: `read=Ok(12046) ours=true`; the
    text does not outlive the process there (no clipboard manager).
42. **Duplicate** copies each object right above itself (so it inherits
    what the original does) and offsets by `DUPLICATE_OFFSET` (10 pt
    right, 10 pt down, provisional).
43. **Snapping belongs to the gesture**: tools call
    `ToolCtx::snap_point(p)` (creation corners, scale blob, rotation
    centre) or `snap_move(nodes, bounds, delta)` (a move: nine anchors,
    smallest correction per axis; not while Constrain holds). Per axis
    nearest wins, ties by priority guide > object > grid; object
    candidates are points, a point candidate wins over an axis pair at
    least as far. Radius 8 device px (provisional). Grid lines every
    `spacing / subdivisions`, integer-exact. Object snap = the picker's
    `object_snap`: corners/edge middles/centres of leaves near the pointer,
    failing those the nearest outline point (XARA-T-0153); it goes through
    the index (a first version walked every object per frame).
44. **Switches are session state** (`EditState::snap`, default: guides on,
    grid and objects off). `Intent::ToggleSnap` re-delivers the last
    pointer as a modifiers change, so a mid-drag toggle re-evaluates at
    once. Guides and the grid are **document** state: `GuideCommand`
    (Add/Move/Delete/DeleteAll/SetGrid) is undoable and survives `.xarast`
    save/reopen; Show guides = the guide layer's visibility (hidden guides
    do not snap). The snap marker is `HandleShape::Snap` while dragging.

## Phase 8: the fill and transparency tools (XARA-US-0039)

| Piece | Where | State |
|---|---|---|
| `FillHandles` derivation (`fill_handles`), `HandleKind`, `Guide`, `stop_target` | `xarast-app/src/fill_handles.rs` | done (T8.3.1) |
| Device-space `hit_handle`, `FILL_PICK_RADIUS_PX` = 5, z-order stop > end > arm | `fill_handles.rs` | done (T8.3.2) |
| Overlay: `OverlayShape::Arrow`, `HandleShape::Fill{Blob,Centre,Stop}[Selected]`; UI `OverlayItem::Arrow`, `HandleKind::{FillBlob, FillCentre}` (stops reuse the `Fill` diamond) | `tool.rs`, `xarast-ui/src/overlay.rs`, shell `viewer.rs` | done (T8.3.3) |
| Shared handle sets for selected objects with equal fills (`fill_sets`) | `fill_tool.rs` | done (T8.3.4) |
| `FillLikeTool<K: FillKind>`, `GradFillTool` (F5), `TransparencyTool` (F6) | `fill_tool.rs`, `tools.rs`, `command.rs` | done (T8.4.1, T8.4.2) |
| Infobars: type (mutate), effect / blend mode, tiling, ramp mapping, bias, gain, stop position, stop transparency | `fill_tool.rs`; `InfobarItem::{Choice, Real}`, `InfobarValue::{Choice, Real}`; UI combo box and slider in `toolbar.rs` | done (T8.4.3, T8.4.4) |
| `EditCommand::Fill { edits: Vec<FillCommand> }` wrapping the `xarast-doc` fill commands | `ops.rs`, `fill_tool.rs` | done (fill half of XARA-T-0212) |
| `Preview::attrs` — attribute overrides the walker applies | `tool.rs`, `walker.rs` | done |

Tests: `tests/fill_tool.rs` (10: drag-out makes one "Set Fill" step and
undoes exactly; a handle drag leaves the digest untouched until release,
previews in pixels and commits pixel-identical to the preview; Esc on a
handle drag and on a drag-out leaves digest and history unchanged; double
click on the arm inserts a stop, drag moves it, Delete removes it; two
objects sharing a fill show one set and move together, and split into two
sets once they differ; type menu mutates; Constrain snaps to 15°; profile,
effect, tiling and mapping fields; the transparency tool's 0→255 drag-out,
blend mode and per-cent level; Shift drags a circular fill; a leaf object
previews as it commits); unit tests in `fill_handles.rs` (3: handle
counts per shape, hits at 5 %, 100 % and 3200 % zoom, z-order).

45. **A fill drag previews, it does not emit.** `phase-08 §W8.4` sketches
    live `MoveFillControl`s per mouse move plus a restore on Esc; that would
    break invariants 2 and 7 (nothing before `DragEnd`, the pick index never
    sees a drag frame). Instead the tool snapshots the fill at the press,
    recomputes it each frame with `xarast_doc::fill_edit::move_control`, and
    puts it in `Preview::attrs`; the walker draws each node with that value
    standing in for its own attribute of the slot. Release emits one
    `EditCommand::Fill` (`MoveFillControl`/`MoveStop` per node with the final
    point, so the step is labelled "Move Fill Handle"/"Move Fill Stop"); Esc
    has nothing to undo. The command's `drag` coalescing key stays unused by
    the tool.
46. **Walker override rule**: a node in `Preview::attrs` gets the value
    pushed right after its `EnterScope` (where the command would add the
    attribute), its own attribute child of that slot is replaced at its
    visit, and a leaf is painted inside a pushed scope holding the value.
    The scope fingerprint mixes a hash of the value so the node's content
    hash changes every frame. `headless::render` renders the preview too.
47. **Handle sets**: the selected objects are grouped by equal fill in force
    (`fill_in_force`, interior slot); a set's drag edits every node in it.
    Outline (stroke) fills have no handles yet.
48. **Drag-out** (`research/04`, `tools/filltool.cpp` OnClick): a drag not on
    a handle makes a new fill of the infobar's type over the pressed object
    (selecting it) or over the selection; flat → linear; Adjust (Shift) →
    circular. Colours: a flat colour runs to itself at zero saturation (the
    W8.2 mutation rule; black/white when already grey), a gradient keeps its
    ends and ramp; transparency 0 → 255 (`Kernel/opgrad.cpp:2796-2802`),
    mix mode unless the object already has one. The end handle is selected
    afterwards, as the original does. The original's "double click then
    drag = conical" is not done (the machine does not report a press after a
    click as such).
49. **Stops**: a double click on an arm inserts a stop with the ramp sampled
    there (Fade), selected; Delete removes the selected stop
    (`ToolAction::Delete`, before object deletion); Esc with a handle
    selected deselects it. The selected stop keeps its identity across a
    re-sort (`ramp_move` returns the new index).
50. **Constrain during a handle drag** turns the handle about the arm's other
    end (or the centre, or the three/four-colour origin) in 15° steps
    (T8.3.6's snap-to-15°; axis and aspect locks not done). Snapping goes
    through `ToolCtx::snap_point`.
51. **Tiling in the infobar is "Simple"/"Repeating"**: Repeating writes
    `Tiling::RepeatExtra` for a graduated fill (the only mapping that tiles
    one, `research/01 §8.3`) and `Tiling::Repeat` for three/four-colour
    fills. Repeat-inverted only renders for bitmaps and is not offered.
52. **Blend modes offered**: the nine `TranspMode`s other than `None`
    (`TRANSP_MODES`); phase 8 lists Hue as a tenth, which `TranspMode` does
    not have yet.

### Shortcuts added (`research/04 §4.2–4.4`)

| Keys | Command | Note |
|---|---|---|
| Ctrl+X / Ctrl+C / Ctrl+V | Cut / Copy / Paste | Backspace, Shift+Del, Ctrl+Ins, Ins, Shift+Ins not bound |
| Ctrl+Shift+V | Paste in place | |
| Ctrl+D | Duplicate | Ctrl+K (clone) not implemented |
| Ctrl+G / Ctrl+U | Group / Ungroup | |
| Ctrl+F / Ctrl+B | Bring to front / Send to back | |
| Ctrl+Shift+F / Ctrl+Shift+B | Forward / Backward one | |
| Ctrl+Shift+U / Ctrl+Shift+D | Layer up / down | |
| Ctrl+Shift+L | Alignment panel | |
| NumPad . / NumPad 2 / NumPad * | Snap to grid / guides / objects | work mid-drag (`works_in_drag`) |
| `#` | Show grid | |
| NumPad 1 | Show guides | keypad only (`ChordKey::NumPad`), so `1` stays 100 % |
| F5 / F6 | Fill tool / Transparency tool | `research/04 §4.4` line 640 |

Clashes: none with the path tools' plain L/C/S/Z/B/J/Enter, Backspace and
Ctrl+Shift+S (XARA-US-0034), checked by `no_two_commands_share_a_key`.
Known ones stay: Ctrl+Shift+Z is Redo (original: zoom
to selection, here `3`); `<`/`>` for undo/redo not bound. Ctrl+D is
duplicate while plain `D` is fit drawing — different chords.

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
10. A fill or transparency drag emits nothing before release; the release
    is one `EditCommand::Fill` step, and what the preview drew is what the
    commit renders (pixel-identical, `tests/fill_tool.rs`).

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

- [ ] Fill tools (phase 8): stroke-slot handles; keyboard nudge of a fill
      handle (T8.3.5); axis/aspect lock during a handle drag (rest of
      T8.3.6); status-line text and per-target cursors (T8.4.6); dropping a
      palette colour on a stop or the arm (W8.7); the Hue blend mode; the
      original's double-click-then-drag conical; linear `end2` (skew) handle;
      bitmap-fill handles (phase 10). Profile slider drags produce one undo
      step per change outside a gesture (XARA-T-0220).

- [x] **Incremental pick index** (decision 37). Image alpha picking
      is still open.
- [ ] `image/svg+xml` and PNG flavours on the clipboard (needs a
      data-control / X11 selection writer beside `arboard`), pasting
      images; Inkscape paste checked manually per release.
- [ ] Guide properties dialog; Delete all guides has no menu item yet;
      snapping of the shape editor's nodes (W6) and of guide drags.
- [ ] Clone (Ctrl+K), duplicate-offset preference, Paste attributes.
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
