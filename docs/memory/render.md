# render

Memory note for the **render engine** (`crates/xarast-render`), Phase 4.

## Current state

`xarast-render` exists and renders. What is in it:

- **Scene → display list → bands → pixels.** `Scene` is retained and is built
  through `SceneBuilder`, which is the contract the `xarast-app` walker fills;
  the crate has no dependency on `xarast-doc` and never sees a node.
  `DisplayList` is immutable and per frame, with every transform resolved into
  device space, every paint *and transparency* mapping moved with its shape,
  and every command's device bounds precomputed. Since XARA-US-0016 a
  `DrawCmd` is 32 bytes of indices: the display list **shares the scene's
  op vector** (`Arc<Vec<SceneOp>>`) and keeps side tables for device
  transforms, device-space paints and transparencies, and image
  placements. `DisplayList::item` resolves a command to a `DrawItem` of
  references. A scene is re-recorded, never mutated in place, so an
  in-flight list keeps its own ops (`Scene::clear` swaps in a fresh vector
  when one is still shared).
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
  culling and `scroll_surface` reprojection. `scene_damage` diffs two scenes
  into the device rectangles an edit changed (XARA-T-0221, "Edit damage"),
  and any rectangle drawn over a frame is exactly that frame's pixels.
- **Validation:** a 120-scene generated feature corpus with committed goldens,
  exact CPU goldens, determinism over 20 runs, the gradient matrix, the blend
  domain, the precision rule (measured *and* grepped), AA level counts and a
  supersampled comparison, and `criterion` benches per budget row.

What is **not** in it, and who owns it:

| Missing | Owner |
|---|---|
| The WGSL compositing pass, ping-pong destination reads, GPU tile planner | **Deferred** by the GPU decision (XARA-US-0011); trigger in XARA-T-0051 |
| Presenting the canvas through `GpuTileCache` | built here (`gpu` feature); **wired** into the shell by XARA-T-0050 (`xarast-shell/src/tiles.rs`, see "The tiles in the viewer") |
| Blur, shadow, feather, bevel, contour, blend, mould | Phase 13; `push_layer`/`pop_layer` and the offscreen machinery they need exist here |
| Fractal (plasma, clouds) generation | Phase 13; `Paint::Fractal` exists and refuses to rasterise until materialised |
| Dither styles, sub-32 bpp output, CMYK separation, UCR/GCR | Deferred (`research/03 §3.9` M8) |
| Glyph rasterisation | Phase 9; this crate renders glyph outlines if handed paths |
| `.xar` corpus rendering end to end | `xarast-cli render` renders all 59 files (timings in `perf.md`); comparing against the original is Phase 11 |

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

*Update 2026-09-23:* there is a reference machine now
(`docs/memory/perf.md` §Reference machine), and G1 and G2 have been
re-measured and settled on it. See "Re-run on the reference machine" after
the decision. The quality gates G3–G7 below were not re-run: they do not
depend on the machine, apart from G4's SIMD-level caveat.

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

The throughput half of the harness is now in the repository as
`benches/spike.rs` (see the re-run section). The quality half is not: it drives `tiny-skia` as a
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

**M2 — GPU throughput. Unmeasured on the container** (zero adapters).
Measured on the reference machine in the re-run below.

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
| G1 CPU throughput ≤ 120 ms single, ≤ 25 ms on 8 cores | **fails — settled on the reference machine** | container: 442 / 194 ms on 4 slow cores. Reference machine: **159–164 / 53–58 ms** (see the re-run below) |
| G2 GPU throughput ≤ 8 ms | **fails on the integrated GPU — settled**; at the line on the discrete GPU | container: no adapter. Reference machine: Intel iGPU **72–86 ms**, RTX 4000 SFF Ada 7.8–8.0 ms |
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
catches. *(The first condition fired on 2026-09-23. It was reviewed and the
decision stands; see the next section.)*

### Re-run on the reference machine (2026-09-23): G1 and G2 settled

The machine is described in `docs/memory/perf.md` §Reference machine: Intel
Core Ultra 9 285 (8 P + 16 E cores), an Intel Arrow Lake iGPU (Mesa 26.1.6)
and an NVIDIA RTX 4000 SFF Ada (580.173.02), COSMIC Wayland, rustc 1.98.1.
Other agents were fuzzing and compiling throughout. Each figure is the median
of 30–50 frames, and a range spans 3–5 runs at load average 6–25.

**The harness now exists in the repository:** `crates/xarast-render/benches/spike.rs`
is the throughput half of the spike (G1, G2, candidate C). It talks to
`vello_cpu`, `vello` and `vello_hybrid` directly, uses the same `bulk`
generator in both object sizes, times the CPU from recording to the finished
pixmap and the GPU from submit to `poll(wait)`, and checks every GPU frame
against the `vello_cpu` one (mean absolute channel difference). Run it with:

```text
cargo bench -p xarast-render --bench spike                        # G1
cargo bench -p xarast-render --bench spike --features spike-gpu   # + G2, every adapter
SPIKE_THREADS=0,7 taskset -c 0-7 cargo bench -p xarast-render --bench spike   # 8 P-cores
```

**M1 / G1 — `vello_cpu` 0.2, u8 pipeline, detected SIMD (AVX2):**

