# Phase 5 — Shell & UI skeleton

> After this phase Xarast is a program you can hand to someone: it opens a real `.xar` file in a native Wayland window, draws it with the Phase 4 engine, and lets them pan, zoom, toggle layers and pick colours — read-only, but real.

## Goal

Deliver the two crates that turn the headless core into an application:

- **`xarast-shell`** — window, event loop, GPU surface, Wayland integration, tablet input,
  clipboard, drag-and-drop, XDG portals, accessibility transport. It owns `winit` and
  `wgpu` and is the *only* crate that names them (architecture §2).
- **`xarast-ui`** — canvas widget, viewport, rulers/grid/guides, layer panel, colour
  panel, status bar, docking layout, theming. Built on `egui`, used as a library, never
  through `eframe` (`research/05 §2.4`).

Plus the glue in `xarast-app` that this phase needs: the document session, the viewport
state, and the arena→`Scene` walker whose contract Phase 4 defined.

This phase also closes two architecture open questions:

- **§7 q2 — does `egui` hold up at professional panel density?** Answered by W1's
  measured UI spike, with a concrete fallback.
- **§7 q3 — is `winit 0.31-beta` stable enough to pin?** Answered by W2's evaluation.

(Architecture §7 labels both as "Phase 4"; the roadmap assigns shell and UI to Phase 5.
The roadmap's numbering is the one in force — note this when updating the architecture
document.)

Milestone M3 ("It draws") closes with this phase.

---

## Scope

### In scope

| # | Item | Reference |
|---|---|---|
| S1 | UI spike at professional density, with go/no-go thresholds and a named fallback | architecture §7 q2, `research/05 §2.1` |
| S2 | `winit 0.31.0-beta.3` pinning decision, with the evaluation that justifies it | architecture §7 q3, `research/05 §3.1` |
| S3 | Window, event loop (`ApplicationHandler`), surface creation, resize, close, frame pacing | `research/05 §3.1` |
| S4 | `wgpu` 30 instance/adapter/device creation and the four-level capability ladder with automatic degradation | `research/05 §4.3` |
| S5 | Wayland specifics: fractional scaling (`wp_fractional_scale_v1`), client-side decorations, trackpad gestures, cursor scaling | `research/05 §3.1` |
| S6 | XDG portals: file open/save dialogs, colour-scheme detection, via `rfd` + `ashpd` on a services thread | `research/05 §2.5`, `§10.2` |
| S7 | Clipboard (text + image) and drag-and-drop with the redesigned winit 0.31 `Drag*` events and URI payloads | `research/05 §3.1` |
| S8 | Tablet input: `TabletSource` trait, winit backend, normalised `StrokeSample`, per-frame event coalescing | `research/05 §3.3` |
| S9 | `xarast-egui-winit`: our own winit 0.31 → `egui::RawInput` translation shim (the replacement for `egui-winit`, which pins winit 0.30) | `research/05 §2.4` |
| S10 | AccessKit wiring through `accesskit_winit` (AT-SPI on Linux) | `research/05 §2.3`, `§2.5` |
| S11 | Canvas widget: shares device, queue and swapchain with the UI; one `CommandEncoder` per frame, canvas pass then egui pass | `research/05 §2.3` |
| S12 | Viewport: pan, zoom, zoom-to-page/drawing/selection/100 %, previous zoom, scroll, view transform in `f64` | `research/04 §1.19` |
| S13 | Rulers, grid (rectangular), guides (drag from ruler, show/hide) — display and interaction only | `research/04 §1.18` |
| S14 | Layer panel: list, visibility, lock, active layer, reorder | `research/04 §1.1`, `§1.11` |
| S15 | Colour panel: on-screen colour line with scrolling palette, colour editor (RGB/HSV/grey), "no colour" | `research/04 §1.5`, `§2.2` |
| S16 | Status bar: pointer coordinates in the document's units, zoom level, render-quality indicator, cache pressure | `research/04 §1.19` |
| S17 | Docking layout with `egui_tiles`, persisted across sessions | `research/05 §2.5` |
| S18 | Dark/light theme following `org.freedesktop.appearance color-scheme`, with manual override | `research/05 §2.5` |
| S19 | High-DPI and fractional scale correctness end to end: window → egui pixels-per-point → canvas device pixels | `research/05 §2.1` item 9 |
| S20 | Keyboard/shortcut infrastructure: the modifier model (`Constrain`/`Adjust`/`Alternative`), live-during-drag modifiers, momentary tool switch — infrastructure only, no tools yet | `research/04 §4.1`, `§4.9` |
| S21 | The document session in `xarast-app`: open a file, hold the arena, own the walker, own the undo history | architecture §4 |
| S22 | Threading: main/render/pool/services split exactly as architecture §5 | `research/05 §10.2` |

### Explicitly out of scope (and which phase owns it)

