# Phase 4 — Render engine

> After this phase a Xarast document turns into correct pixels — on the GPU and on the CPU, with Xara's gradients, Xara's twelve transparency families and antialiasing at least as good as CDraw's — without any UI being present.

## Goal

Build `xarast-render`: our own retained scene, display list, tiler and compositor, with
**two interchangeable backends** (`vello` on `wgpu`, and `vello_cpu`) used only as
antialiased coverage rasterisers. This is architecture §3.3 turned into code.

The phase is the single highest-risk item in the project (`research/04 §2.4`,
`roadmap` principle 2): the rasteriser is what made Xara Xara, and it is the one
component whose failure is not recoverable by working harder elsewhere. Therefore the
phase **opens with a measured spike** (W0) whose go/no-go gate decides which rasteriser
we build on, before a single line of the engine proper is written.

What must be true when the phase closes:

1. A `.xar` document parsed by Phase 3 renders to a PNG through `xarast-cli`, on both
   backends, with the CPU backend bit-deterministic.
2. Every gradient shape, repeat mode, mapping and ramp profile Xara can express renders
   correctly (`research/03 §2.6`).
3. All twelve transparency/blend families render correctly and identically on CPU and GPU
   (`research/03 §2.7`, `§3.4`).
4. A golden-image harness exists and gates CI.
5. Incremental redraw (dirty rects, scroll reprojection, per-node caches) is implemented
   and measured, so Phase 5 can hit the 16 ms pan/zoom budget.

---

## Scope

### In scope

