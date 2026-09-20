# Xarast — recommended technology stack

> **Clean-room notice.** This document analyses the *licensing implications* and
> the technical stack of Xarast against Xara Xtreme (GPL-2.0-only), for
> interoperability and design-decision purposes. It reproduces no source code
> from the original — the only verbatim quotation is the original's licence
> notice, §1.1, reproduced as documentary evidence — and the `file:line`
> references point at the reference tree in `xara-xtreme/` and serve only to
> locate the logic described. Xarast is implemented from this specification, not
> by translating the original.

> **Document:** `docs/research/05-technology-stack.md`
> **Research date:** 19 September 2026
> **Method:** direct verification against the `crates.io` API, `docs.rs`, the
> repositories' `README`/`CHANGELOG` files and web search. Every version,
> licence and update date in these tables was checked **on 2026-09-19**; none of
> it comes from the model's prior knowledge.
> **Scope:** Linux/Wayland as the primary platform; Windows and macOS in later
> phases. Initial packaging as an AppImage.

---

## 0. Executive summary

| Area | Choice | Alternative (plan B) |
|---|---|---|
| Project licence | **GPL-3.0-or-later** (clean room with respect to XaraLX) | LGPL-3.0+ for the core |
| UI | **egui 0.36 + egui_tiles + egui_extras**, without `eframe` | iced 0.14 |
| Window/input | **winit 0.31** (beta branch, `Pointer`/`TabletTool` API) | winit 0.30.13 + `octotablet` |
| Tablet/pressure | **winit 0.31 `TabletToolData`** (Wayland/Windows) + `octotablet` on X11/macOS | `wintab_lite` (Win), `input` (libinput) |
| GPU | **wgpu 30.0** (Vulkan/Metal/DX12/GLES) | GL fallback + lavapipe |
| 2D rasterisation | **vello_cpu 0.2 (reference) + vello 0.10 (GPU)** behind our own façade | lyon 1.0 + our own wgpu pipelines |
| Text | **parley 0.11 + harfrust + fontique + skrifa + peniko** | cosmic-text 0.19 |
| Image | **image 0.25 + zune-jpeg + png + image-webp + ravif + kamadak-exif + resvg/usvg 0.48** | zune-image as the façade |
| Container | **zip 8.6 (ZIP + zstd/deflate) + zstd 0.14** | tar + zstd |
| Geometry | **kurbo 0.13 + i_overlay 9.0 (+ i_curve under observation)** | lyon + flo_curves |
| Serialisation | **serde 1.0 + serde_json (manifests) + rkyv 0.8 (autosave/scratch)** | postcard |
| Undo | **invertible command pattern + persistent tree (imbl 7.0) + periodic snapshots** | pure snapshots |
| Concurrency | **rayon 1.12 + a dedicated render thread + crossbeam-channel**; tokio only minimally, for portals | — |
| Linux packaging | **AppImage** (linuxdeploy + appimagetool static runtime), glibc 2.35 baseline (Ubuntu 22.04) | Flatpak/Flathub as a second channel |
| Testing | **golden images (CPU backend) + image-compare + insta + criterion + cargo-fuzz + nextest** | divan, dify |

---

## 1. The licensing constraint — READ BEFORE CHOOSING ANYTHING

> **Update note (clean-room hygiene pass).** The analysis in this section starts
> from the assumption that Xarast might be a **derivative work** of Xara LX and
> therefore be covered by its GPL-2.0-only. That assumption **no longer holds**:
> Xarast is developed in a **clean room** from the specifications in
> `docs/research/`, without copying or translating code from the original, and
> the standing project decision is to publish it under **MIT OR Apache-2.0**.
> On that premise, the GPL-2.0-only ↔ Apache-2.0 incompatibility described below
> **does not apply to Xarast**, and neither does the "IMMEDIATE ACTION" in the
> DECISIONS section (replacing the `LICENSE` with GPL-3.0). The rest of the
> analysis (per-crate licences, `cargo-deny`, `cargo-about`) remains valid and
> necessary.

This is the most constraining decision in the whole document, and there is a real problem that has to be resolved **today**, not in phase 5.

### 1.1 Exactly which licence the original carries

The licence header of the real Xara LX source was verified (mirror `samuell/xara-xtreme`, `Kernel/group.h`). It says, literally:

```
Xara LX is free software; you can redistribute it and/or modify it
under the terms of the GNU General Public License version 2 as published
by the Free Software Foundation.
```

It is **GPL-2.0-only** ("version 2", with no "or later"). It also includes an *ADDITIONAL RIGHTS* section permitting linking against wxWidgets, wxXtra and the proprietary **CDraw** library (the render engine, which was never released as source).

### 1.2 The practical consequence: Apache-2.0 is incompatible with GPL-2.0-only

Apache-2.0 is **not compatible** with GPLv2 (the patent clause); it is compatible with GPLv3. And it turns out that a good part of the Rust graphics ecosystem is Apache-2.0 **on its own**, with no MIT dual licence:

| Crate | Licence | Compatible with GPL-2.0-only? | Compatible with GPL-3.0+? |
|---|---|---|---|
| `winit` | Apache-2.0 | ❌ **NO** | ✅ |
| `accesskit_winit` | Apache-2.0 | ❌ **NO** | ✅ |
| `xilem` / `masonry` | Apache-2.0 | ❌ NO | ✅ |
| `gpui` | Apache-2.0 | ❌ NO | ✅ |
| `parry2d`, `nalgebra` | Apache-2.0 | ❌ NO | ✅ |
| `flo_curves` | Apache-2.0 | ❌ NO | ✅ |
| `insta` | Apache-2.0 | ❌ NO | ✅ |
| `libdeflater` | Apache-2.0 | ❌ NO | ✅ |
| `unicode-linebreak` | Apache-2.0 | ❌ NO | ✅ |
| `slint` | **GPL-3.0-only** OR commercial | ❌ NO | ✅ (forces GPLv3) |
| `im` / `imbl` | MPL-2.0+ | ✅ (MPL is GPL-compatible) | ✅ |
| `icu_properties` | Unicode-3.0 | ✅ | ✅ |
| `zstd` (wraps zstd C) | BSD-3 (zstd upstream is BSD-3 **OR** GPL-2.0) | ✅ | ✅ |
| `ravif`, `rav1e`, `tiny-skia`, `kamadak-exif` | BSD-2/BSD-3 | ✅ | ✅ |
| rest of the stack | MIT OR Apache-2.0 / MIT | ✅ | ✅ |

**`winit` is pure Apache-2.0.** There is no realistic alternative to winit in Rust for modern Wayland. Therefore:

> ### ⚠️ MANDATORY LEGAL DECISION
> **Xarast CANNOT be GPL-2.0-only.** It must be licensed as **GPL-3.0-or-later** (or GPL-2.0-**or-later**, which allows the combination to be relicensed to v3). And as a direct consequence:
> **Xarast must be a clean-room reimplementation**: XaraLX code may not be copied, translated line by line or derived from, because that code is GPL-2.0-**only** and would drag the project into a licence incompatible with `winit`.
>
> What is lawful and safe: studying the **`.xar` file format** (formats are not copyrightable subject matter), the public documentation of the format, observable behaviour and the UX. Documenting the format in our own `docs/format/` before writing the parser (the classic clean-room procedure: one team reads, another implements).

### 1.3 An inconsistency to fix right away

`/home/user/Xarast/LICENSE` currently contains **MIT** (Copyright 2026 Jose Francisco Rives). That contradicts the stated goal of distributing under the GPL. An explicit decision is needed:

- **Option A (recommended):** `GPL-3.0-or-later` for the complete application. Consistent with the spirit of the original, compatible with the whole stack, and it lets the result be the "spiritual successor" to Xara Xtreme.
- **Option B:** a GPL-3.0+ binary, but the reusable low-level crates (`xarast-geom`, `xarast-xar`) published as `MIT OR Apache-2.0` so the community can use them. This is what Linebender and Graphite do; it maximises the impact of the work.
- **Option C (not recommended):** all MIT. Legal (there is no obligation to inherit the GPL in a clean-room effort), but it breaks the project's implicit promise.

### 1.4 Compliance tooling (mandatory in CI from day 1)

- **`cargo-deny` 0.20.2** (MIT OR Apache-2.0) — a `deny.toml` with `[licenses] allow = [...]`, explicitly forbidding `GPL-3.0-only`, `AGPL-*`, `LicenseRef-Slint-*` and any unlisted licence.
- **`cargo-about` 0.9.2** — generates the `THIRD-PARTY-LICENSES.html` that must ship inside the AppImage and in the "About" dialog.

---

## 2. UI toolkit

### 2.1 What a professional vector editor actually needs

Before comparing: the UI of Xara/Illustrator/Affinity is not an "application" UI, it is a **tool** UI. The requirements that really discriminate:

1. **Density**: 400+ visible controls, 18–22 px rows, none of the padding of a mobile app.
2. **Docking** with tabs, dragging between groups and layout persistence.
3. **Our own canvas at 60–144 fps** with wgpu integration *on the same surface* (not a desynchronised iframe/texture).
4. **Partial update**: dragging a handle must not repaint 400 widgets.
5. **Virtualised galleries** (thousands of thumbnails, layers, fonts, colours).
6. **Numeric fields with drag, units and expressions** (`12mm + 3pt`).
7. **Real accessibility** (AccessKit → AT-SPI on Linux) — a legal requirement in European public procurement.
8. **IME and complex text** for the text tool.
9. **Correct Wayland fractional DPI** (`wp_fractional_scale_v1`).

### 2.2 Comparison