| `bulk` variant | 1 thread | 8 threads (7 workers + 1) | 24 threads |
|---|---|---|---|
| r3..28 px, overdraw ≈ ×27 (the gate scene) | **159–164 ms** (record 104 + raster 56); 175 pinned to the P-cores | **53–58 ms**; 55.6 pinned to the P-cores | 49–51 ms |
| r2..8 px, overdraw ≈ ×2.6 | 83–88 ms (record 64 + raster 20) | 34–37 ms | 35–39 ms |
| r3..28 px, E-cores only (`taskset -c 8-23`) | 227 ms | 119 ms (15 workers; one noisy run, 68–246) | — |

Against the container this is 2.8× faster single-threaded and 3.5× faster in
parallel, and **G1 still fails on both halves.** Parallel scaling stops at
about 3×, and 8, 16 and 24 threads land within 10 % of each other. The serial
part is the recording thread, which feeds paths to the workers. Recording was
already the dominant cost on one thread (104 of 164 ms). More cores do not
close this gate. Only not re-recording does.

**M2 / G2 — `vello` 0.10, area AA, scene retained, submit → `poll(wait)`:**

| Adapter | r3..28 px | r2..8 px | Scene encoding, CPU, once per scene change | Δ vs `vello_cpu` |
|---|---|---|---|---|
| **Intel Arrow Lake iGPU** (the gate's "integrated GPU") | **72–86 ms** (min 48); 197 ms at load 34 | 32–48 ms | 17–25 ms | 0.019 / 0.68 |
| NVIDIA RTX 4000 SFF Ada | **7.8–8.0 ms**; 9.3 ms at load 34 | 5.8–6.0 ms | 17–25 ms | 0.019 / 0.67 |

The Δ column is the mean absolute channel difference against the
`vello_cpu` frame. It is 0.02 where the frame is covered, and it rises on the
light scene's many antialiased edges. `vello` writes straight alpha, so the
comparison is made against the unpremultiplied reference.

**Candidate C, `vello_hybrid` 0.2** (not a gate, recorded for the Phase 5
re-evaluation the decision rule asks for): the GPU half is quick, 5.4–6.0 ms
(light) and 17–18 ms (heavy) on the RTX, but every frame re-records strips on
the CPU at **63–110 ms**, the same cost as `vello_cpu`. It also misbehaved: one
run took 2.5 s per heavy frame on the RTX, and on the Intel iGPU the heavy
frame differs from `vello_cpu` by a mean of 1.77 (0.03 on the RTX).
**Rejected:** it removes nothing that matters.

**Verdicts.**

- **G1 fails, settled.** One thread is 1.3× over and eight P-cores are 2.2×
  over. The machine is no longer the excuse.
- **G2 fails on the integrated GPU, settled**, by 9–11× on the gate scene. On
  the discrete RTX 4000 it sits exactly at the 8 ms line. The integrated GPU
  is also the more fragile of the two: under CPU load its time more than
  doubles.

**What this changes, and what it does not.**

1. **The rasteriser choice stands.** The re-open trigger fired and was
   reviewed. No measured alternative is faster: `tiny-skia` is 3.2× slower,
   and `vello_hybrid` records strips at the same cost. The things `vello_cpu`
   was chosen for (256 levels, determinism, interposability) are unaffected.
2. **A 100k-object full frame is a cold-path cost, not an interactive one.**
   First paint, export and zoom-settle may pay 35–160 ms. A pan or zoom frame
   must not re-record the scene. It must composite cached pixels, per node or
   per tile (the per-node cache the phase already specifies), and re-raster
   only the dirty part: 64 × 64 costs 0.12 ms. This makes the Draft → Final
   scheduler (TODO 8) the thing the 16 ms pan/zoom budget depends on.
3. **The GPU backend does not rescue the integrated-GPU budget on its own.**
   At 72–86 ms for a full `vello` raster, the pan/zoom ≤ 16 ms budget on the
   iGPU also has to come from reusing pixels (transforming cached tiles or
   layers while the gesture runs), not from re-rasterising. On a discrete GPU
   a full re-raster fits in 8 ms.
4. **`vello` 0.10 and `vello_hybrid` 0.2 pin `wgpu` 29**, but the workspace
   and the shell are on `wgpu` 30. The workspace manifest's comment says
   "`vello` 0.10 on `wgpu` 30", and that is wrong. The spike builds against
   `vello::wgpu` behind the bench-only `spike-gpu` feature. Task R5.1 must
   pick one: pin the shell to `wgpu` 29, or wait for a `vello` release on 30.
   Two `wgpu` versions in one
   binary cannot share a device.
5. **Scene encoding for `vello` costs 17–25 ms per 100k objects.** A retained
   `vello::Scene` has to be rebuilt, or appended, when the document changes.
   That is another reason to keep the encoding per node and not per frame.

---

## The GPU decision (XARA-US-0011, 2026-09-23)

**Question.** With the CPU path inside its budgets (100k full frame 19 ms,
Draft pan zoomed 3× 11.1 ms through the scheduler, zoom 1.4 ms) and G1/G2
failed, is a GPU *rasteriser* worth building now, and if not, what should
the GPU do?

**Answer: no GPU rasteriser now. Rasterise on the CPU, keep the pixels on
the GPU as tiles, and let the GPU move them (option a).** The WGSL
paint/blend pass (b) is deferred behind a stated trigger (XARA-T-0051), and
`vello` (c) is rejected until it runs on our `wgpu` *and* fits the iGPU
budget.

### Evidence

Reference machine (`perf.md`), both GPUs selected explicitly through
Vulkan. GPU rows are wall-clock from the first `wgpu` call to `poll(wait)`,
medians of 50–300 frames; a range spans 4 runs at load average 5–25 (other
agents compiling). `cargo bench -p xarast-render --features gpu --bench
tiles`; `WGPU_ADAPTER_NAME=intel|nvidia` selects one adapter.

| | Intel Arrow Lake iGPU (Mesa 26.1.6) | RTX 4000 SFF Ada (580.173) |
|---|---|---|
| empty submit + wait (the floor) | 0.05–0.37 ms | 0.05–0.19 ms |
| **today:** upload a whole 1080p CPU frame | 0.54–1.6 ms | 0.61–0.89 ms |
| (a) pan, tiles resident, 40 × 256² placed | 0.47–1.4 ms | 0.08–0.21 ms |
| (a) pan crossing a tile row: upload 8 tiles + composite | 0.71–2.2 ms | 0.27–0.51 ms |
| (a) Draft zoom 1.3×, 28 tiles | 0.37–1.8 ms | 0.08–0.15 ms |
| (a) Draft zoom-out 0.5×, 144 tiles (today: backdrop border) | 0.71–2.0 ms | 0.15–0.47 ms |
| **today at 4K:** upload a whole 3840 × 2160 frame | **23–27 ms** | 2.6–3.5 ms |
| (a) 4K pan, 144 tiles resident | 1.5–3.0 ms | 0.30–0.54 ms |
| (b) floor: upload 64 MiB of A8 coverage | **27–33 ms** | 5.4–6.9 ms |
| (c) `vello` 0.10 full frame, 100k `bulk` (W0 re-run) | **72–86 ms** | 7.8–8.0 ms |

CPU side, same runs: `scroll_surface` 1080p 0.19–0.73 ms; `compose_cpu`
(the software tier of the same operation) 1.3–3.1 ms at 1080p.

| Criterion | (a) CPU raster + GPU tiles | (b) WGSL paint/blend over CPU coverage | (c) `vello` on `wgpu` 29 |
|---|---|---|---|
| What it speeds up | pan, Draft zoom, 4K presentation; pan latency (the shell can re-composite at input time) | gradient-heavy frames (30 ns/px on the CPU, 1.4 × 10⁸ px per corpus frame) | full re-rasters, on the RTX only |
| iGPU cost | ≤ 3 ms at 4K | ≥ 60 ms of mask upload for the frames it targets | 72–86 ms per frame |
| Parity | **byte-exact** with `compose_cpu`: 0 of 960 composites differ on the iGPU, the RTX and lavapipe | perceptual only (gate B), gradient drift risk K5 | Δ 0.019 mean vs `vello_cpu`, and no Xara paints or families: still needs (b) on top |
| Determinism / goldens | untouched: nothing is re-rasterised on the GPU | export stays CPU; two code paths per paint | as (b) |
| Capability ladder | any tier with a `wgpu` device (lavapipe exact); no device → `compose_cpu` or today's path | per-tier shader validation | tier 0 only (compute) |
| Dependency cost | none: `wgpu` 30 is already in the shell | none | two `wgpu`s: **+4.08 MiB** stripped (probe: 4.27 → 8.35 MB), **+46 crates** (127 → 173 for `xarast-render`), 9 more duplicated crates (`wgpu`, `wgpu-core`, `wgpu-hal`, `wgpu-types`, `naga`, `wgpu-naga-bridge`, `wgpu-core-deps-*`, `bit-set`, `foldhash`); two `wgpu`s cannot share a device. Downgrading the workspace instead means porting the shell back to 29; `vello` alone adds 0.68 MiB over `wgpu`. Licences clean either way |
| Effort | S (done here) + the shell wiring (XARA-T-0050) | L + L + M (R5.2–R5.4) | L, plus (b) |

Reading the table:

1. **The budgets that fail today are presentation budgets, not raster
   budgets.** At 1080p the CPU already meets every interactive row. What
   breaks is a 4K canvas on the integrated GPU: uploading one frame costs
   23–27 ms, over the 16 ms budget before anything is drawn, against 1.5–3
   ms to composite resident tiles. At 1080p on the iGPU the per-frame gain
   is small (composite ≈ upload ≈ 1 ms); the gains there are the CPU work
   removed (scroll, rescale), zoom-out showing content instead of a
   backdrop border, and a pan presented at input time.
2. **(b) is bandwidth-bound on the machine that decides.** Its targets are
   the gradient-heavy files, and feeding their coverage to the iGPU costs
   more than a frame. It becomes interesting only with coverage on the GPU,
   which is (c)'s problem, or after XARA-T-0038 has shown what row-wise SIMD
   leaves on the CPU. Trigger in XARA-T-0051.
3. **(c) loses on every axis that matters here**: slower than the CPU on
   the iGPU, a second `wgpu`, and it would still need (b) for Xara's paints
   and families. Re-open when a `vello` release targets the workspace's
   `wgpu` **and** a full 100k frame fits 16 ms on the iGPU.

### What was built (XARA-T-0040)

- `compose` (always compiled): `TileKey`, `TileGrid` (`covering`,
  `rect`, `tile_view`, `placement`), `TilePlacement` with a `TexelRect`
  valid area, `source_texel` (the one texel rule) and `compose_cpu`.
- `backend::gpu_tiles` (`gpu` feature): `GpuTileCache` — one `Rgba8Unorm`
  2D-array atlas (default 128 × 256², 32 MiB), LRU, `upload(key, surface,
  rect, at)` from any sub-rectangle of a CPU `Surface` to a texel offset,
  `encode` (one instanced draw on the caller's encoder, replace, clear to a
  backdrop) and `compose`; plus `create_target` and `read_back`.
- `tests/parity_tiles.rs`: the 120-scene corpus cut into 32² tiles,
  composited eight ways (identity, whole and fractional pan, zoom ×1.5,
  ×3.7 and ×0.6, a missing tile, a partial tile at an offset beside a tile
  uploaded in halves) on every adapter: **byte-identical** on the Intel
  iGPU, the RTX 4000 Ada and lavapipe (960 composites each). The NVIDIA
  GL adapter fails device creation ("Parent device is lost") and is
  skipped.
- `benches/tiles.rs`: the table above.

**A tile-assembled frame is not a whole frame.** Rasterising the corpus
tile by tile and assembling it gives 105 of 120 cases byte-identical to
the whole-frame render; 12 mesh gradients and `aa_seam` move 1–17 pixels
by 1/255, and two perspective repeating linear gradients move one pixel
on the wrap edge by 230/255 (a sub-ulp difference picks the other side of
the discontinuity). So tiles are for **moving** pixels. A `Final` frame at
rest is still rasterised whole and cut into tiles on upload, which the
texel offsets exist for; the test pins the drift at ≤ 0.2 % of pixels.

### The tiles in the viewer (XARA-T-0050)

The shell (`xarast-shell/src/tiles.rs`) owns the cache. What it needed
from here was one addition, `GpuTileCache::valid(key)`, so that it can
upload only the texels a tile lacks. Facts about the integration that
matter to this crate:

- A **level** is one zoom *and* one pixel grid: the first frame at a zoom
  defines it, and a later frame joins it only if its translation differs
  by whole pixels (1e-3 tolerance, as `reuse.rs`). A Final after a
  fractional pan starts a level. Up to three levels are kept and drawn
  oldest first, so a zoom-out shows older resident content under the
  newest level.
- A tile's valid area is a bounding box, so a piece that would not join
  it into a rectangle restarts the tile (`invalidate` + upload). This
  happens at the trailing edge of a pan, and is most of the per-step
  upload beyond the strips.
- `compose_cpu` is the CPU tier of the same planner, over `256²` tiles in
  memory; a unit test in the shell holds the two tiers byte-identical on
  every adapter it finds, and real-window screenshots of `ProbeX16.xar`
  on the Intel iGPU and the RTX, each tier, have identical canvases.
- An edit keeps the tiles (XARA-T-0221): a frame of a new scene epoch
  whose `base` is the last frame uploaded keeps its level's tiles under
  the view and uploads only its `fresh` damage; tiles holding texels
  outside the view restart, every other tile goes (`TileStore::retain`,
  `GpuTileCache::retain`).
- The composite target is the painter's canvas texture (`Rgba8Unorm`,
  now also a render attachment); the swapchain may be `Bgra8Unorm`, which
  is why the tiles do not draw into it directly.

