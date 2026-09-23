# render

Memory note for the **render engine** (`crates/xarast-render`), Phase 4.

## Current state

`xarast-render` exists and renders. What is in it:

- **Scene → display list → bands → pixels.** `Scene` is retained and is built
  through `SceneBuilder`, which is the contract the `xarast-app` walker fills;
  the crate has no dependency on `xarast-doc` and never sees a node.
  `DisplayList` is immutable and per frame, with every transform resolved into
  device space, every paint mapping moved with its shape, and every command's
  device bounds precomputed.
- **The CPU backend, complete and deterministic.** It uses `vello_cpu` as a
  **coverage rasteriser only**: each primitive is rasterised as opaque white
  into a scratch pixmap the size of its clipped bounds, and its alpha is the
  antialiasing coverage. Paint evaluation and compositing are ours, which is
  what makes conical and diamond gradients, perspective mapping and Xara's
  twelve families possible at all.
- **Paints:** solid; all six gradient shapes (linear, radial, conical,
  diamond, 3-colour and 4-colour mesh) × four repeat modes × affine and
  perspective mappings; image fills with four repeat modes, three filters,
  contone and brightness/contrast/gamma/saturation adjustment.
- **Ramps:** multi-stop, RGB / HSV-short / HSV-long, 256- and 2048-entry
  tables, the Schlick bias/gain profile, the fixed-point transparency path
  with 22 fractional bits, and an interning cache.
- **Blends:** all twelve families, one 256 × 256 table each (768 KiB), one
  generator shared by both backends, plus the three analytic ones.
  Graduated and bitmap-sourced transparency feed the same families.
- **Cache and incremental redraw:** content-hash keys, √2 scale quantisation,
  cost-weighted LRU under a hard byte budget, an admission policy, dirty-rect
  culling and `scroll_surface` reprojection.
- **Validation:** a 120-scene generated feature corpus with committed goldens,
  exact CPU goldens, determinism over 20 runs, the gradient matrix, the blend
  domain, the precision rule (measured *and* grepped), AA level counts and a
  supersampled comparison, and `criterion` benches per budget row.

What is **not** in it, and who owns it:

| Missing | Owner |
|---|---|
| The WGSL compositing pass, ping-pong destination reads, GPU tile planner | Phase 4 follow-up / Phase 5 gate — see the GPU section below |
| Blur, shadow, feather, bevel, contour, blend, mould | Phase 13; `push_layer`/`pop_layer` and the offscreen machinery they need exist here |
| Fractal (plasma, clouds) generation | Phase 13; `Paint::Fractal` exists and refuses to rasterise until materialised |
| Dither styles, sub-32 bpp output, CMYK separation, UCR/GCR | Deferred (`research/03 §3.9` M8) |
| Glyph rasterisation | Phase 9; this crate renders glyph outlines if handed paths |
| `.xar` corpus rendering end to end | Needs `xarast-cli`, Phase 11 |

---

## The W0 decision record

### The machine

**There was no reference machine when the phase opened, and there still is
not one that matters.** Everything below was measured on the development
container:

| | |
|---|---|
| CPU | Intel Xeon @ 2.10 GHz, **4 vCPU** |
| RAM | 15 GiB |
| GPU | **none** — no `/dev/dri`, no Vulkan ICD, no software rasteriser |
| OS / toolchain | Linux 6.18, rustc 1.94.1, `--release`, `lto = "thin"` |
| `wgpu` adapters | **0** (`enumerate_adapters` returns empty; `request_adapter` fails with "vulkan drivers/libraries could not be loaded") |

The phase's gates were written against a machine with eight cores and an
integrated GPU. This one has half the cores, slower ones, and no GPU at all.
Read every throughput verdict below as "on this machine", and re-measure
before treating G1 or G2 as settled.

### The candidates, and what could be measured

