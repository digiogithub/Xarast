# app-core

Memory note for the **application core** (`crates/xarast-app`), Phase 5.
Arbitrated decisions in [`../10-architecture.md`](../10-architecture.md) §2, §3.4,
§3.5b, §4, §5. Phase specification:
[`../phases/phase-05-shell-and-ui.md`](../phases/phase-05-shell-and-ui.md).

`xarast-app` sits between the document model and the renderer, and below
both consumers. It has **no UI toolkit dependency and no windowing
dependency**, it builds and tests with no GPU and no compositor, and
`xarast-ui` and `xarast-shell` depend on it without depending on each
other.

---

## The contract the other Phase 5 crates build against

This is the part to read before writing a line of `xarast-ui` or
`xarast-shell`. It is deliberately small.

### 1. `Intent` in, `Changed` out

```rust
let changed: Changed = session.apply(intent)?;     // or app.apply(intent)
```

[`Intent`] (`intent.rs`) is the **only** input vocabulary. The shell
raises them from platform events; the UI raises the same ones from menus,
panels and buttons. Neither crate has to know the other exists, and
neither reaches into `EditState`, the `Viewport` or the arena directly.
It is `#[non_exhaustive]`: Phase 7 adds variants without breaking either
caller.

[`Changed`] is a bitflag answer — `VIEW`, `SELECTION`, `DOCUMENT`, `UI`,
`CACHE` — with `needs_redraw()` and `needs_scene()` on top. A shell that
ignores it repaints every frame; a shell that honours it parks. An
intent that changes nothing returns `Changed::empty()`, and that is
checked by a test, because "did anything happen" is the question an
idle-CPU budget rests on.

**Application-level intents** (XARA-US-0082): `ShowOpenDialog`,
`OpenFile(path)`, `CloseDocument`, `ClearRecent`, `Quit`. `AppState::apply`
handles them itself; a `Session` ignores them. The two only the platform
can do (`ShowOpenDialog`, `Quit`) are queued as `PlatformRequest`s that the
shell drains with `AppState::take_requests()` and carries out — the core
never shows a dialog or ends the process. `Changed::ACTIVE` says a
different document (or none) now has the canvas: re-title, re-size, frame.

**The command table** (`command.rs`, phase-05 W4.7): `AppCommand` names
each menu operation with its `label()`, its `shortcuts()` as
toolkit-neutral `KeyChord`s (first = the one a menu shows),
`needs_document()` and `intent(canvas_centre)`. The menu bar draws it,
the shell binds keys from it; both therefore raise the same intents.

`Modifiers` is the semantic triple `constrain` / `adjust` /
`alternative` plus `snap`, never `Ctrl`/`Shift`/`Alt`. The shell owns
that mapping table. They are sampled continuously, never latched at drag
start.

### 2. `EditState`, `Viewport` and `Session` are public and `Send`

A `const` block in `lib.rs` asserts `Send` for `EditState`, `Viewport`,
`Session`, `AppState` and `Intent`, so a change that breaks it fails to
compile here rather than in the shell. They are `Send`, not `Sync`: the
document lives on one thread and the render thread gets an immutable
scene and resolver snapshot (architecture §5).

### 3. Scene building

```rust
// Per frame, in the session that owns the walker's caches:
session.rebuild_scene(dirty: Option<DeviceRect>) -> Result<SceneStats, _>
session.scene()      -> &Scene
session.resolver()   -> &Resolver
session.view_params() -> ViewParams

// From a shared reference, allocating afresh:
xarast_app::build_scene(&session, dirty: Option<DeviceRect>) -> BuiltScene
```

`build_scene` is the entry point the phase document names. It returns
**`BuiltScene`, not `Scene`**, and that is deliberate: a `Scene` is *not
self-contained*. Its paints hold `RampId`s and `ImageId`s into a
`xarast_render::Resolver`. Hand the scene to a backend without its
resolver and every gradient renders transparent. `BuiltScene` derefs to
`Scene`, so `&*built` is a `&Scene` wherever one is wanted, and it also
carries the `ViewParams` and the `SceneStats`.

Prefer `Session::rebuild_scene` per frame: it reuses the walker's
interned ramps, its image registry, its attribute cache and the scene's
allocation. `build_scene` builds all of that from nothing each call.