**Per-tile rasterisation is the wrong grain for large scenes.** A culled
`DisplayList::build` is linear in the scene's ops: 6.5–10 ms for a
900 000-op scene, so a single 256² tile costs 5–10 ms and a 1920 × 256
row 12–18 ms. Rasterise one dirty rectangle per frame (as the render
thread already does) and upload its pieces; do not loop over tiles.

### Edit damage (XARA-T-0221, 2026-09-24)

`damage::scene_damage(old, new, view, max_rects)` returns the device
rectangles whose pixels may differ between a frame of `old` and a frame of
`new`. The render thread calls it with the scene of the frame on screen and
repaints only those rectangles (`app-core.md` decision 40); the shell keeps
its tiles across the repaint.

- **The model.** A scene is a tree: pushes (group, clip, transparency scope,
  layer) are inner nodes, fills, strokes and images are leaves. Children of
  two inner nodes with equal push ops are aligned: common prefix and suffix
  pairwise, then the middle by a cheap key (op kind, node id, path bounds
  bits) plus full op equality, cut to the longest order-preserving run
  (patience/LIS). Matched subtrees recurse; every leaf under an unmatched
  child, on either side, is damage, with its display-list device bounds
  (`device_bounds_of`, `mapping_bounds`, `stroke_pad`: the same functions
  the build uses).
