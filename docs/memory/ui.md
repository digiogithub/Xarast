# User interface

Durable notes for `xarast-ui` and `xarast-shell`. Two workstreams write
here; each keeps to its own headings.

## UI toolkit

> Owner: the Phase 5 `xarast-ui` workstream (W1, W5 panels). The shell's
> own findings — winit, portals, clipboard, the `egui-winit` shim — live
> under their own headings in this file; nothing here rewrites them.

### Architecture §7 question 2 — **closed: egui is confirmed**

`egui 0.33`, used as a library and never through `eframe`. Every axis that
can be measured without a display passes, three with room to spare; the
four axes that need a GPU or a compositor are **unmeasured**, not assumed,
and are listed below as open TODOs.

Reproduce with `cargo run -p xarast-ui --release --example density`. The
probe is `xarast_ui::density`, so the same code backs the example, the
`criterion` bench (`cargo bench -p xarast-ui`) and the unit tests.

**The nine numbers.** Measured on the Phase 5 container: no GPU, no
compositor, window 2560×1440, 200 frames per measurement, probe
instantiating **1,789 controls per frame** over six panels, a 5,000-row
virtualised `TableBuilder` tree, a 512-swatch palette strip, a
2,000-thumbnail gallery and 48 drag-adjust unit fields.

| Axis | Threshold | Measured | Verdict |
|---|---|---|---|
| P1 `build_ui_frame()` CPU | p50 ≤ 3 ms, p99 ≤ 8 ms | **p50 1.63–1.65 ms, p99 2.19–3.40 ms** over repeated runs (this container has four slow cores and three build jobs on it) | **PASS** |
| P2 main-thread total | ≤ 8 ms every frame | **4.71–6.68 ms** = build p99 + tessellate p99; event handling and submit unmeasured | **PARTIAL PASS** |
| P3 egui GPU pass | ≤ 1.5 ms | **unmeasured** — no adapter in this environment | **UNMEASURED** |
| P4 5,000-row tree scrolled 10 s | no frame over 16 ms | **worst CPU frame 2.84–2.99 ms** over 600 scrolled frames | **PARTIAL PASS** (CPU side only) |
| P5 partial update (one slider) | within 1 ms of idle | **+0.00 to +0.05 ms** against idle | **PASS** |
| P6 resident growth, all panels | ≤ 50 MB | **9.3–9.4 MB** | **PASS** |
| P7 20 pt rows at 1×/1.25/1.5 | legible, screenshots | rows measure **20 / 25 / 30 device px**; build 1.77–1.79 ms at all three. Legibility itself **unmeasured** (no display) | **PARTIAL** |
| P8 AT-SPI tree | tree, fields and toggles present and labelled | **2,415 nodes, 1,752 labelled**; list ✓, numeric field ✓, toggle ✓ — *after* the fix below | **PASS** |
| P9 pointer → handle update | ≤ 2 frames at 60 Hz | **1 frame** of interface latency by construction (a pointer event and the overlay it moves are the same frame); presentation **unmeasured** | **PARTIAL** |

**Verdict: egui is confirmed and no fallback is taken.** Immediate mode is
not the cost: a frame that instantiates 1,789 controls costs 1.65 ms at
the median, which is a fifth of the 8 ms main-thread rule of
`research/05 §10.2`, and the partial-update measurement (P5) shows that
dragging a slider costs the same as an idle frame — the immediate-mode
objection in the abstract does not survive contact with the measurement,
because virtualisation, not retention, is what decides the cost. Fallback
1 (mitigate inside egui) was not needed, fallback 2 (custom-painted
panels) was not needed, and fallback 3 (`iced 0.14`) is **not taken**. The
`Panel` trait that would make fallback 2 possible exists anyway, because
it costs nothing and it is also how a panel is registered.

**Two findings that came out of the spike and changed the code.**