| Crate | Version | Licence | Maintenance | Pros | Cons | Verdict |
|---|---|---|---|---|---|---|
| **egui** | **0.36.2** (2026-09-08) | MIT OR Apache-2.0 | ⭐ Excellent. 5.4 M downloads/90 d. Releases roughly every 2 months | Immediate mode = state always coherent with the document (ideal for an editor); first-class wgpu integration (`egui-wgpu`); AccessKit built in (Win/macOS/**Linux AT-SPI**); since 0.34 it uses **skrifa + vello_cpu** (hinting + variable fonts, crisp text); density configurable to the pixel; huge ecosystem (`egui_tiles`, `egui_dock`, `egui_extras`, `egui_kittest`); a real professional precedent: **Rerun** | Repaints the whole frame (mitigable with `request_repaint_after` and areas); the "one pass late" layout is awkward in complex designs; no declarative animations; `TextEdit` is not a professional text engine (we will have to build our own with parley for the canvas) | ✅ **CHOSEN** |
| **iced** | 0.14.0 (2025-12-07) | MIT | ⭐ Good, but slow releases (0.14 has been out 9 months) | Elm/retained, native partial update, its own `iced_wgpu` with primitive culling, *time-travel debugging*, *headless testing*, *hot reloading*, IME support; the basis of **COSMIC DE** (proof of scale) | A hard Elm learning curve for 400 widgets; the `canvas` widget is its own 2D one (not wgpu directly — you have to go through the `shader` widget); docking does not exist out of the box; AccessKit not yet integrated; a smaller ecosystem of "pro" widgets | 🟡 **A solid plan B** |
| **slint** | 1.18.0 (2026-09-16) | **GPL-3.0-only** OR royalty-free OR commercial | ⭐ Excellent (a company behind it) | Very polished, declarative DSL, live preview, good performance, `femtovg`/`skia` backends | ❌ **GPL-3.0-only forces GPLv3** and precludes any future relicensing; the royalty-free route requires showing an "AboutSlint" badge; the `.slint` DSL is one more language to maintain; integrating our own wgpu canvas is awkward (it has to go through `Window::set_rendering_notifier`); it is not designed for CAD-style density | ❌ Rejected (licence + fit) |
| **gpui** | 0.2.2 (2025-10-22) | Apache-2.0 | 🟠 Developed **inside Zed**, not as an independent product; pre-1.0 with frequent breakage; the community reports it has been abandoned as an independent open-source effort (a proliferation of `zui`-style forks) | Exceptional performance, excellent text, a very good element architecture | Documentation almost non-existent; unstable API; on Linux its support is what Zed needs, not what a graphics editor needs; no AccessKit | ❌ Rejected (governance risk) |
| **xilem** / **masonry** | 0.4.0 (2025-10-29) | Apache-2.0 | 🟡 Active (Linebender) but **10.7 K downloads in total** = not yet used in production | A first-rate reactive architecture; the same family as kurbo/parley/vello (natural integration); AccessKit from the start | Not production-ready (they say so themselves); a minimal widget catalogue; no docking; no galleries; pure Apache-2.0 | 🔭 **Watch** (review in 2027) |
| **dioxus** (desktop) | 0.7.10 (2026-07-31) | MIT OR Apache-2.0 | ⭐ Very active, a large community | Excellent React-style DX, hot reload, good tooling | The real *desktop* target is a WebView (or `blitz`, still immature) → no high-performance wgpu canvas and no control over stylus latency; a web model means density and shortcuts are a fight | ❌ Rejected (architecture) |
| **floem** | 0.2.0 (**2024-11-14**) | MIT | 🔴 **No crates.io release for 22 months** | Fine-grained reactivity (signals), good performance, the basis of Lapce | Publishing has stalled; small ecosystem; no AccessKit; no docking | ❌ Rejected (maintenance) |
| **makepad** | `makepad-widgets` 1.0.0 (2025-05-13); the `makepad` crate is a 2019 placeholder | MIT OR Apache-2.0 | 🟠 Active on GitHub but 1.2 K downloads/90 d | Shaders in the DSL, brutal performance, designed precisely for creative tools | An ecosystem closed in on itself; almost nobody outside uses it; no AccessKit; sparse documentation; limited IME/complex text | ❌ Rejected (risk) |
| **freya** | 0.4.3 / 0.5.0-rc.6 (2026-09-13) | MIT | 🟡 Active, one principal maintainer | Skia + Dioxus core, a pleasant API, accessibility via AccessKit | 6.6 K downloads/90 d; depends on Skia (large binary, heavy build); bus factor 1 | ❌ Rejected (maturity) |
| **Our own canvas + bespoke immediate-mode UI** | — | — | — | Total control, zero UI dependencies | 2–3 person-years just to reach parity with egui's widgets; reimplementing IME, AccessKit, text selection, accessibility… | ❌ Rejected, **but** see §2.4: we adopt the good half of it |

### 2.3 Firm recommendation: **egui 0.36, used as a library, not as a framework**

**egui** is chosen for three reasons that outweigh the rest:

1. **The immediate mode fits the problem.** In an editor, the UI is a *projection* of the document (layers, attributes of the selected object, galleries). With retained mode you have to keep two state trees in sync, and that is the number one source of bugs in editors. With egui, if the document changes, the UI is already correct on the next frame. The cost (repainting) is affordable: one egui UI frame on a modern GPU costs ~0.5–1.5 ms, and `request_repaint_after` already exists to sit at 0 fps when nothing is happening.
2. **wgpu integration is direct and on the same surface.** `egui-wgpu` gives us a `Renderer` that we hand *our* `wgpu::Device` and *our* `RenderPass`. The Xarast canvas and the UI share a device, a queue and a swapchain; there are no intermediate copies and no tearing between the canvas and the handles.
3. **Accessibility and text already solved and verified.** AccessKit 0.25 has a Unix adapter (AT-SPI over D-Bus) at "approximate parity" with Windows/macOS, including single- and multi-line text fields. And since egui 0.34 text is rasterised with **skrifa + vello_cpu**, with hinting and variable fonts.

### 2.4 An important architectural nuance: **do not use `eframe`**

Verified: `eframe 0.36.2` and `egui-winit 0.36.2` depend on **`winit ^0.30.13`**. But **pen/tablet support with pressure** only exists in **winit 0.31** (see §3). In other words: if we use `eframe`, we give up stylus pressure until egui bumps winit, which is unacceptable for a drawing tool.

**Decision:** write our own *shell* (`xarast-shell`) that:

- owns the `winit 0.31` loop (`ApplicationHandler`, `Window` as a trait),
- owns the `wgpu::Surface`/`Device`/`Queue`,
- translates winit 0.31 events → `egui::RawInput` (a reimplementation of `egui-winit`, ~1000–1500 lines; a good part of it can be ported from the MIT/Apache original),
- wires up `accesskit_winit 0.34` directly,
- and draws: `[Xarast canvas pass] → [egui-wgpu pass]` in the same `CommandEncoder`.

This is exactly the "own canvas + immediate-mode UI" approach from the list, but leaning on egui for the widgets rather than writing them. Cost: ~2 weeks of initial work plus maintenance of the shim on every version bump. Benefit: total independence from `eframe`'s release cadence, stylus pressure from day 1, and control over presentation timing (critical for stroke latency).

### 2.5 Chosen UI add-ons

| Crate | Version | Licence | What for | Notes |
|---|---|---|---|---|
| **egui_tiles** | 0.17.1 (2026-08-18) | MIT OR Apache-2.0 | **Docking / dockable panels** | From Rerun; nestable tabs/linear/grid containers, drag-and-drop between groups, serialisable. It is what a real professional app uses |
| `egui_dock` | 0.21.1 (2026-08-06) | MIT | Docking alternative | More "IDE-like"; well maintained (4.7 M downloads) but `egui_tiles` has the better tree model |
| **egui_extras** | 0.36.2 | MIT OR Apache-2.0 | `TableBuilder` (layer tree, virtualised galleries), `DatePicker`, image loaders | Official |
| **accesskit_winit** | 0.34.0 | Apache-2.0 | AT-SPI/UIA/NSAccessibility | Note: pure Apache-2.0 → reinforces GPLv3 |
| **arboard** | 3.6.1 | MIT OR Apache-2.0 | Clipboard (images + text) | With `smithay-clipboard` on Wayland |
| **rfd** | 0.17.2 | MIT | File dialogs | Uses **XDG portals** on Wayland → works inside a Flatpak/AppImage sandbox |
| **ashpd** | 0.13.13 | MIT | XDG portals (settings, dark/light theme, screenshot, file chooser) | For detecting `org.freedesktop.appearance color-scheme` and respecting the system theme |

### 2.6 Risks in the UI area

| Risk | Probability | Impact | Mitigation |
|---|---|---|---|
| Our own winit 0.31 ↔ egui shim drifts out of sync with upstream | Medium | Medium | Keep it in an isolated crate (`xarast-egui-winit`), with tests; go back to upstream `egui-winit` once it moves to 0.31 |
| winit 0.31 stays in beta and breaks the API | **High** | Medium | Pin `=0.31.0-beta.3`, encapsulate all of winit behind `xarast-shell`; budget 1 week per beta |
| Immediate mode not good enough for very heavy panels (a gallery of 5000 fonts) | Medium | Low | Virtualisation with `egui_extras::TableBuilder` + a thumbnail cache in an atlas texture |
| AccessKit on Linux incomplete for exotic widgets | Medium | Low | The critical controls (menus, fields, layer tree) use standard egui widgets |

---

## 3. Windowing and input

### 3.1 winit: status verified in 2026

- **`winit 0.30.13`** — stable, released 2026-09-04, Apache-2.0, 10.4 M downloads/90 d.
- **`winit 0.31.0-beta.3`** — released 2026-09-04. It is a **major restructuring**: the crate is split into `winit-core`, `winit-wayland`, `winit-x11`, `winit-win32`, `winit-appkit`, `winit-web`, `winit-android`, `winit-orbital`; `ActiveEventLoop` and `Window` become **traits**; `inner_*` is renamed to `surface_*`.

What is new in 0.31, verified against the official changelog and relevant to Xarast:

**Pen/tablet input (the decisive part):**
- *"Add Pen input support on Wayland, Windows, and Web via new Pointer event."*
- *"Add `PointerKind`, `PointerSource`, `ButtonSource`, `FingerId`, `primary` and `position` to all pointer events."*
- `PointerSource::TabletTool { kind: TabletToolKind, data: TabletToolData }`, where **`TabletToolData`** exposes (verified on docs.rs):
  - `force: Option<Force>` — pressure
  - `tangential_force: Option<f32>` — barrel pressure (−1..1)
  - `twist: Option<u16>` — rotation of the tool, 0..359°
  - `tilt: Option<TabletToolTilt>` — tilt in degrees
  - `angle: Option<TabletToolAngle>` — angular position in radians
- On iOS: *"Apple Pencil support with force, altitude, and azimuth data."*

**Wayland:**
- `HoldGesture`, `PanGesture`, `PinchGesture`, `RotationGesture` (trackpad gestures → canvas zoom/pan for free).
- `ext-background-effect-v1` (compositor blur/vibrancy).
- `Window::set_window_icon` implemented.
- Fractional scaling (`wp_fractional_scale_v1`) supported; custom cursors are scaled fractionally too.
- A protocol-error fix for custom cursors on `wl_surface` < v3.
- CSD via SCTK + `sctk-adwaita` (winit handles it; on GNOME it is mandatory).

**Drag and drop redesigned:** `DroppedFile`/`HoveredFile`/`HoveredFileCancelled` are gone; `DragEntered`/`DragMoved`/`DragDropped`/`DragLeft` arrive. In addition `url` is removed from `winit-core`: you have to use `SendData::Uris` / `TypedData::try_as_uris` with `file:` URIs.

### 3.2 Windowing-layer comparison

| Crate | Version | Licence | Maintenance | Pros | Cons | Verdict |
|---|---|---|---|---|---|---|
| **winit** | 0.31.0-beta.3 / 0.30.13 stable | Apache-2.0 | ⭐⭐ The absolute reference. 53.8 M downloads in total | The only one with complete native Wayland (fractional scaling, CSD, gestures, indirect portals), **tablet with pressure/tilt/twist in 0.31**, modern DnD, direct integration with wgpu and accesskit | Pure Apache-2.0 (→ forces GPLv3); the API breaks between minors; 0.31 still beta; does not expose the tablet on X11/macOS | ✅ **CHOSEN** |
| **sdl3** | 0.20.0 (2026-09-07) | MIT (bindings) / SDL itself is Zlib | ⭐ Very active (207 K downloads) | SDL3 has a unified, mature tablet/pen API on **every** platform, macOS included; gamepads, audio, clipboard | An external C dependency (heavier AppImage packaging); does not fit the `ApplicationHandler` model; its Wayland is good but less "native" in the details (CSD delegated to libdecor); it would duplicate the loop with egui | 🟡 Plan B (see §3.4) |
| **smithay-client-toolkit** | 0.21.1 (2026-07-23) | MIT | ⭐⭐ Excellent — it is what winit is built on | Absolute control of Wayland (`tablet_v2`, `text-input-v3`, `cursor-shape-v1`, fractional scale) | **Linux/Wayland only**; the Windows/macOS backend would have to be written by hand | 🔧 Surgical use: reaching protocols that winit does not expose |
| **glazier** | — | Apache-2.0 | 🔴 **Archived** by Linebender (absorbed into winit/masonry) | — | Dead | ❌ Rejected |

### 3.3 Graphics tablets with pressure — the real state of play in Rust (2026)

This is the weakest point in the ecosystem and it deserves an explicit plan.

| Option | Version | Licence | Platforms | Data | Verdict |
|---|---|---|---|---|---|
| **winit 0.31 `TabletTool`** | 0.31.0-beta.3 | Apache-2.0 | Wayland ✅, Windows (Ink) ✅, Web ✅, iOS ✅ · X11 ❌, macOS ❌ | force, tangential_force, tilt, twist, angle | ✅ **Primary route** |
| **octotablet** | 0.1.0 on crates.io (**2024-03-16**), repo with activity | MIT | Wayland `tablet_unstable_v2` ✅ complete, Windows Ink/RTS ✅ complete, X11/XInput2 🟡 "attempted", macOS ❌ | pressure, tilt, distance, wheel, pad buttons, *pads* and *rings* | 🟡 **A complement** for X11 and for pad buttons/rings, which winit does not expose. Risk: crates.io frozen at 0.1.0, 654 downloads/90 d, bus factor 1 → **vendor it** |
| **`input` (libinput)** | 0.10.0 (2026-04-05) | MIT | Linux, requires access to `/dev/input` or a seat via logind | Everything libinput offers | ❌ Not appropriate for a desktop app under Wayland (the compositor already owns the devices) |
| **`wintab_lite`** | 1.0.1 (2024-04-19) | MIT | Windows (Wintab) | pressure, tilt | 🟡 Only if Windows Ink gives trouble with external Wacom/Huion tablets (a known case in Photoshop) |
| **macOS NSEvent** | — | — | macOS | `NSEventTypeTabletPoint`: pressure, tilt, rotation, tangentialPressure | 🔧 **To be implemented by hand** with `objc2`/`objc2-app-kit` in the macOS phase. It is ~200 lines |

**Recommended architecture:** our own `xarast_input::TabletSource` trait with interchangeable implementations:

```
TabletSource
 ├── WinitTabletSource     (Wayland, Windows, Web, iOS)  ← default
 ├── OctotabletSource      (X11, and pads/rings on Wayland) ← optional feature
 └── AppKitTabletSource    (macOS, our own)                ← phase 3
```

And a normalised `StrokeSample { x, y, pressure, tilt_x, tilt_y, twist, timestamp, source }`, with our own **interpolation/prediction**. Important: for stroke latency you have to sample at the device's rate (Wacom ~200 Hz), not at the frame rate; winit delivers events coalesced by the compositor, so you have to accumulate every `PointerMoved` in the frame, not just the last one.

### 3.4 Risks in the input area

| Risk | Prob. | Impact | Mitigation |
|---|---|---|---|
| winit 0.31 does not reach stable within 6 months | Medium | **High** | Pin the exact beta; the shim isolates the API. If it stalls: winit 0.30 + `octotablet` (both work side by side) |
| Pressure unavailable on X11 | High | Medium | `octotablet` (XInput2); document X11 as "degraded support" — the priority is Wayland |
| Wayland compositors without `tablet_v2` (some older wlroots) | Low | Low | Degrade to a mouse with constant pressure; warn in the UI |
| Perceptible stroke latency | Medium | **High** | Presentation with `PresentMode::Mailbox`/`Fifo` as appropriate; render the in-progress stroke in a separate lightweight pass; consider 1-frame prediction |

---

## 4. GPU

### 4.1 wgpu 30

Verified: **`wgpu 30.0.1`**, released **2026-08-22**, `MIT OR Apache-2.0`, 10.6 M downloads/90 d. **Rust MSRV 1.87** with an explicit policy of never going beyond `stable − 3`.

Backends supported in v30: **Vulkan, Metal, DX12, GLES/OpenGL, WebGPU** (+ Vulkan on OpenHarmony).

Relevant v30 news:
- `SurfaceConfiguration.color_space` → **HDR and wide gamut** (important for a graphics app that will one day want Display-P3/Rec.2020).
- `TextureViewDescriptor.swizzle` (`TEXTURE_COMPONENT_SWIZZLE`) → useful for mask/alpha channels without copies.
- `Queue::present(surface_texture)` replaces `SurfaceTexture::present()`.
- Breakages: integer interpolation is no longer `flat` by default (you have to annotate `@interpolate(flat)`); vertex buffer slots and bind group layouts become `Option<_>`.

### 4.2 GPU-layer comparison

| Option | Version | Licence | Maintenance | Pros | Cons | Verdict |
|---|---|---|---|---|---|---|
| **wgpu** | 30.0.1 | MIT OR Apache-2.0 | ⭐⭐ Firefox + Deno + Bevy behind it | One shader language (WGSL) for Vulkan/Metal/DX12/GLES; excellent validation; `wgpu-profiler` and RenderDoc capture; automatic downlevel to GLES for old GPUs; egui already uses it | An abstraction layer means some cost; compute shaders limited in the GLES downlevel (affects Vello GPU) | ✅ **CHOSEN** |
| `ash` (Vulkan directly) | — | MIT | Active | Maximum control, no overhead | Vulkan only → Metal and DX12 would have to be written separately. Triples the work | ❌ |
| `glow` (GL 3.3/ES 3.0) | — | MIT | Active | Works on anything that still breathes | No compute; none of wgpu's guarantees | 🟡 Only as a wgpu backend (`--features glow`) |
| Pure CPU (`vello_cpu`/`tiny-skia`) | — | — | — | Zero GPU dependencies | 5–30× slower | 🟡 A mandatory fallback, see §4.3 |

### 4.3 Old GPUs and software rendering — a tiered strategy

A design app has to start **always**, even over SSH+X11 or in a VM with no GPU. A four-tier plan with automatic degradation:

| Tier | Requirement | wgpu backend | Canvas rasteriser | Expected performance |
|---|---|---|---|---|
| **0 — Fast** | Vulkan 1.1 + compute (GPU ≥ 2016) | Vulkan/Metal/DX12 | `vello` (compute) | 60–144 fps, large documents |
| **1 — Standard** | Vulkan/DX12 without advanced compute, or GL 4.3 | Vulkan/DX12/GLES | `vello_hybrid` (CPU stripping + GPU fill) | 60 fps |
| **2 — Compatible** | GL 3.3 / GLES 3.0 (Intel HD 2010+) | GLES via `glow` | `vello_cpu` into textures + GPU compositing | 20–60 fps |
| **3 — Software** | No GPU: **lavapipe** (`VK_ICD_FILENAMES`) or **llvmpipe** | Vulkan (lavapipe) or GLES (llvmpipe) | `vello_cpu` with `rayon` | 5–20 fps, usable for editing |

Implementation:
- At startup, `Instance::request_adapter` with `power_preference: HighPerformance`; if that fails, retry with `force_fallback_adapter: true` (which is precisely lavapipe/llvmpipe/WARP).
- Inspect `Adapter::get_downlevel_capabilities()` and `Features::…` to pick the tier.
- **Escape-hatch environment variables** `XARAST_RENDERER=cpu|hybrid|gpu` and `WGPU_BACKEND=vulkan|gl`, documented, because 80 % of support bugs on Linux are resolved with them.
- Do **not** bundle Mesa in the AppImage: the system one is used (see §11.3).

### 4.4 2D rasterisation of the canvas (the heart of the matter)

This was not in the list of areas, but it is the technical decision with the most consequences, so it is documented here.

| Option | Version | Licence | Maintenance | Pros | Cons | Verdict |
|---|---|---|---|---|---|---|
| **vello_cpu** | 0.2.0 (2026-08-07) | Apache-2.0 OR MIT | ⭐ Linebender; 3.07 M downloads/90 d (egui uses it!) | The Vello README calls it *"the most mature choice"*; **deterministic** → perfect for golden tests; SIMD; works on any machine | CPU-bound on large documents | ✅ **Reference backend + fallback** |
| **vello** (GPU compute) | 0.10.0 (2026-08-14) | Apache-2.0 OR MIT | ⭐ Active | A prefix-sum pipeline: sorting and clipping on the GPU with no intermediate textures; scales to enormous scenes | *"intended to become the primary renderer… as it matures; Vello CPU is currently overall more mature"*; requires compute; known conflation artefacts | ✅ **Accelerated backend** (tier 0) |
| **vello_hybrid** | 0.2.0 (2026-08-07) | Apache-2.0 OR MIT | 🟡 New (41 K downloads in total) | The CPU does the *stripping*, the GPU fills → works without compute, even on WebGL2 | Very young | 🟡 Tier 1 |
| **lyon** | 1.0.19 (2026-03-08) | MIT OR Apache-2.0 | ⭐ Maintained (2 M downloads/90 d) | Thoroughly proven tessellation to triangles (historically the basis of Firefox/WebRender); total control of the pipeline; straightforward MSAA | You have to implement by hand: clipping, transparency groups, blend modes, complex gradients, feathering; antialiasing quality inferior to analytic AA | 🟡 **Plan B / occasional tessellator** |
| **tiny-skia** | 0.12.0 (2026-02-02) | BSD-3 | 🟡 Maintenance (Linebender) | A Skia port, very correct, the basis of resvg | CPU only, no modern SIMD comparable to vello_cpu, a closed API | 🟡 We will pull it in via resvg |
| Skia (`skia-safe`) | — | BSD-3 | ⭐ Google | The most complete thing in existence | +40 MB of binary, a 40-minute build, C++ | ❌ Rejected (weight and build) |

**Recommendation:** define **our own façade `xarast-raster`** with a `trait Rasterizer`, and **two implementations from day 1** (CPU and GPU), both fed by the same scene model based on **`peniko` 0.6** (brush/gradient/blend types shared with kurbo and vello). Reasons:

1. The CPU version is the **oracle** for the golden tests (§12) and the tier-3 fallback. Having it from the start stops the GPU becoming "the only truth" and stops the tests being irreproducible in CI.
2. Xara's characteristic effects (**feathering**, blends between objects, shadows, per-object transparency) are **not** covered by Vello. They will be **our own wgpu passes** compositing layer textures. That is: Vello rasterises *paths* into layer textures; **our compositor** (our own shaders) does blend modes, feather and live effects. This separation keeps a rasteriser swap (Vello → lyon) local.

---

## 5. Text

### 5.1 Requirements

Text on a path, manual kerning (pair by pair, editable), OpenType features (`liga`, `smcp`, `onum`, `ss01`…), bidi, variable fonts, system fonts on all 3 platforms, and — crucially — **access to glyph outlines** in order to convert text to curves.

### 5.2 Comparison

| Crate | Version | Licence | Maintenance | Pros | Cons | Verdict |
|---|---|---|---|---|---|---|
| **parley** | **0.11.1** (2026-08-16) | Apache-2.0 OR MIT | ⭐⭐ Linebender; 1.72 M downloads/90 d | A complete, coherent stack: shaping with **harfrust**, bidi/segmentation with **ICU4X**, system fonts with **fontique**, parsing with **skrifa**, types with **peniko**; selection/editing utilities; an `accesskit` feature; a `complex-scripts` feature | Pre-1.0 (the API can break); sparse platform documentation; MSRV 1.88 | ✅ **CHOSEN** |
| **harfrust** | **0.13.3** (2026-08-25) | MIT | ⭐⭐ From the **HarfBuzz organisation**; 5.3 M downloads/90 d | A Rust port of HarfBuzz by the HarfBuzz team themselves → the de facto successor to rustybuzz; complete OpenType features; no C dependencies | Relatively new as a name (but the code comes from rustybuzz/HarfBuzz) | ✅ **CHOSEN** (via parley) |
| **fontique** | 0.11.1 (2026-08-16) | Apache-2.0 OR MIT | ⭐ Linebender | System font enumeration + **fallback by script** on Linux (fontconfig), Windows (DirectWrite) and macOS (CoreText); collections and generic families | Pre-1.0 | ✅ **CHOSEN** |
| **skrifa** | **0.47.0** (2026-09-08) | MIT OR Apache-2.0 | ⭐⭐ **Google Fonts** (the `fontations` project); 13.9 M downloads/90 d | Reading of outlines, metrics, variations, hinting, COLRv1, bitmaps; it is Chrome/Skia's font engine in Rust | — | ✅ **CHOSEN** (text→curves) |
| **cosmic-text** | 0.19.0 (2026-04-22) | MIT OR Apache-2.0 | ⭐ System76/COSMIC; 3.07 M downloads/90 d | Heavily proven (it is COSMIC DE's text engine); an editor with cursor/selection; shaping with rustybuzz, swash for rasterising | Aimed at *text editors*, not *design typography*: less direct control of OpenType features, no API designed for text on a path or manual kerning; drags in swash+rustybuzz (superseded by skrifa+harfrust) | 🟡 **Plan B** |
| **swash** | 0.2.10 (2026-07-17) | Apache-2.0 OR MIT | 🟡 One author (dfrg), 4.9 M downloads/90 d | Shaping + scaling + rasterisation in one crate; very fast | Bus factor 1; the ecosystem (parley, egui) is migrating to skrifa/harfrust | ❌ |
| **rustybuzz** | 0.20.1 (**2024-11-12**) | MIT | 🟠 Frozen since 2024; the effort moved to **harfrust** (same org) | A faithful HarfBuzz port, widely used (10 M downloads/90 d out of inertia) | In maintenance mode; harfrust is its successor | ❌ (use harfrust) |
| **harfbuzz_rs** | 2.0.1 (**2021-08-28**) | MIT | 🔴 **Abandoned 5 years ago** | Links the real HarfBuzz C (maximum fidelity) | Unmaintained; a C dependency in the AppImage; 9 K downloads/90 d | ❌ |
| **fontdb** | 0.24.0 (2026-07-29) | MIT | ⭐ RazrFalcon | A simple, solid font database; 11.3 M downloads/90 d | Less capable than fontique at fallback by script | 🟡 It arrives anyway as a dependency of `usvg` |
| `font-kit` | 0.14.3 (2025-05-26) | MIT OR Apache-2.0 | 🟠 Servo, slow pace | A system API on 3 platforms | Superseded by fontique | ❌ |

### 5.3 Firm recommendation

**parley 0.11 + harfrust + fontique + skrifa + peniko.** The decisive reason is the coherence of the stack: kurbo (geometry), peniko (paint), parley (text), vello (rasterisation) and fontique (fonts) are **the same family**, share types and are versioned together. We avoid conversions and type mismatches at the boundaries, which is where precision is lost in a vector editor.

**Xarast-specific functions and how they are built:**

| Requirement | Solution |
|---|---|
| **Text on a path** | Our own: `parley` yields the *glyph runs* with advances; `kurbo::ParamCurveArclen` gives the arc-length parameterisation of the path; we place each glyph with its transform (position + tangent). ~300 lines. No crate provides this ready-made |
| **Manual kerning** | Our own: parley delivers positions after OpenType kerning; we keep a `HashMap<(glyph_idx, glyph_idx), f64>` of the user's adjustments in the document and apply them after shaping |
| **OpenType features** | `parley` allows `FontFeature`/`FontVariation` per style range; harfrust applies them |
| **Bidi** | ICU4X inside parley (`unicode-bidi` is not needed explicitly) |
| **Text → curves** | `skrifa::outline::OutlinePen` → `kurbo::BezPath`. Direct |
| **System fonts** | `fontique` (fontconfig / DirectWrite / CoreText) |
| **Fonts embedded in the document** | `skrifa` reads from a `&[u8]`; we store the file inside the `.xarast` ZIP |

### 5.4 Risks

| Risk | Prob. | Impact | Mitigation |
|---|---|---|---|
| Pre-1.0 parley breaks the API | High | Low | Wrap it in `xarast-text`; Linebender's breakages are mechanical |
| Imperfect CJK/Arabic font fallback on Linux | Medium | Medium | Tests with multi-script documents; the option to force a fallback chain in preferences |
| Shaping performance on text-heavy documents | Medium | Medium | A layout cache per text object, invalidated by a hash of content+style |

---

## 6. Image

| Crate | Version | Licence | Maintenance | Pros | Cons | Verdict |
|---|---|---|---|---|---|---|
| **image** | 0.25.10 (2026-03-10) | MIT OR Apache-2.0 | ⭐⭐ 50.9 M downloads/90 d | A universal façade: PNG, JPEG, GIF, WebP, TIFF, BMP, TGA, DDS, HDR, EXR, QOI, farbfeld, AVIF (with features); resize operations with quality filters; a convenient `DynamicImage` | Some of its own decoders are slower than the specialised ones; a limited colour API (no ICC management) | ✅ **CHOSEN (façade)** |
| **zune-jpeg** | 0.5.15 (2026-09-08) | MIT OR Apache-2.0 OR Zlib | ⭐⭐ 41.5 M downloads/90 d | **The fastest JPEG decoder in Rust** (SIMD); `image` can already delegate to it | — | ✅ **CHOSEN** |
| **png** | 0.18.1 (2026-02-14) | MIT OR Apache-2.0 | ⭐⭐ image-rs; 72 M downloads/90 d | Complete (APNG, 16-bit, ICC, chunks), fast | — | ✅ **CHOSEN** |
| **image-webp** | 0.2.4 (2025-08-27) | MIT OR Apache-2.0 | ⭐ image-rs; 26.9 M downloads/90 d | **Pure Rust**, decodes lossy+lossless+animated, encodes lossless | Does not encode lossy WebP | ✅ **CHOSEN** (decode + lossless encode) |
| `webp` (libwebp) | 0.3.1 (2025-08-29) | MIT OR Apache-2.0 (libwebp: BSD-3) | 🟡 | Reference lossy encoding | A C dependency in the AppImage | 🟡 **Optional feature** `webp-lossy` |
| **ravif** + **rav1e** | 0.13.0 / 0.8.1 | BSD-3 / BSD-2 | ⭐ 15 M downloads/90 d | Quality pure-Rust AVIF encoding; GPL-compatible | Encoding AVIF is slow (use `rayon` + a separate thread) | ✅ **CHOSEN** (AVIF export) |
| **kamadak-exif** | 0.6.1 (2024-11-06) | BSD-2 | 🟡 Stable, unchanged since 2024 (but it is a stable format) | Reads/writes EXIF in JPEG, TIFF, PNG, WebP, HEIF; 3.17 M downloads/90 d | A somewhat verbose API | ✅ **CHOSEN** |
| **resvg** + **usvg** | 0.48.1 (2026-08-02) | Apache-2.0 OR MIT | ⭐ **Now maintained by Linebender** (good news: it used to be bus factor 1 with RazrFalcon); 9.9 M downloads/90 d | Rust's best SVG parser/normaliser; `usvg` yields an already-resolved tree (references, `use`, CSS styles, units) which is exactly what an **importer** needs, not a rasteriser | Drags in `tiny-skia` and `fontdb`; partial SVG 2 | ✅ **CHOSEN** — but use **`usvg` as the parser and translate its tree into our model**, do not use `resvg` to paint |
| **zune-image** | 0.5.0 (2026-01-24) | MIT OR Apache-2.0 OR Zlib | 🟡 100 K downloads in total — little adopted | Very fast, an operation-pipeline architecture | A small ecosystem; fewer formats than `image`; a less stable API | 🟡 Use only its individual decoders (zune-jpeg) |
| `jpeg-decoder` | 0.3.2 (2025-06-21) | MIT OR Apache-2.0 | ⭐ image-rs | Correct, supports progressive JPEG | Slower than zune-jpeg | 🟡 A fallback for rare cases |
| **oxipng** | 10.2.1 (2026-09-02) | MIT | ⭐ Active | Lossless PNG optimisation on export | Slow (run it on a background thread, optional) | ✅ Optional in "Export optimised" |

**An additional decision — colour management:** none of these does CMS. For a professional tool we have to add **`qcms`** (Mozilla, MPL-2.0, GPL-compatible) or `lcms2` (MIT, but it is C) for ICC profiles. This is deferred to phase 2 but **room must be reserved for it in the colour model from day 1** (do not assume sRGB everywhere: store the colour space in every bitmap and in the document).

---

## 7. Compression and the `.xarast` container

### 7.1 Container design

Recommendation: **`.xarast` = a ZIP file** with this structure (the OPC/ORA/KRA model, proven in Krita — not in Blender's `.blend`, but yes in `.ora`, `.kra`, `.sla`, `.afdesign`):

```
document.xarast              (ZIP, unencrypted)
├── mimetype                  (STORED, first entry, uncompressed → "magic bytes" identifiable by file(1)/MIME)
├── manifest.json             (DEFLATE — format version, index of parts, checksums)
├── document.bin              (ZSTD  — serialised object tree, our own binary format)
├── thumbnail.png             (STORED — 256×256, for file managers)
├── preview.png               (DEFLATE — full render, for "open recent")
├── resources/
│   ├── bitmaps/<uuid>.png|.jpg|.avif   (STORED — already compressed)
│   ├── fonts/<hash>.ttf                (DEFLATE)
│   └── profiles/<hash>.icc             (DEFLATE)
└── history/                  (optional, ZSTD — persistent undo stack)
```

Advantages of ZIP over a monolithic in-house format: it can be inspected with `unzip`, it allows **partial reading** (opening the thumbnail without decompressing the document), it allows **incremental writing** when saving (rewriting only the changed entries), and existing data-recovery tools work on it.

### 7.2 Comparison

| Crate | Version | Licence | Maintenance | Pros | Cons | Verdict |
|---|---|---|---|---|---|---|
| **zip** | **8.6.0** (2026-08-11) | MIT | ⭐⭐ 71 M downloads/90 d; `zip-rs/zip2` is the active, maintained fork | A simple synchronous API; **supports out of the box deflate (with the `zlib-rs` backend), deflate64, bzip2, zstd, lzma, xz, zopfli**; AES; streaming writes; ZIP64 | 9.0 is in pre-release (pin to 8.x); many features on by default → turn off the ones we do not use | ✅ **CHOSEN** with `default-features = false, features = ["deflate", "zstd"]` |
| `rc-zip` | 5.4.1 (2025-11-19) | Apache-2.0 OR MIT | 🟡 545 K downloads/90 d | An elegant sans-io design, very tolerant of corrupt ZIPs | Read-only (writing via a separate `rc-zip-tokio`/sync); a smaller ecosystem | 🟡 Plan B for the **tolerant importer** of damaged files |
| `async_zip` | 0.0.19 (2026-08-22) | MIT | 🟡 Active but 0.0.x | Async | **We do not need async** for local files; version 0.0.x | ❌ |
| **zstd** | **0.14.0** (2026-09-04) | BSD-3 (zstd C is BSD-3 **OR** GPL-2.0) | ⭐⭐ 82 M downloads/90 d | The best ratio/speed on the market; levels 1–22; **dictionaries** (key point: training a dictionary on typical documents greatly improves small files); multithreaded | A C dependency (built with `cc`, no problem in an AppImage since it is static) | ✅ **CHOSEN** for `document.bin` |
| **flate2** | 1.1.10 (2026-08-28) | MIT OR Apache-2.0 | ⭐⭐ 158 M downloads/90 d | Backends: `miniz_oxide` (the default, pure Rust), **`zlib-rs` (pure Rust, the fastest)**, `zlib-ng` (C), `zlib` (C), `cloudflare_zlib` | — | ✅ **CHOSEN** with the **`zlib-rs`** backend: zlib-ng speed with no C dependency, better for the AppImage and for cross-compiling to aarch64 |
| `brotli` | 9.0.0 (2026-09-02) | BSD-3 AND MIT | ⭐⭐ 60 M downloads/90 d | A better ratio than deflate on text | Slower than zstd at the same ratio; adds nothing over zstd here | ❌ (it would be useful if there is ever a web export) |
| `libdeflater` | 1.26.1 (2026-09-17) | Apache-2.0 | ⭐ | The fastest deflate for PNG | Pure Apache-2.0; the `png` crate already does fine | ❌ |
| `lz4_flex` | 0.14.0 (2026-07-14) | MIT | ⭐⭐ 34 M downloads/90 d | **Ultra-fast** compression (GB/s) | Poor ratio | ✅ **Yes, but for something else**: compressing the render cache *tiles* and the undo snapshots in RAM, where latency matters, not size |

### 7.3 Decision

- **Container:** `zip 8.6` (`deflate`+`zstd`), deflate backend = `zlib-rs`.
- **Main payload:** `zstd 0.14` level 3 on save (fast), level 19 for "Save maximally compressed".
- **In-memory cache and undo:** `lz4_flex`.
- **Legacy `.xar`:** our own read-only parser, fed by `cargo-fuzz` (§12).

---

## 8. Geometry

### 8.1 Comparison of the base

| Crate | Version | Licence | Maintenance | Pros | Cons | Verdict |
|---|---|---|---|---|---|---|
| **kurbo** | **0.13.1** (2026-05-13) | Apache-2.0 OR MIT | ⭐⭐ Linebender; 17.6 M downloads/90 d | **Verified on docs.rs, and it covers almost everything we need**: `stroke()`/`stroke_with()` (**stroke-to-path / stroke expansion**), an `offset` module (offsetting of cubics), a `simplify` module + `fit_to_bezpath`/`fit_to_bezpath_opt`/`fit_to_cubic` (**simplification and refitting**), `dash()` (**dash patterns**), `ParamCurveArclen` (arc length → text on a path), `ParamCurveArea`/`ParamCurveMoments` (area and moments), `ParamCurveNearest` (snapping), ellipses, arcs. Double f64. Author: Raph Levien | Booleans **not** included; curve-curve intersections limited | ✅ **CHOSEN (core)** |
| **lyon** | 1.0.19 (2026-03-08) | MIT OR Apache-2.0 | ⭐ Active (2 M downloads/90 d) | Fill/stroke tessellation to triangles, hit-testing, `lyon_algorithms` (walk, raycast, aabb) | f32; we do not need tessellation if we use Vello | 🟡 **Rasterisation plan B** + occasional tools |
| `euclid` | 0.22.14 (2026-03-18) | MIT OR Apache-2.0 | ⭐ Servo; 27.7 M downloads/90 d | **Typed spaces** (`Point2D<f64, DocumentSpace>` vs `ScreenSpace`) — it prevents transform bugs, very valuable in an editor with several coordinate systems | Duplicates types with kurbo | 🟡 Consider only for the space *newtypes*; alternative: our own newtypes over `kurbo::Point` |
| `parry2d` | 0.31.1 (2026-09-18) | **Apache-2.0** | ⭐ Dimforge, very active | Collision, BVH, distance, convex hull | Designed for physics, not for editing; **pure Apache-2.0**; its "shape" model does not match Bézier paths | ❌ Rejected |
| `geo` | 0.33.1 (2026-04-20) | MIT OR Apache-2.0 | ⭐ GeoRust | Robust predicates, simplification (Douglas-Peucker, VW), polygon booleans | A GIS world: polylines only, no Béziers; geographic coordinates | ❌ Rejected |
| `glam` | 0.33.7 (2026-09-07) | MIT OR Apache-2.0 | ⭐⭐ 49 M downloads/90 d | SIMD, it is what wgpu/bytemuck expect | f32 | ✅ **For uniforms/GPU only.** The document model stays in f64 with kurbo |
| `nalgebra` | 0.35.0 | Apache-2.0 | ⭐ | General algebra | Heavy, pure Apache-2.0, unnecessary | ❌ |

### 8.2 Boolean path operations — the analysis you asked for

This is the most delicate subsystem in a vector editor. Status verified on 2026-09-19:

| Crate | Version | Licence | Maintenance | Pros | Cons | Verdict |
|---|---|---|---|---|---|---|
| **i_overlay** | **9.0.0** (released **today, 2026-09-19**) | MIT OR Apache-2.0 | ⭐⭐ **The most active by a distance**: 4.04 M downloads/90 d out of 8.39 M in total → almost half of its historical downloads are from the last 90 days. Constant releases | Union/intersection/difference/xor + **self-intersections**; even-odd and non-zero rules; **integer and floating-point APIs**, with a *fixed-scale/grid_size* mode that gives **stable, predictable** results (this is gold for an editor: reproducible booleans); an API with reusable buffers (no allocations in a loop); first-rate performance; used in GIS/CAD | **Polygons only**: the Béziers have to be flattened first and the curves refitted afterwards | ✅ **CHOSEN** |
| **i_curve** | **0.2.0** (**2026-09-19**) | MIT | 🔴 Brand new: **168 downloads in total**, released today | Booleans **natively on cubic Béziers and rational elliptical arcs** — exactly the ideal; built on `i_overlay 9`; a `CurveBuilder`/`FloatCurveOverlay` API; 100 % documented | Far too new for production; no bug history; the API will change | 🔭 **Watch closely.** It is the natural candidate to replace the flatten/refit pipeline in 2027. Evaluate in a 3-day *spike* in phase 2 |
| `flo_curves` | 0.8.1 (2026-08-25) | **Apache-2.0** | 🟡 Active but small (67 K downloads/90 d) | It does do booleans on curves; a good collection of algorithms (offset, fit, intersection) | **Pure Apache-2.0** (→ forces GPLv3, acceptable but it adds up); inferior robustness in degenerate cases; the Graphite project explicitly rejected it in favour of kurbo for "naive and unoptimised algorithms" (said of bezier-rs, from the same space) | 🟡 Plan C |
| `geo-booleanop` | 0.3.2 (**2020-06-27**) | MIT | 🔴 **Abandoned 6 years ago** | Implements Martínez-Rueda | Dead; besides, `geo` has since integrated booleans | ❌ |
| `path-bool` / `path_bool` | **does not exist on crates.io** (404 under both names) | — | — | It is the **internal** module of the Graphite editor, unpublished | Not an available dependency | ❌ (but its GPL code is a reference to study, and Graphite is Apache-2.0/to be checked) |
| `bezier-rs` | 0.5.0 (2025-08-15) | MIT OR Apache-2.0 | 🟠 Graphite **migrated to kurbo** and left it behind | A friendly API for Bézier segments | The team themselves say kurbo is superior in performance and correctness | ❌ |
| `cavalier_contours` | 0.9.0 (2026-08-20) | MIT OR Apache-2.0 | 🟡 Active, a CAD niche | **Very robust polyline offsetting with arcs** (better than naive offset) and polyline booleans | Polylines with bulge, not Béziers | 🟡 **A real candidate for "Contour offset"** (the inset/outset tool), which is distinct from stroke-to-path |

### 8.3 Recommended geometry architecture

```
xarast-geom (our own crate)
├── types: Path (Vec<SubPath>), SubPath (Vec<Segment>), Segment = Line|Quad|Cubic|Arc
│          — all f64, bidirectional conversion with kurbo::BezPath
├── stroke_to_path()   → kurbo::stroke()          [verified available]
├── offset()           → kurbo::offset + cavalier_contours for cases with arcs
├── simplify()/fit()   → kurbo::simplify + fit_to_bezpath_opt
├── dash()             → kurbo::dash
├── arclen/nearest     → kurbo ParamCurve*
└── boolean()          → OUR OWN PIPELINE:
      1. adaptive flattening to polylines with tolerance = f(zoom, document precision)
         and a traceability table original_segment → point range
      2. i_overlay 9 with a fixed grid_size (determinism)
      3. refit: for every stretch of the result that comes entirely from one
         original segment, restore the original curve; for the mixed stretches,
         kurbo::fit_to_bezpath_opt with a tolerance
      4. cleanup: merging of coincident points, removal of micro-segments
```

**Why this pipeline and not a direct curve boolean:** exact curve-curve intersection arithmetic is where every editor fails (Illustrator included). Flattening with a controlled tolerance + a deterministic integer boolean + refitting is what Inkscape (livarot), Blender and — according to their blog — Graphite do in practice. And with `i_overlay` in `grid_size` mode we get **bit-for-bit reproducibility**, which is indispensable for the golden tests and for "undo + redo" to give the same result.

**A future decision point:** if `i_curve` matures (say ≥ 1.0 and ≥ 6 months with no serious bugs), steps 1–3 are replaced by a single call. Design `boolean()` as a façade so that the change is a day's work.

### 8.4 Risks

| Risk | Prob. | Impact | Mitigation |
|---|---|---|---|
| Curve quality degrading after repeated booleans | **High** | **High** | A traceability table (step 3); property tests with `proptest`: `union(A, ∅) == A`, `A ∩ A == A`, area preserved within ε |
| `i_overlay` breaking the API in 10.0 | Medium | Low | It sits behind `xarast-geom::boolean()` |
| Degenerate cases (self-intersections, tangencies, nested subpaths) | High | High | A regression corpus of pathological SVGs; `i_overlay` declares support for self-intersections |
| Performance on paths with 100 K nodes | Medium | Medium | `i_overlay` has an API with reusable buffers; parallelise per subpath with rayon |

---

## 9. Serialisation and undo/redo

### 9.1 Serialisation

| Crate | Version | Licence | Maintenance | Pros | Cons | Verdict |
|---|---|---|---|---|---|---|
| **serde** | 1.0.229 (2026-07-18) | MIT OR Apache-2.0 | ⭐⭐ The standard | Universal, `serde_json` for readable manifests and preferences | Deserialising a large document = building the whole tree | ✅ **CHOSEN** (manifests, preferences, clipboard, tests with insta) |
| **rkyv** | 0.8.18 (2026-08-05) | MIT | ⭐⭐ 38 M downloads/90 d | **Zero-copy**: `mmap` the file and access it directly without deserialising → instant autosave and "open a huge document"; validation with `bytecheck` | A rigid format (evolving the schema needs care); less ergonomic | ✅ **CHOSEN** for **autosave/scratch/persistent undo**, NOT for the public format |
| `bincode` | 3.0.0 (2025-12-16) | MIT | ⭐⭐ | Simple and compact | No schema evolution; no zero-copy | 🟡 |
| `postcard` | 1.1.3 (2025-07-24) | MIT OR Apache-2.0 | ⭐ | Very compact (varint), designed for embedded | The same evolution limits | 🟡 |

**Decision on the `.xarast` format (`document.bin`):** **neither serde nor rkyv directly**, but a **hand-written versioned record format** (magic header + version + a table of TLV records), with serde for the *values* inside each record where that suits. Reason: a document format has to survive 20 years and support *forward compatibility* (an old version opens a new file, ignoring unknown records). Neither serde nor rkyv gives that for free, and it is exactly the design the original `.xar` uses (records with an ID and a length). It also makes **fuzzing** natural (§12).

### 9.2 Undo/redo

| Strategy | Pros | Cons | Verdict |
|---|---|---|---|
| **Command pattern (op + inverse)** | Minimal memory; allows coalescing (dragging a node = 1 entry); readable names in the history panel; a natural basis for scripting and macros | Every operation needs its inverse written and **tested**; inverse bugs corrupt the document silently | ✅ **Chosen basis** |
| **Full snapshots** | Trivially correct | Unworkable with large bitmaps | ❌ |
| **Persistent snapshots (structural sharing)** | Correct *and* cheap: cloning the tree is amortised O(1); undo = swapping a pointer; it allows history branches and version comparison | Indirection overhead on access; the heavy payloads (bitmaps) have to be moved outside | ✅ **Chosen complement** |
| **CRDT / operational transform** | Real-time collaboration | Enormous complexity, not a requirement | ❌ (do not close the door: the command pattern is the road towards it) |

**Recommendation: a hybrid in three layers.**

> ### ⚠️ SUPERSEDED — see `../10-architecture.md` §3.1
> Layer 1 below (the live document *being* a persistent `imbl` structure) was
> **overruled by architecture §3.1** in favour of a generational arena
> (`SlotMap`) with persistent snapshots layered over it, because traversal cost
> decides it: render, hit-testing and layout look nodes up by id millions of
> times per second, and a HAMT lookup costs two to five pointer hops against an
> index plus a generation check. Layers 2 and 3 (the invertible command log and
> the persisted history) stand unchanged, and `imbl` remains in use — for the
> checkpoints built *over* the arena, not for the live store. The analysis below
> is retained because the trade-off it sets out is still the right one to
> understand.

1. **Document** = a node tree in a persistent structure (`imbl::Vector` / `imbl::HashMap`) with the *heavy payloads* (bitmaps, fonts) behind `Arc<Resource>` (shared copy-on-write).
2. **History** = a stack of `Command`s with `apply`/`invert`, **plus** a pointer to the previous persistent snapshot. Undo normally restores the pointer (O(1)); the `Command` serves for the readable label, coalescing and replay.
3. **History persistence** = serialise the command stack with `rkyv` into `history/` inside the `.xarast` (optional, in preferences) → "undo after closing and reopening", which is a real differentiator.

| Crate | Version | Licence | Notes |
|---|---|---|---|
| **imbl** | 7.0.2 (2026-09-09) | **MPL-2.0+** (GPL-compatible ✅) | A maintained fork of `im` (which has been stalled since 2022). 2.58 M downloads/90 d. Maintained by jneem (Linebender). ✅ **CHOSEN** |
| `im` | 15.1.0 (**2022-04-29**) | MPL-2.0+ | 🔴 Abandoned. Use `imbl` |
| `rpds` | 1.2.1 (2026-05-15) | MIT | A maintained alternative with a simpler licence; "purer" structures (lists, tries) but less optimised for large vectors. 🟡 Plan B if MPL-2.0 is uncomfortable |
| `slotmap` / `thunderdome` | — | Zlib/MIT | A node arena with stable (generational) IDs — **indispensable** for references between document objects |

---

## 10. Concurrency

### 10.1 Is tokio needed?

**Not for the core.** Xarast is not a server: there are no thousands of connections and no massive concurrent I/O. What there is is **data parallelism** (rasterising tiles, tessellating, decoding images, booleans per subpath) and **a handful of long-lived threads**. Bringing in tokio would drag along a runtime, colour functions with `async` and complicate the event loop, which must be synchronous and deterministic.

**A minimal async runtime is needed** in two places: `ashpd` (XDG portals, which are D-Bus and async by nature) and the update check. Solution: `tokio` with `features = ["rt", "macros", "time"]` (a 1-thread, **current_thread** runtime) confined to a "services" thread, or simply `pollster::block_on` on a worker thread. I recommend the former, for `ashpd`'s robustness.

### 10.2 Threading model

```
┌─ Main thread (UI) ───────────────────────────────────────────┐
│  winit event loop → egui → builds a ScenePatch               │
│  NEVER blocks. No disk I/O. No heavy rasterisation.          │
└──────────────┬───────────────────────────────────────────────┘
               │ crossbeam-channel (ScenePatch, priority)
┌──────────────▼── Render thread ──────────────────────────────┐
│  owns wgpu::Device/Queue; compiles the scene; tiles;         │
│  submit + present. Uses rayon for CPU tiles.                 │
└──────────────┬───────────────────────────────────────────────┘
               │
┌──────────────▼── rayon global pool (N-2 threads) ────────────┐
│  tessellation, booleans, image decoding, filters,            │
│  thumbnail generation                                        │
└──────────────────────────────────────────────────────────────┘
┌─ Services thread (tokio current_thread) ─────────────────────┐
│  XDG portals (ashpd), autosave, font watcher, updates        │
└──────────────────────────────────────────────────────────────┘
```

| Crate | Version | Licence | Use |
|---|---|---|---|
| **rayon** | 1.12.0 (2026-04-14) | MIT OR Apache-2.0 | Data parallelism. ⭐⭐ 125 M downloads/90 d |
| **crossbeam-channel** | (crossbeam 0.8.5) | MIT OR Apache-2.0 | Inter-thread channels with `select!` |
| **parking_lot** | 0.12.5 | MIT OR Apache-2.0 | Faster Mutex/RwLock with no poisoning |
| **tokio** | 1.53.1 | MIT | Only `rt` + `macros` + `time`, the services thread |
| **tracing** + **puffin** / **tracy-client** | 0.1.44 / 0.20.0 / 0.19.0 | MIT / MIT OR Apache-2.0 | Instrumentation; `puffin` for the in-app profiler (an egui panel), `tracy` for deep analysis |
| **wgpu-profiler** | 0.28.0 | MIT OR Apache-2.0 | Per-pass GPU timestamps |

**A golden rule to encode in CI:** the UI thread must not exceed 8 ms per frame. Add a performance test with `criterion` over the `build_ui_frame()` function with a synthetic document of 10,000 objects.

---

## 11. Linux packaging

### 11.1 AppImage — tools verified (2026)

| Tool | Status in 2026 | Licence | Pros | Cons | Verdict |
|---|---|---|---|---|---|
| **appimagetool** (AppImageKit) | Active. **Key change: it now uses the static `runtime` by default**, which resolves the historical startup failure on distributions that ship only **libfuse3** (Ubuntu ≥ 24.04, Arch, recent Fedora) | MIT | It is the reference; total control of the AppDir | It requires you to build the AppDir yourself | ✅ **CHOSEN** (final step) |
| **linuxdeploy** | Active; **it also switched to the static runtime by default** | MIT | Automates library copying (`ldd`), the desktop entry, icons, `AppRun`; plugins (`appimage`, `gtk`, `qt`); supports `UPDATE_INFORMATION` for **zsync** | **Does not cross-compile to ARM**: aarch64 AppImages have to be built on an ARM runner | ✅ **CHOSEN** (collection step) |
| `cargo-appimage` | 2.4.0 (2025-11-24) | **GPL-3.0** (it is a tool, it is not linked → it does not contaminate) | Convenient `cargo` integration | Very thin (it wraps linuxdeploy); 1.5 K downloads/90 d; little control | ❌ We prefer an explicit script |
| `cargo-packager` | 0.11.8 (2025-11-27) | MIT OR Apache-2.0 | Multi-format (AppImage, deb, MSI, NSIS, .app, dmg) from one config | Less fine control over the AppImage; useful later for Windows/macOS | 🟡 **Yes for phases 2–3** (Windows/macOS) |
| `cargo-dist` | 0.32.0 (2026-05-22) | MIT OR Apache-2.0 | Generates the CI workflows and the installers/releases | The axo project shut down; the future is uncertain | 🟡 Only as inspiration for the workflow |

### 11.2 glibc compatibility — the baseline decision

Binaries link against versioned glibc symbols and **only work on an equal or newer glibc**. The baseline is therefore set by the build image:

| Build base | glibc | Covers | Recommendation |
|---|---|---|---|
| Ubuntu 20.04 | 2.31 | Practically everything still alive, including RHEL 8 derivatives | 🟡 Now unsupported (EOL April 2025); an old toolchain |
| **Ubuntu 22.04** | **2.35** | Debian 12, Ubuntu 22.04+, Fedora 36+, RHEL 9, openSUSE 15.5+, SteamOS 3.5+ | ✅ **CHOSEN.** It is the sweet spot in 2026 and there is an official ARM runner (`ubuntu-22.04-arm`) |
| Ubuntu 24.04 | 2.39 | Only 2024+ distributions | ❌ Too new |
| `manylinux_2_28` (AlmaLinux 8) | 2.28 | Maximum compatibility | 🟡 Plan B if complaints appear; a more awkward toolchain for Rust+Wayland dev headers |

Reinforcements:
- Build in an `ubuntu:22.04` **Docker container** (not on the runner directly) so the baseline is reproducible even if GitHub changes its images.
- `cargo build --release` with `target-cpu=x86-64-v2` (SSE4.2/POPCNT, safe since 2009) and runtime detection for AVX2 in the image kernels.
- Do **NOT** link musl: it breaks `dlopen` of the system's GL/Vulkan drivers.

### 11.3 What NOT to put inside the AppImage

The rule: **anything that talks to the user's hardware or compositor must come from the system.** Exclusion list for `linuxdeploy` (`--exclude-library`):

```
libGL.so*, libGLX*, libEGL*, libgbm*, libdrm*, libvulkan.so*,
libwayland-client.so*, libwayland-egl.so*, libwayland-cursor.so*,
libX11*, libxcb*, libxkbcommon*,
libc.so*, libstdc++.so*, libgcc_s.so*, libm.so*, libpthread.so*, libdl.so*,
libfontconfig.so*, libfreetype.so*   (fontconfig must be the system's, so the user's fonts are visible)
```

What is packaged: our binary, static `libzstd` (it comes inside the binary), the assets, icons, the `.desktop`, the AppStream metainfo and `THIRD-PARTY-LICENSES.html`.

> Note: since we use **fontique**, which uses the system fontconfig on Linux, it is **critical** not to bundle fontconfig: if we do, the user will not see their fonts.

### 11.4 Contents of the AppDir

```
Xarast.AppDir/
├── AppRun                       (script: exports XDG_DATA_DIRS and launches the binary)
├── xarast.desktop               (root, mandatory)
├── xarast.png                   (256×256, root, mandatory)
├── .DirIcon -> xarast.png
└── usr/
    ├── bin/xarast
    ├── lib/                     (non-excluded deps)
    └── share/
        ├── applications/xarast.desktop
        ├── icons/hicolor/{16,22,24,32,48,64,128,256,512}x.../apps/xarast.png
        ├── icons/hicolor/scalable/apps/xarast.svg
        ├── mime/packages/xarast.xml          ← registration of .xarast and .xar
        ├── metainfo/es.digio.Xarast.metainfo.xml   ← AppStream (needed for Flathub and for GNOME Software)
        └── doc/xarast/THIRD-PARTY-LICENSES.html
```

A minimal `xarast.desktop`:
```ini
[Desktop Entry]
Type=Application
Name=Xarast
GenericName=Vector Graphics Editor
Comment=Vector graphics and photo editor
Exec=xarast %F
Icon=xarast
Categories=Graphics;VectorGraphics;RasterGraphics;2DGraphics;
MimeType=application/x-xarast;application/x-xara;image/svg+xml;image/png;image/jpeg;
StartupNotify=true
StartupWMClass=xarast
```

> Important for Wayland: the `app_id` that winit passes (`WindowAttributes::with_name(app_id, _)` on X11 / `with_application_id` on Wayland) **must match** the name of the `.desktop` (`xarast`), or the icon will not appear in the GNOME/KDE task bar.

The MIME type (`xarast.xml`):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/x-xarast">
    <comment>Xarast document</comment>
    <comment xml:lang="en">Xarast document</comment>
    <glob pattern="*.xarast"/>
    <magic priority="60">
      <match type="string" value="PK\003\004" offset="0">
        <match type="string" value="application/x-xarast" offset="38"/>
      </match>
    </magic>
    <icon name="application-x-xarast"/>
  </mime-type>
  <mime-type type="application/x-xara">
    <comment>Xara drawing</comment>
    <glob pattern="*.xar"/>
    <magic priority="50"><match type="string" value="XARA" offset="0"/></magic>
  </mime-type>
</mime-info>
```

### 11.5 Delta updates with zsync

`linuxdeploy --output appimage` with `UPDATE_INFORMATION` embeds the update string in the AppImage itself:

```bash
export UPDATE_INFORMATION="gh-releases-zsync|digio-es|Xarast|latest|Xarast-*-x86_64.AppImage.zsync"
```

This generates a `.zsync` alongside the `.AppImage` which has to be uploaded to the same release. `AppImageUpdate` (or `appimageupdatetool`) downloads **only the changed blocks** — typically 3–8 MB instead of 60 MB. We can also implement the check inside the app (reading the ELF `.upd_info` section) and offer "Check for updates".

### 11.6 AppImage vs Flatpak vs Snap

| Criterion | AppImage | Flatpak | Snap |
|---|---|---|---|
| Installation | None: `chmod +x` and run | `flatpak install` | `snap install` |
| Sandbox | **None** by default | **Bubblewrap + portals**, the most mature model | AppArmor + interfaces |
| Access to the user's files | Direct (good for an editor) | Via portals (the `rfd` dialog already supports it) | Via interfaces |
| Size / disk | 1 file, ~60–90 MB, nothing shared | A shared runtime (`org.freedesktop.Platform 24.08`) → +1 GB for the first app, cheap thereafter | Larger, one loop mount per snap |
| Delta updates | zsync (manual or AppImageUpdate) | OSTree, excellent | Delta, excellent |
| Discovery | None (you have to go to the website) | **Flathub**: 3200+ apps, 433 M downloads in 2025; integrated into GNOME Software and KDE Discover | Snap Store (Ubuntu) |
| Tablet/stylus, GPU | No problems (it uses everything from the system) | It works, but it requires `--device=all` permissions and there is historical friction with Wacom | Known friction with hardware |
| Acceptance in the Linux community | Good among "power users" and creatives | **It is the de facto standard in 2026** | Polarised (rejected outside Ubuntu) |
| Maintenance effort | Low | Medium (a YAML manifest + Flathub review) | Medium-high |

**Recommendation:** **AppImage as the primary channel** (right for the early phase: no sandbox, zero friction with tablets and GPUs, a single file the user tries and throws away). **Flatpak/Flathub as a second channel as soon as there is a public beta** — it is where software discovery on Linux lives today, and its portal model is already covered by `ashpd`+`rfd`. **Snap: no**, it adds nothing over the other two and the cost of maintaining a third channel does not pay for itself.

### 11.7 CI — GitHub Actions

```yaml
# .github/workflows/appimage.yml
name: AppImage
on:
  push: { tags: ['v*'] }
  workflow_dispatch:

jobs:
  build:
    strategy:
      fail-fast: false
      matrix:
        include:
          - { runner: ubuntu-22.04,     arch: x86_64,  target: x86_64-unknown-linux-gnu  }
          - { runner: ubuntu-22.04-arm, arch: aarch64, target: aarch64-unknown-linux-gnu }
    runs-on: ${{ matrix.runner }}
    steps:
      - uses: actions/checkout@v4

      # An explicit 22.04 container to pin the glibc baseline (2.35)
      # even if GitHub updates the runner image.
      - name: Build dependencies
        run: |
          sudo apt-get update
          sudo apt-get install -y --no-install-recommends \
            build-essential pkg-config cmake \
            libwayland-dev libxkbcommon-dev libx11-dev libxcursor-dev \
            libxrandr-dev libxi-dev libgl1-mesa-dev libvulkan-dev \
            libfontconfig-1-dev libdbus-1-dev libudev-dev \
            desktop-file-utils appstream file wget

      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with: { key: ${{ matrix.arch }} }

      - name: Build
        env:
          RUSTFLAGS: "-C target-cpu=x86-64-v2"   # x86_64 only; on ARM use the default
        run: cargo build --release --locked --target ${{ matrix.target }}

      - name: Prepare the AppDir
        run: |
          install -Dm755 target/${{ matrix.target }}/release/xarast     AppDir/usr/bin/xarast
          install -Dm644 packaging/linux/xarast.desktop                 AppDir/usr/share/applications/xarast.desktop
          install -Dm644 packaging/linux/xarast.xml                     AppDir/usr/share/mime/packages/xarast.xml
          install -Dm644 packaging/linux/es.digio.Xarast.metainfo.xml   AppDir/usr/share/metainfo/es.digio.Xarast.metainfo.xml
          for s in 16 22 24 32 48 64 128 256 512; do
            install -Dm644 assets/icons/${s}.png \
              AppDir/usr/share/icons/hicolor/${s}x${s}/apps/xarast.png
          done
          install -Dm644 assets/icons/xarast.svg \
            AppDir/usr/share/icons/hicolor/scalable/apps/xarast.svg
          cargo install --locked cargo-about
          cargo about generate packaging/about.hbs \
            > AppDir/usr/share/doc/xarast/THIRD-PARTY-LICENSES.html

      - name: Validate the metadata
        run: |
          desktop-file-validate AppDir/usr/share/applications/xarast.desktop
          appstreamcli validate --no-net AppDir/usr/share/metainfo/es.digio.Xarast.metainfo.xml

      - name: AppImage tools
        run: |
          A=${{ matrix.arch }}
          wget -q https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-$A.AppImage
          wget -q https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/continuous/linuxdeploy-plugin-appimage-$A.AppImage
          chmod +x linuxdeploy-*.AppImage

      - name: Build the AppImage
        env:
          UPDATE_INFORMATION: >-
            gh-releases-zsync|digio-es|Xarast|latest|Xarast-*-${{ matrix.arch }}.AppImage.zsync
          OUTPUT: Xarast-${{ github.ref_name }}-${{ matrix.arch }}.AppImage
          # static runtime -> starts with libfuse2 AND libfuse3
          LDAI_RUNTIME_FILE: ""
        run: |
          # --appimage-extract-and-run avoids needing FUSE inside the runner
          ./linuxdeploy-${{ matrix.arch }}.AppImage --appimage-extract-and-run \
            --appdir AppDir \
            --desktop-file AppDir/usr/share/applications/xarast.desktop \
            --icon-file AppDir/usr/share/icons/hicolor/256x256/apps/xarast.png \
            --exclude-library "libGL*" --exclude-library "libEGL*" \
            --exclude-library "libvulkan*" --exclude-library "libwayland-*" \
            --exclude-library "libX11*" --exclude-library "libxcb*" \
            --exclude-library "libxkbcommon*" --exclude-library "libdrm*" \
            --exclude-library "libgbm*" --exclude-library "libfontconfig*" \
            --exclude-library "libfreetype*" \
            --output appimage

      - name: Smoke test (headless startup)
        run: |
          ./Xarast-*-${{ matrix.arch }}.AppImage --appimage-extract-and-run --version

      - uses: actions/upload-artifact@v4
        with:
          name: appimage-${{ matrix.arch }}
          path: |
            Xarast-*.AppImage
            Xarast-*.AppImage.zsync

  release:
    needs: build
    runs-on: ubuntu-latest
    permissions: { contents: write }
    steps:
      - uses: actions/download-artifact@v4
        with: { path: dist, merge-multiple: true }
      - run: |
          cd dist && sha256sum Xarast-* > SHA256SUMS
      - uses: softprops/action-gh-release@v2
        with:
          files: |
            dist/Xarast-*.AppImage
            dist/Xarast-*.AppImage.zsync
            dist/SHA256SUMS
```

**Critical notes on the workflow:**
- `ubuntu-22.04-arm` is a **native ARM runner, free for public repositories** (~10 min per build). This resolves the limitation that `linuxdeploy` does not cross-compile to ARM.
- `--appimage-extract-and-run` is mandatory: GitHub's runners have no FUSE.
- The **static runtime** (the default since 2026) is what guarantees that the AppImage starts both on Ubuntu 22.04 (libfuse2) and on Arch/Ubuntu 24.04+ (libfuse3 only). **Verify it in the smoke test.**
- Add a separate job that runs the AppImage in `debian:12`, `fedora:41` and `archlinux:latest` containers with `xvfb`, to catch forgotten dependencies.

---

## 12. Windows and macOS packaging (phases 2 and 3)

### 12.1 Windows

| Aspect | Recommendation | Notes |
|---|---|---|
| Format | **MSI** (enterprises, GPO deployment) **+ NSIS/EXE** (users) | `cargo-wix 0.3.9` (MIT/Apache) generates an MSI with WiX; `cargo-packager 0.11.8` does MSI and NSIS from a single config → **prefer cargo-packager** so as not to maintain two toolchains |
| Toolchain | `x86_64-pc-windows-msvc` (**not** GNU) | Better compatibility with Windows Ink and with debugging |
| ARM | `aarch64-pc-windows-msvc` in phase 3 | Surface Pro X and successors |
| Signing | **Azure Trusted Signing** (~€10/month, no physical HSM required) | Alternative: an EV certificate on a token (~€400/year). **Without a signature, SmartScreen blocks downloads**; with an ordinary OV signature you have to accumulate reputation |
| Runtime | Static VCRedist: `RUSTFLAGS=-C target-feature=+crt-static` | Avoids the VCRedist installer |
| File association | `.xarast`, `.xar` in the registry; a `ProgID` + icon | The MSI does this declaratively |
| Tablet | Windows Ink via winit 0.31 (already verified as supported); **plan B `wintab_lite`** if there are complaints with external Wacom/Huion tablets | It is a known, real problem (see Photoshop's documentation on Ink vs Wintab) |
| Updates | Our own check + downloading the installer | A Winget manifest as an extra |

### 12.2 macOS

| Aspect | Recommendation | Notes |
|---|---|---|
| Format | **an `.app` inside a `.dmg`** | `cargo-packager` or `cargo-bundle 0.11` |
| Universal binary | `cargo build --target x86_64-apple-darwin` + `aarch64-apple-darwin`, then **`lipo -create`** | Or `cargo-packager` with `--target universal-apple-darwin` |
| Deployment target | `MACOSX_DEPLOYMENT_TARGET=11.0` (Big Sur) | Covers everything from 2020 |
| Signing | **Apple Developer Program, $99/year**, mandatory | `codesign --deep --force --options runtime --timestamp` with a *Developer ID Application* |
| Notarisation | `xcrun notarytool submit --wait` + `xcrun stapler staple` | Without this, Gatekeeper blocks it. In CI: keep the `.p12` and the app-specific password as secrets |
| Entitlements | `com.apple.security.cs.disable-library-validation` only if needed | A hardened runtime is mandatory in order to notarise |
| Tablet | **winit does NOT support the tablet on macOS.** It has to be implemented: `objc2-app-kit`, `NSEventTypeTabletPoint` / `NSEventSubtypeTabletPoint`, with `pressure`, `tilt`, `rotation`, `tangentialPressure` | ~200 lines. It is obligatory work in the macOS phase |
| GPU backend | Metal via wgpu (no MoltenVK) | |
| Wayland-equivalent notes | Trackpad gestures are already given by winit | |

---

## 13. Testing

### 13.1 Render tests (golden images) — the most important thing

The problem: GPU renders are **not bit-for-bit reproducible** across drivers. A two-level solution:

| Level | What | Tool | Tolerance |
|---|---|---|---|
| **A. Golden CPU (mandatory in CI)** | Render the corpus with `vello_cpu` (deterministic) and compare pixel by pixel | exact comparison + `image-compare` for the failure report | **0** differences |
| **B. GPU↔CPU parity (nightly, on a runner with lavapipe)** | The same corpus with the GPU backend vs the CPU one | `image-compare` (RMS / SSIM) or `dify` | ΔRMS < 0.5 %, no pixel with Δ > 8/255 |
| **C. Visual regression in PRs** | Only the tests touched; upload the diff images as an artifact | our own script + `actions/upload-artifact` | — |

| Crate | Version | Licence | Use |
|---|---|---|---|
| **image-compare** | 0.5.0 (2025-08-18) | MIT | RMS, SSIM, hybrid comparison; returns a difference map → perfect for the failure artifact |
| `dify` | 0.8.0 (2026-01-04) | MIT | A pixelmatch port, fast, good anti-aliasing awareness | An alternative to image-compare |
| **insta** | 1.48.0 (2026-06-11) | **Apache-2.0** | **Text** snapshots: the serialised document tree, the `.xar` parser's output, the render plan, geometry after booleans (as SVG path data). `cargo insta review` is excellent. ⚠️ Pure Apache-2.0 → reinforces GPLv3 |
| **egui_kittest** | 0.36.2 | MIT OR Apache-2.0 | Tests of the egui UI **with image snapshots included**; it simulates clicks and the keyboard. Official from egui |

**Recommended golden-test corpus** (`tests/corpus/`): `.xarast` documents covering every primitive and every troublesome combination — gradients (linear, radial, conical, mesh), per-object transparency, blend modes, feathering, nested clipping paths, text on a path with OpenType, bidi text, bitmaps in different colour spaces, strokes with caps/joins of every kind, dashed lines, booleans over self-intersections. Target: **≥ 120 cases before the beta**.

### 13.2 Benchmarks

| Crate | Version | Licence | Verdict |
|---|---|---|---|
| **criterion** | 0.8.2 (2026-02-04) | Apache-2.0 OR MIT | ✅ The standard; statistical detection of regressions; HTML charts |
| `divan` | 0.1.21 (2025-04-10) | MIT OR Apache-2.0 | 🟡 A nicer API, much faster, but no releases since April 2025 |

Mandatory benchmarks: booleans on paths of 1 K/10 K/100 K nodes, shaping 10,000 characters, decoding a 24 MP JPEG, rendering a 10,000-object document, `build_ui_frame()`, serialising/deserialising a 200 MB `.xarast`.

### 13.3 Fuzzing the `.xar` parser

| Crate | Version | Licence | Use |
|---|---|---|---|
| **cargo-fuzz** | 0.13.2 (2026-06-09) | MIT OR Apache-2.0 | libFuzzer over the parser. **Indispensable**: `.xar` is a third-party binary format; a parser in Rust has no UB but it does have `panic!`, OOM (a malicious record length) and infinite loops, and all of those are DoS |
| **arbitrary** | 1.4.2 (2025-08-14) | MIT OR Apache-2.0 | Generating structured valid documents for round-trip fuzzing |
| **proptest** | 1.11.0 (2026-03-24) | MIT OR Apache-2.0 | **Geometry invariants**: `union(A,∅)==A`, `A∩A==A`, `difference(A,A)==∅`, area preserved, `stroke_to_path` produces closed paths, round-trip `serialize→deserialize==identity` |

Minimum fuzz targets: `fuzz_xar_parse`, `fuzz_xarast_parse`, `fuzz_svg_import` (usvg), `fuzz_path_boolean`, `fuzz_text_shape`. Run them in **nightly** CI, 30 min per target; the corpus is cached between runs.

Defences that fuzzing must verify: a hard memory limit per document, a limit on group nesting depth, validation of lengths before reserving, a timeout per operation.

### 13.4 The rest

| Tool | Version | Use |
|---|---|---|
| **cargo-nextest** | 0.9.145 (2026-09-16) | A parallel test runner, 2–3× faster, better output, retries |
| `cargo-deny` | 0.20.2 | **Licences** (§1.4) + security advisories (RustSec) + duplicates |
| `cargo-about` | 0.9.2 | Generating the third-party licence file |
| `trybuild` | 1.0.121 | Only if we write our own derive macros |

---

## DECISIONS

### Licence

> ### ⚠️ SUPERSEDED — see `../11-licensing-and-clean-room.md`
> The recommendation below is **overruled**: the project ships under
> `MIT OR Apache-2.0`, because the original is GPL-2.0-**only** and therefore
> cannot be combined with GPL-3 at all, whereas MIT can be folded into a
> GPL-2.0-only project while remaining permissive. The "IMMEDIATE ACTION" of
> replacing `LICENSE` with GPL-3.0 must **not** be carried out. The per-crate
> licence analysis in §1.2 and the `cargo-deny`/`cargo-about` tooling in §1.4
> remain valid; only the licence conclusion moved.

> **Obsolete — see the update note in §1.** The block that follows reflects the
> earlier decision, taken under the derivative-work assumption. The standing
> decision is **MIT OR Apache-2.0** for the whole project, upheld by clean-room
> discipline.

```
Xarast is published under GPL-3.0-or-later.
The reusable crates (xarast-geom, xarast-xar, xarast-raster) are published
under MIT OR Apache-2.0 to maximise their use by third parties.
Xarast is a CLEAN-ROOM reimplementation: no XaraLX code is copied or
translated (GPL-2.0-only, incompatible with winit/Apache-2.0).
IMMEDIATE ACTION: replace /home/user/Xarast/LICENSE (MIT today) with GPL-3.0.
```

### Workspace structure

> **Note on crate names.** The crate that was proposed elsewhere as
> `xarast-model` is now called **`xarast-doc`** (as it already appears below).
> The **authoritative** crate list is the one in `../10-architecture.md` §2;
> where this layout and that one disagree, the architecture document wins.

```
Xarast/
├── Cargo.toml                 # workspace
├── crates/
│   ├── xarast-app/            # binary: startup, CLI, preferences
│   ├── xarast-shell/          # winit 0.31 + wgpu 30 + bridge to egui + accesskit
│   ├── xarast-ui/             # panels, galleries, dialogs, tools (egui)
│   ├── xarast-doc/            # document model, undo, imbl, commands
│   ├── xarast-geom/           # kurbo + booleans (i_overlay) + stroke/offset/simplify
│   ├── xarast-raster/         # trait Rasterizer + vello / vello_cpu backends + wgpu compositor
│   ├── xarast-text/           # parley + skrifa: layout, text on a path, manual kerning
│   ├── xarast-image/          # decoding/encoding, colour management
│   ├── xarast-format/         # .xarast (ZIP+zstd) read/write
│   ├── xarast-xar/            # legacy .xar parser (read-only) + fuzz targets
│   └── xarast-input/          # trait TabletSource + winit/octotablet/appkit
├── fuzz/                      # cargo-fuzz
├── packaging/{linux,windows,macos}/
└── tests/corpus/              # golden tests
```

### Root `Cargo.toml` — ready to copy

```toml
[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.package]
version       = "0.1.0"
edition       = "2024"
rust-version  = "1.90"
license       = "GPL-3.0-or-later"
repository    = "https://github.com/digio-es/Xarast"
authors       = ["Jose Francisco Rives <jose@digio.es>"]

[workspace.dependencies]

# ── Window, input, GPU ─────────────────────────────────────────────────────
# winit 0.31 is MANDATORY: it is the only version with TabletToolData
# (pressure, tilt, twist). Pinned to the exact beta on purpose.
winit            = { version = "=0.31.0-beta.3", default-features = false, features = ["wayland", "wayland-dlopen", "x11", "rwh_06"] }
wgpu             = { version = "30.0",  default-features = false, features = ["wgsl", "vulkan", "metal", "dx12", "gles", "fragile-send-sync-non-atomic-wasm"] }
raw-window-handle = "0.6"
bytemuck         = { version = "1.25", features = ["derive"] }
glam             = { version = "0.33", features = ["bytemuck"] }
pollster         = "1.0"

# Tablet: a complement to winit for X11 and for pad buttons/rings.
# WARNING: crates.io frozen at 0.1.0 (2024). VENDOR it in third_party/.
octotablet       = { version = "0.1", optional = true }

# ── UI ─────────────────────────────────────────────────────────────────────
# eframe is NOT used: it pins winit 0.30 and we would lose the stylus.
egui             = { version = "0.36", features = ["accesskit", "serde", "rayon"] }
egui-wgpu        = { version = "0.36", features = ["winit"] }
egui_extras      = { version = "0.36", features = ["image", "svg", "serde"] }
egui_tiles       = "0.17"
accesskit        = "0.25"
accesskit_winit  = "0.34"
arboard          = { version = "3.6", features = ["wayland-data-control", "image-data"] }
rfd              = { version = "0.17", default-features = false, features = ["xdg-portal", "tokio"] }
ashpd            = { version = "0.13", default-features = false, features = ["tokio"] }

# ── 2D rasterisation ───────────────────────────────────────────────────────
peniko           = "0.6"                  # shared paint types
vello            = { version = "0.10", optional = true }   # GPU compute backend
vello_hybrid     = { version = "0.2",  optional = true }   # GPU backend without compute
vello_cpu        = "0.2"                  # reference backend + fallback (ALWAYS)
lyon             = { version = "1.0", optional = true }    # plan B / occasional tessellation

# ── Geometry ───────────────────────────────────────────────────────────────
kurbo            = { version = "0.13", features = ["serde"] }
i_overlay        = "9.0"                  # polygon booleans (deterministic with grid_size)
cavalier_contours = { version = "0.9", optional = true }   # robust offsetting with arcs
# i_curve        = "0.2"                  # NOT yet: native booleans on Béziers,
#                                         # released 2026-09-19, 168 downloads. Reassess in 2027.

# ── Text ───────────────────────────────────────────────────────────────────
parley           = { version = "0.11", features = ["system", "accesskit"] }
fontique         = { version = "0.11", features = ["system"] }
skrifa           = "0.47"                 # glyph outlines -> kurbo::BezPath
harfrust         = "0.13"                 # shaping (arrives via parley; explicit for fine control)

# ── Image ──────────────────────────────────────────────────────────────────
image            = { version = "0.25", default-features = false, features = ["png", "jpeg", "gif", "tiff", "bmp", "webp", "hdr", "openexr", "qoi", "rayon"] }
zune-jpeg        = "0.5"                  # fast JPEG decoding (SIMD)
png              = "0.18"
image-webp       = "0.2"
ravif            = { version = "0.13", optional = true }   # AVIF export
kamadak-exif     = "0.6"
usvg             = "0.48"                 # SVG parser/normaliser (importer)
resvg            = { version = "0.48", optional = true }   # only for reference previews
oxipng           = { version = "10.2", optional = true }   # optimised PNG export
qcms             = { version = "0.3", optional = true }    # ICC colour management (MPL-2.0)

# ── Format / compression ───────────────────────────────────────────────────
zip              = { version = "8.6", default-features = false, features = ["deflate", "zstd", "time"] }
zstd             = "0.14"
flate2           = { version = "1.1", default-features = false, features = ["zlib-rs"] }  # pure Rust and fast
lz4_flex         = "0.14"                 # tile cache and undo in RAM

# ── Data, undo, serialisation ──────────────────────────────────────────────
serde            = { version = "1.0", features = ["derive", "rc"] }
serde_json       = "1.0"
rkyv             = { version = "0.8", features = ["bytecheck"] }   # autosave / persistent history
imbl             = { version = "7.0", features = ["serde"] }       # document tree (MPL-2.0, GPL-compatible)
slotmap          = "1.0"                  # arena with generational IDs
smallvec         = { version = "1.15", features = ["union", "const_generics"] }
memmap2          = "0.9"
uuid             = { version = "1.18", features = ["v4", "serde"] }

# ── Concurrency ────────────────────────────────────────────────────────────
rayon            = "1.12"
crossbeam-channel = "0.5"
parking_lot      = "0.12"
# tokio ONLY for the services thread (XDG portals, updates). No async in the core.
tokio            = { version = "1.53", default-features = false, features = ["rt", "macros", "time", "sync"] }

# ── Observability and errors ───────────────────────────────────────────────
tracing          = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
puffin           = { version = "0.20", optional = true }
puffin_egui      = { version = "0.30", optional = true }
wgpu-profiler    = { version = "0.28", optional = true }
thiserror        = "2.0"
anyhow           = "1.0"
directories      = "6.0"

# ── Dev / test ─────────────────────────────────────────────────────────────
[workspace.dependencies.criterion]
version = "0.8"
features = ["html_reports"]

[workspace.dependencies.insta]
version = "1.48"
features = ["json", "redactions", "filters"]

[workspace.dependencies.image-compare]
version = "0.5"

[workspace.dependencies.proptest]
version = "1.11"

[workspace.dependencies.arbitrary]
version = "1.4"
features = ["derive"]

[workspace.dependencies.egui_kittest]
version = "0.36"
features = ["wgpu", "snapshot"]

# ── Profiles ───────────────────────────────────────────────────────────────
[profile.dev]
opt-level = 1            # our own code, minimally optimised

[profile.dev.package."*"]
opt-level = 3            # dependencies ALWAYS optimised: without this, vello_cpu
                         # and zune-jpeg make debug mode unusable

[profile.release]
opt-level     = 3
lto           = "thin"
codegen-units = 1
panic         = "unwind"   # NOT "abort": we want to recover the document after a panic
strip         = "debuginfo"

[profile.dist]             # packaging profile
inherits      = "release"
lto           = "fat"
debug         = 1          # symbols for symbolicating crash reports
```

### `deny.toml` (licence compliance, mandatory in CI)

```toml
[licenses]
version = 2
confidence-threshold = 0.93
allow = [
  "MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception",
  "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib",
  "MPL-2.0",                # GPL-compatible
  "Unicode-3.0", "Unicode-DFS-2016",
  "CC0-1.0", "Unlicense",
  "OFL-1.1",                # bundled fonts
  "GPL-3.0-or-later",       # our own code
]
# Explicitly forbidden: any third-party GPL-*-only (it would tie us to that
# exact version) and non-free licences.
exceptions = []

[bans]
multiple-versions = "warn"
deny = [
  { name = "openssl-sys" },   # use rustls if there is ever networking
]

[advisories]
version = 2
yanked = "deny"
```

### Minimum requirements declared to the user

| | Minimum | Recommended |
|---|---|---|
| **Linux** | glibc 2.35 (Ubuntu 22.04, Debian 12, Fedora 36, RHEL 9), Wayland or X11, GL 3.3 | Wayland, Vulkan 1.1+, Mesa 23+ |
| CPU | x86-64-v2 (SSE4.2) or aarch64 | 4 cores, AVX2 |
| RAM | 4 GB | 16 GB |
| GPU | Any with GL 3.3, or **none at all** (lavapipe/llvmpipe) | A discrete GPU with Vulkan 1.1 and compute |

---

## RISKS AND FALLBACK PLANS

### Critical risks (they can sink the project)

| # | Risk | Prob. | Impact | Early signal | Plan B |
|---|---|---|---|---|---|
| **R1** | **Licensing:** copying/translating XaraLX code (GPL-2.0-only) contaminates the project and makes it **incompatible with winit (Apache-2.0)**, forcing a rewrite of the windowing layer | Medium | **Fatal** | Any PR that cites XaraLX files | A documented clean-room procedure from day 1: a `docs/format/` document written by reading the original, and the implementation made **only** from that document. Record who has read what. If it has already happened: rewrite the affected module with a different person |
| **R2** | **winit 0.31 does not stabilise** or breaks the API repeatedly; without it there is no stylus pressure | High | High | A beta cadence > 3 months with no RC | (a) Stay on the pinned beta indefinitely (it is usable); (b) go back to **winit 0.30.13 + `octotablet`**, which covers Wayland and Windows Ink completely; (c) in the worst case, our own Wayland backend with `smithay-client-toolkit` (the `tablet_v2` protocol directly) |
| **R3** | **Vello GPU does not mature** enough for large documents, or it shows unacceptable conflation artefacts | Medium | High | The GPU golden tests do not converge with the CPU ones | The `vello_cpu` backend **is already in production from day 1** (it is the reference). The real plan B: `lyon 1.0` + our own wgpu pipelines with 4× MSAA. The `xarast-raster` façade keeps the change local |
| **R4** | **Boolean quality** insufficient: curve degradation after chained operations | High | High | `proptest` area-preservation tests failing | (a) A traceability table to restore the original curves (already in the design); (b) migrate to **`i_curve`** once it matures; (c) our own implementation based on Graphite's algorithm (GPL, compatible with GPLv3) |
| **R5** | **Scope:** Xara Xtreme has 15 years of features; the project drowns before becoming useful | **Very high** | **Fatal** | A phase 1 that does not finish in 6 months | Define a **brutal MVP**: open/save `.xarast`, draw/edit paths, basic fills and strokes, simple text, layers, undo, export PNG/SVG. **Everything else is phase 2+.** Ship a usable alpha at 6 months even if it does little |

### High risks

| # | Risk | Prob. | Impact | Plan B |
|---|---|---|---|---|
| R6 | egui does not hold up under the density of a professional UI (100+ controls per panel) | Medium | Medium | Already mitigated by the architecture: the UI lives in `xarast-ui` on top of a trait; replacing it with `iced 0.14` would cost ~2 months, not the project. Prototype **the most complex panel (colour gallery + layer tree) in week 2** to find out early |
| R7 | The in-house `winit 0.31 ↔ egui` shim consumes more maintenance than expected | Medium | Medium | Contribute the work upstream to `egui-winit`; or migrate to `eframe` as soon as egui moves to winit 0.31, keeping `octotablet` as the pressure source |
| R8 | Wayland: compositor-specific bugs (GNOME/Mutter vs KWin vs wlroots vs Hyprland) | **High** | Medium | A CI matrix with the 4 compositors under headless `weston`/`sway` + `wlr-randr`; a bug-report channel with `XARAST_DEBUG_WAYLAND=1` that dumps the protocol |
| R9 | AppImage startup performance (squashfs decompression of 80 MB) | Medium | Low | `appimagetool --comp zstd`; measure with `hyperfine`; target < 800 ms cold |
| R10 | glibc 2.35 shuts out users of older LTS distributions (RHEL 8, Ubuntu 20.04) | Low | Low | A second AppImage built on `manylinux_2_28`, on major releases only |
| R11 | Without a Windows signature, SmartScreen blocks downloads and kills adoption | High (phase 2) | High | Budget for Azure Trusted Signing (~€120/year) from the start of the Windows phase |
| R12 | macOS: winit gives no tablet; the AppKit backend has to be written | Certain | Medium | Already budgeted (~1 week). Alternative: use SDL3 on macOS only, for tablet input |

### Medium / low risks

| # | Risk | Mitigation |
|---|---|---|
| R13 | `octotablet` unpublished on crates.io since 2024 (bus factor 1) | **Vendor it** in `third_party/octotablet` with the commit pinned; it is MIT |
| R14 | Pre-1.0 crates on the critical path (parley, vello, fontique, kurbo) | They are all Linebender, with mechanical breakages and a good changelog. Wrap each one in a crate of our own. Pin exact versions in `Cargo.lock` and bump in quarterly batches |
| R15 | `i_overlay` moving to 10.0 with breakages | It sits behind `xarast-geom::boolean()`. Cost: 1–2 days |
| R16 | Binary size (Rust + wgpu + vello + parley + image) exceeds 100 MB | `strip`, `lto = "fat"`, `opt-level = "s"` in the rarely used codec crates, optional features for AVIF/oxipng. Target: an AppImage < 80 MB |
| R17 | `zstd` is C: cross-compilation problems to aarch64 | It builds with `cc` without trouble on a native ARM runner (which is what we use). A pure-Rust alternative: `ruzstd` (decompression only) + optional `zstd-safe` |
| R18 | Golden tests unstable across `vello_cpu` versions | Pin the exact `vello_cpu` version; regenerate the corpus deliberately on every bump, reviewing the diffs |
| R19 | Colour management ignored until late → a painful redesign | Put the `ColorSpace` field into the colour model **from commit 1**, even if it only supports sRGB at first |
| R20 | AccessKit incomplete on Linux for the canvas (the drawing's objects are not accessible) | Expose only the UI controls as accessible; the canvas as a single node with a description. It is what Inkscape and Krita do |

### Decisions that have to be taken BEFORE writing code

1. **The definitive licence** and replacement of the current MIT `LICENSE`. (§1.3)
2. **A clean-room commitment**, documented and signed, with a record of who reads the original source. (§1.2)
3. **The MVP scope** settled in writing. (R5)
4. **A 2-week prototype** that validates the three riskiest assumptions in one go: (a) egui + winit 0.31 + wgpu 30 on the same surface; (b) stylus pressure arriving end to end on Wayland; (c) `vello_cpu` rendering a path with a gradient and comparing bit for bit in CI. If any of this fails, the stack changes now and not in six months' time.

---

## Sources

Consulted on 19 September 2026.

**Version, licence and maintenance data:** the crates.io API (`https://crates.io/api/v1/crates/<crate>`) for the ~110 crates evaluated; `docs.rs` for features and APIs.

- [crates.io](https://crates.io/) — versions, licences, publication dates and downloads
- [docs.rs/winit/0.31.0-beta.3](https://docs.rs/crate/winit/0.31.0-beta.3) and [`TabletToolData`](https://docs.rs/winit/0.31.0-beta.3/winit/event/struct.TabletToolData.html), [`PointerSource`](https://docs.rs/winit/0.31.0-beta.3/winit/event/enum.PointerSource.html)
- [winit CHANGELOG v0.31](https://raw.githubusercontent.com/rust-windowing/winit/master/winit/src/changelog/v0.31.md)
- [egui CHANGELOG](https://raw.githubusercontent.com/emilk/egui/main/CHANGELOG.md) · [docs.rs/crate/eframe/0.36.2/features](https://docs.rs/crate/eframe/0.36.2/features) · [docs.rs/crate/egui-winit/0.36.2/features](https://docs.rs/crate/egui-winit/0.36.2/features)
- [AccessKit README](https://raw.githubusercontent.com/AccessKit/accesskit/main/README.md)
- [Slint LICENSE.md](https://github.com/slint-ui/slint/blob/master/LICENSE.md)
- [wgpu CHANGELOG](https://raw.githubusercontent.com/gfx-rs/wgpu/trunk/CHANGELOG.md)
- [Vello README](https://raw.githubusercontent.com/linebender/vello/main/README.md)
- [kurbo 0.13 docs](https://docs.rs/kurbo/latest/kurbo/)
- [Parley README](https://raw.githubusercontent.com/linebender/parley/main/README.md) · [docs.rs/crate/parley/0.11.1/features](https://docs.rs/crate/parley/0.11.1/features)
- [iOverlay](https://github.com/iShape-Rust/iOverlay) · [i_curve docs](https://docs.rs/i_curve/latest/i_curve/) · [iCurve](https://github.com/iShape-Rust/iCurve)
- [octotablet](https://github.com/Fuzzyzilla/octotablet) · [wintab_lite](https://github.com/thehappycheese/wintab_lite)
- [docs.rs/crate/zip/8.6.0/features](https://docs.rs/crate/zip/8.6.0/features) · [docs.rs/crate/flate2/1.1.10/features](https://docs.rs/crate/flate2/1.1.10/features)
- [Xara LX — licence header (Kernel/group.h)](https://github.com/samuell/xara-xtreme/blob/master/Kernel/group.h) · [Xara Xtreme LX (Wikipedia)](https://en.wikipedia.org/wiki/Xara_Xtreme_LX) · [xaraxtreme.org — Licensing and contributing](http://www.xaraxtreme.org/Developers/developers-licensing-a-contributing.html)
- [AppImage — Best practices](https://docs.appimage.org/reference/best-practices.html) · [linuxdeploy user guide](https://github.com/AppImage/docs.appimage.org/blob/master/source/packaging-guide/from-source/linuxdeploy-user-guide.rst) · [AppImage (Wikipedia)](https://en.wikipedia.org/wiki/AppImage)
- [Snap vs Flatpak vs AppImage — a 2026 comparison](https://computingforgeeks.com/snap-vs-flatpak-vs-appimage/) · [Flatpak vs Snap vs AppImage in 2026](https://sumguy.com/flatpak-vs-snap-vs-appimage/)
- [The Rust GUI Landscape in 2026](https://wrenlearnsrust.com/posts/2026-03-11-rust-gui-landscape-2026.html) · [The State of Rust GUI — Rust Bytes](https://weeklyrust.substack.com/p/the-state-of-rust-gui-the-good-and) · [Thanks for All the Frames: Rust GUI Observations](https://tritium.legal/blog/desktop)
- [Iced 0.14 (Phoronix)](https://www.phoronix.com/news/Iced-0.14-Rust-GUI-LIbrary) · [Release 0.14.0 · iced-rs/iced](https://github.com/iced-rs/iced/releases/tag/0.14.0)
- [Graphite — progress report](https://graphite.art/blog/graphite-progress-report-q3-2024/) · [Bezier-rs (lib.rs)](https://lib.rs/crates/bezier-rs)