- **Why it is sufficient.** A pixel outside the damage is covered only by
  matched leaves, in the same order, under equal pushes, so it is
  composited from the same inputs. That rests on two renderer properties,
  both pinned: a leaf touches nothing outside its display-list bounds, and
  a clip, group or layer with no leaf on a pixel leaves it alone.
- **Resources are compared by content.** Equal ops can name different
  ramps: an evicted ramp slot is reused by the next intern. A matched op's
  ramp ids (colour table and transparency table) and image ids are checked
  against both resolvers, memoised per id.
- **It is command-agnostic by design.** It covers a group attribute that
  recolours its siblings, a named colour redefinition (every user of the
  colour and nothing else, tested), z-order, undo, redo and previews,
  with no per-command extent to get wrong. `None` only when the scenes'
  qualities differ or one is unbalanced.
- **`damage::coalesce`** merges to at most `max_rects`, cheapest union
  first, and merges below the limit whenever a union costs no area; above
  256 raw rectangles it first chunks them in reading order.
- **Tests.** Unit tests (recolour, move, z-swap, group transform, reused
  ramp slot, LIS, coalescing); `properties::repainting_the_damage_gives_
  the_new_frame` (512 random step sequences × 1–3 random edits including
  layers, clips and blend families, byte-exact); the corpus test in
  `xarast-shell/tests/edit_damage.rs`. Mutation-checked: dropping the leaves
  of an unmatched subtree, or matching on the key alone, fails the property.