| | A `vello_cpu` 0.2 | B `vello` 0.10 | C `vello_hybrid` 0.2 | D `tiny-skia` 0.12 (control) | E `lyon` + own pipeline |
|---|---|---|---|---|---|
| Measured? | **yes** | no adapter | no adapter | **yes** | not attempted |

B, C and E were not measured, and saying so is the honest result rather than
an omission. B and C cannot run without an adapter. E was budgeted as the
escape hatch for a G3 failure; G3 did not fail, so spending a week building a
tessellator to price a branch nobody is taking would have been waste. If the
vello family ever fails G3 on real hardware, that estimate still has to be
produced before the branch is taken.

### The numbers

The spike harness itself is not in the repository: it drives `tiny-skia` as a
*renderer* rather than as a test oracle, and it carries its own ground-truth
supersampler. Its five scenes survive in two places, which is what matters
for rebuilding it — the `edges`, `hairlines`, `seams` and `aa_*` cases of
`crates/xarast-render/src/corpus.rs`, and the `bulk` generator in
`crates/xarast-render/benches/render.rs`. The painter's-model reference it
established is `golden::downsample`.

**M1 — CPU throughput.** `bulk`: 100 000 filled paths averaging twelve
segments, 1920 × 1080, median of five frames including scene recording.
Two object-size variants, because the phase does not fix the size and the
overdraw factor dominates:

| Configuration | `bulk` r3..28 px (overdraw ×27) | `bulk` r2..8 px (overdraw ×2.6) |
|---|---|---|
| `vello_cpu`, 1 thread, u8 pipeline | **442 ms** (record 273 + raster 170) | **250 ms** (record 183 + raster 66) |
| `vello_cpu`, 4 threads | 194 ms (×2.28) | 91 ms (×2.74) |
| `vello_cpu`, 1 thread, f32 pipeline | 414 ms | 265 ms |
| `vello_cpu`, 1 thread, baseline SIMD | 516 ms | 323 ms |
| `tiny-skia`, 1 thread | 1 438 ms | 757 ms |
| `vello_cpu`, 4 threads, 3840 × 2160 | 187 ms | — |

Two things in that table matter more than the totals. **Recording dominates**:
273 of 442 ms is turning paths into strips, not filling pixels, and
`vello_cpu` has no scene-reuse API, so that cost is paid every frame. That is
the argument for the per-node cache holding *pixels*, which is what the phase
specifies. And **`tiny-skia` is 3.2× slower**, which settles it as a control
rather than a candidate.

**M2 — GPU throughput. Unmeasured.** Zero adapters.

**M3 — incremental cost.** A 64 × 64 dirty rect of the 100 000-object scene,
with binning done once outside the timed loop:

| | ms |
|---|---|
| `vello_cpu` | **0.814** |
| `tiny-skia` | 3.75 |
| A naive 100 000-object bbox scan, for comparison | 36.5 |

The third row is the finding: the scan costs forty times the render. The
display list must carry precomputed bounds and a tile bin, which it now does.

**M4 — AA fidelity**, 512 × 512, mean and p99 ΔE₀₀:

| Reference | `vello_cpu` u8 | `vello_cpu` f32 | `tiny-skia` |
|---|---|---|---|
| `edges` vs 16×16 supersampling | 0.352 / 7.82 | 0.339 / 7.82 | 1.026 / 10.39 |
| `edges` vs 48×48 supersampling | 0.310 / 7.82 | 0.285 / 7.83 | 1.024 / 10.27 |
| `edges` vs a **painter's-model** 48×48 reference | **0.072 / 0.647** | 0.038 / 0.557 | 0.879 / 7.64 |
| `hairlines` vs painter's-model 48×48 | **0.046 / 0.723** | 0.022 / 0.569 | 0.225 / 3.92 |
| one non-overlapping 0.5° edge vs 16×16 | 0.002 / 0.000 | 0.002 / 0.000 | 0.009 / 0.000 |