| Item | Owner |
|---|---|
| Any editing tool, selection, handles, transforms | Phase 7 |
| Interactive fill and transparency handles, colour drag-and-drop onto objects | Phase 8 |
| Text tool, IME, text editing on canvas | Phase 9 |
| Bitmap gallery, photo panels | Phase 10 |
| Export dialogs and encoders | Phase 11 |
| i18n catalogues and full accessibility audit | Phase 12 (the AccessKit *transport* is wired here; the audit is not) |
| Windows and macOS shells | Phase 14 |
| Magnetic snapping to objects; isometric grids; grid tool | Phase 7 (grid/guide snapping) and later (object snap is P1) |
| Multiple views of one document, full-screen mode | Later (P2 in `research/04 §1.19`) |
| `.xarast` open/save from the UI | Phase 6 provides it; the shell wires the menu entry when Phase 6 lands |

---

## Prerequisites

| Need | Source | Hard or soft |
|---|---|---|
| AppImage pipeline, CI, licence gate | Phase 0 | Hard |
| `xarast-geom`, `xarast-color` | Phase 1 | Hard |
| `xarast-doc` arena, layers, resources | Phase 2 | Hard |
| `xarast-xar` importer producing documents from the corpus | Phase 3 | Hard (this is what makes the viewer worth looking at) |
| `xarast-render` with both backends, `Scene`/`SceneBuilder`/`DisplayList`, `FrameTimings` | Phase 4 | Hard |
| Phase 4's measured budgets in `docs/memory/perf.md` | Phase 4 | Hard — this phase is judged against them |

---

## Workstreams

### W1 — UI spike: does egui survive professional density?

Run this **before** any panel is written. Architecture §7 q2 is open, and the cost of
discovering the answer after fifteen panels exist is the phase.

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| U1.1 | Build the density probe: 6 docked panels, ≥ 400 visible controls, 18–22 px rows, no app-style padding | `xarast-ui` (`examples/density`) | M | — |
| U1.2 | Add a 5,000-row virtualised layer tree with `egui_extras::TableBuilder` | probe | M | U1.1 |
| U1.3 | Add a 512-swatch palette strip and a 2,000-thumbnail gallery backed by a texture atlas | probe | M | U1.1 |
| U1.4 | Add numeric fields with drag-adjust, unit parsing (`10mm`, `1in`, `3p6`) and bump buttons | probe | M | U1.1 |
| U1.5 | Instrument: `criterion` on `build_ui_frame()`, `wgpu-profiler` on the UI pass, allocation counter | probe | S | U1.1 |
| U1.6 | Run the probe with the Phase 4 canvas rendering `bulk` underneath at Draft quality | probe | M | U1.5 |
| U1.7 | Check AccessKit output: is the tree, are the fields, are the toggles exposed to AT-SPI? | probe | M | U1.1 |
| U1.8 | Decision record into `docs/memory/ui.md` | — | S | U1.5–U1.7 |

**What is measured.**

| Axis | Metric | Threshold |
|---|---|---|
| P1 UI build cost | `build_ui_frame()` CPU time, panels fully visible | p50 ≤ 3 ms, p99 ≤ 8 ms |
| P2 Main-thread total | UI build + event handling + submit, canvas busy | ≤ 8 ms every frame (`research/05 §10.2` golden rule) |
| P3 UI GPU pass | egui render pass duration | ≤ 1.5 ms |
| P4 Tree scrolling | 5,000-row tree scrolled at 60 fps for 10 s | no frame over 16 ms |
| P5 Partial update | moving one slider must not cost a full-panel rebuild beyond P1 | frame time within 1 ms of idle |
| P6 Memory | resident growth from opening all panels | ≤ 50 MB |
| P7 Density | 20 px rows legible at 1× and at 1.25/1.5 fractional scale | visual check, screenshots attached |
| P8 Accessibility | layer tree, numeric fields and toggles present and labelled in the AT-SPI tree (`accerciser` or `busctl` dump) | all three present |
| P9 Latency | pointer-move to on-screen handle update | ≤ 2 frames at 60 Hz, measured with a high-speed capture or a frame-tagged log |

**Go/no-go and the fallback.** If all of P1–P9 pass, egui is confirmed and the rest of the
phase proceeds on it. Otherwise, in order:

1. **P1/P2 fail only under the heaviest panel (gallery or tree).** Mitigate inside egui
   first: virtualise harder, cache thumbnails in a texture atlas, split the offending
   panel into its own `egui::Context` with an independent repaint schedule, and use
   `request_repaint_after` so idle panels cost nothing. Re-measure. This is cheap and is
   the expected outcome if anything fails.
2. **P1/P2 fail broadly (immediate mode itself is the cost).** Move the two worst panels
   to custom widgets that we paint directly into the canvas pass, keeping egui for the
   rest. The boundary that makes this possible is built in W5: `xarast-ui` talks to the
   application only through `xarast-app`, and every panel implements one `Panel` trait, so
   a panel can change its painting technology without touching anything else.
