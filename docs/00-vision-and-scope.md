# Xarast — vision and scope

> Root document of the project. It defines **what** we are building, **why** in
> this shape, and **how far** each release goes. The technical decisions are
> developed in `docs/research/*` and the execution plan in `docs/phases/*`.

---

## 1. What Xarast is

**Xarast** is a native **Rust** reimplementation of *Xara Xtreme / Xara LX*: a
**vector** illustration program with integrated **photo editing**, focused on
**raw performance** and **direct on-canvas interaction**.

The goal is to bring back what made Xara singular — instant redraw,
best-in-class antialiasing, live manipulation of fills, transparencies, blends
and effects directly on the canvas — on a modern, portable, maintainable base.

**Platforms, in priority order:**

| Order | Platform | Deliverable | Status |
|---|---|---|---|
| 1 | **Linux** (native Wayland; X11 via XWayland/fallback) | **AppImage** x86_64, then aarch64 | Absolute priority |
| 2 | Windows 10/11 | MSI + portable | Later |
| 3 | macOS 12+ (Apple Silicon + Intel) | Notarised universal `.app` | Later |

---

## 2. Why reimplement instead of port

Analysis of the original sources (`/xara-xtreme`, a fork of Xara LX) confirms a
direct port is not viable:

1. **The render engine is a closed binary.** All vector rasterisation lives in
   `libs/{x86,x86_64,ppc,darwin}/libCDraw.a`, shipped **binary-only** under a
   bespoke interim licence (`libs/LIBS-LICENSE`) that was never opened. The
   public headers (`GDraw/gdraw.h`, `gdraw2.h`, `gconsts.h`) describe the API
   but carry **no implementation**. CDraw is precisely the part that made Xara
   both easy and powerful: antialiased rasterisation, gradients with non-linear
   profiles, bespoke transparency blend modes, fractal fills, blur and bevel.
   Without it the program cannot draw a single pixel.
2. **No modern targets.** The binaries are 2006 ELF objects for x86/x86_64,
   linked against contemporary glibc and libstdc++. There is no aarch64 build,
   no Apple Silicon, no 64-bit Windows.
3. **wxWidgets 2.6/2.8 on GTK2.** The UI layer (`wxOil/`, 182 `.cpp` files)
   depends on toolkits with no Wayland support and no maintenance at those
   versions.
4. **~1,200 C++ files** (559 `.cpp` + 627 `.h` in `Kernel/` alone) written in
   1990s idiom — bespoke macros, hand-rolled RTTI, manual memory management,
   private `INT32`/`UINT32` types — behind a build system (`autogen.sh`,
   `configure.in`) that no longer runs.
5. **The port was never finished.** Xara LX was an incomplete port of the
   Windows product; whole subsystems sit behind `#if 0` or a `PORTNOTE`.

**The original remains an enormous asset** — as an *executable specification*:

- It is the only existing documentation of the **`.xar` format**
  (`Kernel/cxf*.{cpp,h}`, 211 tags in `Kernel/cxftags.h`).
- It defines the **document model** (node hierarchy, attributes, layers).
- It records the **semantics** of every tool, fill, transparency and effect.
- `testfiles/` and `Designs/` are a real-world **validation corpus**.

> **Project rule:** the C++ original is a *normative reference for behaviour*,
> never a codebase to translate line by line. See
> `docs/11-licensing-and-clean-room.md`.

---

## 3. Design principles

1. **Performance first.** Interactive redraw is the identity of the product.
   Target: 60 fps pan and zoom over a 100k-object document on integrated
   graphics.
2. **Wayland as a first-class citizen.** Fractional scaling, client-side
   decorations, XDG portals for file dialogs, pressure-sensitive tablets via
   `tablet_v2`.
3. **An open, readable native format.** `.xarast` is a compressed container
   wrapping **SVG**. A Xarast file must open — with graceful degradation — in a
   browser or Inkscape, and with full fidelity in Xarast.
4. **Faithful `.xar` import.** Two decades of existing Xara documents must open
   without visible loss. This is a product requirement, not a nice-to-have.
5. **Memory safety, no `unsafe` outside justified FFI boundaries.** A parser for
   a legacy binary format is attack surface: the `.xar` importer is fuzzed from
   day one.
6. **A core decoupled from the UI.** Model, geometry, I/O and rendering live in
   crates with no toolkit dependency, which buys us a CLI converter, headless
   tests, and the freedom to replace the UI without a rewrite.
7. **Incremental, measurable parity.** `docs/research/04-feature-inventory.md`
   is the parity backlog; every phase closes a verifiable subset of it.
8. **Determinism and visual regression tests.** Every render change is checked
   against golden images.

---

## 4. Scope per release

### v0.1 — "Opens and draws" (Linux AppImage) — MVP
- Imports `.xar` (the subset covering the bulk of real documents).
- Reads and writes `.xarast`.
- High-quality rendering of paths, flat and gradient fills, transparency,
  strokes, groups, layers and bitmaps.
- Fluid navigation (pan/zoom), layer panel, colour panel.
- Tools: selector (move/scale/rotate/skew), rectangle, ellipse, basic node
  editing, simple text.
- PNG and SVG export.
- Undo/redo.
- A working x86_64 AppImage on Wayland.

### v0.2 — "A real editor"
- Interactive on-canvas fill and transparency tools.
- Shape boolean operations, alignment, Z-order, grouping.
- QuickShapes, full freehand/bézier, clipping tool.
- Advanced text (on a path, paragraph and character formatting, OpenType).
- Galleries: colours, layers, bitmaps, fonts.
- JPEG, WebP and PDF export.

### v0.3 — "What made Xara special"
- Blends, moulds (envelope/perspective), contours.
- Shadows, bevels, feathering — live effects.
- Fractal and bitmap fills.
- Photo work: non-destructive adjustments, cropping, masks.

### v1.0 — Cross-platform
- Windows and macOS.
- Wider import/export (EPS/PDF, EMF, animation).
- Production-grade performance and stability.

**Explicit non-goals:** binary compatibility with Xara plugins; recreating
Xara's UI pixel for pixel; writing the `.xar` format (read-only); real-time
collaborative editing (post-1.0).

---

## 5. Documentation map

| Document | Contents |
|---|---|
| `00-vision-and-scope.md` | This document |
| `research/01-xar-format.md` | The `.xar` binary format specification |
| `research/02-document-model.md` | The original's node, attribute and layer model |
| `research/03-render-engine.md` | What CDraw did and how to reimplement it |
| `research/04-feature-inventory.md` | Full parity backlog |
| `research/05-technology-stack.md` | Crate and tooling choices |
| `research/06-xarast-format.md` | The native `.xarast` format specification |
| `10-architecture.md` | Crate architecture and data flow |
| `11-licensing-and-clean-room.md` | Licence choice and clean-room policy |
| `phases/*` | Phase-by-phase execution plan |
| `memory/*` | Durable per-subsystem knowledge notes |
| `../CLAUDE.md` | Operating memory for agents working in this repository |