**A rectangle drawn over a frame is exactly that frame (invariant 15).**
`vello_cpu` rasterises in `f32` relative to the top left of the pixmap it
is given. Coverage used to start at the draw area's corner (a column, a
column tile, a pan strip, a dirty rectangle), which moved `aa_edge_45` by
1/255 in up to 53 pixels and SimpleSphere's gradients in a few; the old
tests allowed "within 1/255" for scrolls and columns. Coverage now starts at
`max(bounds.x0, 0)` and the band's top (clips at the surface's left edge).
The right edge does not matter (coverage accumulates from the left).
Aligning the origin to 4 px, vello's tile width, was **not** enough:
`aa_edge_45` still moved. One-call renders are unchanged, so every golden
is; `determinism::coverage_does_not_depend_on_the_draw_area` pins five
rectangles × 120 cases × both configurations, and the scroll and column
tests in `render_thread.rs` are now exact. Cost in `perf.md`.

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

**Export has its own entry and its own band geometry (2026-09-23,
XARA-US-0057).** `export::render_export_strips` maps a document rectangle
exactly onto a `w × h` grid and renders it in strips through
`CpuBackend::render_rows`, which draws device rows `y0..y0+h` of a display
list on the **absolute** band grid with a caller-given band height. The
entry constructs `CpuConfig::deterministic()` itself and takes no backend
parameter: the GPU cannot reach an export. Band height is
`export_band_lines(w, h)` — about 64 bands, 16 rows minimum, 1 MiB maximum —
and a strip is a whole number of bands starting on the grid, so the thread
count and the strip budget never change a byte
(`tests/export.rs`: runs, strip budgets, 1 vs 8 threads, over every
synthetic case). This is deliberately *not* `band_budget_bytes`: the
on-screen deterministic configuration keeps its 1 MiB bands so the goldens
do not move, while export gets enough bands to use the machine
(XARA-T-0038's parallelism half: the gradient files export in 0.36–0.9 s
against 2.0–4.5 s through `render`). See `export.md`.


**Display-list commands index into shared scene ops (2026-09-23,
XARA-US-0016).** Copying path, paint, style and transparency into every
command made the 100k list 38 MB. That was past glibc's mmap threshold, so
each build page-faulted it fresh and each drop walked 200k cold `Arc`
counters: 20.8 ms, superlinear. With indices it is 1.9 ms. Boxing only the
stroke payload, the first idea on the TODO list, would have left the
command over 300 bytes. Consequences: `DrawCmd` is `Copy` and opaque, and
backends go through `DisplayList::item`. `DisplayList::from_commands` is
gone; the immediate-mode facade records into `ListParts` (its own op
vector plus commands).

**Culling entries beside the ops (XARA-T-0033).** `Scene::cull` holds 40
bytes per op: a primitive's padded document bounds. A *culled* build (a
dirty rectangle smaller than the viewport) reads only those for rejected
primitives. Under an axis-aligned transform it first rejects in document
space against the dirty rectangle mapped back and grown by 4 px. A full
build ignores them, because reading both arrays made it slower (3.4 ms
against 1.9). A test compares culled builds with the filtered full build,
command for command.

**Compositing over a destination that is not opaque (2026-09-23,
XARA-T-0231).** The twelve families are defined against an opaque
destination. `blend::composite` now treats a destination of alpha `a_d`
as that opaque colour over `a_d` of the pixel and as *nothing* over the
rest: the family's result on the first part, the source alone at
`α_s = coverage·(1 − t)` on the second, alpha `a_d + (1 − a_d)·α_s`, colour
the premultiplied sum divided by it. For Mix that is exactly source-over.
Every non-Mix family behaves as Mix over nothing (it has nothing to read),
which is the decision the task asked for; the PDF exporter still renders
those families' backdrops over the paper, as the editor shows them. An
opaque destination takes the old code path byte for byte, which is why
119 of the 120 goldens did not move; `layer_nested` did (1 904 px, max
16/255): its inner Mix layer lands on the outer isolated layer's
transparent area, which used to darken it. It was regenerated with
`XARAST_UPDATE_GOLDEN=1 cargo test -p xarast-render --test golden_cpu`.
Surfaces stay premultiplied; `unpremultiply_rgba_in_place` and
`premultiply_rgba` (in `surface`) convert at the image-file boundary (see
`export.md`). A `LayerKind::Plain` pop still composites with coverage 255,
ignoring the layer's own alpha; nothing but the synthetic corpus pushes
one, so it is left alone (its golden did not move).

**Stroke ends and dash phase follow `stroke_to_path` (XARA-T-0231).** The
CPU stroker sets `with_start_cap`/`with_end_cap` separately, and starts
the pattern at `xarast_geom::reduced_dash_offset(offset, resolved)` on
both routes: the whole-path dasher, and `cull_stroke_input`, which now
takes the phase and adds it to each kept run's arc length. Adding a
non-zero phase exposed a latent seam bug in the culled route: when the
run that ends on a closed subpath's start vertex itself *starts* inside a
dash, the dasher holds that first dash back and emits it last, so the
dash ending on the vertex is not the tail's last one and the seam was not
joined (a missing mitre corner). The tail's dash ending on the vertex is
now found and moved last before the join.
`tests/stroke_ends_and_alpha.rs` checks both caps, the offset (shift,
whole-period invariance, parity with a filled `stroke_to_path` outline)
and the offset under culling.