3. **The toolkit itself is the problem.** Fall back to **`iced` 0.14** (`research/05 §2.2`
   names it the solid plan B: retained, partial updates native, its own `iced_wgpu`, used
   at scale by COSMIC). The cost is real and must be stated when the decision is taken:
   no docking out of the box (`egui_tiles` has no iced equivalent — we would write one),
   AccessKit not yet integrated, and the canvas must go through iced's `shader` widget
   rather than sharing our pass directly. The migration surface is `xarast-ui` only, plus
   the `xarast-egui-winit` shim which is discarded.

The decision, with its numbers, goes into `docs/memory/ui.md` and closes architecture
§7 q2. Do not write "egui was fine" — write the nine numbers.

---

### W2 — Shell foundations and the winit pinning decision

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| U2.1 | Evaluate `winit 0.31.0-beta.3`: build, run, and exercise pointer/tablet/DnD/gesture paths on Wayland | `xarast-shell` | M | — |
| U2.2 | Pinning decision and the fallback plan, recorded | — | S | U2.1 |
| U2.3 | `ApplicationHandler` event loop, window creation, `Window`/`ActiveEventLoop` as traits (the 0.31 restructure), `surface_*` naming | `xarast-shell` | M | U2.2 |
| U2.4 | `wgpu` 30 init: instance, adapter with `HighPerformance`, fallback adapter retry, device/queue, surface configuration | `xarast-shell` | M | U2.3 |
| U2.5 | Capability ladder levels 0–3 with automatic selection and the `XARAST_RENDERER` / `WGPU_BACKEND` escape hatches | `xarast-shell` | M | U2.4 |
| U2.6 | Frame pacing: present mode selection, redraw request policy, idle throttling to 0 fps | `xarast-shell` | M | U2.3 |
| U2.7 | Thread topology: main / render / rayon pool / services, with `crossbeam-channel` and a prioritised `ScenePatch` queue | `xarast-shell`, `xarast-app` | L | U2.4 |
| U2.8 | Crash-safety: panic hook that writes a report and attempts an emergency autosave hand-off | `xarast-shell` | S | U2.7 |

**The pinning question, decided by evidence.** `winit 0.31` is the only version exposing
`PointerSource::TabletTool { data: TabletToolData }` with `force`, `tangential_force`,
`tilt`, `twist` and `angle` — without it there is no stylus pressure, which is
unacceptable for a drawing tool, and `eframe`/`egui-winit` pin `winit ^0.30.13`, which is
why we write our own shim (W4). The evaluation is:

- **E1** Build and run on three compositors: GNOME/mutter, KDE/kwin, sway (wlroots).
- **E2** Tablet: pressure, tilt and twist arrive on all three with a real device, and with
  a virtual `uinput` tablet in CI. If no hardware is available, `uinput` alone, and record
  that hardware validation is outstanding.
- **E3** Fractional scaling at 1.25, 1.5 and 2.0: window content, cursors and egui
  pixels-per-point all correct, no blur, no half-pixel offsets.
- **E4** CSD: window decorations appear and behave on GNOME (where they are mandatory) via
  SCTK + `sctk-adwaita`.
- **E5** Drag-and-drop with the redesigned `DragEntered`/`DragMoved`/`DragDropped`/
  `DragLeft` events, receiving `file:` URIs through `SendData::Uris`.
- **E6** Gestures: `PinchGesture`, `PanGesture`, `RotationGesture` reach the viewport.
- **E7** Stability: an 8-hour soak with synthetic input, no leak, no protocol error.

**Decision rule.** Pass E1–E7 → pin `winit = "=0.31.0-beta.3"` exactly (never a range, so
a new beta cannot land silently) and budget one week per beta bump, as `research/05 §2.6`
already assumes. Fail E2 → fall back to `winit 0.30.13` **plus vendored `octotablet`**
in `third_party/` (its crates.io release is frozen at 0.1.0 from 2024, bus factor 1, so
vendoring is not optional), which also buys X11 pressure and pad buttons/rings that winit
does not expose at all. Fail E1/E3/E4 on one compositor only → ship, document the
limitation, file upstream. Either way, **all of winit is behind `xarast-shell`**: no other
crate imports it, so the fallback is a one-crate change.

---

### W3 — Wayland integration, portals, clipboard, drag and drop

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| U3.1 | Fractional scale plumbing: scale factor → surface size → egui `pixels_per_point` → canvas device rect, one source of truth | `xarast-shell` | M | W2 |
| U3.2 | CSD, window title, app id (`org.xarast.Xarast`), window icon | `xarast-shell` | S | W2 |
| U3.3 | Services thread: `tokio` current-thread runtime hosting `ashpd` | `xarast-shell` | M | U2.7 |
| U3.4 | File dialogs via `rfd` with the `xdg-portal` backend (works inside AppImage and Flatpak sandboxes) | `xarast-shell` | M | U3.3 |
| U3.5 | Colour-scheme detection via `org.freedesktop.appearance` with live change notification | `xarast-shell` | S | U3.3 |
| U3.6 | Clipboard via `arboard` with `wayland-data-control`, text and image | `xarast-shell` | M | W2 |
| U3.7 | Drag-and-drop: files onto the canvas, with URI decoding and a drop-target model the UI can extend | `xarast-shell` | M | W2 |
| U3.8 | X11 behaviour under XWayland: same code path, documented degradations (no tablet pressure without the octotablet feature) | `xarast-shell` | S | W2 |

