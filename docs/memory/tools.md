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
| Text tool (F8): caret, selection, visual/logical bidi navigation, pending point/column story; `Intent::TextNav`; text stories pickable by their line box; selector double click on text → text tool | `text_edit.rs`, `text_tool.rs`, `picking.rs` | done (XARA-US-0047, T9.4.1–T9.4.4 + the T9.4.5 gestures); typing T9.4.6 done; IME composition and the text clipboard (T9.4.7–T9.4.8) done (XARA-T-0224, decision 63) |
| Text infobar (font, size, B/I/U, align, spacing, tracking), OpenType panel, text ruler; `EditCommand::SetTextAttr`; `ToolRequests::current` | `text_infobar.rs`, `text_tool.rs`, `ops.rs`, `tool.rs` | done (XARA-T-0225, T9.4.9–T9.4.10; decision 59) |

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
    **Text joins it (XARA-US-0049):** the same command converts a text
    story into a group of glyph outlines (`crate::convert`, `text.md`
    "Convert to shapes"), and a selected group converts every shape and
    story inside it, as the original's command does. The session runs
    `convert::ConvertCommand` (like `StructureCommand`, it reports
    `(before, after)` pairs through a `RefCell`) and reselects: a story's
    group replaces it in the selection, shapes keep their id. Nothing
    convertible → `NOTHING_TO_DO`, no undo step. `EditCommand::
    ConvertToPaths` shares `convert_nodes` for tools that emit it.
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

## Phase 9: the text tool (XARA-US-0047)