### 4. Units and the one place the Y axis flips

Millipoints in, device pixels out. Document space is integer millipoints
with **`y` up**; device space is `f64` pixels with **`y` down**. The flip
belongs to `Viewport` and to nothing else — neither `xarast-doc` nor
`xarast-render` has an opinion about which way up a document is, and
giving a second component one is how drawings come out mirrored. `DPI`
likewise has exactly one owner: the shell computes it from the fractional
scale factor and calls `Intent::SetDpi`.

---

## Current state

| Module | Holds |
|---|---|
| `geometry` | `DevicePoint`, `DeviceSize`, `DocPoint`, `DocPointF`, `DocRect`, the saturating `f64 → Mp` quantiser |
| `edit` | `EditState`, `ControlPoints`, `SelectMode`, `Modifiers`, `ToolId`, `ToolState`, `selectable_objects` |
| `viewport` | `Viewport`, `ZoomTarget`, `page_rect`, `spread_rect`, `drawing_rect` (no pages), `drawing_or_page_rect`, `content_rect`, `nodes_rect` |
| `intent` | `Intent`, `Changed`, `PlatformRequest`, `PointerButton`, `PointerSample` |
| `command` | `AppCommand`, `KeyChord`, `ChordKey`, `ZOOM_STEP` — the command table |
| `recent` | `RecentFiles`, `MAX_RECENT` (10), `default_store_path` (`$XDG_STATE_HOME/xarast/recent`) |
| `paint` (private) | document fill → `xarast_render::Paint`, document transparency → `Transparency` |
| `walker` | `SceneWalker`, `WalkStats` — the arena→`Scene` walk |
| `commands` | `SetLayerVisible`, `SetLayerLocked`, `RenameLayer`, `SetActiveLayer`, `AddLayer`, `DeleteNode` |
| `session` | `Session`, `DocumentId`, `FileKind`, `Dirty`, `SessionError`, `build_scene`, `BuiltScene` |
| `headless` | `render`, `render_to_png`, `convert_to_png`, `HeadlessOptions`, `HeadlessFrame`, `HeadlessResult` (with `WalkStats`, `SceneStats`, zoom and view) |
| `prefs` | `Preferences`, `Unit`, `ThemePref`, `RendererPref` |
| `app` | `AppState` (with `open_replacing`, `with_recent_store`, `take_requests`), `DocumentSessions`, `DiagnosticLog` |
| `render_thread` | `RenderThread`, `RenderRequest`, `FrameJob`, `RenderedFrame`, `FrameReuse`, `RenderStats`, `FrameRenderer`, `CpuFrameRenderer` — the one channel to the render thread, and the worker that reuses pixels |
| `reuse` (private) | the pixel-reuse policy: `plan`, scroll, nearest-neighbour rescale, the zoom-out ring, Final columns |
| `ops` | `EditCommand` (transform, delete, create shape, set shape params), `CommandSink`, `on_locked_layer` — the commands tools emit (phase 7) |
| `tool` | `ToolMachine`, `Tool`, `ToolCtx`, `GestureEvent`, `Preview`, `Infobar`, `InfobarValue`, `Anchor`, `OverlayShape`, `pick` |
| `picking` | `Picker`, `PickMode`, `HitResult`, `HitPart` — precise picking over `xarast_geom::HitIndex` |
| `selector` | the unified selector: dual state, scale/rotate/skew, infobar |
| `shapes` | the rectangle and ellipse tools and quick-shape helpers |
| `tools` | `builtin()`, push, zoom and pending tools |
| `schedule` | `QualityScheduler` (the Draft → Final policy, clock injected), `Canvas` (it joined to a `RenderThread` and a `Session`), `Backdrop`, `FINAL_AFTER` |

Tests: 13 viewport, 11 session/selection/command, 8 corpus (the 59 real
`.xar` files, through `XARAST_XAR_CORPUS`, never copied into the
repository). `examples/render_headless.rs` is the whole headless path in
one file.

**Measured against the corpus.** All 59 files import, walk into a
balanced scene and render headless with no GPU: 264 339 primitives in
total, 38 files with ink, 21 blank — and every blank one is blank for a
reason the walker *reports*, which is its own test.

---

## Decisions taken (and why)