**Tricky parts.**

*Fractional scale has exactly one owner.* The classic bug is three components each
computing their own scale: winit reports a fractional factor, egui wants
`pixels_per_point`, and the canvas wants physical device pixels. `xarast-shell` computes
one `ScaleFactor` per frame and hands it to both; the canvas never reads the window
directly. Off-by-a-half-pixel rulers are the symptom that this rule was broken.

*Portals are async and the event loop is not.* Everything D-Bus lives on the services
thread; the UI thread only ever sees a completed `ShellEvent::FileChosen(..)`. No
`async` leaks into the core (`research/05 §10.1`).

*Clipboard on Wayland only works while the window has focus.* A copy performed and then
the window closed loses the data unless a data-control manager is present. Document it in
the status-bar behaviour rather than pretending otherwise.

---

### W4 — Input: the egui shim and tablet

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| U4.1 | `xarast-egui-winit`: winit 0.31 events → `egui::RawInput`, including modifiers, IME, scroll, touch and the new pointer model (~1,000–1,500 lines) | `xarast-shell` | L | W2 |
| U4.2 | `accesskit_winit` 0.34 wiring: tree updates from egui, action requests back into the UI | `xarast-shell` | M | U4.1 |
| U4.3 | `TabletSource` trait + `WinitTabletSource` | `xarast-shell` | M | W2 |
| U4.4 | `OctotabletSource` behind an optional feature (X11 pressure, pad buttons and rings) | `xarast-shell` | M | U4.3 |
| U4.5 | `StrokeSample` normalisation and per-frame coalescing: accumulate **every** pointer move in the frame, not just the last | `xarast-shell` | M | U4.3 |
| U4.6 | Modifier model: `Constrain`/`Adjust`/`Alternative`, live during drag, plus momentary tool switch plumbing | `xarast-app` | M | U4.1 |
| U4.7 | Shortcut table and dispatcher, keyed by command id, data-driven so Phase 7 only adds rows | `xarast-app` | M | U4.6 |

**Tricky parts.**

*Coalescing is a correctness issue, not an optimisation.* A Wacom samples at ~200 Hz; the
compositor coalesces to frame rate. If the freehand tool (Phase 7) only sees the last
`PointerMoved` of each frame, strokes lose their shape at speed. `StrokeSample` therefore
carries a timestamp and the shell pushes **all** samples per frame into a ring buffer that
tools drain.

*Modifiers must be sampled continuously, not latched at drag start.* `research/04 §3` item
15 calls this out as one of the eighteen things that made Xara feel like Xara: `Ctrl`,
`Shift` and `Alt` change behaviour *during* a drag, and snapping is toggled mid-drag with
the numeric keypad (`WorksInDrag`). The shim must deliver modifier-change events even when
no pointer event accompanies them.

*Writing our own `egui-winit` is a maintenance commitment.* Keep it in one module with its
own tests, and keep a tracking note: when `egui` moves to winit 0.31, delete it.

---

### W5 — Canvas, viewport and the UI shell

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| U5.1 | Frame composition: one `CommandEncoder`, canvas pass then egui pass, shared device/queue/swapchain | `xarast-shell` | M | W2, W4 |
| U5.2 | `CanvasWidget`: an egui region whose pixels come from the Phase 4 backends, with correct hit region and scroll/zoom handling | `xarast-ui` | L | U5.1 |
| U5.3 | `Viewport`: `f64` view transform, pan, zoom about a point, zoom-to-page/drawing/selection/100 %, previous zoom, scroll bars | `xarast-app` | L | — |
| U5.4 | Scene walker: arena + dirty region + attribute stack → `SceneBuilder` calls (fills the Phase 4 contract) | `xarast-app` | L | Phase 4 |
| U5.5 | Render-thread protocol: immutable `DisplayList` handoff, backpressure, cancellation of superseded frames | `xarast-app`, `xarast-shell` | L | U5.4 |
| U5.6 | Draft/Final scheduling from the UI side: Draft while interacting, Final after 120 ms idle | `xarast-app` | M | U5.5 |
| U5.7 | Overlay pass: rulers, grid, guides, page edges, drawn into the Phase 4 overlay surface | `xarast-ui` | M | U5.2 |
| U5.8 | `Panel` trait and the docking host with `egui_tiles`, layout persisted to preferences | `xarast-ui` | M | W1 |
| U5.9 | Layer panel | `xarast-ui` | M | U5.8 |
| U5.10 | Colour panel and on-screen colour line | `xarast-ui` | M | U5.8 |
| U5.11 | Status bar with coordinates in document units, zoom, quality, cache pressure | `xarast-ui` | S | U5.8 |
| U5.12 | Theme: dark/light tokens, follow system, manual override, persisted | `xarast-ui` | M | U3.5 |
| U5.13 | Menus and the command palette skeleton, wired to the W4.7 shortcut table | `xarast-ui` | M | U4.7 |
| U5.14 | Preferences store (units, theme, renderer, autosave cadence, cache budget) | `xarast-app` | M | — |