| # | Item | Reference |
|---|---|---|
| S1 | Rasteriser spike and go/no-go decision | `research/03 §3.2`, this doc W0 |
| S2 | `Rasterizer` facade trait (the 1:1 replacement for `GDrawContext`) | `research/03 §3.3` |
| S3 | Retained `Scene` + immutable per-frame `DisplayList` with explicit layer push/pop | `research/03 §3.5(1,2)` |
| S4 | The doc→scene adapter contract (walker lives in `xarast-app`, see "Crate boundary" below) | architecture §2, §4 |
| S5 | Path fill (nonzero, evenodd, and their inverse variants) and stroke | `research/03 §2.2`, R1, R3 |
| S6 | Tiling (GPU, 256×256) and banding (CPU, ≥16 scanlines) with bbox and clip culling | `research/03 §3.5(3)` |
| S7 | Per-node render cache keyed by `(content hash, quantised resolution, variant)` with cost-weighted LRU eviction | `research/03 §3.5(1)`, architecture §4 |
| S8 | Incremental redraw: dirty-rect accumulation, scroll reprojection for pan, cached-rescale for zoom | `research/03 §3.5(4)`, R11 |
| S9 | All five gradient shapes × {simple, repeat, mirror, repeat-HQ} × {affine, perspective}, 2-colour arbitrary ramps + 3/4-colour meshes | `research/03 §2.6.1`, R4 |
| S10 | Ramp construction with the exact Schlick bias/gain profile, RGB / HSV-short / HSV-long interpolation, 256- and 2048-entry tables | `research/03 §2.6.2`, `§2.6.3` |
| S11 | The twelve blend/transparency families, as 256×256 LUTs plus the three analytic ones (Saturation, Luminosity, Hue), mirrored bit-for-bit on CPU and GPU | `research/03 §2.7.2`, `§3.4`, R5 |
| S12 | Graduated and bitmap-sourced transparency feeding the same twelve families | `research/03 §2.7.3`, R6 |
| S13 | Image paint: parallelogram and perspective mapping, four repeat modes, nearest/bilinear/HQ filtering | `research/03 §2.8`, R7 |
| S14 | Antialiasing quality levels and the AA validation method against the original | `research/03 §2.3`, `§3.8` |
| S15 | Render quality levels `Draft` / `Final` with deferred, cancellable, tile-prioritised upgrade | `research/03 §3.5(5)`, `§2.4` |
| S16 | Golden-image harness: CPU exact, GPU↔CPU perceptual, corpus runner, failure artefacts | `research/05 §13.1` |
| S17 | Precision policy enforcement (never absolute millipoints in `f32`) and an explicit rounding rule | `research/03 §3.7` |
| S18 | Overlay surface API for handles/marquees (the replacement for Xara's XOR blob rendering), drawn by Phase 5/7 but owned here | `research/04 §1.19`, `§3` item 3 |

### Explicitly out of scope (and which phase owns it)

| Item | Owner |
|---|---|
| Window, surface creation, presentation to screen, `wgpu` instance/adapter selection ladder | Phase 5 (`xarast-shell`) |
| Any UI, panels, handle interaction | Phase 5 / Phase 7 |
| Boolean path ops, stroke→path, offsetting, flattening primitives | Phase 1 (`xarast-geom`) — this phase *consumes* them |
| Text shaping, layout, glyph rasterisation | Phase 9 (`xarast-text`); Phase 4 renders glyph outlines only if handed paths |
| Bitmap import/decode, photo adjustments, contone, bitmap gallery | Phase 10 (`xarast-image`) — this phase renders an already-decoded `ImageRef` |
| Fractal/plasma fill generation (`fracfill` midpoint displacement) | Phase 13; Phase 4 only defines `Paint::Fractal` as materialising to an `Image` |
| Live effects: shadow, feather, bevel, contour, blend, mould | Phase 13 — but `push_layer`/`pop_layer` and the offscreen capture machinery they need is built **here** |
| Interactive on-canvas fill/transparency handles | Phase 8 |
| Export encoders (PNG/JPEG/WebP/PDF) | Phase 11 — this phase produces pixel buffers only |
| Dither styles, <32 bpp output, CMYK separation, UCR/GCR | Deferred (`research/03 §3.9` M8): only `DITHER_NONE` is needed before v0.1; the rest is legacy-print work with no phase yet |

---

## Prerequisites

| Need | Source | Hard or soft |
|---|---|---|
| Workspace, CI, `cargo deny`, AppImage job | Phase 0 | Hard |
| `xarast-geom`: `Path`, `Rect`, `Matrix`, millipoint type, flattening, stroke→path | Phase 1 | Hard |
| `xarast-color`: colour models and conversions, `ColorU8` | Phase 1 | Hard |
| `xarast-doc` node arena (only for the adapter in `xarast-app` and for the corpus runner) | Phase 2 | Soft — `xarast-render` itself must compile and be tested with no `xarast-doc` dependency |
| `.xar` corpus parsed into documents, for the golden corpus | Phase 3 | Soft — the golden corpus starts from hand-written scenes and grows into `.xar` files as Phase 3 lands |
| A reference render of the original (Xara LX + `libCDraw.a` in an x86-64 VM) | This phase, W0/W7 | Hard for S14 validation |

**Crate boundary decision (settles an ambiguity in architecture §4).** The architecture
crate table gives `xarast-render` no dependency on `xarast-doc`, yet §4 says "the arena is
walked once per dirty region into an immutable display list". Both hold if the walker is
not in `xarast-render`. Therefore:

- `xarast-render` owns `Scene`, `DisplayList`, `Paint`, backends, compositor and caches,
  and knows nothing about nodes, attributes or layers.
- The walker (`arena + dirty region + attribute stack → Scene`) lives in
  `xarast-app::scene` and depends on both crates.
- `xarast-render` defines the contract the walker fills (`SceneBuilder` below) and ships
  its own tests built from `SceneBuilder` directly, with no document model present.

---

## Workstreams

### W0 — Rasteriser spike (do this first; nothing else starts until its gate passes)

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| R0.1 | Define and freeze the reference machine (CI runner CPU/GPU, driver versions) and record it in `docs/memory/perf.md` | — | S | — |
| R0.2 | Build the spike harness: five synthetic scenes + a fixed measurement protocol | `xarast-render` (`benches/spike`) | M | R0.1 |
| R0.3 | Candidate A: `vello_cpu` 0.2 | spike | M | R0.2 |
| R0.4 | Candidate B: `vello` 0.10 on `wgpu` 30 | spike | M | R0.2 |
| R0.5 | Candidate C: `vello_hybrid` 0.2 | spike | S | R0.2 |
| R0.6 | Candidate D (control): `tiny-skia` 0.12 — reference only, never a production dependency | spike | S | R0.2 |
| R0.7 | Candidate E (escape hatch): `lyon` tessellation + our own wgpu pipeline, measured only on the AA and conflation axes | spike | M | R0.2 |
| R0.8 | Ground-truth generator: 16×16 box-supersampled renderer, deliberately slow and obviously correct | spike | M | R0.2 |
| R0.9 | Original-output capture: render the AA probe set with Xara LX + `libCDraw.a` in an x86-64 VM | — | M | R0.1 |
| R0.10 | Write the decision record into `docs/memory/render.md`; open the phase gate | — | S | R0.3–R0.9 |

**The five spike scenes.** All are generated by code, checked into `tests/spike/`, and
identical for every candidate.

1. `edges` — 2,000 near-horizontal and near-vertical edges at angles 0.1°…5°, the case
   where CDraw's 17×5 sub-scanline scheme is weakest and analytic coverage wins.
2. `hairlines` — 10,000 strokes of 0.2 pt at random angles: tests coverage on sub-pixel
   geometry.
3. `bulk` — 100,000 filled paths averaging 12 segments each, laid out over an A3 spread,
   rendered at 1920×1080 and again at 3840×2160.
4. `seams` — 5,000 pairs of shapes sharing an exact edge (the conflation-artefact probe).
5. `layers` — 200 nested offscreen layers, each composited with a destination-reading
   blend, which is the structure Xara's captures produce (`research/03 §2.5`).

**What is measured, exactly.**

| Axis | Metric | How |
|---|---|---|
| M1 Throughput (CPU) | ms/frame for `bulk` at 1920×1080, single-thread and with `rayon` over all cores | `criterion`, 50 samples, median |
| M2 Throughput (GPU) | ms/frame for `bulk` at both resolutions, GPU timestamp query | `wgpu-profiler` |
| M3 Incremental cost | ms to redraw a 64×64 dirty rect in `bulk` | `criterion` |
| M4 AA fidelity | mean ΔE₀₀ and p99 ΔE₀₀ of `edges` + `hairlines` against the W0.8 ground truth | our diff tool |
| M5 AA level count | number of distinct coverage values produced along a 0.5° edge | histogram of the rendered ramp |
| M6 Conflation | count of pixels on `seams` whose value differs from the ground truth by >1/255 | our diff tool |
| M7 Determinism | byte-identical output over 100 runs on one machine, then across two x86_64 machines and one aarch64 | hash of the buffer |
| M8 Interposability | can a layer be rendered into an offscreen texture/buffer and composited by *our* pass, with no patch to the crate? yes/no + lines of glue | code |
| M9 Weight | added binary size (stripped, release) and cold `cargo build` time | `ls -l`, `cargo build --timings` |
| M10 Licence | must be `MIT OR Apache-2.0` or compatible; `cargo deny check licenses` clean | CI |

**Go/no-go thresholds.** Numbers are relative to the R0.1 reference machine and are
recorded there; they are absolute pass/fail, not guidance.

- **G1 (CPU throughput).** `bulk` at 1920×1080 ≤ 120 ms single-thread and ≤ 25 ms with
  `rayon` across 8 cores.
- **G2 (GPU throughput).** `bulk` at 1920×1080 ≤ 8 ms on the reference integrated GPU
  (this is the headroom the 16 ms pan/zoom budget needs once the UI, compositor and
  present are added).
- **G3 (AA fidelity).** mean ΔE₀₀ < 1.0 and p99 ΔE₀₀ < 3.0 against ground truth, **and**
  ≥ 128 distinct coverage levels on M5. CDraw gives 85 levels in normal mode and 132 in
  high quality (`research/03 §2.3`); matching 132 is the bar, because the stated aim is to
  equal or beat the original, never to reproduce its artefacts.
- **G4 (determinism).** The CPU candidate must pass M7 byte-identically on the same
  architecture. Cross-architecture divergence is tolerated **only** if it is documented
  and the golden baseline is then stored per architecture; if it cannot even be made
  stable per architecture, the candidate fails outright, because the whole export and
  golden-test strategy rests on it (architecture §3.3).
- **G5 (conflation).** ≤ 0.05 % of `seams` pixels exceeding 1/255, after applying our
  intended layer strategy. A candidate that only passes by rendering everything into one
  flat layer fails, because live effects need nested layers.
- **G6 (interposability).** M8 must be "yes" with ≤ 300 lines of glue. A candidate that
  requires forking its compositor to get our blend modes in fails: forking is exactly the
  maintenance trap `libCDraw.a` taught us to avoid.
- **G7 (weight).** ≤ 8 MB added to the stripped binary (the AppImage budget is 80 MB
  total) and no C++ toolchain.

**Decision rules.**

- Pass G1+G3+G4+G5+G6+G7 on `vello_cpu` and G2 on `vello` → proceed exactly as
  architecture §3.3 states. This is the expected outcome.
- `vello` fails G2 but `vello_cpu` passes → ship CPU-only for Phase 4, move the GPU
  backend behind a feature flag, re-evaluate `vello_hybrid` at the Phase 5 gate. The
  16 ms budget then becomes Phase 5's risk, not Phase 4's.
- `vello_cpu` fails G4 → it cannot be the deterministic oracle. Fall back to `tiny-skia`
  as the oracle for golden tests only (it is BSD-3, test-only use is already the plan)
  and record that production stays on the vello family.
- Both vello backends fail G3 → escalate: build our own analytic scanline rasteriser
  following the `Edge`/`Curve`/`Strip` model documented in `research/03 §2.2`. Budget for
  that branch is **to be determined in this phase**: it is estimated by timing R0.7
  (the lyon+own-pipeline candidate) to completion and extrapolating, and the estimate
  goes in `docs/memory/render.md` before the branch is taken.

Nothing in W1–W7 starts before R0.10 is written. If W0 slips, the whole phase slips; that
is the intended behaviour, because building on an unmeasured rasteriser is the failure
mode this phase exists to prevent.

---

### W1 — Scene, display list and the facade

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| R1.1 | `Scene` retained structure: nodes, content hashes, bounds, transform, paint refs | `xarast-render` | L | W0 |
| R1.2 | `SceneBuilder` API (what the `xarast-app` walker calls) | `xarast-render` | M | R1.1 |
| R1.3 | `DisplayList`: flat, immutable, ordered `DrawCmd` stream with explicit `PushLayer`/`PopLayer` | `xarast-render` | M | R1.1 |
| R1.4 | `needs_dst_read` propagation and strict ordering constraints | `xarast-render` | M | R1.3 |
| R1.5 | `Rasterizer` trait and `Surface`/`DeviceRect`/`DirtyRect` types | `xarast-render` | M | R1.3 |
| R1.6 | Precision layer: `f64` transform algebra, tile-relative `f32` conversion, the rounding rule | `xarast-render` | M | R1.1 |
| R1.7 | Overlay surface: a second display list composited above the document, never cached | `xarast-render` | S | R1.3 |

**Tricky parts.**

*The display list must be fully ordered before tiling.* Xara's exotic blends read the
destination (`research/03 §3.4`), so they cannot use fixed GPU blend state and cannot be
reordered. `DrawCmd::needs_dst_read` is set by the paint/transparency resolution, and the
tile planner must treat a `needs_dst_read` command as a barrier inside its tile: everything
before it in that tile is resolved, then a ping-pong swap happens. Getting this wrong
produces output that is *nearly* right, which is the worst kind of bug; W7's GPU↔CPU
parity test is the detector.

*Precision is a hard rule, not a guideline* (`research/03 §3.7`). A 5 m wide document is
3.6×10⁸ millipoints; `f32` resolves that to ~43 mp ≈ 0.04 pt, visible when zoomed. Every
`f64 → f32` conversion in this crate happens **after** subtracting the tile origin. Enforce
it with a newtype: `TileLocal(f32)` can only be constructed by
`Tile::localise(p: Point64) -> TileLocal`, and clippy denies raw `as f32` in the crate via
`#![deny(clippy::cast_possible_truncation)]` plus a review rule.

*The rounding rule.* CDraw truncates on device conversion and the application compensates
by adding half a pixel (`grndrgn.cpp:5290-5300`). We do not inherit the compensation:
Xarast rounds **half away from zero** on device conversion, with no added offset. This is
a deliberate 1-pixel difference from the original and must be stated in the golden-image
comparison methodology, or W7 will chase phantom misalignments.

---

### W2 — CPU backend and compositor

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| R2.1 | `CpuRasterizer` over the W0-selected CPU rasteriser: fill, stroke, clip | `xarast-render` | L | W1 |
| R2.2 | Banding planner with the `GRenderDIB` memory heuristic (≥16 scanlines, band count from available RAM) | `xarast-render` | M | R2.1 |
| R2.3 | CPU compositor: `u8` sRGB, premultiplied working buffers, layer stack | `xarast-render` | L | R2.1 |
| R2.4 | `rayon` parallelism over bands, with deterministic merge order | `xarast-render` | M | R2.2, R2.3 |
| R2.5 | Offscreen layer allocation and reuse (the `Capture` equivalent) | `xarast-render` | M | R2.3 |

**Tricky parts.**

*Compositing happens in 8-bit non-linear sRGB, not linear* (`research/03 §2.10`). All of
CDraw's LUTs are 256-entry tables defined on encoded sRGB; compositing in linear light
gives visibly different Stained Glass and Bleach. The CPU buffers are therefore
`Rgba8Unorm`-equivalent and the GPU target must be `Rgba8Unorm`, **never** `Rgba8UnormSrgb`
— otherwise the hardware linearises behind our back. There is one exception: blur and
high-quality resampling work in `u16` or linear `f32` internally and come back to `u8`,
to avoid banding without changing blend semantics.

*Determinism survives parallelism only if the merge order is fixed.* `rayon` may finish
bands in any order; the write-back must be by band index, and any floating-point
accumulation that crosses band boundaries is forbidden. This is what makes G4/W7-A work.

---

### W3 — Paints: gradients, ramps, images

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| R3.1 | `Paint` and `GradShape` types; parameter validation | `xarast-render` | M | W1 |
| R3.2 | Linear, radial (elliptical, two independent radii), conical, diamond (L∞), 3-colour and 4-colour mesh evaluation | `xarast-render` | L | R3.1 |
| R3.3 | Repeat modes: simple, repeat, mirror, repeat-HQ | `xarast-render` | M | R3.2 |
| R3.4 | Perspective mapping (projective interpolation with the fourth point D) plus validity check | `xarast-render` | M | R3.2 |
| R3.5 | `Profile` (Schlick bias/gain) with the exact `biasgain.cpp` formulas and the identity short-circuit | `xarast-render` | S | R3.1 |
| R3.6 | Ramp builder: multi-stop, RGB / HSV-short / HSV-long, 256- and 2048-entry tables, fixed-point interpolation path | `xarast-render` | M | R3.5 |
| R3.7 | `RampCache` keyed by stops+profile+space+length | `xarast-render` | S | R3.6 |
| R3.8 | Image paint: parallelogram/perspective mapping, 4 repeat modes, nearest/bilinear/HQ filters | `xarast-render` | L | R3.1 |
| R3.9 | GPU-side paint evaluation in WGSL mirroring R3.2–R3.4 | `xarast-render` | L | R3.2, W5 |

**Tricky parts.**

*Conical and diamond gradients, and perspective mapping, exist in no candidate library*
(`research/03 §3.2`). They are ours. Conical in CDraw is implemented with an arctangent
table and per-quadrant blitters; we compute `s = atan2(...)` normalised to `[0,1)` in
`f32` in the shader and in `f64` on the CPU, and must verify the two agree within 1/255
after ramp lookup — the arctangent implementations differ between WGSL and Rust. Diamond
is `s = max(|u|, |v|)` in the A,B,C frame (the L∞ metric).

*The application, not the rasteriser, builds the ramp.* `gradtbl.cpp` builds the table and
hands CDraw a flat array; we do the same, which is why the profile, the HSV paths and the
spot-colour fallback (`gradtbl.cpp:453` forces RGB when spot colours are present) all live
in our ramp builder and not in any shader.

*`Length` is 256 or 2048.* Large tables exist to kill banding on big gradients
(`LargeGradTables`). Draft quality uses 256, Final uses 2048 (`research/03 §3.5(5)`).

*The exact profile formulas* (`research/03 §2.6.3`), reproduced here so implementers do not
have to open the research document:

```
b = (B + 1)·(0.5 − ε) + ε,  g = (G + 1)·(0.5 − ε) + ε,  ε = 1e-5,  B,G ∈ [−1,1]
bias(b, x) = x·b / ((1 − 2b)·(1 − x) + b)
C          = (1 − 2g)·(1 − 2x)
gain(g, x) = x·g / (C + g)           if x < 0.5
           = (C − x·g) / (C − g)     if x ≥ 0.5
profile(x) = gain(g, bias(b, x));   B = G = 0 ⇒ identity (short-circuit it)
```

---

### W4 — Transparency and the twelve blend families

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| R4.1 | `BlendFamily` enum and `Transparency` type; `TranspType`→family mapping including the two contiguous ranges | `xarast-render` | S | W1 |
| R4.2 | LUT generator (`build_blend_lut`) — one source of truth, used by both backends | `xarast-render` | M | R4.1 |
| R4.3 | CPU implementations of all twelve families, including the three analytic ones | `xarast-render` | L | R4.2, W2 |
| R4.4 | Recover CDraw's default luminance weights empirically and record them | `xarast-render` | M | R4.1 |
| R4.5 | LUT extraction harness against the original binary, for validation | `tests/` | M | R4.4 |
| R4.6 | Graduated and bitmap-sourced transparency feeding the same families | `xarast-render` | M | R4.3, W3 |
| R4.7 | Layer isolation semantics: what a `push_layer`/`pop_layer` pair means for alpha and for destination reads | `xarast-render` | M | R4.3 |

**Tricky parts.**

*All twelve families are one machine*: `L = f_family(Y(src), t)` then
`out_c = LUT_family[L][dst_c]`. That maps to a `256×256 R8Unorm` texture per family, or a
12-layer `TEXTURE_2D_ARRAY` at 768 KiB resident. Three families break the pattern:
**Saturation** and **Luminosity** read the *destination's* luminance, and **Hue** needs
RGB↔HSV; these are computed analytically in both backends. The formulas are in
`research/03 §2.7.2` and must be transcribed exactly — in particular Xara's convention is
**0 = opaque, 255 = fully transparent**, the inverse of ordinary alpha, and getting that
backwards produces plausible-looking wrong output.

*R4.4 is a blocker for correctness and is easy to forget.* Every family's `Y(c)` depends
on the luminance weights installed by `GColour_SetGreyConversionValues`, and **Xara LX
never calls it**, so CDraw's internal defaults apply and are not in any header. The
working hypothesis is ITU-R BT.601 (0.299 / 0.587 / 0.114). Method: render a Darken
transparency over a known 256-step grey-to-colour ramp with the original in the VM, solve
for the three weights by least squares, and confirm the residual is below 1/255. Record
the recovered weights in `docs/memory/render.md`; if they turn out not to be BT.601, every
formula in `research/03 §2.7.2` stays valid but the constant `W` in the shader changes.

*R4.5 is cheap and worth it.* `GDraw::CalcTransparencyX` are exported symbols with a known
signature `(GDraw*, BGR*, BGR, u8)`. A small C harness that dlopens the original and dumps
each family's full 256×256×t response gives us a reference table to diff our LUTs against
directly, instead of inferring correctness from rendered images.

---

### W5 — GPU backend

> **Superseded in part on 2026-09-23 (XARA-US-0011).** Measured on the
> reference machine, a `vello` raster misses the integrated-GPU budget by
> 9–11× and `vello` pins a different `wgpu`, so R5.1–R5.4 are not built.
> The GPU composites CPU-rasterised tiles instead (`GpuTileCache`), and the
> WGSL pass waits on the trigger in XARA-T-0051. Decision and numbers:
> `docs/memory/render.md`, "The GPU decision".

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| R5.1 | `GpuRasterizer` over `vello` + `wgpu` 30; device/queue supplied by the caller, never created here | `xarast-render` | L | W0, W1 |
| R5.2 | Tile planner: 256×256 binning by bbox, per-tile command lists | `xarast-render` | L | R5.1 |
| R5.3 | Composite pass in WGSL: paint resolution + family dispatch + LUT sampling | `xarast-render` | L | R5.2, W3, W4 |
| R5.4 | Ping-pong destination-read strategy for `needs_dst_read` tiles | `xarast-render` | M | R5.3 |
| R5.5 | Offscreen layer textures, pooling and reuse | `xarast-render` | M | R5.3 |
| R5.6 | Capability detection and the backend ladder hook (levels 0–3 of `research/05 §4.3`) | `xarast-render` | M | R5.1 |
| R5.7 | `wgpu-profiler` timestamps per pass, exported as a `FrameTimings` struct | `xarast-render` | S | R5.1 |

**Tricky parts.**

*The render target format must be `Rgba8Unorm`.* See W2. If someone "fixes" it to
`Rgba8UnormSrgb` because the colours look washed out, every blend family silently changes.
Put an assertion in `begin_frame` and a comment pointing at `research/03 §2.10`.

*Destination reads need strict ordering and locality.* Tiles exist precisely so the
ping-pong textures stay small and cache-resident. A tile containing a `needs_dst_read`
command is resolved in two halves with a texture swap between them; a tile with many such
commands degenerates into many swaps, which is the performance cliff to watch. Measure it:
the `layers` spike scene is exactly this case.

*wgpu 30 has breaking details worth writing down:* integer interpolation is no longer
`flat` by default (annotate `@interpolate(flat)` on the family index), vertex buffer slots
and bind group layouts are `Option<_>`, and presentation is `Queue::present(surface_texture)`.

*The GPU backend never owns the device.* Phase 5 creates the adapter, device and queue and
passes them in. This keeps `xarast-render` testable headless with `lavapipe` and keeps the
downlevel ladder a shell concern.

---

### W6 — Tiling, caching, incremental redraw, quality levels

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| R6.1 | `CacheKey` = content hash + quantised scale + quality variant; `NodeCache` slots with `cost` in µs | `xarast-render` | M | W1 |
| R6.2 | Cache admission policy: only expensive nodes (transparent groups, effects, fractal fills, groups over N primitives) | `xarast-render` | M | R6.1 |
| R6.3 | Cost-weighted LRU eviction under a byte budget | `xarast-render` | M | R6.1 |
| R6.4 | Scale quantisation to powers of √2 with ±41 % rescale tolerance and background regeneration | `xarast-render` | M | R6.1 |
| R6.5 | Dirty-rect accumulation: union of old and new node bounds; `end_frame` returns it | `xarast-render` | M | W1 |
| R6.6 | Pan: scroll reprojection of valid pixels, rasterise only the new strips | `xarast-render` | M | R6.5 |
| R6.7 | Zoom: present rescaled cache immediately, queue the exact re-render | `xarast-render` | M | R6.4 |
| R6.8 | `RenderQuality::{Draft, Final}` and the deferred, cancellable, visible-tiles-first upgrade with a 120 ms idle timer | `xarast-render` | L | R6.5 |

**Tricky parts.**

*Admission policy matters more than eviction policy.* Caching every node turns the cache
into a memory leak with extra steps. Xara's own criterion is encoded in the capture flags
`cfDIRECT`/`cfALLOWDIRECT` (`research/03 §2.5`): a node earns a cache slot when
regenerating it is expensive relative to compositing it. Start with: transparent groups,
any live-effect subtree, fractal fills, and groups above a threshold of primitives whose
value is **to be determined in this phase** — determined by sweeping the threshold over
the corpus in the W7 benchmark and picking the knee of the frame-time curve.

*Draft vs Final is not "lower resolution".* Per `research/03 §3.5(5)`: Draft keeps AA on
but multiplies flatness by 5, uses nearest sampling for images, 256-entry ramps, and does
not recompute effects; Final restores full flatness, HQ filtering and 2048-entry ramps.
AA stays on in Draft because turning it off is exactly the visual regression users notice.
Note that the original's default quality of 100 means AA was actually **off** by default in
Xara LX and tools raised quality to 110 (`research/03 §2.4`) — we do not copy that default;
Xarast is antialiased always.

---

### W7 — Validation: golden images and parity

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| R7.1 | Golden harness: run a scene or document, produce PNG, compare, emit diff artefacts on failure | `tests/`, `xarast-cli` | L | W2 |
| R7.2 | Level A gate: CPU backend, exact byte equality against committed goldens | CI | M | R7.1 |
| R7.3 | Level B gate: GPU↔CPU parity on a `lavapipe` runner | CI | M | R7.1, W5 |
| R7.4 | Level C: per-PR visual diff artefacts | CI | S | R7.1 |
| R7.5 | Feature corpus: ≥ 120 scenes covering every gradient shape × repeat × mapping, every blend family, every stroke join/cap, clipping, nested layers | `tests/corpus/` | L | R7.1 |
| R7.6 | Original-comparison suite: the `.xar` corpus rendered by Xara LX in the VM at several zooms, with and without AA | `tests/` | L | R0.9 |
| R7.7 | Blend-LUT unit tests against the R4.5 extraction | `xarast-render` | M | R4.5 |
| R7.8 | `criterion` benchmarks wired to the phase budgets, failing CI on regression | `benches/` | M | W6 |

**Tricky parts.**

*Architecture §7 open question 5 ("perceptual diff or exact match?") is assigned to Phase 3
but is answered by the harness built here.* The answer is both, at different levels, per
`research/05 §13.1`: **exact** on the CPU backend (tolerance 0 — that is the whole point of
having a deterministic oracle), **perceptual** for GPU↔CPU parity (ΔRMS < 0.5 %, no pixel
differing by more than 8/255), and **perceptual with a wider band** against the original
(mean ΔE₀₀ < 1.0, p99 < 3.0). Record this in `docs/memory/render.md` so Phase 3 does not
re-litigate it.

*Against the original, flat interiors must match exactly.* `research/03 §3.8` is precise
about this and it is the most useful part of the comparison: inside a fill, away from any
edge, antialiasing plays no role, so only the ramp maths and the blend formula are being
tested. Any difference there is a real bug. Edges are where we allow divergence, because
our coverage is analytic and CDraw's is 17×5 supersampled — better, not equal.

---

## Public API introduced

```rust
// ─────────────────────────── crates/xarast-render/src/lib.rs

/// The 1:1 replacement for `GDrawContext`. Backends implement it; nothing above
/// `xarast-render` names a backend type.
pub trait Rasterizer {
    fn begin_frame(&mut self, target: &mut Surface, clip: DeviceRect);
    fn fill_path(&mut self, path: &PathRef, rule: FillRule, paint: &Paint, xf: &Transform2D);
    fn stroke_path(&mut self, path: &PathRef, style: &StrokeStyle, paint: &Paint, xf: &Transform2D);
    fn draw_image(&mut self, img: &ImageRef, mapping: &Mapping, paint: &ImagePaint);
    /// Begins an offscreen layer. The equivalent of a Xara "capture".
    fn push_layer(&mut self, kind: LayerKind, bounds: DeviceRect) -> LayerId;
    fn pop_layer(&mut self, id: LayerId, blend: Blend, opacity: Transparency);
    /// Ends the frame and returns the union of everything touched (≈ `GetChangedBBox`).
    fn end_frame(&mut self) -> DirtyRect;
    fn capabilities(&self) -> RasterizerCaps;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillRule { NonZero, EvenOdd, NonZeroInverse, EvenOddInverse }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerKind { Plain, Isolated, DestinationReading }

#[derive(Debug, Clone, Copy)]
pub struct RasterizerCaps {
    pub deterministic: bool,
    pub max_texture_dim: u32,
    pub supports_dst_read: bool,
    pub tile_size: u32,
}

// ─────────────────────────── scene and display list

/// Retained scene. Built by `xarast-app`'s walker; `xarast-render` never sees a node.
pub struct Scene { /* … */ }

pub struct SceneBuilder<'a> { /* … */ }

impl<'a> SceneBuilder<'a> {
    pub fn begin(scene: &'a mut Scene, quality: RenderQuality) -> Self;
    pub fn push_group(&mut self, id: SceneNodeId, xf: Transform2D, hint: CacheHint) -> SceneNodeId;
    pub fn pop_group(&mut self);
    pub fn push_clip(&mut self, path: &PathRef, rule: FillRule);
    pub fn pop_clip(&mut self);
    pub fn push_transparency(&mut self, t: Transparency);
    pub fn pop_transparency(&mut self);
    pub fn fill(&mut self, id: SceneNodeId, path: &PathRef, rule: FillRule, paint: Paint);
    pub fn stroke(&mut self, id: SceneNodeId, path: &PathRef, style: StrokeStyle, paint: Paint);
    pub fn image(&mut self, id: SceneNodeId, img: ImageRef, mapping: Mapping, paint: ImagePaint);
    /// Content hash of the subtree just emitted; the cache key is derived from it.
    pub fn finish_node(&mut self, id: SceneNodeId) -> ContentHash;
    pub fn finish(self) -> SceneStats;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheHint { Never, Auto, Always }

/// Immutable, per-frame, thread-safe: this is what crosses to the render thread.
pub struct DisplayList { /* … */ }

impl DisplayList {
    pub fn build(scene: &Scene, view: &ViewParams, dirty: &DirtyRect) -> Arc<DisplayList>;
    pub fn commands(&self) -> &[DrawCmd];
    pub fn bounds(&self) -> DeviceRect;
    pub fn needs_dst_read(&self) -> bool;
}

#[derive(Debug, Clone)]
pub enum DrawCmd {
    Fill  { node: SceneNodeId, path: PathRef, rule: FillRule, paint: Paint, xf: Transform2D },
    Stroke{ node: SceneNodeId, path: PathRef, style: StrokeStyle, paint: Paint, xf: Transform2D },
    Image { node: SceneNodeId, img: ImageRef, mapping: Mapping, paint: ImagePaint },
    PushLayer { kind: LayerKind, bounds: DeviceRect, needs_dst_read: bool },
    PopLayer  { blend: Blend, opacity: Transparency },
    CachedSurface { node: SceneNodeId, key: CacheKey, bounds: DeviceRect },
}

#[derive(Debug, Clone, Copy)]
pub struct ViewParams {
    /// Document→device, in f64. Device units are pixels.
    pub transform: Transform2D,
    pub viewport:  DeviceRect,
    pub quality:   RenderQuality,
    pub dpi:       f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderQuality { Draft, Final }

// ─────────────────────────── paints

#[derive(Debug, Clone)]
pub enum Paint {
    Solid(ColorU8),
    Gradient {
        shape:   GradShape,
        mapping: GradMapping,
        repeat:  Repeat,
        ramp:    RampId,
    },
    Image {
        image:   ImageId,
        mapping: GradMapping,
        repeat:  Repeat,
        filter:  Filter,
        contone: Option<(ColorU8, ColorU8, EffectSpace)>,
        adjust:  BitmapAdjust,
    },
    /// Materialised to an `Image` by the (Phase 13) generator before rasterisation.
    Fractal(FractalParams),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradShape { Linear, Radial, Conical, Diamond, Mesh3, Mesh4 }

#[derive(Debug, Clone, Copy)]
pub enum GradMapping {
    /// A = origin, B = perpendicular axis, C = end. Matches Xara's A/B/C control points.
    Affine      { a: Point64, b: Point64, c: Point64 },
    Perspective { a: Point64, b: Point64, c: Point64, d: Point64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repeat { Simple, Repeat, Mirror, RepeatHq }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter { Nearest, Bilinear, HighQuality }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectSpace { Rgb, HsvShort, HsvLong }

// ─────────────────────────── ramps and the bias/gain profile

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Profile { pub bias: f64, pub gain: f64 }   // both in [-1, 1]; (0,0) = linear

impl Profile {
    pub const LINEAR: Profile = Profile { bias: 0.0, gain: 0.0 };
    /// Schlick bias/gain, identical to `CProfileBiasGain` (research/03 §2.6.3).
    pub fn map(&self, x: f64) -> f64;
    pub fn is_linear(&self) -> bool;
}

#[derive(Debug, Clone, Copy)]
pub struct Stop { pub offset: f32, pub color: ColorU8 }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RampLength { Short = 256, Long = 2048 }

pub fn build_ramp(
    stops: &[Stop], profile: Profile, space: EffectSpace, len: RampLength,
) -> Vec<ColorU8>;

pub struct RampCache { /* … */ }
impl RampCache {
    pub fn intern(&mut self, stops: &[Stop], profile: Profile,
                  space: EffectSpace, len: RampLength) -> RampId;
    pub fn get(&self, id: RampId) -> &[ColorU8];
}

// ─────────────────────────── transparency and blend

/// Xara's twelve families. Convention: 0 = opaque, 255 = fully transparent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlendFamily {
    Mix, StainedGlass, Bleach,
    Contrast, Saturation, Darken, Lighten,
    Brightness, Luminosity, Hue, Bevel,
    None,
}

#[derive(Debug, Clone)]
pub struct Transparency {
    pub family: BlendFamily,
    pub source: TranspSource,
}

#[derive(Debug, Clone)]
pub enum TranspSource {
    Flat(u8),
    Gradient { shape: GradShape, mapping: GradMapping, repeat: Repeat, ramp: RampId },
    Image    { image: ImageId, mapping: GradMapping, repeat: Repeat },
}

/// One source of truth for both backends: the CPU compositor indexes it, the GPU
/// uploads it as a 12-layer 256×256 R8Unorm array.
pub fn build_blend_lut(family: BlendFamily, weights: LumaWeights) -> [[u8; 256]; 256];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LumaWeights { pub r: f32, pub g: f32, pub b: f32 }
impl LumaWeights {
    /// Working hypothesis until R4.4 recovers CDraw's real defaults empirically.
    pub const BT601: LumaWeights = LumaWeights { r: 0.299, g: 0.587, b: 0.114 };
}

// ─────────────────────────── caching and incremental redraw

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub content: ContentHash,
    /// Scale quantised to powers of √2, as an exponent.
    pub scale_step: i16,
    pub quality: RenderQuality,
}

pub struct RenderCache { /* … */ }
impl RenderCache {
    pub fn with_budget(bytes: usize) -> Self;
    pub fn get(&mut self, key: CacheKey) -> Option<&CachedSurface>;
    pub fn insert(&mut self, key: CacheKey, surface: CachedSurface, cost_us: u32);
    pub fn invalidate_node(&mut self, node: SceneNodeId);
    pub fn stats(&self) -> CacheStats;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DirtyRect(pub Option<DeviceRect>);
impl DirtyRect {
    pub fn union(self, other: DirtyRect) -> DirtyRect;
    pub fn is_empty(&self) -> bool;
}

/// Pan: reuse valid pixels, rasterise only the newly exposed strips.
/// The equivalent of `GDraw_ScrollBitmap`.
pub fn scroll_surface(s: &mut Surface, dx: i32, dy: i32) -> [DirtyRect; 2];

// ─────────────────────────── backends and frame timing

pub struct CpuBackend { /* … */ }
impl CpuBackend {
    pub fn new(cfg: CpuConfig) -> Self;
    pub fn render(&mut self, dl: &DisplayList, target: &mut Surface) -> FrameTimings;
}

#[cfg(feature = "gpu")]
pub struct GpuBackend { /* … */ }
#[cfg(feature = "gpu")]
impl GpuBackend {
    /// Device and queue are created by `xarast-shell` and handed in; this crate
    /// never creates a `wgpu::Instance`.
    pub fn new(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>, cfg: GpuConfig)
        -> Result<Self, BackendError>;
    pub fn render(&mut self, dl: &DisplayList, target: &mut Surface) -> FrameTimings;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FrameTimings {
    pub build_us: u32,
    pub raster_us: u32,
    pub composite_us: u32,
    pub tiles: u32,
    pub cache_hits: u32,
    pub cache_misses: u32,
}
```

---

## Acceptance criteria

Each is a command that can be run or a number that can be read off its output.

1. **Spike gate recorded.** `docs/memory/render.md` contains the W0 table with one measured
   number per candidate per axis M1–M10, and an explicit statement of which gates
   G1–G7 each candidate passed. No implementation commit predates this file.
2. **Both backends render the corpus.**
   `cargo run -p xarast-cli --release -- render tests/corpus/*.xar --backend cpu --out /tmp/cpu`
   and `--backend gpu` both exit 0 for every corpus file.
3. **CPU determinism.** `cargo test -p xarast-render --test determinism` renders the whole
   feature corpus 20 times and asserts identical BLAKE3 hashes; the same test run on a
   second architecture records its own baseline and is compared against itself.
4. **Golden level A.** `cargo test -p xarast-render --test golden_cpu` passes with
   **zero** differing pixels over ≥ 120 committed scenes.
5. **Golden level B.** `cargo test -p xarast-render --test parity_gpu_cpu --features gpu`
   on a `lavapipe` runner reports ΔRMS < 0.5 % and no pixel differing by more than 8/255,
   for every corpus scene.
6. **Gradient coverage.** The feature corpus contains at least one scene per cell of
   {Linear, Radial, Conical, Diamond, Mesh3, Mesh4} × {Simple, Repeat, Mirror, RepeatHq} ×
   {Affine, Perspective} that is legal for that shape; `cargo test -p xarast-render
   --test gradient_matrix` asserts the cell count and that none is `todo!()`.
7. **Profile exactness.** `cargo test -p xarast-render profile::` verifies `Profile::map`
   against 1,000 tabulated `(B, G, x)` triples generated from the formulas in
   `research/03 §2.6.3`, to within 1e-12, and verifies the identity short-circuit.
8. **Blend-family exactness.** `cargo test -p xarast-render --test blend_luts` compares all
   twelve families, over the full 256×256 domain, against the tables extracted from the
   original binary (R4.5). Tolerance: **0** for the eight LUT families; ≤ 1/255 for
   Saturation, Luminosity and Hue.
9. **Luminance weights recovered.** `docs/memory/render.md` states the measured weights,
   the residual of the fit (< 1/255), and whether they are BT.601.
10. **Antialiasing beats the original.** `cargo run -p xarast-cli -- aa-report` on the AA
    probe set reports ≥ 132 distinct coverage levels and mean ΔE₀₀ < 1.0 / p99 < 3.0
    against the 16×16 supersampled ground truth — the original scores 85 (normal) and
    132 (high quality) on the level metric.
11. **Flat interiors match the original exactly.** `cargo test --test vs_original
    -- --interiors-only` reports 0 differing pixels in regions more than 2 px from any
    edge, over the `.xar` comparison corpus.
12. **Incremental redraw works.** `cargo bench -p xarast-render -- incremental` shows that
    redrawing a 64×64 dirty rect in the 100k-object scene costs < 2 % of a full frame, and
    that a 200 px pan rasterises only the newly exposed strips (asserted by counting
    rasterised pixels, not by timing).
13. **Cache behaves.** `cargo test -p xarast-render --test cache` asserts: a repeated frame
    with no changes issues zero rasterisation work; editing one node invalidates exactly
    that node's subtree; the cache never exceeds its byte budget under a 10,000-insert
    stress.
14. **No UI, no window.** `cargo test -p xarast-render` passes in a container with no
    display server and no GPU (CPU backend only), and `cargo tree -p xarast-render` shows
    no `winit`, `egui` or `xarast-doc` dependency.
15. **Precision rule enforced.** `cargo test -p xarast-render --test precision` renders a
    5 m wide document at 4000 % zoom and asserts geometry error below 0.25 device pixels;
    a grep-based test asserts no bare `as f32` on document coordinates in the crate.
16. **Budgets met and recorded.** `cargo bench -p xarast-render` meets every row of the
    table below, and the numbers are written into `docs/memory/perf.md`.

---

## Performance budgets

Measured on the R0.1 reference machine, release profile, and enforced by `criterion` with
a CI-failing regression threshold of +10 % over the recorded baseline.

| Budget | Target | Scene | Notes |
|---|---|---|---|
| GPU full frame, 100k objects, 1920×1080, Draft | ≤ 8 ms | `bulk` | Leaves 8 ms for UI + present inside the 16 ms roadmap budget |
| GPU full frame, 100k objects, 3840×2160, Draft | ≤ 16 ms | `bulk` | Degraded target; 4K is not the v0.1 gate |
| CPU full frame, 100k objects, 1920×1080, Final, 8 cores | ≤ 25 ms | `bulk` | The level-3 software path stays usable |
| Incremental 64×64 dirty rect, 100k objects | ≤ 0.3 ms | `bulk` | Dominated by cache lookup, not rasterisation |
| Pan by 200 px, 100k objects | ≤ 4 ms | `bulk` | Scroll reprojection + new strips only |
| Display-list build from a warm scene, 100k nodes | ≤ 3 ms | `bulk` | This runs on the render thread, not the UI thread |
| Ramp build, 2048 entries, 8 stops, with profile | ≤ 40 µs | — | Cached; this is the cold cost |
| Blend LUT set generation (12 families) | ≤ 15 ms, once at startup | — | Or moved to `build.rs` if it exceeds this |
| Resident LUT memory | ≤ 768 KiB | — | 12 × 256×256 R8 |
| Default render-cache budget | 256 MiB, configurable | — | Eviction must keep it under, measured |
| Draft→Final upgrade latency after idle | starts at 120 ms, first visible tile ≤ 30 ms later | — | Cancellable |

---

## Risks and mitigations

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| K1 | `vello`'s API moves under us (it is explicitly still evolving) | High | Medium | Pin exact versions; the `Rasterizer` facade is the only place that names vello types; budget one week per minor bump; the CPU backend keeps working regardless |
| K2 | `vello_cpu` is declared **alpha** by its authors | Medium | High | It is our determinism oracle, so alpha status is a real hazard: keep `tiny-skia` wired into the test harness as a second opinion, and keep the golden baselines regenerable from a single command |
| K3 | Destination-reading blends force many ping-pongs and blow the GPU budget | Medium | High | The `layers` spike scene measures it before we commit; fall back to resolving destination-reading layers on the CPU into a texture, which is exactly what Xara's captures did |
| K4 | The recovered luminance weights are wrong, so every family is subtly off | Medium | High | R4.4 solves for them empirically with a stated residual bound, and R4.5 cross-checks the full LUT against the original binary rather than against rendered images |
| K5 | Conical/perspective gradients diverge between WGSL and Rust (`atan2`, projective division) | Medium | Medium | Parity test per gradient cell with a 1/255 bound; if WGSL's `atan2` is the culprit, replace it with a shared polynomial approximation used by both backends |
| K6 | GPU golden tests are not reproducible across drivers | High | Medium | Already accepted: exact match is CPU-only; GPU is perceptual and runs on a pinned `lavapipe` |
| K7 | Cache admission policy leaks memory on pathological documents | Medium | Medium | Hard byte budget with an eviction test at 10,000 inserts; the budget is a user preference, and the status bar shows cache pressure (Phase 5) |
| K8 | Scene build becomes the bottleneck instead of rasterisation | Medium | Medium | Content hashes let the walker skip untouched subtrees; the display-list build budget above is measured separately so the bottleneck is visible |
| K9 | The whole vello family fails the AA gate and we must write our own rasteriser | Low | Very high | Detected in W0, before anything is built on top; the escape route is documented (`research/03 §2.2` describes the edge/strip model in enough detail to implement) and the cost estimate is produced from R0.7 |
| K10 | Original-output comparison is impossible because the 2006 binaries will not run | Medium | Medium | Fall back to the 16×16 supersampled ground truth for AA, and to the R4.5 symbol-level harness for blends; note in memory that exact numeric parity with the original is then unverifiable and state what replaces it |

---

## Test plan

**Unit.**
- `Profile::map` against tabulated values, plus property tests: monotonic in `x`, fixed
  points at 0 and 1, identity when `bias = gain = 0`.
- Ramp interpolation in RGB, HSV-short and HSV-long; the fixed-point path with 22
  fractional bits matches the `f64` path within 1/255.
- Gradient parameter evaluation: `s` for every shape at a grid of sample points, against
  closed-form expectations written independently of the implementation.
- Blend LUT generation per family (criterion 8 above).
- `DirtyRect` union/empty algebra; `scroll_surface` strip computation.
- `CacheKey` scale quantisation: adjacent zoom steps map to the same key inside ±41 %.

**Property (`proptest`).**
- Any display list rendered with the CPU backend twice yields identical bytes.
- `push_layer`/`pop_layer` are balanced for any random command sequence the builder can
  emit; unbalanced sequences are rejected at build time, not at render time.
- Rendering a scene at scale `s` then at `2s` never differs by more than the AA tolerance
  in flat interiors.

**Golden.**
- Level A (CPU, exact) over ≥ 120 feature scenes — blocking on every PR.
- Level B (GPU↔CPU, perceptual) over the same scenes — nightly plus pre-merge on
  render-touching PRs.
- Level C: per-PR diff artefacts uploaded for any failure, using `image-compare` for the
  SSIM/RMS report and a heat-map PNG.
- Against the original: the `.xar` corpus at 25 %, 100 % and 400 % zoom, AA on and off,
  with the "flat interiors exact, edges perceptual" split.

**Fuzz.**
- `fuzz_display_list`: arbitrary command sequences into the CPU backend, asserting no
  panic, no OOM, no unbounded loop (timeout 5 s/case). Degenerate geometry (NaN-free by
  construction, but zero-length, coincident and self-intersecting paths) is in-domain.
- `fuzz_ramp`: arbitrary stop arrays and profiles; assert output length and no panic.

**Benchmarks.** One `criterion` group per budget row, with the baseline committed and a
CI job that fails on a 10 % regression.

**Manual, once per phase.** Side-by-side screenshots of five real `Designs/` documents,
original vs Xarast, at 100 % zoom, attached to the phase-closing note. This catches the
class of error no metric catches: something that is numerically close and visibly wrong.

---

## Memory note

Update **`docs/memory/render.md`** (create it from the `INDEX.md` template as the phase
starts). It must end the phase containing:

- **Current state:** which backends exist, what they cover, what is stubbed.
- **The W0 decision record:** the full measurement table, the gate results per candidate,
  and the sentence "we build on X because Y" with the numbers that justify it. This is the
  single most valuable artefact of the phase — it is what stops a future agent from
  re-opening the rasteriser question on intuition.
- **Recovered constants:** CDraw's luminance weights with the fit residual; the effective
  AA level counts measured; the chosen flatness values for Draft and Final.
- **Decisions:** the rounding rule (round-half-away-from-zero, no half-pixel
  compensation); `Rgba8Unorm` and non-linear sRGB compositing, with the reason; the
  golden-image gate policy (exact on CPU, perceptual on GPU and against the original) —
  which also closes architecture §7 question 5; the cache admission threshold and how it
  was chosen; the crate-boundary decision that the doc→scene walker lives in `xarast-app`.
- **Invariants:** never put absolute millipoints in `f32`; never composite in linear
  space; the CPU backend is the oracle and must stay deterministic; `xarast-render` must
  not depend on `xarast-doc`, `winit` or `egui`; the LUTs have exactly one generator
  shared by both backends.
- **Dead ends:** anything tried in W0 and rejected, with its numbers — especially any
  candidate that looked attractive and failed a gate, so nobody re-tries it.
- **Open TODOs:** the deferred items (dither styles, sub-32bpp output, CMYK/UCR/GCR,
  fractal generation, live effects) with the phase that owns each.

Also add the measured budget rows to **`docs/memory/perf.md`**, including the reference
machine specification, since Phase 5 is judged against them.