1. **`Viewport` is parameterised by zoom and centre, not by an
   accumulated matrix.** A matrix multiplied through a thousand pan and
   zoom events drifts and eventually shears; two numbers cannot. The
   transform is derived on demand and is the only one in the
   application.
2. **Document `y` is up and the flip lives in the viewport.** See the
   contract above.
3. **Zoom is "one document inch covers `dpi` device pixels at 100 %".**
   Clamped to 1 %–25 000 %, as the original's range. Every hostile input
   — `NaN`, infinity, zero, a zero-sized viewport — is ignored rather
   than propagated, and a test asserts the viewport is bit-identical
   afterwards.
4. **The selection is an `IndexSet`, not a `HashSet`.** Selection order
   is user-visible: "align to the last object selected" and the key
   object both depend on it, and an unordered set makes the overlay
   order flicker between frames.
5. **`EditState::prune` runs after every mutation.** It is the whole
   price of keeping the selection out of the arena, and it is linear in
   the *selection*, not in the document. A control-point overlay may
   only exist on a selected node, so deselecting drops it.
6. **The scene id is the node's `Tag`, not its `NodeId`.** `Tag` is
   unique, stable across save and reload, and deterministic; a `NodeId`
   is an arena slot and would make the scene depend on allocation order.
   This is what makes `the_walk_is_stable_across_runs_and_across_walkers`
   pass.
7. **Ink is painted at `LeaveScope` when a node has children and at
   `Visit` when it has none.** A `.xar` path stores its fill as its own
   child and a parent paints after its children, so the child scope must
   still be in force when the parent's ink is emitted. `RenderWalk`
   emits scope events only for nodes that have children, so the two
   cases never overlap — the protocol `xarast_doc::walk` documents.
8. **A transparency ramp borrows its id from the colour ramp cache.**
   `Resolver::transparency_ramps` is a parallel `Vec` indexed by the same
   `RampId` space, so interning one means interning a *colour* ramp whose
   stops are the transparency levels as greys and writing the real table
   at that index. Two transparency ramps with the same levels share an
   id, which is the point; a collision with a real grey gradient is
   harmless, because the two tables are read by different code paths and
   both are correct.
9. **`push_transparency`/`pop_transparency` only wrap a non-opaque
   object.** Wrapping every object would double the op count for
   nothing.
10. **An invisible fill emits no command at all.** "No colour" in the
    model is a flat colour with zero alpha; emitting it would put a
    no-op in every display list and in every tile bin.
11. **The walker takes `&Document` and a test asserts the canonical
    digest is unchanged.** The signature makes mutation impossible by
    assignment; the digest is what would catch it happening through an
    interior-mutability cache. Bounds are read from the cache where it
    is warm and computed on the stack where it is not — filling the
    cache would make rendering an edit.
12. **`commands::SetActiveLayer` uses `Tx::act`, not `Tx::set_kind`.**
    See "Invariants" below; this is the one real friction with
    `xarast-doc` found in this phase.
13. **A guide layer may not become the active layer.** The model's own
    repair never elects one, so letting a command do it would produce a
    document the next repair silently undoes.
14. **`FileKind` is the Phase 6 seam.** `.xar` import and `.xarast` open
    are wired (`.xarast` since W4, 2026-09-23: `Session::open_bytes` →
    `xarast_format::open_reader` over the bytes; container and reader
    diagnostics go into `Session::diagnostics`; a refusal is
    `SessionError::Xarast`). `Session::open`, argv and File › Open all go
    through `open_bytes`; the Open dialog lists "Xarast documents
    (*.xarast)" first. Every save still returns
    `SessionError::Unsupported` with the reason in the message. The
    session does not keep the package reader yet: the save path will want
    `OpenedDocument::package` for `save_opened`'s raw copies. Writing
    `.xar` says "a permanent non-goal" and a test asserts that wording,
    because that is a decision, not a gap.
15. **Session state is flat and public.** `Session`'s fields are `pub`
    because reading them is the whole point; mutation is still funnelled
    through `dispatch`, `undo`, `redo` and `apply`, which are the only
    things that keep `modified`, the prune and the dirty tracking
    consistent.
16. **`RendererPref` lives here even though `xarast-shell` owns the
    capability ladder.** It is the *persisted user preference*; the
    shell converts. Keeping it here is what lets the headless tools and
    the window agree on it.