**Strokes are cut to the band before dashing (XARA-T-0022).**
`stroke_cull` clips the centre line, in device space, to the band plus the
stroke's reach, but only when the stroke reaches more than 256 px past the
band. Anything smaller is expanded exactly as before, which is why no
golden moved. Each kept run is dashed from its own arc length, and a closed
subpath cut open is rejoined at its start vertex. The dasher emits the first
dash last, and that dash is merged into the run that ends on the vertex. On
top of that, a **work budget** of 10⁶ flattened segments: the dash count
times the cost of each dash (`4 + 2·√(half-width px / tolerance px)`). Past
it the pattern is dropped and the stroke drawn solid. Culling alone cannot
bound a stroke wide enough to reach the band from anywhere: the fuzzer's
second finding was a 5.6 × 10⁵ mp stroke with 43 mp dashes under a
degenerate view.

**`CpuConfig::deterministic` uses every core.** Bands merge by index and the
determinism test proves thread count cannot change a byte. The band height
and the SIMD level are what must stay pinned, and they do.

**Interactive bands are 512 KiB (16 per 1080p frame) and one-band areas are
cut into column tiles.** The column tiles apply to the interactive
configuration only: at most 32 tiles, each at least 64 px wide, a count that
depends on the area and not on the thread count. A tile edge clips coverage
horizontally, as a dirty-rect edge always did. A determinism test pins tiled
against serial byte for byte. The deterministic configuration never tiles
and keeps 1 MiB bands, so the goldens do not depend on either change.

**Hot-path arithmetic avoids libm and repeats nothing per pixel.** The
baseline x86-64 target has no SSE4.1, so `f64::floor/ceil/round` and `%`
are library calls. `DeviceRect::enclosing`, `apply_repeat`'s `rem_euclid`
and the ramp index use exact integer-cast equivalents, each pinned by a
test against the standard library over edge cases and random bits.
`PaintSampler` and `LevelSampler` invert a gradient's mapping once per
primitive; `eval_paint` is a one-point `PaintSampler`.

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

**Bitmap fill orientation (XARA-T-0171, 2026-09-23).** `Paint::Image` and
`TranspSource::Image` sample `v = 0` at the image's **top** row: decoded
images are top-down, and a placed bitmap's walker frame starts at its
top-left corner. A bitmap *fill's* control points start at the image's
**bottom-left**: start = bottom-left, end = bottom-right, second end =
top-left (and, with perspective, `p2` = top-left, `p3` = top-right). The
original passes start, end, second end as the tile plotter's first three
corners (`wxOil/grndrgn.cpp:3501-3521`), and the plotter maps its first
corner to the first stored row of a bottom-up DIB — the plain bitmap plot
passes a rectangle's low corner first (`wxOil/grndrgn.cpp:4862-4865`);
converting a bitmap object into a fill makes its bottom-left corner the
start point and its top-left the second end (`Kernel/nodebmp.cpp:1210-1212`).
The `.xarast` writer's `<pattern>` agrees (its tile starts at `axis_y`).
The translation lives in `xarast-app::paint::bitmap_frame`, which starts
the renderer's frame at the top edge; the renderer keeps its one
convention. This is not a Y flip between document and device — that stays
the `Viewport`'s — but the image's own row order. Mirror tiling is
symmetric about tile edges, so shifting the frame origin by one tile keeps
the mirrored tiles where the original puts them. Measured against resvg on
the `.xarast` SVG (8 × 8 grey SSIM, before → after): Fill Types simple
0.844 → 0.880, scope3 simple 0.926 → 0.959, JagSS100 simple 0.969 → 0.995,
WATCH 0.980 → 0.991, Spitfire 0.850 → 0.958, TestBitmapFill 0.985 → 0.999.
No golden encoded the bug (none has a bitmap fill); pinned by
`crates/xarast-app/tests/bitmap_orientation.rs`.

**A two-row guard band on every band's coverage.** Without it the band height
changed the picture by 1/255 at band boundaries, because the rasteriser's
strips start at the viewport edge. Antialiasing influence is local to one
pixel, so two rows make a banded render identical to an unbanded one; the
determinism suite asserts it over four band heights.