1. **AccessKit exposes no list, tree, table or row structure.** The first
   P8 census found the 5,000-row tree published as a flat run of
   `Button` nodes inside anonymous `GenericContainer`s: labelled, but
   structureless, so a screen reader reads a toolbar where a user expects
   a layer stack. egui derives a role from its own widget type and has no
   widget type for a row. The fix is `Context::accesskit_node_builder`,
   wrapped in `xarast_ui::a11y::{set_role, set_list_item, set_label}`:
   the node stays egui's, the role and the position-in-set are ours. Any
   panel with a list must do this — it is not automatic.
2. **An icon-sized control is anonymous.** `Checkbox::new(&mut v, "")` has
   no accessible name at all. Every such control in this crate now sets
   one explicitly (`Hide layer Sky`, not a drawn eye), and the
   `accessibility` integration test fails the build if any interactive
   node in the layer or colour panel has an empty name.

**Version note.** `research/05 §2.2` names egui 0.36.2; this workspace
pins stable **1.94** and `rust-version = 1.90`, while egui 0.36 declares
`rust-version = 1.95` and 0.34/0.35 declare 1.92. 0.33.3 is therefore the
newest version that builds here. The upgrade is two lines — the workspace
`rust-version` and the `egui`/`egui_tiles` versions — and nothing in the
spike's conclusion depends on the difference.

**Measured with, and to re-measure on hardware:** the four unmeasured or
partial axes (P3, P7, P9 and the presented halves of P2 and P4) need a
display. Until then, no claim is made about them in either direction.

## Panels and canvas

### Current state

`xarast-ui` builds, tests and benchmarks with no window, no GPU and no
compositor. `cargo test -p xarast-ui` runs 115 tests in about a tenth of
a second.

| Module | State |
|---|---|
| `workspace` | `Workspace::ui()` — the whole interface in one call: dock, canvas, status bar, theming. This is the shell's entry point |
| `canvas` | Region reservation and reporting in whole device pixels, transparent so the shell's canvas pass shows through, wheel/Ctrl-wheel/pinch/middle-drag/keyboard navigation, guide creation and dragging, page edge, grid and guides |
| `overlay` | Handles (bounds, rotate, node, fill, centre), lines, rectangles, dashes, hit-testing; device-pixel snapped |
| `rulers` | `Ruler::draw` plus the pure `ticks()`/`major_step()` used by the tests; 1-2-5 steps, imperial and pica subdivisions |
| `grid`, `guides` | Rectangular grid with subdivision dropout and snapping arithmetic; guides created by dragging from a ruler, moved, deleted by dropping back |
| `panels::layers` | Virtualised list, visibility, lock, active layer, reorder, rename, full keyboard operation |
| `panels::colour` | Colour line with "no colour", RGB/HSV/grey/CMYK editor over `xarast-color`, named colours marked |
| `panels::status` | Coordinates in the document's unit, zoom, quality, cache pressure, renderer tier |
| `panel` | `Panel` trait, `PanelId`, `UiHost` over `egui_tiles`, versioned `LayoutState` |
| `theme` | Dark and light token sets, density, WCAG-asserted contrast, live scheme change |
| `scale` | The one `Scale` of a frame, hairline and edge snapping, device rectangles |
| `units` | `10mm`, `1in`, `3p6`, `12mm + 3pt`, bumps, formatting that round-trips |
| `a11y` | Names and roles for what egui does not name itself |
| `density` | The spike probe, shared by the example, the bench and the tests |