**Tricky parts.**

*The canvas and the UI share one surface.* This is the main reason `eframe` is rejected
and the reason for the shim: `egui-wgpu`'s `Renderer` takes *our* device and *our* render
pass, so there is no intermediate texture, no extra copy and no tearing between the canvas
and the handles drawn over it. Keep the pass order fixed — canvas, overlay, egui — and
keep the canvas pass writing to `Rgba8Unorm`, not `Rgba8UnormSrgb` (Phase 4's rule; the
surface format must be chosen to match, converting explicitly if the swapchain insists on
sRGB).

*The document lives on the main thread; the render thread never sees the arena*
(architecture §5). The walker runs on the main thread (or the pool) and produces an
immutable `DisplayList`; the render thread owns the GPU queue. That is what keeps
`xarast-doc` lock-free. The protocol must drop superseded frames rather than queue them,
or a fast pan builds a backlog and the window goes soft.

*Ruler and guide geometry is in document units but drawn in device pixels.* Round to
device pixels *after* the `f64` transform, snap to half-pixel for crisp hairlines, and
re-derive on scale change — do not cache in physical pixels across a fractional-scale
change.

---

### W6 — First usable viewer and packaging

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| U6.1 | Open `.xar` from the portal dialog and from the command line; recent files | `xarast-app` | M | W3, Phase 3 |
| U6.2 | Document session lifecycle: open, close, multiple documents in tabs, unsaved-state plumbing (no save yet) | `xarast-app` | M | U6.1 |
| U6.3 | Progress and cancellation for long loads, on the I/O thread | `xarast-app` | M | U6.1 |
| U6.4 | Error surface: a non-modal problem list for importer diagnostics, not a modal box per warning | `xarast-ui` | S | U6.1 |
| U6.5 | AppImage updated to ship the desktop file, icon and MIME registration | `packaging/` | S | U6.1 |
| U6.6 | Startup budget work: cold start to window ≤ 400 ms, first paint of a 5 MB `.xar` ≤ 500 ms | all | M | U6.1 |
| U6.7 | `egui_kittest` snapshot tests for the panels | `xarast-ui` | M | U5.9–U5.11 |

---

## Public API introduced