**This is the most important row of the spike, and it is about the
*reference*, not the candidate.** Against a supersampled ground truth that
resolves sub-pixel *visibility* between overlapping shapes, p99 is 7.8 and
refining the ground truth from 16×16 to 48×48 does not move it — so it is not
quantisation. It is conflation between separately drawn overlapping shapes,
which **no painter's-algorithm compositor avoids**, CDraw included. Against a
reference that composites exact per-shape coverage in order — the thing a
painter's model can actually reach — the same renderer scores p99 0.65. The
phase's G3 threshold was therefore being applied to a property nothing has.
`golden::downsample` documents the reference the golden harness uses, and the
corrected G3 is stated below.

**M5 — coverage levels** on a 0.5° edge, 1024 × 256:

| | levels |
|---|---|
| `vello_cpu`, u8 and f32 | **256** |
| 16×16 supersampled reference | 143 |
| `tiny-skia` | 17 |
| CDraw normal / high quality (`research/03 §2.3`) | 85 / 132 |

256 against a bar of 132. `tiny-skia`'s 17 on a shallow edge is the clearest
single reason it is not the production rasteriser.

**M6 — conflation**, `seams`, 512 × 512, 400 shared-edge pairs, all one
colour, counting interior pixels that are not exactly the fill colour:

| Case | `vello_cpu` u8 | `vello_cpu` f32 | `tiny-skia` |
|---|---|---|---|
| Two abutting shapes, drawn separately | 5 640 / 76 949 (2.15 % of the frame) | 5 525 | 4 403 |
| One merged nonzero path, **mixed** subpath winding | 4 283 / 56 134 (1.63 %) | 4 283 | **0** |
| One merged nonzero path, **consistent** winding | **0** | 0 | 0 |

The middle row is a real, reproducible property of `vello_cpu` and it is
worth knowing: its nonzero fill accumulates *fractional signed area*, so two
subpaths of opposite winding that share an edge cancel along it and leave a
seam. `tiny-skia`'s exact per-sample winding does not. Forcing the subpaths to
one direction removes it completely. Note that this is not simply a bug to
route around: for a donut the cancellation is exactly what nonzero *means*, so
blanket orientation normalisation would fill the holes. See the invariants.

The first row is inherent: abutting shapes drawn one after another always
conflate in a painter's model.

**M7 — determinism.** 100 renders of a 5 000-object scene, hashed:

| | distinct hashes |
|---|---|
| u8, 1 thread, detected SIMD | **1** |
| u8, 4 threads, detected SIMD | **1** |
| u8, 1 thread, baseline SIMD | **1** |
| f32, 1 and 4 threads | **1** |
| `tiny-skia`, 50 runs | 1 |

One thread and four threads produce **identical** bytes. Detected (AVX2) and
baseline SIMD do not: **3 of 786 432 channel samples differ, by at most
1/255**. Small, but "at most 1/255" is not "zero", and a golden baseline
cannot be "nearly". `CpuConfig::deterministic()` therefore pins the level.

**M8 — interposability. Yes**, in about twenty lines of glue: render a layer
into our own buffer, composite it with our own pass. 200 nested full-canvas
1024² layers with a multiply composite took 948 ms total, ≈ 4.7 ms per layer —
an upper bound, since a real layer is bounded by its content.

**M9 — weight.** Stripped, `lto = "thin"`, against a 341 KiB hello-world:

| | stripped size | delta | cold build |
|---|---|---|---|
| baseline | 341 KiB | — | — |
| `+ vello_cpu` | 1.39 MiB | **+1.05 MiB** | 45.6 s on 4 cores |
| `+ wgpu 30 + vello 0.10` | 3.20 MiB | +2.86 MiB | ≈ 5 min |

No C++ toolchain anywhere.

**M10 — licence.** `vello_cpu`, `vello` and `wgpu` are all
`Apache-2.0 OR MIT`. `tiny-skia` is BSD-3 and is a dev-dependency only.
`cargo deny check licenses` is clean.