**Stubbed or absent on purpose:** menus and the command palette (they need
the shortcut table, which is `xarast-app`'s W4.7), the problem list
(needs the diagnostics feed), galleries, and anything Phase 7 and later
own. No tool handles are produced — the overlay takes them, it does not
invent them.

### Decisions taken (and why)

- **The interface is a projection, not a model.** Each frame it reads a
  `UiModel` the application fills and writes `UiCommand`s into a
  `CommandSink`. Nothing in this crate mutates a document, a viewport or a
  selection. That is what makes every panel testable headlessly, and it
  is the seam that let this crate be written while `xarast-app` was being
  written next to it.
- **The canvas is not a dockable pane.** It shares the surface with the
  shell's passes; a pane can be dragged into a tab group, which the
  canvas cannot survive. The dock sits beside it.
- **The canvas region is transparent and reported in whole device
  pixels**, rounded *outwards*, so the document pass covers every pixel
  egui left clear. Rounding inwards leaves a seam.
- **The view transform is read-only here.** Pan and zoom are commands.
  There is exactly one owner of the transform and it is not the
  interface.
- **Handles are drawn with a light-on-dark outline pair, not XOR.** XOR is
  unavailable on a composited surface and looks wrong over antialiased
  content; the outline pair buys the same "visible on any background"
  property.
- **Hairlines snap to the centre of a device pixel, filled edges to the
  boundary**, after the `f64` transform, at every scale factor.
- **`LayoutState` is versioned and refuses an incompatible or unknown
  tree** rather than opening an empty window; a layout naming one missing
  panel still loads and says which panel went.
- **Focus and selection look different.** egui 0.33 paints a focused
  widget with `widgets.active`, so the focus ring is a two-point stroke in
  a `focus` token that is deliberately not the accent.
- **Labels, not icons, for state.** Partly accessibility, partly the
  clean-room rule: no icon of another program is copied, and any icon
  this crate ever needs is drawn with the painter.

### Invariants that must not be broken

- `egui` appears at `xarast-ui` and above, never below. This crate does
  not name `winit` or `wgpu`.
- No `unsafe`: the crate is `#![forbid(unsafe_code)]`.
- Every public item is documented (`#![deny(missing_docs)]`).
- A panel never keeps document state between frames. The only state a
  panel may keep is interaction state (which row is being renamed, which
  guide is being dragged).
- Every interactive widget has a non-empty accessible name; the
  `accessibility` test enforces it for the shipped panels.
- Ruler, grid and guide geometry is derived in document units and snapped
  to device pixels **after** the transform, and never cached across a
  scale change.
- Tick and grid-line generation is bounded (2,048 and 4,096 per axis) so
  that an absurd zoom cannot become an unbounded allocation.

### Dead ends (do not retry)

- **`egui::Checkbox` with an empty label as a compact toggle.** It is
  invisible to AT-SPI. Set the name explicitly.
- **Expecting `egui_extras::TableBuilder` to publish a table to
  AccessKit.** It does not; see the P8 finding.
- **Deriving `serde` on anything holding `xarast_geom::Mp`.** `Mp` has no
  serde support; `crate::serde_mp` adapts it as a raw count.
- **Asserting interaction in a single headless egui frame.** egui
  resolves hovering and dragging from the *previous* frame's widget
  rectangles, so a one-frame test asserts that the widget ignored an
  event it had not been told about. The canvas tests run a warm-up frame.
- **egui 0.34–0.36 on this toolchain.** They need rustc ≥ 1.92/1.95.

### Open TODOs

- Re-measure P3, P7, P9 and the presented halves of P2 and P4 on hardware
  with a display; until then they stay "unmeasured" in this note.
- `egui_kittest` **image** snapshots (phase criterion 17) need the `wgpu`
  feature and an adapter. The AccessKit-tree tests stand in for them
  here; add the image baselines when CI has `lavapipe`.
- **For the shell:** `accesskit_winit` must be the release built against
  the same AccessKit major as egui 0.33, which is **0.21** — the pinned
  workspace version. `accesskit_winit 0.34` pairs with AccessKit 0.25 and
  would put two incompatible `TreeUpdate` types at the shell↔UI boundary.
  Either pick the matching adapter or upgrade egui and both together.
- Bump to egui 0.36 when the workspace moves to rustc 1.95, and drop
  `egui_tiles` 0.14 for 0.17 at the same time.
- Wire the menus and the command palette when `xarast-app`'s shortcut
  table lands, and the problem list when the diagnostics feed does.
- Replace the crate-local `UiModel`/`UiCommand` adapter with a direct
  mapping onto `xarast_app::{AppState, Intent}` now that that crate has
  landed its API: `UiCommand` → `Intent` is a one-function translation,
  and `Unit`, `ZoomTarget` and the theme preference exist on both sides
  and should converge on the `xarast-app` spelling.

---

## Shell and platform integration

> Owner: the Phase 5 `xarast-shell` workstream (W2 shell foundations, W3
> Wayland/portals/clipboard/DnD, W4 input and tablet). The toolkit and
> panel findings above are the `xarast-ui` workstream's; nothing here
> rewrites them.

### Architecture §7 question 3 — **closed: do not pin the `winit` 0.31 beta yet**

**Verdict: stay on `winit 0.30.13`.** Deferred, not rejected — and it must
be re-opened before v0.1, because a drawing application without stylus
pressure is not finishable.

The phase document's decision rule is "pass E1–E7 → pin". **E1–E7 are all
runtime criteria and not one of them can be executed in this
environment**: there is no Wayland socket, no X11 socket, no GPU adapter,
no tablet and no `uinput`. Pinning a beta on the strength of evidence that
does not include running it would be exactly the claim this phase is not
allowed to make. What *could* be checked was checked, and is recorded
below so the next attempt starts from facts rather than from the
changelog.

**E1–E7: what was and was not measured.**

| ID | Criterion | Result |
|---|---|---|
| E1 | Build and run on GNOME/mutter, KDE/kwin, sway | **Builds**: `winit 0.31.0-beta.3` with `wgpu 30.0.1` on rustc 1.94.1, clean, 55 s, in a throwaway probe crate. **Running on any compositor: unmeasured.** |
| E2 | Pressure, tilt, twist on all three, real device or `uinput` | **Unmeasured.** The API and the Wayland implementation are present (see below); no device, no `uinput`, no compositor here. |
| E3 | Fractional scaling at 1.25 / 1.5 / 2.0 | **Unmeasured.** |
| E4 | CSD on GNOME via SCTK + `sctk-adwaita` | **Unmeasured.** The `wayland-csd-adwaita` feature still exists in 0.31 and compiles. |
| E5 | Redesigned `DragEntered`/`DragMoved`/`DragDropped`/`DragLeft` with `SendData::Uris` | **Unmeasured.** The events exist in `winit-core` 0.31. |
| E6 | `PinchGesture`, `PanGesture`, `RotationGesture` reach the viewport | **Unmeasured**, but the Wayland implementation exists: `winit-wayland-0.31.0-beta.3/src/seat/pointer/pointer_gesture.rs`. |
| E7 | Eight-hour soak, no leak, no protocol error | **Unmeasured.** |

**What the source says, which is the part that could be checked.**

1. `winit-core-0.31.0-beta.3/src/event.rs` really does carry
   `PointerSource::TabletTool { kind: TabletToolKind, data: TabletToolData }`,
   and `TabletToolData` really does carry `force: Option<Force>`,
   `tangential_force: Option<f32>`, `twist: Option<u16>`,
   `tilt: Option<TabletToolTilt>` and `angle: Option<TabletToolAngle>`.
2. The Wayland backend implements it for real, not as a stub:
   `winit-wayland-0.31.0-beta.3/src/types/wp_tablet_input_v2.rs` dispatches
   `zwp_tablet_tool_v2`, maps every `ToolType` onto a `TabletToolKind`, and
   fills tilt and the tool buttons.
3. **X11 gains nothing.** `winit-x11-0.31.0-beta.3/src` contains no tablet
   code at all — a grep for `TabletTool` finds one unrelated comment. X11
   pressure needs `octotablet` in 0.31 exactly as much as in 0.30, so the
   beta is not the answer to the X11 half of the problem.
4. **`winit 0.30` has no tablet API whatsoever.** Its richest pressure
   channel is `Touch { force }`, documented as always `None` on Wayland
   and X11. So the fallback's cost is not "worse pressure", it is *no*
   pressure.
5. **No published `accesskit_winit` supports `winit` 0.31.** Every
   release from 0.29.1 to 0.34.0 requires `winit ^0.30.5` — read straight
   off the crates.io index, and confirmed by adding `accesskit_winit 0.34`
   to the 0.31 probe, which resolved a *second* `winit` into the graph
   (`cargo tree -i winit@0.30.13` → `accesskit_winit v0.34.0`). Adopting
   0.31 today therefore means either two `winit` versions linked into one
   binary or a hand-written AT-SPI adapter over `accesskit_unix`. The
   changelog does not mention this, and it is the single biggest reason
   the beta is not worth taking now: the accessibility transport (S10 /
   U4.2) is in scope for this phase and 0.31 removes the only way to
   build it.

   The version pairing is tight in both directions, so record it here.
   `xarast-ui` pins `egui 0.33`, which resolves `accesskit 0.21.1`; the
   `accesskit_winit` that pairs with `accesskit 0.21.1` is **0.29.2**, and
   it wants `winit ^0.30.5`. That is the combination to use when the
   transport is wired — *not* `accesskit_winit 0.34`, which pairs with
   `accesskit 0.25` and would put two incompatible `TreeUpdate` types at
   the shell↔UI boundary. `xarast-shell` deliberately has **no** AccessKit
   dependency today, so nothing in this crate constrains that choice yet.

   One loose end, not this crate's to fix: `[workspace.dependencies]`
   declares `accesskit = "0.25"`, which no crate references, so it does
   not appear in `Cargo.lock`. The first crate to write
   `accesskit = { workspace = true }` will pull 0.25 against egui's
   0.21.1 and fail. The line should become `0.21` when the transport
   lands, or be removed.
6. 0.31 is a structural break, not a version bump: the crate is split into
   `winit-core`/`-common`/`-wayland`/`-x11`/…, `ActiveEventLoop` and
   `Window` become `dyn` traits, `create_window` returns
   `Box<dyn Window>`, `inner_size` becomes `surface_size`, window creation
   moves from `resumed` to a new `can_create_surfaces` callback, and the
   `rwh_06` Cargo feature is gone (raw-window-handle 0.6 is now
   unconditional). Migrating blind — with no way to run the result — while
   two sibling crates are being written against the 0.30 loop is the
   highest-risk change available in this phase.

**What the fallback costs us, precisely.**

| Loss | Consequence today | When it is paid back |
|---|---|---|
| No tablet axes at all | `StrokeSample::pressure` is always `None`; tools draw at full width through `pressure_or_full()` | A backend swap. The whole pipeline — `ToolAxes` → `normalise` → `SampleQueue` — is written and unit-tested against axes that no backend yet supplies |
| No trackpad gestures on Linux | `winit 0.30`'s `PinchGesture`/`PanGesture`/`RotationGesture` are documented **macOS and iOS only**; on Wayland they never fire. Pinch-zoom degrades to the scroll wheel | Same swap; `GestureEvent` already exists and the translator already routes them |
| No drop position, no multi-file grouping | `winit 0.30` emits one `HoveredFile`/`DroppedFile` **per file, with no coordinates** | `DragEvent` already carries `paths: Vec<PathBuf>` and `at: Option<PhysicalPos2>`; the newer backends simply fill them |
| No `file:` URI payloads | Nothing today; the decoder exists and is tested | `input::translate::parse_uri_list` is already written and covered, because X11 XDND and `winit` 0.31 both hand over `text/uri-list` |

**The fallback that was *not* taken, and why.** `winit 0.30 + vendored
octotablet` is the phase document's pre-planned answer to a failed E2. It
is not adopted now, for a measured reason on top of the known one
(crates.io frozen at 0.1.0 since 2024, bus factor 1): **`octotablet 0.1.0`
has no `dlopen` path for `wayland-client`.** Building it here fails in
`wayland-sys`'s build script demanding `wayland-client.pc`, because
`octotablet` exposes no feature that turns `wayland-sys/dlopen` on. That
hard-links `libwayland-client.so` into the binary, which collides with
`packaging.md` decision 2 and invariant 3 — the AppImage must start on a
host that has no Wayland client library. Vendoring it therefore means
vendoring *and patching* it, not just copying it. That is a real cost and
it should be paid deliberately, when someone can also run E1–E7.

**Re-open this when** any of the following happens, and before v0.1 in any
case: a machine with a compositor and a tablet is available to run E1–E7;
`winit` 0.31 reaches stable; or `accesskit_winit` and `egui` adopt 0.31.
All of `winit` is behind `xarast-shell` and, within it, behind
`input::translate` — criterion 16 holds and the switch is one module plus
a version line.

### Current state

`crates/xarast-shell`, building on the phase 0 walking skeleton (window,
surface, resize, scale change, `--selftest-window`, cold-start
instrumentation), which was extended rather than replaced.

| Module | What it owns | State |
|---|---|---|
| `scale` | `ScaleFactor`, `PhysicalSize`, `LogicalSize`, `PhysicalPos` | Done; the single owner of the fractional scale |
| `display` | `DisplayServer`, `DisplayEnvironment`, `PlatformCapabilities`, `headless_skip_reason` | Done; the X11-versus-Wayland difference table in executable form |
| `decorations` | `DecorationPlan`, `DecorationMode` | Done; GNOME ⇒ CSD mandatory, KDE/wlroots ⇒ SSD expected, XWayland ⇒ SSD |
| `input::event` | `ShellEvent` and everything under it | Done; the platform-neutral contract |
| `input::keyboard` | `Modifiers` with the Xara roles, `Key`, `KeyEvent`, `ModifierTracker`, `Shortcut`, `ShortcutMap<C>` | Done |
| `ime` | `ImeEvent`, `ImeState`, `ImeCursorArea` | Seam only, fully tested; phase 9 fills it |
| `input::tablet` | `ToolAxes`, `StrokeSample`, `normalise`, `TabletSource`, `MouseOnlySource`, `ScriptedSource` | Done; no backend supplies real axes yet (see the verdict above) |
| `input::coalesce` | `SampleQueue` | Done; never drops a sample |
| `input::translate` | `winit` 0.30 → `ShellEvent`, `parse_uri_list` | Done; **the only module phase 14 rewrites** |
| `portal` | `PortalService` on a services thread, `PortalHandle`, `rfd`+`ashpd` | Done; **the dialogs themselves are unmeasured** — no D-Bus session bus here |
| `clipboard` | `Clipboard` trait, `SystemClipboard` (`arboard`), `NullClipboard` | Done; **unmeasured** — no display server here |
| `window` | Event loop, `Gpu`, `ShellCtx`, `ShellApp`, `FrameRequest` | Done; **the GPU path is unmeasured** — `wgpu` enumerates zero adapters here |

100 unit tests, all passing with no compositor and no GPU.

### Decisions taken (and why)

1. **`winit 0.30.13`, not the 0.31 beta.** Above.
2. **One `ScaleFactor` owner.** `scale::ScaleFactor` is constructed in the
   translator from the window and handed downwards; the canvas never reads
   the window and the UI never computes its own `pixels_per_point`. Its
   constructor is validating — a non-finite or non-positive factor
   collapses to 1.0 and anything outside 0.25–8.0 is clamped, because a
   poisoned scale divides into every later coordinate. Sizes **round**,
   they do not truncate: at 1.5× a 801-unit window truncates to 1201 px
   and leaves an unpainted column.
3. **`ShellEvent` is the contract, `input::translate` is the only thing
   phase 14 replaces.** No type above the shell mentions `winit`. The
   model is shaped to the *richer* platform — `DragEvent` carries a path
   list and an optional position even though 0.30 supplies neither — so
   the newer backends fill fields rather than change signatures.
4. **`ToolAxes` is the tablet seam, not a `winit` type.** A backend
   reports raw axes; `tablet::normalise` is the one place that decides
   what they mean. That is what makes pressure testable with no device:
   `ScriptedSource` replays a stroke through the real pipeline.
5. **The sample queue grows; it is not a ring.** Every pointer sample of a
   frame must reach the application (a Wacom samples at ~200 Hz and the
   compositor coalesces to frame rate). A fixed ring would make sample
   loss silent and load-dependent, which is the worst failure mode
   available to a drawing tool. `SampleQueue` records a high-water mark
   instead, so an unreasonable burst shows up in diagnostics rather than
   in the geometry.
6. **Modifiers are sampled, never latched.** `ModifierTracker` publishes a
   change even when no pointer event accompanies it, because `Ctrl`,
   `Shift` and `Alt` change what a drag is doing *while* it happens
   (`research/04 §4.9` item 4). Losing focus clears them: a stale `Ctrl`
   silently constrains the next drag. Xara's names are used for the roles
   — `constrain()`, `adjust()`, `alternative()` — so the code reads as the
   feature inventory does and macOS can remap one function in phase 14.
7. **`Shortcut::works_in_drag` is opt-in.** Everything else is suppressed
   mid-drag, so a stray keystroke cannot run a command in the middle of
   one; the numeric-keypad snapping toggles are the reason the exception
   exists.
8. **The shortcut *table* is not the shell's.** `ShortcutMap<C>` is
   generic over the command type. The command vocabulary belongs to
   `xarast-app`; the shell owns only the matching rule.
9. **Portals on a services thread, and no `async` above it.** `rfd` with
   `xdg-portal` only — its `gtk3` and `wayland` backends are off, so the
   portal path is the one taken inside an AppImage or a Flatpak, and no C
   toolkit reaches the image. `ashpd` with `async-io`, not `tokio`:
   nothing needs a full runtime and `research/05 §10.1` says no async in
   the core. The main thread posts a `PortalRequestId` and later receives
   a `PortalEvent`; a dialog can never wedge the frame loop.
10. **Portal availability is decided before the first request.** If
    `DBUS_SESSION_BUS_ADDRESS` is unset and `$XDG_RUNTIME_DIR/bus` is
    absent, every request answers `Failed` with that reason immediately,
    instead of a forty-second D-Bus timeout.
11. **The clipboard's Wayland caveat is modelled, not hidden.**
    `Clipboard::persists_after_focus_loss()` is `false` on Wayland and
    `true` on X11, and the status bar is meant to say so. A copy followed
    by a quit loses the data unless a data-control manager is present;
    `arboard`'s `wayland-data-control` feature is on so that the cases
    where it *can* persist do.
12. **Nothing fails for want of a display.** `run` returns
    `ShellError::NoDisplay` with a reason and `is_missing_display()` says
    so; `--selftest-window` prints "skipped" and exits **0**;
    `system_clipboard()` returns a `NullClipboard` that explains itself.
    A red CI result on a machine that simply has no compositor teaches
    everyone to ignore the check.
13. **`ShellApp` has defaults on every method.** The self-tests and the
    cold-start measurement implement nothing, and the default
    `FrameRequest` is `Idle`, so an application with no opinion parks the
    loop rather than spinning it.
14. **No `unsafe` anywhere in the crate.** None was needed: `winit` and
    `wgpu` are safe interfaces and the platform-specific setters are
    behind `cfg`, not behind pointers.

### Invariants that must not be broken

1. **`winit` and `wgpu` appear only in `xarast-shell`** (architecture §2,
   phase criterion 16). Nothing else may name them, and within the crate
   `winit` types appear only in `input::translate` and `window`.
2. **The app id, the desktop entry basename and `StartupWMClass` are all
   `xarast`** (`packaging.md` invariant 1). `lib.rs`'s
   `app_id_matches_the_desktop_entry` test enforces it; it reads the real
   `packaging/linux/xarast.desktop`.
3. **One `ScaleFactor` per frame, computed by the shell.** A surface size
   and the scale it was computed under always travel together —
   `ShellEvent::Resized` carries both — so no consumer can pair a new size
   with a stale factor.
4. **Every pointer sample of a frame reaches the application.**
   `SampleQueue::received() == delivered()` after a drain, asserted.
5. **No `async` outside the services thread.**
6. **A missing display, a missing session bus or a missing adapter is a
   diagnosis, never a panic.** Every public entry point either returns an
   error naming the cause or returns a working object that explains
   itself.
7. **A surface is never configured at zero size.** `PhysicalSize::new`
   clamps to 1×1 and `PhysicalSize::is_degenerate` is what the resize path
   checks; a minimised window reports 0×0 on Wayland.

### Dead ends (do not retry)

- **Adding `accesskit_winit 0.34` alongside `winit 0.31`.** It resolves a
  second `winit 0.30.13` into the graph. Measured with `cargo tree -i`.
- **`winit`'s `rwh_06` feature on 0.31.** It no longer exists; passing it
  fails resolution outright. raw-window-handle 0.6 is unconditional there.
- **`octotablet 0.1.0` as a drop-in.** Its `wayland-client` has no
  `dlopen` feature, so it needs `wayland-client.pc` to build and links
  `libwayland-client.so` hard. Vendor *and patch*, or not at all.
- **`rfd` with default features.** They include the `wayland` backend,
  which drags in the same non-`dlopen` `wayland-client`; `xdg-portal`
  alone is what builds and what belongs in the AppImage.
- **`arboard` without `image-data`.** `get_image`/`set_image` simply do
  not exist; the feature is what compiles them in.
- **A fixed-capacity ring buffer for stroke samples.** See decision 5.

### Open TODOs

- **Run E1–E7 for real.** The whole verdict above is provisional on a
  machine with a compositor. Nothing in this phase measured a compositor
  behaviour, and nothing here should be quoted as if it had.
- **Hardware tablet validation is outstanding** (phase risk K6), and so is
  the `uinput` virtual tablet: neither exists in this environment.
  `ScriptedSource` covers the pipeline; it does not cover the driver.
- Portal dialogs, clipboard round-trips and drag-and-drop are **written
  and unmeasured**. They need a session bus and a compositor.
- `wgpu` adapter selection, the four-level capability ladder (S4/U2.5) and
  frame pacing beyond `Wait`/`WaitUntil` are still the walking skeleton's:
  one adapter request, one clear pass. They need an adapter to develop
  against.
- **Live** colour-scheme change notification: the settings portal is read
  once at start-up. `ashpd` exposes a signal stream; wiring it needs a bus
  to test against.
- AccessKit transport (S10/U4.2) is **not wired**. When it is, use
  `accesskit_winit 0.29.2` (pairs with `accesskit 0.21.1`, which is what
  `egui 0.33` resolves) and fix the unused `accesskit = "0.25"` line in
  `[workspace.dependencies]` at the same time. `xarast-shell` has no
  AccessKit dependency today, so this is an addition, not a migration.
- The `egui`→`winit` shim (S9/U4.1) is **not written**. It is `xarast-ui`'s
  boundary as much as the shell's, and it should be built directly on
  `ShellEvent` rather than on `winit`, so that phase 14 gets it for free.
- **Bridge `ShellEvent` to `xarast_app::Intent`.** `xarast-app` landed its
  `Intent`/`Changed` contract while this crate was being written, so the
  shell still defines its own `Modifiers` and `PointerButton` at the
  platform boundary. That is correct layering — the shell's are physical,
  the app's are semantic — but the translation from one to the other has
  to be written, and it belongs in `xarast-app` or in a thin adapter, not
  in `input::translate`.
- X11 pressure via `octotablet` remains deferred; X11 is a documented
  degradation (`PlatformCapabilities::X11`), not a target.