```rust
// ─────────────────────────── crates/xarast-shell/src/lib.rs

/// Everything the application sees of the platform. No other crate imports winit.
pub struct Shell { /* … */ }

pub struct ShellConfig {
    pub title: String,
    pub app_id: String,                 // "org.xarast.Xarast"
    pub initial_size: (u32, u32),
    pub renderer: RendererPreference,   // XARAST_RENDERER override applied here
    pub vsync: PresentPolicy,
}

impl Shell {
    pub fn run<A: ShellApp>(cfg: ShellConfig, app: A) -> Result<(), ShellError>;
    pub fn gpu(&self) -> &GpuContext;
    pub fn scale(&self) -> ScaleFactor;
    pub fn clipboard(&mut self) -> &mut dyn Clipboard;
    pub fn request_redraw(&self);
    /// Fire-and-forget portal request; the answer arrives as a ShellEvent.
    pub fn portal(&self) -> &PortalHandle;
}

/// Implemented by `xarast-app`. The shell owns the loop; the app owns the state.
pub trait ShellApp: 'static {
    fn on_event(&mut self, ev: ShellEvent, ctx: &mut ShellCtx);
    fn on_frame(&mut self, ctx: &mut ShellCtx) -> FrameRequest;
    fn on_exit(&mut self, ctx: &mut ShellCtx);
}

#[derive(Debug, Clone)]
pub enum ShellEvent {
    Resized { physical: (u32, u32), scale: ScaleFactor },
    ScaleChanged(ScaleFactor),
    ColorSchemeChanged(ColorScheme),
    Pointer(PointerEvent),
    Stroke(StrokeSample),
    Key(KeyEvent),
    ModifiersChanged(Modifiers),
    Gesture(GestureEvent),
    DragEntered { uris: Vec<PathBuf>, at: PhysicalPos },
    DragMoved   { at: PhysicalPos },
    DragDropped { uris: Vec<PathBuf>, at: PhysicalPos },
    DragLeft,
    FileChosen  { request: PortalRequestId, paths: Vec<PathBuf> },
    Accessibility(accesskit::ActionRequest),
    CloseRequested,
}

#[derive(Debug, Clone, Copy)]
pub enum FrameRequest {
    /// Nothing changed; sleep until the next input.
    Idle,
    /// Repaint now.
    Redraw,
    /// Repaint after this delay (used for the 120 ms Draft→Final timer).
    RedrawAfter(Duration),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScaleFactor(pub f64);   // fractional; 1.0, 1.25, 1.5, 2.0 …

pub struct GpuContext {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    pub surface_format: wgpu::TextureFormat,
    pub tier: RendererTier,
}

/// The four-level ladder of research/05 §4.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererTier { Fast, Standard, Compatible, Software }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererPreference { Auto, ForceGpu, ForceHybrid, ForceCpu }

// ─────────────────────────── tablet input

/// One normalised sample, whatever the platform source.
#[derive(Debug, Clone, Copy)]
pub struct StrokeSample {
    pub x: f64,
    pub y: f64,
    pub pressure: Option<f32>,        // 0..1
    pub tilt_x: Option<f32>,          // degrees
    pub tilt_y: Option<f32>,
    pub twist: Option<f32>,           // degrees, 0..360
    pub tangential: Option<f32>,      // -1..1
    pub timestamp: Instant,
    pub source: InputSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputSource { Mouse, Touch, Pen, Eraser, Unknown }

pub trait TabletSource: Send {
    fn poll(&mut self, out: &mut Vec<StrokeSample>);
    fn capabilities(&self) -> TabletCaps;
}

pub struct WinitTabletSource { /* … */ }
#[cfg(feature = "octotablet")]
pub struct OctotabletSource { /* … */ }

// ─────────────────────────── crates/xarast-ui/src/lib.rs

/// Every dockable panel implements this. It is also the seam that lets a panel
/// change its painting technology (W1 fallback 2) without touching anything else.
pub trait Panel {
    fn id(&self) -> PanelId;
    fn title(&self) -> &str;
    fn ui(&mut self, ui: &mut egui::Ui, app: &mut AppState);
    fn min_size(&self) -> egui::Vec2 { egui::vec2(180.0, 80.0) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PanelId(pub &'static str);

pub struct UiHost { /* egui_tiles tree + registered panels */ }
impl UiHost {
    pub fn new(theme: Theme) -> Self;
    pub fn register(&mut self, panel: Box<dyn Panel>);
    pub fn run(&mut self, ctx: &egui::Context, app: &mut AppState) -> UiOutput;
    pub fn save_layout(&self) -> LayoutState;
    pub fn load_layout(&mut self, s: &LayoutState);
}

pub struct CanvasWidget { /* … */ }
impl CanvasWidget {
    /// Returns the region the canvas occupies plus any input the UI did not consume.
    pub fn show(&mut self, ui: &mut egui::Ui, app: &mut AppState) -> CanvasResponse;
}

pub struct CanvasResponse {
    pub rect_device: DeviceRect,
    pub hovered: bool,
    pub unconsumed: Vec<CanvasInput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme { Dark, Light, FollowSystem }

// ─────────────────────────── crates/xarast-app/src/lib.rs

/// Everything the UI is a projection of. Owned by the main thread.
pub struct AppState {
    pub docs: DocumentSessions,
    pub active: Option<DocumentId>,
    pub prefs: Preferences,
    pub diagnostics: DiagnosticLog,
}

pub struct DocumentSession {
    pub id: DocumentId,
    pub doc: xarast_doc::Document,
    pub edit: xarast_app::EditState, // session state: see architecture §3.5b
    pub viewport: Viewport,
    pub path: Option<PathBuf>,
    pub scene: xarast_render::Scene,
}

/// The view transform, in f64 throughout (architecture §3.4).
pub struct Viewport {
    pub transform: Transform2D,   // document (millipoints) → device (pixels)
    pub size: DeviceSize,
}

impl Viewport {
    pub fn pan_by(&mut self, dx_device: f64, dy_device: f64);
    pub fn zoom_about(&mut self, factor: f64, anchor_device: DevicePoint);
    pub fn zoom_to(&mut self, target: ZoomTarget, doc: &Document, sel: &Selection);
    pub fn scale(&self) -> f64;
    pub fn doc_to_device(&self, p: DocPoint) -> DevicePoint;
    pub fn device_to_doc(&self, p: DevicePoint) -> DocPoint;
    pub fn visible_doc_rect(&self) -> DocRect;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomTarget { Page, Spread, Drawing, Selection, Percent100, Previous }

/// Fills the Phase 4 contract: arena + dirty region → Scene.
pub struct SceneWalker { /* … */ }
impl SceneWalker {
    pub fn rebuild(&mut self, doc: &Document, edit: &EditState,
                   vp: &Viewport, dirty: DocRect,
                   scene: &mut xarast_render::Scene);
}

/// Handoff to the render thread. Never carries a reference to the arena.
pub enum RenderRequest {
    Frame { doc: DocumentId, list: Arc<xarast_render::DisplayList>,
            view: ViewParams, generation: u64 },
    Cancel { up_to_generation: u64 },
    Shutdown,
}
```

---

## Acceptance criteria