### Gate results

| Gate | Verdict | Number |
|---|---|---|
| G1 CPU throughput ≤ 120 ms single, ≤ 25 ms on 8 cores | **fails as measured; unsettled** | 442 / 194 ms on 4 slow cores (250 / 91 ms on the lighter scene) |
| G2 GPU throughput ≤ 8 ms | **unmeasured** | no adapter |
| G3 AA: mean ΔE < 1.0, p99 < 3.0, ≥ 132 levels | **passes**, with the reference corrected | 0.072 / 0.647, 256 levels |
| G4 determinism, byte-identical per architecture | **passes**, with the SIMD level pinned | 1 hash / 100 runs; 3 of 786 432 samples move if it is not |
| G5 conflation ≤ 0.05 % | **passes** for a single path with consistent winding | 0.0000 %; 1.63 % with mixed winding; see the invariant |
| G6 interposability, ≤ 300 lines of glue | **passes** | ≈ 20 lines |
| G7 ≤ 8 MB added, no C++ | **passes** | +1.05 MiB (CPU), +2.86 MiB (with wgpu) |

### The decision

**We build on `vello_cpu` 0.2 as the coverage rasteriser, with our own scene,
display list, paints, blend families and compositor above it — exactly
architecture §3.3 — and we ship Phase 4 CPU-only, with the GPU backend behind
a feature flag.**

Why, with the numbers attached:

1. **Quality.** 256 coverage levels against CDraw's 132 and `tiny-skia`'s 17;
   mean ΔE₀₀ 0.07 against a reachable reference, where `tiny-skia` scores
   0.88. The goal was to equal or beat the original, and this clears it.
2. **Determinism.** Byte-identical over 100 runs and across thread counts,
   which is what the entire export and golden-image strategy rests on.
   `tiny-skia` is also deterministic, and would have been the fallback oracle
   had this failed; it did not.
3. **Interposability.** The compositor stays ours, in twenty lines. That is
   the whole point: nothing else on the list brings Xara's blend families, its
   conical and diamond gradients or its perspective mapping, and none of them
   would let us add them without a fork.
4. **Speed, relatively.** 3.2× faster than the control on the same scenes.
5. **Weight and licence.** 1.05 MiB, `Apache-2.0 OR MIT`, no C++.

And the honest counterweight: **G1 fails on this machine and G2 was never
measured**, so the performance half of the gate is not closed. The phase's own
decision rule for an unmeasurable GPU is "ship CPU-only for Phase 4, move the
GPU backend behind a feature flag, re-evaluate at the Phase 5 gate", and that
is what has been done. Re-run the spike on the real reference machine before
anyone claims the budgets are met.

Do **not** re-open the rasteriser question on intuition. Re-open it if, on
real hardware, `bulk` at 1920 × 1080 cannot reach 25 ms on eight cores, or if
`vello_cpu`'s alpha status produces a correctness regression the golden suite
catches.

---

## Recovered constants — and the ones that were not recovered

**The luminance weights were not recovered.** Task R4.4 needs the original
running in an x86-64 VM; this container has no VM, no copy of `libCDraw.a`,
and the clean-room rule keeps `GDraw/*.h` out of reach regardless.
`LumaWeights::BT601` (0.299 / 0.587 / 0.114) therefore stands as the stated
hypothesis, with **no measured residual**. `blend_luts::the_luminance_weights_
are_the_recorded_hypothesis` is the test that will fail loudly when the
constant changes, which is the point of writing it down.

**Four families are unverified against the original**, for the same reason:

| Family | Status |
|---|---|
| Mix, Stained Glass, Bleach, Darken, Lighten, Brightness | transcribed from formulas the research recovered instruction by instruction; exact |
| Saturation, Luminosity | reconstructed from the *semantics* the disassembly shows (`aSaturationTable1/2`, `Recip`, `Off`), not from their contents |
| Contrast, Bevel | documented approximations: `aContrastTable` and the bevel pair are static tables in the binary that no header describes |
| Hue | the RGB↔HSV path is exact; the hue byte interpolation follows the described `Mul`/`IMul` |