17. **Nothing is serialised in this crate.** Preferences have no on-disk
    form yet: Phase 6 owns that, and inventing one here would be a
    format to migrate later.
18. **The render-thread channel is a latest-wins mailbox, not a queue**
    (`render_thread`, XARA-T-0002). One slot each way: a submitted frame
    replaces the waiting one (`RenderStats::superseded`), a finished frame
    replaces an uncollected one (`dropped`). That *is* the backpressure —
    at most one frame waits and one is in flight — and `submit` never
    blocks. Built on `std::sync::{Mutex, Condvar}`; `crossbeam-channel`
    (named in `phase-05 §U2.7`) was not needed and would have been a
    queue to fight.
19. **Generations are allocated by `RenderThread::submit`** from a
    main-thread counter, strictly increasing. `RenderRequest::Cancel {
    up_to_generation }` drops the waiting frame and makes the worker
    discard an in-flight result instead of publishing it; a result is
    never published over a newer one. A `Final` is drawn in columns and
    abandoned between them for newer input (decision 27); a `Draft` is
    never interrupted.
20. **`FrameJob` carries the `Resolver`, which the §U5.5 sketch omits.**
    `Arc<Scene>` + scene epoch + `Arc<Resolver>` + `ViewParams` + ink
    rectangle + pasteboard and page colours — invariant 6 applied across
    threads. (It carried an `Arc<DisplayList>` until XARA-US-0005; see
    decision 24.) `Resolver` and
    `RampCache` derive `Clone` for this; `Session::resolver_snapshot`
    clones once per scene rebuild and hands the same `Arc` to every frame
    of a pan.
21. **The page is a backdrop the render thread fills, not scene ink.**
    `Session::frame_job(pasteboard, page)` derives the page's device
    rectangle through the `Viewport`, so the Y flip still has one owner.
22. **The renderer behind the thread is a trait (`FrameRenderer`).**
    Production is `CpuFrameRenderer` (interactive config); the tests use a
    gated renderer to hold a frame in flight deterministically, which is
    the only way to test supersession without sleeps deciding the result.
    It is called once per rectangle drawn (a strip, a column), not once
    per frame, and receives the display list the worker built.
23. **Draft while interacting, one Final after 120 ms idle**
    (XARA-US-0005, `schedule`). `QualityScheduler` is a pure state
    machine: `note(now, changed)` marks interaction (anything that
    `needs_redraw`) and returns the generation of a Final to cancel;
    `next(now, doc)` says `Submit(Draft|Final)`, `Wait` or `Idle`;
    `deadline()` is when the owed Final is due. Time is always passed in,
    so the tests use a synthetic clock and no sleeps. `Canvas` wraps it
    around a `RenderThread`: `note`, `pump` (rebuilds the scene if the
    session says so, submits, returns the deadline), `take_latest`,
    `is_settled`. **The first frame of a document is Final**: nothing is
    on screen to refine, and Draft-then-Final would be two full renders.
24. **The worker builds the display lists, so a job carries the scene.**
    Which list to build depends on what the worker holds (its kept
    frame), which the main thread cannot know without racing it.
    `Session` keeps its scene in an `Arc` and rebuilds in place when it
    is the only owner, else into a fresh one (never clone-then-clear);
    `scene_epoch` counts rebuilds so the worker can tell "same scene".
25. **Pixel reuse lives on the render thread** (`reuse.rs`). The worker
    keeps the last frame it published (and publishes a clone, ~1 ms at
    1080p). A pan by whole pixels scrolls it (`scroll_surface`) and
    draws only the exposed strips, at either quality, but a `Final` only
    reuses pixels that are themselves exact `Final` pixels. A `Draft` pan
    by a fractional offset is **snapped** to whole pixels and the frame
    reports the snapped view; the `Final` redraws exactly. A `Draft` zoom
    resamples the kept frame (nearest) and paints only the backdrop into
    the border a zoom-out uncovers (decision 26). Anything else (new
    scene epoch, resize, dpi, colours, rotation) is a full frame.
    `RenderedFrame::{reuse, exact}` say what happened; the viewer's
    screenshot waits for an exact frame. Scrolled and column-drawn frames
    match a one-call render to within 1/255 (tests).