53. **The text tool (XARA-US-0047)** is a state machine over two states
    (`text_tool.rs` module docs): **Story** (a `TextSelection { story,
    anchor, head }` of byte offsets with affinity, tool state only; the
    model's `TextCursor` is derived on demand with `cursor(doc)`) and
    **Pending** (`{ at, column }`: where typing *will* create a point
    story or a column). No story is created by a click or a column drag:
    the first typed character will (T9.4.6), so a stray click leaves no
    empty story and no undo step (the original creates the story at once
    and deletes it if left empty). Gestures: click text → caret at the
    nearest stop (Shift+click extends in the same story); drag on text →
    select; double click → word (UAX #29 segment, spaces included);
    triple click → the laid-out line; click empty canvas → pending point
    story at the click (the baseline origin); drag on empty canvas →
    pending column of the drag's width, first line hanging from the drag's
    top (narrower than 8 px → point story); Esc mid-drag restores the
    caret as it was. Esc with no drag leaves the text, story still
    selected. Ctrl+A selects the story's text. Delete, Backspace and
    Enter belong to the text while a caret is up (decision 57), so they
    never delete the story object. Choosing the tool
    with one story selected enters it with the caret at the end; the
    selector's double click on a story does that (decision 27).
54. **The tool lays stories out itself**, with the walker's own function
    (`text::lay_story`: bridge, `layout_text`, and on a path the path's
    column plus the `PathFit`) with the attributes resolved at the story
    (`resolve_inherited`), cached per story and dropped wholesale when
    `Document::epoch` moves. Hit testing walks every story on a visible,
    unlocked, non-guide layer (topmost wins), inverse-maps the point with
    the story matrix and asks `CaretMap::hit_point` for the caret and the
    distance to the text (line boxes, or fitted cluster boxes on a path),
    with a 4 px tolerance. Fonts are the process's shared service
    (`TextTool::with_fonts` for pinned fonts).
55. **Text is picked by its line box.** The pick index adds each
    `TextStory` as one `Geometry::Bounds` leaf (the `story_rect` of its
    laid-out lines, with the attribute stack in force) and skips its
    subtree: before this, the selector could not click text at all (text
    items have no geometry and no cached bounds).
56. **Navigation keys are an intent, not `ToolAction`s**: `Intent::TextNav(
    TextNav { key: TextKey, word, extend })` → `Tool::text_nav`, offered
    only while `Tool::text_editing()` is `Some`. Ctrl = by word
    (arrows), to the story's ends (Home/End); Shift = extend. Mapping and
    shell routing: `ui.md` decision 40.
57. **Typing (T9.4.6, XARA-T-0223)** is `Intent::TextInput(TextInput {
    kind: Insert(text) | Backspace { word } | Delete { word }, time_ms })`
    → `Tool::text_input`; the tool emits `EditCommand::TypeText { story,
    replace, text, burst }`, `DeleteText { story, range, burst }` or, at a
    pending caret, `CreateText { layer, story, attrs, text, burst }`
    (active layer, current attributes). **One undo step per burst**: the
    burst number is the command's `CoalesceKey` (`gesture: burst`, kind
    `"text-typing"` or `"text-delete"`), so the history's existing
    merge-on-equal-key does it — no gesture is opened and nothing is
    time-based inside the history (the phase doc's "one implementation, in
    the history"). The tool decides the burst: the next key joins when it
    is the same kind, in the same story, at the caret the last one left,
    **less than 500 ms** (`TYPING_BURST_MS`) after it, with a collapsed
    caret, and the document's epoch is still the one the tool saw right
    after its own edit. The last check is what makes "any other command
    ends the burst" (undo included) hold without the session telling
    tools: **`Tool::after_commands(doc, created)`** is called by
    `run_tool` after the tool's commands applied, with the object a
    creation made. The text tool uses it to note the epoch and to adopt the
    story `CreateText` made (the caret moves into it). Burst numbers come
    from a process-wide counter, so two documents never share one. Setting
    the caret any other way (`TextTool::set`) ends the burst. Labels:
    "Typing", "Delete Text", "New Text" (a merged step keeps its first
    label, so undoing a new story's first burst says "Undo New Text" and
    removes the story too). `ToolAction::Delete` (Edit › Delete) deletes
    forwards and `ToolAction::Finish` breaks the paragraph, never joining a
    burst.
58. **Text on a path in the tool (XARA-T-0250).** The overlay is built
    from story-space geometry the `CaretMap` computes
    (`caret_segments`, `selection_quads`), mapped by the story matrix
    only; the tool no longer builds carets or highlights from
    `caret_geometry`/`selection_rects` itself. So on a path the caret is
    the fitted cluster's edge, turned (and sheared) with the glyph, the
    highlight is one `OverlayShape::Highlight` quad per selected cluster
    (plus one for a selected paragraph break), and no shell or `xarast-ui`
    change was needed (`Caret` was already any segment, `Highlight` any
    quad). Caret motion (arrows, Home/End, Up/Down with a goal x) stays in
    the straight layout: bending a line changes neither logical nor
    visual order.

59. **Text attributes from the infobar (XARA-T-0225, T9.4.9–T9.4.10).**
    The text tool describes font family (`InfobarItem::FontFamily`,
    `InfobarValue::Choice` = index into the list it offered), size
    (`InfobarItem::Scalar`, points), bold/italic/underline (`Toggle`),
    alignment (`Choice`), line spacing and tracking (`Scalar`), the
    OpenType panel (`InfobarItem::Features`, `InfobarField::TextFeature(tag)`
    + `Toggle`) and, for a straight story with a caret, the tab kind choice
    and `InfobarItem::TextRuler` (drawn by the UI on the horizontal ruler,
    raising `TextLeftMargin`/`TextRightMargin`/`TextFirstIndent`/
    `TextTabAdd`/`TextTabMove(i)`/`TextTabRemove(i)`). All through the
    existing `Intent::InfobarEdit`, so no shell change. **Where an edit
    goes**, in order: a selection in a story → one `EditCommand::SetTextAttr`
    step (label from the slot: "Bold", "Font Size", "Tab Stops"…); a caret →
    a paragraph attribute at once on its paragraph, a character attribute
    into the tool's **pending style** (document untouched, bar shows it),
    which the next `TypeText` gets through a `SetTextAttr` with the same
    burst key (**one undo step "Typing"**); any caret change drops it; a
    pending caret keeps its style (any attribute) and `CreateText` gets it
    over the current attributes; no caret but stories selected → each whole
    story, all in one step (a shared fresh burst key); nothing selected →
    `ToolRequests::current` (new, additive) and the session sets the current
    attributes. An attribute edit ends a typing burst. An edit whose value
    is already what the bar shows emits nothing (no empty undo steps). The
    ruler commits **on release**, one step per drag.
60. **Invariant 11 widened**: the text tool mutates the document only by
    typing and by attribute edits the user asked for; choosing an attribute
    at a caret changes nothing until text is typed.
63. **IME and the text clipboard (XARA-T-0224, T9.4.7–T9.4.8).** New tool
    hooks, all defaulting to "not taken": `Tool::text_preedit(Option<
    Preedit>)` (from `Intent::TextPreedit`), `Tool::text_clipboard(
    TextClipOp::{Cut, Paste(Arc<StyledText>)})`, `Tool::text_copy(doc)`
    and `Tool::text_caret(view)` (the caret's segment even while it is
    hidden, for `Session::ime_cursor_area`). `Preview` gained `text:
    Option<TextPreview>`: a composition is drawn through the preview, so it
    never touches the document or the undo history (`text.md`, "IME
    composition and the text clipboard"). **While a caret is up, Copy,
    Cut and Paste are the text's** (`AppState::copy`, `Intent::Cut`,
    `paste_text` check `Session::text_editing()` first): Copy/Cut with a
    collapsed caret do nothing — never the story object under it; the
    copy is kept as `AppState::text_clipboard` (at most one of it and the
    object clipboard is set) and its plain text goes out through
    `SetClipboardText`; Paste still asks the shell (`ReadClipboard`) and
    the answer pastes our styled copy when the text matches it (or there
    is no clipboard), the text as plain text otherwise, and refuses our
    own object SVG with a status line ("leave the text (Esc) to paste
    them"). Edits: `EditCommand::PasteText { target: PasteTarget::{Story,
    New}, text }` ("Paste", no coalescing) and `CutText` ("Cut"); the
    session treats `PasteTarget::New` as a creation (selected, reported to
    `after_commands`).
70. **A story emptied by deletion is removed (XARA-T-0237, maintainer's
    decision 2026-09-24).** `DeleteText`, `CutText`, and `TypeText`/
    `PasteText` replacing a range with nothing, remove the story *inside
    their own transaction* when it is left holding only its final break
    (`xarast_doc::remove_empty_story`), so the removal is part of the step
    that emptied it and merges with the rest of a Backspace burst: one
    `Ctrl+Z` restores story and text (the original gets the same effect by
    merging its story deletion into the previous operation). The tool
    predicts the emptying when it emits (the range covers the whole laid-out
    text and nothing is inserted: `Emptying::of`) and, in
    `after_commands`, finding the story gone: the caret becomes **pending
    at the story's origin** (its matrix translation; a column keeps its
    width, rotation is not kept) and gets the removed text's attributes that
    differ from the current ones as its **pending style**, so typing on
    makes a story that looks like the old one; a story **on a path** leaves
    its path as an ordinary shape (as the original keeps it,
    `tools/textops.cpp:4281-4293`) and editing ends — there is no straight place for a caret. Selection:
    the story is pruned from it (the session prunes after every command),
    the freed path is not selected (the original deselects it). **Leaving
    the text (Esc, a tool switch, a click elsewhere) removes nothing**: that
    keeps invariant 11 (leaving is no edit, no hidden undo step), and the
    only stories that can be left empty-looking are ones holding nothing but
    paragraph breaks (typed Enter), which the original would remove on
    leaving — a deliberate difference.

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
| F8 | Text tool | `research/04` (`TOOL21`, F8); arrows/Home/End/PgUp/PgDn (+Ctrl, +Shift) move the caret while one is up |

Clashes: none with the path tools' plain L/C/S/Z/B/J/Enter, Backspace and
Ctrl+Shift+S (XARA-US-0034), checked by `no_two_commands_share_a_key`.
Known ones stay: Ctrl+Shift+Z is Redo (original: zoom
to selection, here `3`); `<`/`>` for undo/redo not bound. Ctrl+D is
duplicate while plain `D` is fit drawing — different chords.

## Phase 8: colour drops (XARA-US-0042)

Numbered after phase 9's decisions because the colour bar landed later.

61. **The fill tools report their selected handle** (XARA-US-0042):
    `Tool::fill_selection` (default `None`) → `FillSelection { channel,
    nodes, handle }`, read through `ToolMachine::fill_selection`. The
    colour bar's click and the colour editor use it to target that stop
    (`colour.md` decisions 27, 32); the tool itself is unchanged.
62. **`Picker::pick_drop`** is the colour-drop variant of `pick`: leaf
    mode, an unpainted closed interior counts as the fill (leaves keep
    `interior: Option<FillRule>`), an outline is hit within its half-width
    or 3 px. `pick`, `enclosed` and object snapping are unchanged.

## Phase 10: bitmap fills and placing bitmaps (XARA-T-0271, XARA-T-0272)

| Piece | Where | State |
|---|---|---|
| Bitmap fill virtual points, `move_bitmap_control`, `set_bitmap_dpi`, `bitmap_fill_dpi`, `natural_length`; commands `MoveBitmapControl`, `SetBitmapTiling`, `SetBitmapDpi` | `xarast-doc/src/bitmap_fill.rs` | done (T10.3.1) |
| Bitmap fill handles (centre + edge middles; four corners in perspective), `stop_target` for a contone pair | `fill_handles.rs` | done (T10.3.2) |
| Fill tool on bitmap fills: drag, Constrain about the centre, Adjust aspect lock, tiling with Repeat inverted, `BitmapDpi` field, Natural size button (`ToolAction::NaturalSize`) | `fill_tool.rs`, `tool.rs` | done |
| `default_bitmap_attrs`, `bitmap_node_centred`, `PlaceBitmap` | `xarast-doc/src/bitmap_place.rs` | done (T10.3.6) |
| `Intent::ImportImage` (drop), `Intent::PasteImage` (clipboard picture), `Session::place_image`, `place` module (decode once, store, natural size, `bitmap_pixels`) | `place.rs`, `app.rs`, `session.rs`, `intent.rs` | done (T10.3.8) |
| Shell: image files in a drop are placed at the drop point; Paste falls back to the clipboard's picture | `xarast-shell/src/viewer.rs` | done |

Tests: `tests/bitmap_fill.rs` (6: handle positions and arms; an edge drag
previews without writing, commits one "Move Fill Handle" step
pixel-identical to its preview, undo and redo exact; the centre moves the
whole fill and Esc leaves nothing; Adjust locks the aspect; tiling offers
Repeat inverted and writes both places; natural size and a typed
resolution resize about the centre, undo exact); `tests/place_bitmap.rs`
(4: paste at natural size in the view with the default attributes, one
exact step, decoded by the walker; a drop centred on its point at the
file's own 300 dpi; a non-image and a malformed picture change nothing;
the same picture twice shares one resource); unit tests in
`xarast-doc/src/bitmap_fill.rs` (6), `bitmap_place.rs` (1),
`xarast-app/src/place.rs` (4); shell: viewer tests (4: a synthetic drop
places at the drop point and still opens a `.xar` beside it, off-canvas
goes to the view centre; a non-image drop; Ctrl+V with a fake clipboard
picture is one undo step; our own copy wins over a picture) and
`tests/bitmap_damage.rs` (2: every preview frame and the commit of a
bitmap fill drag, and a placement, repaint only their object and match a
full render byte for byte).

64. **Bitmap fill handles are virtual points** (facts:
    `Kernel/opgrad.cpp:3701-3747`, `Kernel/fillattr.cpp:14041-14215`).
    The fill stores three corners (`origin` = bottom-left, `axis_x` =
    bottom-right, `axis_y` = top-left); the canvas shows the centre (moves
    the whole fill, `FillHandle::Centre`), the middle of the x-axis edge
    (`End`) and of the y-axis edge (`End2`). A drag moves one virtual point
    and the corners are rebuilt from the three (`bitmap_real_points`,
    exact in integers), so the centre stays while an edge handle turns and
    stretches its axis. `move_control` on a bitmap fill delegates here, so
    `MoveFillControl` means the same thing. A fill **in perspective** shows
    its four corners (`Start`, `End`, `End2` = the perspective's top-left,
    which `axis_y` follows, `End3` = top-right) and the centre of the
    quadrilateral — ours; the original's perspective bitmap handles were
    not researched.
65. **Constrain and Adjust on a bitmap edge handle**, as the original:
    Constrain turns it about the centre (in our 15° steps, decision 50);
    **Adjust locks the aspect** — the other axis turns with it, stays
    perpendicular and scales by the same ratio. Ours: the lock keeps the
    side the other axis was on, so a mirrored fill stays mirrored (the
    original always turns +90°). Because the lock changes the result, the
    commit is `MoveBitmapControl { lock_aspect }` (label "Move Fill
    Handle"), not `MoveFillControl`: the command must reproduce the preview
    exactly (invariant 10).
66. **Bitmap tiling is written in both places.** The renderer lets a
    fill's own `tiling` win over the mapping attribute (`image.md`), so
    `SetBitmapTiling` sets both and neither can contradict the other. The
    menu is Simple / Repeating / Repeat inverted for bitmap fills only
    (decision 51 still holds for the others); unset shows as Repeating, the
    original's default.
67. **Resolution and natural size.** `SetBitmapDpi` keeps the centre and
    the axis directions and gives each axis `pixels × 72 000 / dpi` mp
    (`natural_length`, 96 dpi when unknown); labelled "Natural Size" when
    the dpi is the image's own, "Bitmap Resolution" when typed. The field
    shows the horizontal resolution the fill renders at, rounded. The
    image's size comes from the resource's layout, or from a header probe
    of its bytes when the `.xar` importer left the layout empty
    (`place::bitmap_pixels`). Perspective fills are left alone.
68. **Placing an image** (T10.3.8) is `Intent::ImportImage { path, at }`
    (a drop) or `Intent::PasteImage { width, height, rgba }` (a clipboard
    picture), both handled by `AppState` so a refusal becomes a notice
    ("Could not place the image: …") that changes nothing. The image is
    decoded once under `DecodeLimits::default()`; PNG, JPEG and GIF keep
    their bytes, anything else is stored as a lossless PNG with its
    resolution in `pHYs`; a clipboard picture has none and is 96 dpi. The
    resource goes in **outside the history**, deduplicated by content, as a
    pasted fragment's bitmaps do (decision 41); then `PlaceBitmap` (one
    step, "Import Bitmap" or "Paste") puts a `BitmapNode` of the natural
    size on the active layer, centred on the drop point or in the view, and
    the session selects it. The object carries the original's default
    bitmap attributes as its own children: **no line colour, no fill
    colour, zero line width** (`Kernel/nodebmp.cpp:997-1055`, facts) — not
    a bitmap fill as `research/02 §8.6` said; the object draws its image
    itself.
69. **Which drop is a placement, and which paste is a picture**
    (`xarast-shell/src/viewer.rs`). A drop with a document open places
    every file with an image extension (`place::is_image_path`) and opens
    the rest as before; the drop point is mapped through the canvas region,
    and a point off the canvas means "the view's centre". Paste asks the
    clipboard for a picture only when its text is neither our own copy nor
    SVG and no text caret is up; our copy therefore always wins.

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
11. The text tool never mutates the document except by typing and by
    infobar/ruler attribute edits: entering, selecting, navigating,
    preparing a new story and choosing a character attribute at a caret
    leave the canonical digest and the undo history as they were
    (`tests/text_tool.rs`, `tests/text_infobar.rs`); a click on empty
    canvas never creates an empty story, and leaving the text never
    removes one. A deletion that empties a story removes it in that same
    undo step (decision 70).
12. While a text caret is up, Delete/Backspace/Enter never reach the
    object-level commands.
13. A typing burst is one undo step and one `Ctrl+Z` restores the digest
    from before it, story creation included
    (`typing_200_characters_is_one_undo_step`,
    `the_first_character_at_a_pending_caret_creates_the_story`).
14. An infobar attribute edit is at most one undo step, and none when the
    value is already in force; at a caret it merges into the typing step
    it styles.
15. A bitmap fill handle drag emits nothing before release and commits
    one step whose render equals the preview (`tests/bitmap_fill.rs`); a
    placed bitmap is one step and adds at most one resource
    (`tests/place_bitmap.rs`). Undo of a placement restores everything but
    the resource table, which is outside the history (decision 68).

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
      original's double-click-then-drag conical; linear `end2` (skew) handle.
      Bitmap-fill handles are done (decision 64); a bitmap fill's
      stroke slot and the original's perspective bitmap handles are not.
      Profile slider drags produce one undo
      step per change outside a gesture (XARA-T-0220).

- [x] **Incremental pick index** (decision 37). Image alpha picking
      is still open.
- [ ] `image/svg+xml` and PNG flavours on the clipboard (needs a
      data-control / X11 selection writer beside `arboard`); Inkscape
      paste checked manually per release. Pasting a *picture* is done
      (decision 68); pasting a copied image *file* (a file manager's
      `text/uri-list`) is not.
- [ ] Dropping a bitmap on an object to make it that object's bitmap fill
      (the original's bitmap drag), and the gallery's drag-to-place, belong
      with the bitmap gallery (XARA-US-0055).
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