All twelve have the right endpoints — every one is the identity at t = 255,
which is tested over the whole domain — and all twelve go through one
generator, so the two backends cannot drift apart. Closing the gap is task
R4.5: a small harness that `dlopen`s the original and dumps
`GDraw::CalcTransparencyX` over its full 256 × 256 × t domain.

**Antialiasing, measured.** 256 distinct coverage levels on a 0.5° edge; mean
ΔE₀₀ 0.137 and max channel 24/255 against a 16× supersampled reference for an
ellipse at `Final` quality, 0.746 and 119/255 at `Draft`. Both residuals are
the *flatness tolerance*, not the coverage maths: a chord error of `t` device
pixels moves an edge by up to `t` of a pixel. For a straight edge, where box
averaging of exact sub-areas is exactly the area, the agreement is **bit for
bit at every angle tested**.

**Flatness.** `Final` allows a chord error of **0.1 device pixels** and
`Draft` five times that, 0.5 px. That is the original's rule read the right
way round: it uses half a pixel and divides by five when antialiasing is on
(`research/03 §2.2`), and Xarast is always antialiased, so its antialiased
value is our baseline and Draft lands on its non-antialiased one. Flatness is
**ours**, applied by the backend before the rasteriser sees the path;
otherwise `RenderQuality` would control nothing, which is exactly what it did
until this was wired.

---

## Decisions taken (and why)

**Rounding: half away from zero, no half-pixel compensation.** CDraw truncates
on device conversion and the application adds half a pixel back
(`grndrgn.cpp:5290`). We inherit neither. `precision::round_device` is the one
implementation, and the deliberate one-pixel difference from the original is
part of the golden-image methodology rather than a bug to chase.

**`Rgba8Unorm`, never `Rgba8UnormSrgb`; compositing in non-linear sRGB.**
Every blend family is a 256-entry table defined on encoded sRGB
(`research/03 §2.10`). An sRGB-converting target makes the hardware linearise
behind our back and silently changes Stained Glass and Bleach. The GPU backend
refuses the wrong format rather than producing plausible wrong colours.

**The golden-image gate — this closes architecture §7 question 5.** Both, at
different levels, and each level is chosen by what the comparison can honestly
promise:

| Gate | Compared | Tolerance | Why |
|---|---|---|---|
| A | CPU backend against a committed golden | **exact**, zero differing pixels | the whole point of a deterministic backend is that its output is a fact |
| B | GPU against CPU | ΔRMS < 0.5 %, no channel over 8/255 | GPU results are not reproducible across drivers; an exact gate would be a driver-version detector |
| C | against the original | mean ΔE₀₀ < 1.0, p99 < 3.0, **flat interiors exact** | our coverage is analytic and CDraw's is 17×5 supersampled — better, not equal — but inside a fill only the ramp maths and the blend formula are under test |

Gate A is only honest because the SIMD level is pinned; the spike measured
what happens otherwise (3 samples in 786 432, by 1/255). Phase 3 should not
re-litigate this.

**The AA reference is a painter's-model one**, not full supersampled
visibility. Measured above: the same renderer scores p99 7.8 against the
latter and 0.65 against the former, and the difference is a property no
painter's compositor has.

**Cache admission threshold: 64 primitives**, plus every transparent group and
every live-effect subtree unconditionally. Chosen from the `cache_threshold`
bench sweep, which is close to linear rather than showing a sharp knee: at
512 x 512 a group of 8 costs 4.41 ms, 32 costs 4.52, 64 costs 4.82, 128 costs
5.69 and 512 costs 10.08. 64 is the last size whose own cost stays inside a
tenth of a frame; beyond it the marginal cost per primitive roughly doubles. **The curve is weak evidence and the
number should be re-derived from the `.xar` corpus once it renders.**