1. **UI spike decision recorded.** `docs/memory/ui.md` contains P1–P9 with measured
   numbers, the verdict, and — if any threshold failed — which fallback was taken and why.
   Architecture §7 q2 is marked closed there.
2. **winit decision recorded.** `docs/memory/ui.md` contains the E1–E7 results per
   compositor and the pinned version string. Architecture §7 q3 is marked closed.
3. **It opens and draws.** `xarast path/to/file.xar` opens a window and renders the
   document; run over the whole Phase 3 corpus by
   `cargo run -p xarast-cli -- smoke-open tests/corpus/*.xar --headless-window`,
   exiting 0 for every file.
4. **Pan/zoom budget.** `cargo bench -p xarast-app -- viewport` reports ≤ 16 ms per frame
   for a 100,000-object document on the reference integrated GPU, including UI, and the
   number is written to `docs/memory/perf.md`.
5. **Main-thread budget.** A frame-time histogram captured over a 60 s scripted
   pan/zoom/panel-interaction session shows p99 main-thread time ≤ 8 ms.
6. **Cold start.** `hyperfine 'xarast --quit-after-first-frame'` reports ≤ 400 ms mean.
7. **First paint.** Opening a 5 MB `.xar` shows the first rendered frame in ≤ 500 ms,
   measured by a timestamped log line asserted in an integration test.
8. **Fractional scaling.** Screenshots at scale 1.0, 1.25, 1.5 and 2.0 show rulers, grid
   and panel text crisp, with the ruler origin aligned to the page corner within 0.5
   device pixels — asserted by a pixel probe in `egui_kittest`, not by eye alone.
9. **Tablet pressure.** With a virtual `uinput` tablet, a scripted stroke produces
   `StrokeSample`s with monotonically varying `pressure`, and the per-frame coalescing
   test asserts that **every** emitted sample reaches the application (none dropped).
10. **Portals.** File open and save dialogs work inside the AppImage on GNOME, KDE and
    sway; a CI test asserts the portal code path is taken (not a GTK fallback) by checking
    the `rfd` backend in use.
11. **Clipboard and DnD.** Copying an image to the clipboard and pasting it into another
    application succeeds; dropping a `.xar` file onto the canvas opens it. Both covered by
    a scripted test on a nested compositor, plus a manual checklist entry.
12. **Accessibility.** `busctl`/`accerciser` shows the layer tree, the colour fields, the
    menu bar and the canvas exposed with roles and labels; an automated test asserts the
    AccessKit tree contains ≥ N nodes with non-empty names for the default layout.
13. **Theme follows the system.** Changing `org.freedesktop.appearance color-scheme` at
    runtime flips the theme without a restart (integration test against a mock portal).
14. **Docking persists.** Rearranging panels, quitting and restarting restores the layout;
    covered by a round-trip test of `LayoutState`.
15. **Degradation ladder.** `XARAST_RENDERER=cpu`, `=hybrid`, `=gpu` and
    `WGPU_BACKEND=gl` each start successfully and report the selected tier in the status
    bar and in `--version --verbose`; a CI job runs the full smoke test under `lavapipe`.
16. **No layering violations.** `cargo tree` shows: `winit` and `wgpu` only under
    `xarast-shell`; `egui` only under `xarast-ui` and `xarast-shell`; `xarast-doc` and
    `xarast-render` free of both.
17. **Panel snapshots.** `cargo test -p xarast-ui` runs `egui_kittest` image snapshots for
    the layer panel, colour panel and status bar, passing at the committed baselines.

---

## Performance budgets

| Budget | Target | Measured how |
|---|---|---|
| Pan/zoom frame time, 100k objects, integrated GPU, including UI | ≤ 16 ms | `cargo bench -p xarast-app -- viewport`; roadmap budget |
| Main-thread time per frame (UI build + events + submit) | ≤ 8 ms p99 | frame histogram, scripted session |
| egui UI build (`build_ui_frame`), full default layout | ≤ 3 ms p50 | `criterion` |
| egui GPU pass | ≤ 1.5 ms | `wgpu-profiler` |
| Cold start to window | ≤ 400 ms | `hyperfine`; roadmap budget |
| Open a 5 MB `.xar` to first paint | ≤ 500 ms | integration test; roadmap budget |
| Pointer-to-pixel latency (pan) | ≤ 2 frames at 60 Hz | frame-tagged log / high-speed capture |
| Idle CPU with a document open, no input | ≤ 0.5 % of one core | `top` over 60 s; `request_repaint_after` must actually park |
| Resident memory, empty document | ≤ 180 MB | `/proc/self/status` at steady state |
| Resident memory, 5 MB `.xar` open, default cache budget | ≤ 600 MB | same |
| AppImage size after this phase | ≤ 80 MB | CI; roadmap budget |

---