26. **A Draft zoom-out's border is left to the Final.** Rasterising it
    cost ~190 ms a frame over 100 000 objects: the border is short, wide
    strips, and the CPU backend runs a strip shorter than a band on one
    core (XARA-T-0034). With the backdrop there instead, a zoom frame is
    ~1.6 ms. Revisit when T-0034 lands.
27. **A Final is drawn in up to four full-height columns** (no narrower
    than 256 px), each its own display list, checking between columns
    for a cancel or a newer job. Columns, not row slabs: the backend's
    parallelism is across bands, and 192-row slabs doubled a 1080p Final.
28. **Strips outside the scene's ink are backdrop, with no list built.**
    `FrameJob::ink` is `viewport::content_rect` (whole-tree bounds, a
    superset of anything drawn, taken at rebuild) in device space plus
    2 px. A `DisplayList::build` scans every scene op however small the
    rect (XARA-T-0033), so not building is the only cheap build.
29. **Draft is a view parameter; the scene stays at the session's
    quality.** Re-walking at Draft quality would shorten ramps and switch
    images to nearest, but the walk is ~100 ms at 250 000 nodes, on the
    first and last frame of every gesture. So Draft frames only scale
    flatness (XARA-T-0035 moves the other two knobs into the view). The
    session's quality is still the ceiling: `Canvas::pump` never submits
    above it.
30. **Headless framing is an enum** (XARA-T-0012): `HeadlessFrame::
    {Session, FitDrawing, Fit(rect), Fixed { zoom, centre_on }}` plus
    `dpi: Option<f64>`, and `HeadlessResult` returns `WalkStats`,
    `SceneStats`, the zoom and the `ViewParams`. `drawing_rect` now means
    the drawing without the pages (the active spread's visible, non-guide
    layers); `drawing_or_page_rect` is what fitting frames;
    `content_rect` keeps the whole-tree bounds for culling.
