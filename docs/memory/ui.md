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