## Risks and mitigations

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| K1 | `winit 0.31` stays in beta and breaks API across betas | High | Medium | Exact pin; all of winit behind `xarast-shell`; one week budgeted per bump; fallback to 0.30 + vendored `octotablet` is pre-planned, not improvised |
| K2 | Our `egui-winit` shim drifts from upstream | Medium | Medium | Keep it in one isolated module with its own tests; track upstream; delete it the day egui adopts winit 0.31 |
| K3 | egui fails the density bar | Medium | High | W1 measures it before anything is built on it, and names three graded fallbacks ending in `iced` |
| K4 | The 16 ms budget is missed once the UI is added to Phase 4's 8 ms canvas | Medium | High | Budgets are split and measured separately (canvas, UI build, UI pass, present) so the guilty party is identifiable; Draft quality and the render cache are the levers |
| K5 | Fractional scaling produces half-pixel blur in rulers and hairlines | High | Medium | One `ScaleFactor` owner; device-pixel snapping for hairlines; explicit pixel-probe tests at four scale factors |
| K6 | No tablet hardware available to validate | Medium | Medium | `uinput` virtual tablet in CI covers the protocol; record hardware validation as outstanding in `docs/memory/ui.md` and close it before v0.1 |
| K7 | Portals behave differently across desktops (or are absent) | Medium | Medium | Test on three compositors; `rfd` falls back gracefully; never block startup on a portal call |
| K8 | AccessKit's Linux adapter is incomplete for our widgets | Medium | Low | Keep critical controls on standard egui widgets (`research/05 §2.6`); custom-painted panels must still publish an AccessKit subtree, checked by criterion 12 |
| K9 | Render-thread backlog makes fast pans feel soft | Medium | Medium | Generation counters and `Cancel` in the protocol; drop superseded frames rather than queueing them; assert in a test that a burst of 100 pan events produces ≤ N submitted frames |
| K10 | The document-per-tab session model leaks memory across open/close cycles | Low | Medium | Open/close 200 documents in a loop test and assert resident memory returns within 10 % of baseline |

---

## Test plan

**Unit.** Viewport maths (`doc_to_device` ∘ `device_to_doc` is the identity to 1e-9;
`zoom_about` keeps the anchor fixed); scale-factor propagation; shortcut table resolution
including modifier combinations; `StrokeSample` normalisation from synthetic winit events;
`LayoutState` serialisation round-trip.

**Widget snapshots.** `egui_kittest` image snapshots for every panel at scale 1.0 and 1.5,
in both themes. These catch accidental density and padding regressions, which is exactly
the class of regression W1 exists to prevent.

**Integration, on a nested compositor** (`sway --config` headless or `weston --backend
=headless`): open a document, pan, zoom, toggle a layer, change the theme, drop a file,
copy an image; assert on logged state rather than pixels where possible.

**Performance.** `criterion` for `build_ui_frame` and the viewport benchmark; a scripted
60 s session producing a frame-time histogram, with p50/p99 asserted and archived as a CI
artefact.

**Compatibility matrix**, run nightly: {GNOME, KDE, sway} × {native Wayland, XWayland} ×
{`Fast`, `Compatible`, `Software` tiers}. Not every cell needs hardware; the software tier
runs under `lavapipe` in CI, the rest is a documented manual checklist per release.

**Soak.** Eight hours of synthetic input with a document open; assert no growth in
resident memory beyond 5 % and no Wayland protocol errors in the log.

**Manual checklist (per release, recorded in the phase note).** Real tablet with pressure;
real HiDPI monitor at 1.25 and 1.5; screen reader walking the layer panel; dragging a file
from the file manager; copy/paste with GIMP and Inkscape.

---

## Memory note

Update **`docs/memory/ui.md`** (create it from the `INDEX.md` template at the start of the
phase). It must contain, by the end:

- **Current state:** which panels exist, what the canvas can do, what is stubbed.
- **The two closed questions**, each with its evidence table: the egui density spike
  (P1–P9) closing architecture §7 q2, and the winit evaluation (E1–E7) closing §7 q3,
  including the exact pinned version and the fallback that was *not* taken and why.
- **Decisions:** no `eframe`, and the reason (winit 0.30 pin kills stylus pressure); one
  `ScaleFactor` owner; pass order canvas → overlay → egui in a single encoder; the
  `Panel` trait as the toolkit-swap seam; thread topology and the rule that the render
  thread never sees the arena; which present mode is used when.
- **Invariants:** `winit`/`wgpu` appear only in `xarast-shell`; `egui` never below
  `xarast-ui`; no `async` outside the services thread; every pointer sample of a frame
  reaches the application; the canvas target format is not sRGB.
- **Dead ends:** anything tried and rejected during the spike — particularly any egui
  layout approach that looked dense enough and was not.
- **Open TODOs:** hardware tablet validation if it is still outstanding; X11 pressure via
  `octotablet` if deferred; the "delete the shim when egui adopts winit 0.31" tracking
  item; panels deferred to later phases.

Add the measured budget rows to **`docs/memory/perf.md`**, and note in
**`docs/memory/packaging.md`** any new runtime dependency the shell introduces (portals,
`libwayland`, Mesa expectations) and the current AppImage size.