31. **A fill-mapping value is interpreted per fill family, not at face
    value** (XARA-T-0037, `research/01 §8.3` "How the mapping renders").
    `Tiling` holds the original's values 0–4 (`None`, `Simple`, `Repeat`,
    `RepeatInverted`, `RepeatExtra`). In `paint.rs`:
    * `gradient_repeat`: a linear, radial, conical or diamond fill clamps
      unless the value is `RepeatExtra`, which becomes `Repeat::RepeatHq`;
    * `mesh_repeat`: a three- or four-colour fill clamps only on `Simple`,
      and otherwise tiles, the default included;
    * `repeat_of`: a bitmap fill takes the value as is.
    The original's factory default is `Repeat` (2), and a gradient ignores
    it. Taking it at face value wrapped every gradient into hard bars
    (SimpleSphere, Fill Types, leafgirl's horizon).
32. **A circular fill's minor axis is the major one turned a quarter
    turn** (`paint::radial_frame`). The record has no second axis, and the
    importer repeats the edge point. The frame was degenerate, so every
    circular fill painted nothing.
33. **`drawing_rect` includes half of each object's line width**, from its
    own attribute stack (`viewport::ink_rect`, used when the layer's
    cached box is cold, which is always after import). It is not the
    culling extent, which allows for a full mitre spike and is four times
    as wide. Fitting with a zero extent cut thick strokes off at the image
    border.
34. **A bitmap fill with no decoded image counts in `images_pending`**, as a
    bitmap node does, so a file never looks complete while it is not.
35. **A rendered frame says what it covers and what is new**
    (XARA-T-0050). `RenderedFrame` carries `scene_epoch`, `covered` (the
    rectangle holding picture, not placeholder backdrop), `fresh` (what
    was rasterised: the viewport, a scroll's strips, nothing for a
    rescale) and `base` (the generation a scroll moved pixels from). A
    presenter that already holds `base` uploads only `fresh`; one that
    never saw it (dropped, or not collected) uploads all of `covered`.
    The kept frame carries its generation and cover for this.
36. **A presenter that resamples at input time turns CPU rescale off**
    (`FrameJob::cpu_rescale`, `Canvas::set_cpu_rescale`). The worker then
    skips a Draft whose zoom differs from its kept frame and publishes
    nothing (`RenderStats::skipped`, not counted in `rendered`); the
    Final after the gesture draws the new zoom whole. The viewer always
    sets it off: both canvas tiers composite retained tiles. Decision 26's
    backdrop border is therefore gone from the window; the headless tools
    keep the CPU rescale.

37. **Single-document model for the first usable viewer**
    (XARA-US-0082). `Intent::OpenFile` → `AppState::open_replacing`: the
    new session is opened *first* and the others are closed only once it
    succeeded, so a failed open leaves the current document on screen
    (the error is in the problem list and returned). A path that fails is
    dropped from the recent list. `DocumentSessions` still holds many;
    tabs are later.
38. **Recent files are state, not settings**: `$XDG_STATE_HOME/xarast/recent`
    (a relative or empty `XDG_STATE_HOME` is ignored, as the spec says),
    plain text under a `xarast-recent 1` header, one absolute path per
    line (raw bytes on Unix, so non-UTF-8 paths survive; paths with a line
    break are refused). Parsing never fails: an unknown header is an empty
    list, bad lines are skipped, at most 10 are kept. Written through a
    temporary file and a rename. Pruned of missing files at load. The
    store is opt-in (`with_recent_store`): tests and probes keep the list
    in memory and never touch the user's state directory.

---

## Invariants that must not be broken

1. **No UI toolkit, no windowing, no `unsafe`.** `#![forbid(unsafe_code)]`
   is in `lib.rs`, and the dependency list is `xarast-*` plus `indexmap`,
   `bitflags`, `thiserror` and `kurbo`.
2. **`EditState`, `Viewport`, `Session`, `AppState` and `Intent` stay
   `Send`.** Asserted at compile time in `lib.rs`.
3. **The walker never mutates the document**, and never fills a cache in
   it. Asserted by the canonical digest over the whole corpus.
4. **The walk is output-deterministic.** Same document, edit state and
   viewport → byte-identical scene, from a fresh walker or a reused one.
5. **The attribute stack is pushed and popped exactly once per scope.**
   `EnterScope` pushes, `LeaveScope` pops, an attribute node pushes a
   value. Never open-code it.
6. **A `Scene` is never handed anywhere without its `Resolver`.**
7. **Session state never enters the arena** (architecture §3.5b), and
   never enters the canonical digest or the undo log.
8. **A device rectangle passed as `dirty` prunes the walk and nothing
   else.** The scene is always rebuilt whole; culling may only ever
   remove work, which is a corpus test.
9. **Tools never touch the arena.** Every mutation is a
   `xarast_doc::Command` dispatched through `Session::dispatch`; tools
   emit `EditCommand`s through `ToolCtx::commands` and hold only
   `&Document` (compile-fail doctest in `tool.rs`).
11. **Scroll bounds are refreshed at `rebuild_scene`**, never in
    `after_mutation` (34 ms per undo at 250 000 nodes; `tools.md`).
12. **New intents** (phase 7): `Cancel`, `DeleteSelection`,
    `InfobarEdit` (typed `InfobarValue`), `AutoScroll`,
    `SetCurrentAttribute`; pointer intents now drive the `ToolMachine`,
    and `ChooseTool`/`MomentaryTool` switch its tool. A tool may ask for a
    tool change (`ToolRequests::tool`); a `CreateShape` selects what it
    created. `Session` holds a `Picker` invalidated in `after_mutation`.
10. **No two commands share a key chord** (`no_two_commands_share_a_key`),
    and the core never performs a platform action: it queues a
    `PlatformRequest`.

### The one friction with `xarast-doc`

`Tx::set_kind`, `Tx::attach`, `Tx::delete` and `Tx::move_node` each run
`keep_one_active_layer` afterwards, and that repair fires whenever the
number of active layers in a spread is **not exactly one**. Moving the
active flag necessarily passes through zero or two, so a pair of
`set_kind` calls can never move it:

* clear the old one first → zero active → the repair re-elects the first
  non-guide layer, which is the one just cleared;
* set the new one first → two active → the repair clears the new one.

`Tx::act` applies one `Action` and records its inverse **without** the
repair, so `SetActiveLayer` uses that. It is still correct by the
invariant's own terms: `document-model.md` invariant 19 says a
transaction may not *leave* the invariant broken, and this one does not.
`the_active_layer_stays_unique_and_undoes_with_the_rest` pins it,
including byte-identical undo.

Worth raising with whoever next touches `xarast-doc`: the repair would
be better run once at `Tx::commit` than after every call.

---

## Dead ends (do not retry)

- **Row slabs for an interruptible Final.** 192-row slabs cost 730 ms
  against 370 ms for one call at 224 000 primitives: each slab is one or
  two bands, so most cores idle. Use full-height columns.
- **One display list for the view, filtered per column.** Cloning the
  surviving `DrawCmd`s cost 65 ms at 224 000 primitives, more than
  building each column's list from the scene.
- **Re-walking the document with the strip as the dirty rect** to get a
  small scene for a pan strip: 30–130 ms per strip on the synthetic
  document, slower than the `DisplayList::build` scan it was meant to
  avoid. Group bounds there cover most of the page, so the walk culls
  late.
- **Re-walking the scene at `Draft` quality for interaction.** ~100 ms
  at 250 000 nodes, on the first and last frame of a gesture. See
  decision 29.
- **Deciding pixel reuse on the main thread.** It cannot know which
  frame the worker holds (superseded frames are never drawn), so any
  plan it makes can be for the wrong base. The worker decides.

- **`build_scene` returning a bare `Scene`.** It compiles and it renders
  nothing but solid colours, because the ramp and image ids in the
  paints point into a `Resolver` that was dropped with the walker. This
  cost an hour of "why are all the gradients transparent" before the
  type was changed to carry both.
- **Using `NodeId` as the scene id.** It is an arena slot: it is stable
  within a session, but it makes the scene depend on allocation order,
  which breaks the stability test and any cross-run scene diff.
- **A pair of `Tx::set_kind` calls to move the active layer.** See
  above. It silently does nothing, which is the worst kind of nothing.
- **Pruning a subtree because its *attribute* children missed the dirty
  rectangle.** Attribute nodes have no bounding box; they are never
  culled, and a node's own attribute children only ever applied to that
  node. Culling them would change the colours of the siblings.
- **Cloning `AttrValue` per attribute node per frame.** The stack wants
  an `Arc<AttrValue>` and `AttrNode` holds a bare `AttrValue`; the naive
  bridge clones a `Ramp`'s `Vec` on every frame for every gradient
  attribute in the document. The walker caches the `Arc` by `NodeId` and
  drops the cache when `Document::epoch` moves.
- **Comparing "dirty rect the size of the viewport" with "no dirty
  rect" and expecting the same scene.** They differ, correctly: the
  viewport-sized rectangle culls everything off screen. Compare against
  a rectangle larger than the document instead.

---

## Open TODOs

- [ ] **Perspective gradients are mapped provisionally.**
      `fill::Perspective`'s `p2`/`p3` are taken as the images of `(0,1)`
      and `(1,1)`. Nothing in the corpus proves the ordering; a file
      with a perspective gradient and a golden image would.
- [ ] **`RampMapping::Sin` is ignored.** The renderer's `build_ramp`
      takes a bias/gain profile but no sine easing, so a sine-mapped
      gradient renders linear. Either bake the easing into the stop list
      here or add it to `xarast-render`.
- [x] **When a gradient becomes `Repeat::RepeatHq`**: exactly when its
      mapping is `Tiling::RepeatExtra`, which is also the only mapping
      that makes a gradient tile (decision 31).
- [ ] **Bitmap fills ignore the fill-mapping attribute** (XARA-T-0054).
      They use the per-fill `tiling` the importer leaves at `None`. The
      original tiles them by the attribute, whose default is repeat.
- [ ] **`FrameJob::ink` / `content_rect` use a zero stroke extent** when
      the cache is cold. A thick stroke reaching outside every page could
      be left out of a pan strip. It was not seen in the corpus, because
      pages cover the drawings. If it shows up, reuse `viewport::ink_rect`
      with the culling extent.
- [ ] **`ClipViewMode::Outside` drops the clip** and counts it in
      `WalkStats::clips_unsupported`. The renderer has no "keep the
      outside" clip; it needs either an inverted path or a mask layer.
- [ ] **Bitmaps render only when decoded.** The `.xar` importer keeps
      the encoded bytes and leaves `pixels` empty until Phase 10, so
      `WalkStats::images_pending` is non-zero on every file with a
      bitmap. The registration seam is `SceneWalker::register_images`.
- [ ] **Quick shapes with no cached path draw nothing**
      (`WalkStats::shapes_pending`). Generating the path from the
      parameters is Phase 7; `testfiles/RedStar.xar` and
      `testfiles/Test00.xar` are the corpus files that show it.
- [ ] **Text draws nothing** (`WalkStats::text_pending`). Phase 9. That
      is 13 of the 21 blank corpus files.
- [ ] **Live effects draw nothing** (`WalkStats::live_pending`).
      Phase 13.
- [x] **`ContentHash` is per node** (phase 7): tag + `content_rev` + a
      fold of the attribute scope's node versions + the resources'
      revision. An edit re-keys only what it touched and the siblings its
      attribute applies to (`tests/tools.rs`). See `tools.md`.
- [x] **Hit testing is precise** (XARA-T-0152, `picking.rs`): painted
      fill and stroke through `xarast_geom::HitIndex`; see `tools.md`
      decision 25. The index is rebuilt lazily after a change (62 ms at
      100k); incremental updates are open.
- [x] **Render-thread protocol** (XARA-T-0002): `render_thread`, see
      decisions 18–22. The running binary uses it (XARA-T-0003).
- [x] **Draft/Final scheduling (U5.6)** (XARA-US-0005): `schedule`,
      decisions 23–29, wired into `xarast-shell::viewer` (one commit, that
      file only). The status bar still shows a fixed "Final"; showing the
      scheduled quality is a `viewer.rs`/`xarast-ui` change.
- [ ] **Pan misses 16 ms when every strip has ink**: 51–57 ms at 3× zoom
      over 100 000 objects, of which ~35 ms is two `DisplayList::build`
      scans (XARA-T-0033) and ~15 ms raster of strips that run on one
      core (XARA-T-0034). See `perf.md`. Nothing more to gain here
      without those, short of overscan (render a margin round the view so
      most pan frames are a pure crop), which trades a 20–40 % costlier
      Final for occasional re-centring spikes.
- [ ] **The Draft rescale is nearest-neighbour and cumulative**: a long
      continuous zoom resamples resampled pixels. It is blurry by design
      and the Final replaces it, but a bilinear filter from the last exact
      frame would look better.
- [ ] **No surface reuse on the render thread.** Every frame allocates a
      fresh `Surface`; a return path (`recycle(surface)`) would save a
      canvas-sized allocation per frame.
- [ ] **The gradient-heavy corpus files stay slow interactively**
      (XARA-T-0014): 4–5 s from open to first frame in the window for
      `testfiles/*GradFilledShapes*.xar`. Supersession keeps the window
      responsive while they render, but each pan costs a full render.
- [x] **`SimpleSphere.xar` rendered its sphere black** (XARA-T-0025,
      XARA-T-0037). The cause was neither the walker nor the renderer: the
      importer took the file's `TAG_CURRENTATTRIBUTES` (a black current
      fill) for the document defaults, so the unfilled frame drawn last
      covered everything. The gradients also wrapped (decision 31).
      `tests/fidelity.rs` pins both on a synthetic `.xar`. Dead end: the
      first suspicion was the transparency stack.
- [x] **`xarast-cli` is wired** (XARA-T-0004), and since XARA-T-0012 it
      needs no workarounds: it frames through `HeadlessOptions`
      (`HeadlessFrame::Fit`/`Fixed` plus `dpi`) and reads the zoom and the
      `WalkStats` from `HeadlessResult`. The former gaps were no walk
      stats (the CLI walked twice), no fixed zoom/dpi (it set the session
      viewport by hand), and `drawing_rect` including the page nodes (it
      kept its own `ink_rect`).
- [ ] **Quick shapes with a cached path have zero-area bounds**
      (XARA-T-0013). `RedStar`, `Test00` and `TestBitmapFill` walk to one
      primitive, but the display list culls it, so they paint nothing.
      That is why the corpus has 38 files with scene primitives but 35
      with painted pixels.
- [x] **`cargo bench -p xarast-app -- viewport`** (XARA-US-0006,
      acceptance criterion 4): `benches/viewport.rs`, numbers in
      `perf.md`.
