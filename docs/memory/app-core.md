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

`Modifiers` is the semantic triple `constrain` / `adjust` /
`alternative` plus `snap`, never `Ctrl`/`Shift`/`Alt`. The shell owns
that mapping table. They are sampled continuously, never latched at drag
start.

### 2. `EditState`, `Viewport` and `Session` are public and `Send`

A `const` block in `lib.rs` asserts `Send` for `EditState`, `Viewport`,
`Session`, `AppState` and `Intent`, so a change that breaks it fails to
compile here rather than in the shell. They are `Send`, not `Sync`: the
document lives on one thread and the render thread gets an immutable
display list (architecture §5).

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
| `viewport` | `Viewport`, `ZoomTarget`, `page_rect`, `spread_rect`, `drawing_rect`, `nodes_rect` |
| `intent` | `Intent`, `Changed`, `PointerButton`, `PointerSample` |
| `paint` (private) | document fill → `xarast_render::Paint`, document transparency → `Transparency` |
| `walker` | `SceneWalker`, `WalkStats` — the arena→`Scene` walk |
| `commands` | `SetLayerVisible`, `SetLayerLocked`, `RenameLayer`, `SetActiveLayer`, `AddLayer`, `DeleteNode` |
| `session` | `Session`, `DocumentId`, `FileKind`, `Dirty`, `SessionError`, `build_scene`, `BuiltScene` |
| `headless` | `render`, `render_to_png`, `convert_to_png`, `HeadlessOptions` |
| `prefs` | `Preferences`, `Unit`, `ThemePref`, `RendererPref` |
| `app` | `AppState`, `DocumentSessions`, `DiagnosticLog` |
| `render_thread` | `RenderThread`, `RenderRequest`, `FrameJob`, `RenderedFrame`, `RenderStats`, `FrameRenderer`, `CpuFrameRenderer` — the one channel to the render thread |

Tests: 11 viewport, 11 session/selection/command, 8 corpus (the 59 real
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
14. **`FileKind` is the Phase 6 seam.** `.xar` import is wired;
    `.xarast` open and every save return `SessionError::Unsupported`
    with the reason in the message. Writing `.xar` says "a permanent
    non-goal" and a test asserts that wording, because that is a
    decision, not a gap.
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
    never published over a newer one. A rasterisation in progress is not
    interrupted (the CPU backend renders a frame as one call).
20. **`FrameJob` carries the `Resolver`, which the §U5.5 sketch omits.**
    `Arc<DisplayList>` + `Arc<Resolver>` + `ViewParams` + pasteboard and
    page colours — invariant 6 applied across threads. `Resolver` and
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
   `xarast_doc::Command` dispatched through `Session::dispatch`.

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
- [ ] **`Tiling::Repeat` never becomes `Repeat::RepeatHq`.** The
      high-quality repeating mode exists to stop a tiled gradient
      banding; deciding when to use it is a quality question nobody has
      measured yet.
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
- [ ] **`ContentHash` is coarse**: the node's tag plus the document
      epoch, so any edit invalidates every cached node. Correct — it
      only over-invalidates — but the per-node render cache cannot pay
      for itself until this is a real content hash. Phase 7.
- [ ] **Hit testing is a bounding-box test.** `Session::select_in_rect`
      exists so the selection plumbing is real and testable;
      `xarast_geom::hit_fill`/`hit_stroke` and the click path are
      Phase 7.
- [x] **Render-thread protocol** (XARA-T-0002): `render_thread`, see
      decisions 18–22. The running binary uses it (XARA-T-0003).
- [ ] **Draft/Final scheduling (U5.6) is not wired.** The viewer always
      renders `Final`; `Intent::SetQuality` exists, the 120 ms idle timer
      and `FrameRequest::RedrawAfter` are the pieces to join.
- [ ] **No surface reuse on the render thread.** Every frame allocates a
      fresh `Surface`; a return path (`recycle(surface)`) would save a
      canvas-sized allocation per frame.
- [ ] **The gradient-heavy corpus files stay slow interactively**
      (XARA-T-0014): 4–5 s from open to first frame in the window for
      `testfiles/*GradFilledShapes*.xar`. Supersession keeps the window
      responsive while they render, but each pan costs a full render.
- [ ] **`SimpleSphere.xar` renders its sphere black**, identically in the
      window and through `examples/render_headless.rs`, so it is a
      walker/renderer fidelity gap (gradient + transparency stack), not a
      composition bug.
- [x] **`xarast-cli` is wired** (XARA-T-0004). `xarast-cli render` and
      `xarast-cli smoke-open` use only the public API: `Session::open`,
      `Session::viewport`, `headless::render` and `Session::walk_stats`.
      Three API gaps are worked around in `crates/xarast-cli/src/render.rs`
      and tracked as XARA-T-0012:
      1. `HeadlessResult` has no `WalkStats`, so the CLI walks a second time.
      2. `HeadlessOptions` cannot express a fixed zoom or dpi, so the CLI
         sets `session.viewport` itself and passes `fit_drawing: false`.
      3. **`drawing_rect` includes the page nodes**, because
         `compute_bounds_with` gives `Page` its rectangle. "The drawing" is
         therefore pages ∪ ink, and `fit_drawing` frames the whole page
         around a small drawing. The CLI uses its own `ink_rect`: the
         children of the active spread's visible, non-guide layers.
- [ ] **Quick shapes with a cached path have zero-area bounds**
      (XARA-T-0013). `RedStar`, `Test00` and `TestBitmapFill` walk to one
      primitive, but the display list culls it, so they paint nothing.
      That is why the corpus has 38 files with scene primitives but 35
      with painted pixels.
- [ ] **No `criterion` bench yet** for `cargo bench -p xarast-app --
      viewport` (acceptance criterion 4). It needs the 100 000-object
      synthetic document from `xarast_doc::synth` and a scripted
      pan/zoom; the walk itself is already measured indirectly by the
      corpus test's 21 s for 59 files × 6 walks.