**Ramp construction contract (phase 8, XARA-US-0040).** `build_ramp` /
`build_transparency_ramp` in `xarast-render/src/ramp.rs` are the only table
builders, used by both backends through `RampCache`; the bias/gain curve
itself is `xarast_geom::BiasGain::map`, the one implementation (the
document's `Ramp::sample` calls it too). The identity short-circuit is in
`build_ramp`; `the_identity_profile_is_exactly_linear_over_2048_entries`
pins it exact. Lengths: 256 entries at `Draft`, 2048 at `Final` — and a
node a gesture previews (`Preview::attrs`) always builds 256, because a new
ramp is made every drag frame; the frame after the release walks it at the
session's quality again. Pixel comparisons of a preview against its commit
are exact only at `Draft`.

**`RampCache` evicts least recently used.** The owner calls `begin_frame`
before a scene build and `evict(budget)` after (the walker, through
`Resolver::begin_frame` / `trim_ramps`, budget `RAMP_CACHE_BUDGET` = 16 MiB).
Only tables not used in the current frame are dropped, so every `RampId` of
the scene just built stays valid; freed slots are reused, and `trim_ramps`
empties the `transparency_ramps` entry of each evicted id (that side table
is indexed by ramp id and rebuilt when its length is wrong). The render
thread's resolver is a snapshot clone, untouched by eviction. A cache that
never calls `begin_frame` (export, corpus tools) never evicts.

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
9. **Every position-dependent paint input moves to device space in
   `DisplayList::build`: paints, image placements and transparency
   mappings.** The backends evaluate at device pixel centres. A mapping left
   in document units looks like the gradient maths failing: the whole ramp
   sits thousands of pixels away. Transparency was missed until 2026-09-23,
   and the four `transparency_graduated_*` goldens had been blessed from
   that bug.
10. **`Scene::ops` and `Scene::cull` have the same length.** Every op goes
    through `SceneBuilder::push`.
11. **A fast path must be pinned to the slow one by a test**, bit for bit:
    the opaque-replace write, the fast floor/ceil, `rem_euclid_pow2`,
    `ramp_index`, `FrameMap::param`, and culled against full builds.
12. **The tile texel rule lives in two places and they must stay one
    expression**: `compose::source_texel` and `fs_main` in
    `backend/gpu_tiles.rs` compute `floor((p + 0.5 - origin) * inv_scale)`
    in `f32`, subtraction then multiplication, and the shader reads with
    `textureLoad`, never a sampler. That is what makes the GPU composite
    byte-identical to `compose_cpu`; `tests/parity_tiles.rs` fails on any
    device where it is not. Do not "simplify" it to `p * inv - origin *
    inv` (an FMA candidate) or to filtered sampling.
13. **Composition replaces and never blends.** A tile is an opaque piece of
    the canvas, background included. Anything that needs blending belongs
    in the rasteriser, where the families and the goldens are.
14. **An image paint's `v = 0` is the image's top row**; a bitmap fill's
    start point is its bottom row. Only `paint::bitmap_frame` in
    `xarast-app` translates between the two (see "Bitmap fill
    orientation").
15. **Coverage never depends on the draw area.** A primitive is rasterised
    from `max(bounds.x0, 0)` and its band's top minus the guard, a clip
    from the surface's left edge; the draw area (column, tile, strip,
    dirty rectangle) only limits what is composited. Otherwise an edit's
    repaint is not the frame a full render gives, and the damage tests
    fail by 1/255 (XARA-T-0221, "Edit damage").
16. **`scene_damage` compares ops by equality, never by `ContentHash`.**
    The walker's hash is per node version and scope and does not see
    viewport-dependent output or a reused ramp slot; op equality plus the
    resource check does.

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
- **Boxing part of `DrawCmd` to shrink it.** A per-command allocation, and
  the paint and transparency alone are 208 bytes. Index into shared ops.
- **Reading the compact culling entries on full builds too.** Two streams
  instead of one: 3.4 ms against 1.9 ms at 100k.
- **Inverting the view to cull strokes in document space.** A degenerate
  view (zero x scale) has no inverse, so nothing was culled and the fuzzer
  ran out of memory. Test the centre line in device space.
- **Shrinking the deterministic band to parallelise export.** Band height
  is not quite invisible. The 120 golden cases are band-invariant, but the
  100k `bulk` scene differs in 8 of 8.3 M bytes (by at most 2) between 1 MiB
  and 256 KiB bands. Tile the interactive path instead, or re-bless
  deliberately (XARA-T-0038).
- **Hunting libm and divisions in the gradient samplers.** Replacing
  `fmod`/`round` and skipping the `u` division moved the gradient files by
  under 5 %. The time is the scalar per-pixel pipeline as a whole (≈ 30 ns),
  not one call.
- **A GPU rasteriser to meet the interactive budgets (XARA-US-0011).** The
  CPU meets them at 1080p; what fails is presentation (a 4K upload on the
  iGPU). `vello` is 72–86 ms on the iGPU and needs a second `wgpu`; a WGSL
  pass over CPU coverage pays 27–33 ms per 64 MiB of masks on the iGPU.
  Re-open only on the triggers in "The GPU decision".
- **Checking bitmap orientation with the `.xarast` round trip.** Both sides
  go through our renderer, so an upside-down bitmap fill round-trips
  pixel-identical (XARA-T-0171 survived it). Compare against resvg on the
  exported SVG, or pin it with a synthetic asymmetric bitmap.
- **Snapping the coverage origin to vello's 4 px tile grid** to keep a
  dirty rectangle's pixels exact. Translating by a multiple of 4 still
  changes the `f32` geometry: `aa_edge_45` moved in 35 pixels. Start at the
  primitive's left edge (invariant 15).
- **Using the walker's `ContentHash` for edit damage.** See invariant 16.
- **Rasterising tile by tile.** Each call pays a culled display-list build
  linear in the scene (6.5–10 ms at 900k ops), and the assembled frame is
  not byte-identical to a whole one. Rasterise one dirty rectangle and
  upload its pieces.

---

## Open TODOs

| # | Item | Owner |
|---|---|---|
| 1 | ~~Re-run the W0 spike on a real reference machine and settle G1 and G2~~. **Done 2026-09-23**: both fail, settled. See "Re-run on the reference machine" above | done (XARA-US-0010) |
| 2 | The WGSL compositing pass: paint evaluation, family dispatch, LUT sampling, ping-pong destination reads (R5.3, R5.4). **Deferred** by the GPU decision; re-open on its trigger | XARA-T-0051 |
| 15 | ~~Present the canvas through `GpuTileCache` (shell/app wiring, capability ladder)~~. **Done 2026-09-23**; see "The tiles in the viewer" | done (XARA-T-0050) |
| 16 | The iGPU `write_texture` cliff: 1080p 0.6 ms, 4K 24 ms | XARA-T-0052 |
| 3 | Recover CDraw's luminance weights by least squares (R4.4) and extract the twelve tables via `GDraw::CalcTransparencyX` (R4.5) | needs an x86-64 VM |
| 4 | Verify Contrast, Bevel, Saturation and Luminosity against those tables | after 3 |
| 5 | Render the `.xar` corpus end to end and compare against the original at 25 %, 100 % and 400 % | our side done (`xarast-cli render --zoom`); the original cannot run here (closed CDraw, 2006 binaries), so XARA-T-0248 compares with the previews each `.xar` embeds and keeps the zoom comparison VM-gated |
| 6 | Re-derive the cache admission threshold from corpus data | after 5 |
| 7 | ~~`DisplayList::build` at 100k: 20.8 ms against 3 ms~~. **Done 2026-09-23**: 1.9 ms; the cause and the fix are under "Decisions taken". The 100k CPU full frame is 19 ms against 25 | done (XARA-US-0016) |
| 8 | Deferred `Draft → Final` upgrade with the 120 ms idle timer and cancellation (R6.8) — the quality levels exist and differ, the scheduler does not. After the G1/G2 re-run this is **what the 16 ms pan/zoom budget depends on**: a 100k full frame costs 35–160 ms on the CPU and 72–86 ms on the iGPU | Phase 5, which owns the idle timer |
| 9 | ~~Fuzz targets `fuzz_display_list` and `fuzz_ramp`~~ — done 2026-09-23, nightly in CI; see below | — |
| 10 | Dither styles, sub-32 bpp output, CMYK separation, UCR/GCR | deferred, no phase |
| 11 | ~~Strokes dashed whole before clipping~~. **Done 2026-09-23** (`stroke_cull`, XARA-T-0022); `fuzz_display_list` now spans the whole extent with unlimited dash patterns | done |
| 13 | Gradient-heavy export: ≈ 30 ns per composited pixel, and only three 1 MiB bands on a 766 px image. The three `*GradFilledShapes*` files take 2.2–4.5 s | XARA-T-0038 |
| 14 | `SimpleSphere.xar` is still black. The renderer is right; the walker fills an unfilled 12 pt frame opaque black over the whole drawing. The gradient repeat default is also suspect | XARA-T-0037 (app/doc) |
| 17 | ~~Dirty region for a fill edit (T8.5.4)~~. **Done 2026-09-24**, for every edit: the render thread diffs scenes (`scene_damage`) and repaints only the damage; see "Edit damage" | done (XARA-T-0221) |
| 18 | Golden images: every fill shape × every exposed blend mode × {flat, graduated} (T8.5.5) | XARA-T-0222 |
| 12 | ~~Reconcile `wgpu` versions~~. **Decided 2026-09-23**: no `vello` in the product until it targets the workspace's `wgpu` (two `wgpu`s cost +4.08 MiB and 46 crates, and cannot share a device); the spike keeps building against `vello::wgpu` behind `spike-gpu` | done (XARA-US-0011) |

### Vector export (PDF, XARA-US-0059, 2026-09-23)

- **Export is CPU-only, including vector export's fallback.** PDF's
  fidelity ladder rasterises through `export::ListRasteriser`, which names
  `CpuBackend` with `CpuConfig::deterministic()`; `DisplayList::with_commands`
  reuses a list's side tables with a chosen command stream and
  `DisplayList::op_of` identifies an object across lists built from one
  scene for different views. Guarded by
  `tests/export.rs::a_list_rasterised_through_its_own_commands_matches_the_export`.
- **The PDF fidelity matrix** (native / workaround / rasterise, per
  feature) and the T11.4.1 crate spike are in `export.md`, section "PDF".
  **Per-family blend decision: not measured yet** (T11.4.6, XARA-T-0227);
  until it is, every family except Mix is rasterised with its backdrop by
  default, and `PreferNative` maps only Stained Glass → Multiply and
  Bleach → Screen. Darken, Lighten, Hue, Saturation and Luminosity are
  deliberately *not* mapped to PDF's same-named modes.
- **Fixed (XARA-T-0231, 2026-09-23):** partly transparent Mix over a
  transparent destination darkened (≈ `c·a²`), and the CPU stroker
  ignored `StrokeStyle::cap_end` and `DashPattern::offset`. See the
  decision "Compositing over a destination that is not opaque" above.

### Fuzzing, first runs (2026-09-23)

- `fuzz_ramp`: NaN stop offsets — possible, `Stop` and `TranspStop` have
  public fields — made the sort's comparator inconsistent, and the
  standard library's sort **panics** when it detects that. Both ramp
  builders now map a NaN offset to 0 (as `Stop::new` does) and sort by
  `total_cmp`. Never sort floats with `partial_cmp().unwrap_or(Equal)`.
  Clean rerun: 4.66 M execs at ~7 760 exec/s.
- `fuzz_display_list`: renders every accepted scene twice on the
  deterministic backend and compares bytes; asserts the display list is
  balanced and inside the viewport. Found only TODO 11. Clean rerun with
  the bounds in place: 565 k execs at ~940 exec/s.
- `fuzz_display_list` with the bounds lifted (2026-09-23), three runs:
  1. An OOM after 121 k execs: the thick-stroke work-budget case under a
     degenerate view.
  2. A panic: `i64` overflow on `x0 + 1` in bilinear image sampling at a
     saturated coordinate. Now `saturating_add`.
  3. Clean: **317 k execs in 600 s** (~530 exec/s), peak RSS 813 MB.