**The doc→scene walker lives in `xarast-app`.** The crate table gives
`xarast-render` no dependency on `xarast-doc`, and §4 says the arena is walked
into a display list; both hold only if the walker is not here. `SceneBuilder`
is the contract it fills, and `cargo tree -p xarast-render` shows no
`xarast-doc`, `winit` or `egui`.

**Two deliberate deviations from the phase's API sketch**, both documented at
their definitions:

- `Paint::Gradient`'s ramp is `GradRamp` — a table *or* mesh corner colours —
  rather than a bare `RampId`, because a mesh has no ramp and a meaningless
  field on two of six shapes invites a `todo!()`. The shape enumeration is
  unchanged, so the matrix still has its 6 × 4 × 2 cells.
- `SceneBuilder::finish` returns `Result<SceneStats, SceneError>`. The phase
  asks for unbalanced push/pop to be rejected at build time; a return type is
  how that is said in Rust.

**`Profile` is `xarast-geom`'s `BiasGain`, not a second implementation.** They
are the same Schlick bias and gain; they differ only in the epsilon that keeps
the parameter off its poles, 1e-6 there against the 1e-5 that
`research/03 §2.6.3` records. `tests/profile.rs` measures the worst deviation
over a thousand tabulated triples and asserts it stays below 1e-4 — it is a
few times 1e-5, a hundredth of an 8-bit step, invisible in any table. If
`xarast-geom` ever adopts 1e-5 the test says so.

**A two-row guard band on every band's coverage.** Without it the band height
changed the picture by 1/255 at band boundaries, because the rasteriser's
strips start at the viewport edge. Antialiasing influence is local to one
pixel, so two rows make a banded render identical to an unbanded one; the
determinism suite asserts it over four band heights.

---

## Invariants that must not be broken

1. **Never put absolute millipoints into an `f32`.** A 5 m document is
   3.6 × 10⁸ mp and `f32` loses 0.04 pt of it. Every `f64 → f32` conversion of
   a coordinate happens relative to a tile origin, through `Tile::localise`.
   Enforced twice: a measured test at 4000 % zoom, and a grep test requiring
   every other `as f32` in the crate to carry an `// f32-ok:` justification.
2. **Never composite in linear light**, and never let a render target be
   `Rgba8UnormSrgb`.
3. **The CPU backend is the oracle and stays byte-reproducible.** Pin the SIMD
   level (`CpuConfig::deterministic`), merge parallel bands by index, and let
   no floating-point accumulation cross a band boundary.
4. **Exactly one blend-LUT generator**, `build_blend_lut`, shared by both
   backends. Two would drift, and the drift would be invisible until a user
   saw it.
5. **Xara's transparency is 0 = opaque, 255 = fully transparent**, the inverse
   of ordinary alpha. Getting it backwards produces plausible wrong output.
6. **`xarast-render` must not depend on `xarast-doc`, `winit` or `egui`**, and
   must build and test with no GPU and no windowing system.
7. **A nonzero path whose subpaths abut with opposite winding will show a seam
   along the shared edge**, because `vello_cpu` accumulates fractional signed
   area. Do **not** "fix" this by normalising every subpath's direction: for a
   donut the cancellation is what nonzero means. If it ever matters for real
   documents, split the offending subpaths into separate fills.
8. **A command whose family reads the destination is a barrier** in its tile.
   The display list is fully ordered before tiling and must stay that way.

---

## Dead ends (do not retry)

- **`tiny-skia` as the production rasteriser.** 3.2× slower on `bulk`, 17
  coverage levels on a 0.5° edge against `vello_cpu`'s 256, mean ΔE₀₀ 0.88
  against 0.07. It stays wired into the test harness as a second opinion, and
  as the fallback oracle if `vello_cpu`'s alpha status ever bites, which is
  the only reason to keep it.
- **A 16×16 supersampled "ground truth" as the antialiasing gate.** It
  resolves sub-pixel visibility between overlapping shapes, which no
  painter's-algorithm compositor reproduces; refining it to 48×48 moved p99 by
  0.00. Use the painter's-model reference (`golden::downsample`).
- **Trusting the rasteriser's internal flattening.** It made `Draft` and
  `Final` produce identical pixels and capped curve fidelity at whatever the
  default happened to be. Flatness is ours.
- **Clipping a primitive's coverage exactly to its band.** Costs 1/255 at
  every band boundary and makes the band height visible in the output. Use the
  guard band.
- **Writing paint mappings in device pixels.** They live in document space,
  like the geometry they follow. Getting this wrong makes a gradient frame a
  thousandth of a pixel across, which renders as a single flat colour and
  looks like the gradient maths failing. Cost: an hour.
- **Rendering each primitive into a band-sized scratch buffer.** A full-width
  clear per primitive is 123 KiB of memset per command; size the scratch to
  the primitive's clipped bounds instead.

---

## Open TODOs

| # | Item | Owner |
|---|---|---|
| 1 | Re-run the W0 spike on a real reference machine and settle G1 and G2 | Phase 5 gate |
| 2 | The WGSL compositing pass: paint evaluation, family dispatch, LUT sampling, ping-pong destination reads (R5.3, R5.4) | Phase 4 follow-up, on hardware |
| 3 | Recover CDraw's luminance weights by least squares (R4.4) and extract the twelve tables via `GDraw::CalcTransparencyX` (R4.5) | needs an x86-64 VM |
| 4 | Verify Contrast, Bevel, Saturation and Luminosity against those tables | after 3 |
| 5 | Render the `.xar` corpus end to end and compare against the original at 25 %, 100 % and 400 % | needs `xarast-cli`, Phase 11 |
| 6 | Re-derive the cache admission threshold from corpus data | after 5 |
| 7 | `DisplayList::build` costs 106 ns per command (2.12 ms for 20 000), so 100 000 nodes is ~10.6 ms against a 3 ms budget. The cause is the size of `DrawCmd`; boxing the stroke payload is the obvious next step | Phase 4 follow-up |
| 8 | Deferred `Draft → Final` upgrade with the 120 ms idle timer and cancellation (R6.8) — the quality levels exist and differ, the scheduler does not | Phase 5, which owns the idle timer |
| 9 | ~~Fuzz targets `fuzz_display_list` and `fuzz_ramp`~~ — done 2026-09-23, nightly in CI; see below | — |
| 10 | Dither styles, sub-32 bpp output, CMYK separation, UCR/GCR | deferred, no phase |
| 11 | The CPU backend strokes and dashes the **whole** path in document space before clipping to the band. A thick, round-capped, finely dashed stroke along a long path at deep zoom exhausts memory (two 1.8 GB allocations found by `fuzz_display_list`). Needs culling to the band plus the stroke's reach, with the dash phase preserved, or a work budget. `fuzz_display_list` bounds geometry (±10 000 000 mp) and dash counts (≤ 2 000 per path) until then; lift both when fixed. gintrack XARA-T-0022 | Phase 4 follow-up |

### Fuzzing, first runs (2026-09-23)

- `fuzz_ramp`: NaN stop offsets — possible, `Stop` and `TranspStop` have
  public fields — made the sort's comparator inconsistent, and the
  standard library's sort **panics** when it detects that. Both ramp
  builders now map a NaN offset to 0 (as `Stop::new` does) and sort by
  `total_cmp`. Never sort floats with `partial_cmp().unwrap_or(Equal)`.
  Clean rerun: 4.66 M execs at ~7 760 exec/s.
- `fuzz_display_list`: renders every accepted scene twice on the
  deterministic backend and compares bytes; asserts the display list is
  balanced and inside the viewport. Found only TODO 11.
