# perf

Memory note for **performance**: budgets, the measurements behind them, and
the regressions to watch for.

## Current state

Phase 0 set the budgets in the phase documents. Phase 2 is the first phase to
measure against them at scale, with `criterion` over a synthetic 100 000-node
document (`xarast_doc::synth`).

**The budgets are pinned to the reference machine below** (story
XARA-US-0010, 2026-09-23). Its numbers are the ones that count, and they are
in the next two sections. The Phase 2, 4 and 5 sections further down were
measured on the old 4-vCPU development container with no GPU. They are kept
as history and explain the older decisions, but they are not gate verdicts.
Wiring the budgets into CI, so that a regression fails the build, is still
owed.

## Reference machine

Every budget in the phase documents and in `docs/phases/00-roadmap.md` is
judged on this machine. If it changes, re-run everything in
[Reference-machine numbers](#reference-machine-numbers) before comparing.

| | |
|---|---|
| CPU | **Intel Core Ultra 9 285** (Arrow Lake). **8 P-cores** (CPUs 0–7, up to 5.6 GHz) + **16 E-cores** (CPUs 8–23, up to 4.7 GHz), 24 threads, no SMT. L2 40 MiB, L3 36 MiB. AVX2 + AVX-VNNI, **no AVX-512**. `intel_pstate`, `powersave` governor |
| RAM | 93.6 GiB |
| GPU, integrated | **Intel Graphics (Arrow Lake)**, Mesa 26.1.6 ANV, Vulkan 1.4.354 |
| GPU, discrete | **NVIDIA RTX 4000 SFF Ada Generation** (AD104GL, 20 GiB), driver 580.173.02, Vulkan 1.4.312, PCIe 16 GT/s |
| OS | Pop!_OS 24.04 LTS, kernel 7.1.5-76070105-generic |
| Compositor | COSMIC (`cosmic-comp` 0.1), Wayland (`wayland-1`) |
| Toolchain | rustc / cargo 1.98.1; `release` = `lto = "thin"`, `codegen-units = 1` |
| `wgpu` adapters | Intel iGPU and NVIDIA through Vulkan; `llvmpipe` also enumerates and is skipped |

**Two GPUs.** The phase gates and the pan/zoom budget are written against
an *integrated* GPU. That is the Intel adapter, and it is the one that
decides a GPU verdict. The NVIDIA numbers are recorded so that we know what
the software can do when the hardware is not the bottleneck.

**Hybrid cores.** "8 cores" in a gate means the 8 P-cores
(`taskset -c 0-7`). The E-cores are about 1.3× slower single-threaded on
this workload.

**Load during the 2026-09-23 round.** Other agents were fuzzing (3–4 cores,
often P-cores) and compiling throughout, with the load average between 5 and
34. Every figure below is the median of several runs, and a range means the
spread across runs. Runs taken at load ≥ 25 are discarded unless noted. The
integrated GPU is the most sensitive to this, because it shares power and
memory bandwidth with the busy cores.

## Reference-machine numbers

Measured 2026-09-23 (XARA-US-0010). Budgets that are breached are in bold.

### Budget table

| Budget | Target | Measured | Verdict |
|---|---|---|---|
| Undo of a single-node edit | ≤ 1 ms | 0.28 µs | passes |
| Redo of a single-node edit | ≤ 1 ms | 0.10 µs | passes |
| Dispatch (commit) of that edit | — | ~~1.04 ms~~ **101 ns** | fixed (XARA-T-0030), see below |
| `walk_render`, 100 000 nodes | ≤ 2.0 ms | 0.87–0.92 ms | passes |
| `preorder`, 100 000 nodes | ≤ 1.5 ms | 1.07–1.08 ms | passes (2.09 ms on the container) |
| Random `Tree::get` | ≤ 5 ns | 5.10–5.67 ns | at budget; the figure includes the bench's own index arithmetic |
| `AttrStack::push` + `pop_scope` | ≤ 20 ns | 12.1–12.9 ns | passes |
| `AttrResolver::resolve`, warm / cold | ≤ 30 ns / ≤ 2 µs | 18.9 ns / 0.87–0.92 µs | passes |
| `DocumentBuilder`, 100 000 nodes | ≤ 150 ms | ~~40–46 ms~~ 17.9 ms | passes (candidates recorded at `node()`, O(n) validate) |
| `Document::canonical_digest()` | ≤ 30 ms | 4.1–5.7 ms | passes |
| `Document::snapshot()` | ≤ 25 ms | ~~43–80 ms~~ **9.7 ms** | passes (dense map, not a HAMT) |
| `compute_bounds`, 100 000 nodes, cold | ≤ 12 ms | 7.6–8.1 ms | passes |
| W0 G1: `vello_cpu`, `bulk` 1080p, 1 thread | ≤ 120 ms | **159–164 ms** | **fails** |
| W0 G1: `vello_cpu`, `bulk` 1080p, 8 P-cores | ≤ 25 ms | **53–58 ms** | **fails** |
| W0 G2: `vello`, `bulk` 1080p, integrated GPU | ≤ 8 ms | **72–86 ms** | **fails, 9–11×** |
| W0 G2: same, discrete GPU (information only) | ≤ 8 ms | 7.8–8.0 ms | at the line |
| CPU full frame, production path, 1080p, 100 000 objects | ≤ 25 ms | 19.0 ms (was 44.7) | passes since 2026-09-23 (XARA-US-0016) |
| Incremental 64 × 64 dirty rect | ≤ 0.3 ms | 0.11 ms | passes |
| `DisplayList::build`, 100 000 commands | ≤ 3 ms | 1.9–2.0 ms (was 20.8) | passes since 2026-09-23 (XARA-US-0016) |
| `DisplayList::build`, 1920 × 40 pan strip, 100 000 ops | — | 0.26 ms (was ≈ 15–19 ms in the app bench) | XARA-T-0033 |
| Render the 1920 × 40 pan strip, 100 000 ops, interactive | — | 1.17 ms (was 9.4) | XARA-T-0034 |
| Ramp build, 2048 entries, 8 stops, with profile | ≤ 40 µs | 34.5 µs | passes (86.6 µs on the container) |
| Blend LUT set, 12 families | ≤ 15 ms | 0.28 ms | passes |
| `.xar` full import, `ProbeX16.xar` (7.4 MB) | ≤ 350 ms | ~~644 ms~~ **329–343 ms** | passes, thin margin (XARA-T-0031) |
| `.xar` full import, whole corpus (59 files, 12.3 MB) | ≤ 3 s | ~~0.87–0.89 s~~ 0.46–0.47 s | passes |
| Pan, 100k objects, CPU, Draft through the scheduler, fit page | ≤ 16 ms | ~~5.3–6.6 ms~~ 1.58 ms | passes (after the render-perf merge, 2026-09-23, load ~5) |
| Pan, 100k objects, CPU, Draft through the scheduler, zoomed 3× | ≤ 16 ms | ~~51–57 ms~~ **11.1 ms** | passes after XARA-T-0033/T-0034 (linear display list, document-space culling, column tiles); re-measured 2026-09-23 at load ~5 |
| Zoom (wheel notch), 100k objects, CPU, Draft through the scheduler | ≤ 16 ms | 1.4 ms | passes; the zoom-out border is left to the Final |
| Pan/zoom, 100k objects, integrated GPU, input → presented, live window | ≤ 16 ms | pan p99 **1.3 ms** (GPU idle p99 5.5 ms), zoom p99 **0.5 ms** (GPU idle 2.8 ms); canvas 1796 × 1338 | **passes** (XARA-T-0050/T-0008, 2026-09-23); table in "Pan and zoom end to end" |
| Pan/zoom, 100k objects, integrated GPU, 4K canvas, offscreen | ≤ 16 ms | pan p99 **9.6 ms**, zoom p99 **5.8 ms** (to GPU idle) | passes; the CPU tier at 4K is 35–41 ms (T-0052's upload cliff) |
| Present one CPU frame, 4K, integrated GPU (the pre-T-0050 full `write_texture`) | — | **23–27 ms** (1080p: 0.54–1.6 ms) | off the interactive path since XARA-T-0050; still paid by the CPU tier and by the Final at rest; XARA-T-0052 investigates the cliff |
| Save a 20 MB `.xarast`, container part (no SVG serialisation yet) | ≤ 1 s | **273 ms** in memory, 285 ms through `write_atomic` to disk (miniz_oxide; 92 ms with zlib-rs) | passes (XARA-US-0021, 2026-09-23); table in "`.xarast` container" |
| Save a `.xarast` end to end (SVG + meta + container, atomic), from `.xar`, real corpus | ≤ 1 s for 20 MB | ProbeX16 (518 k nodes, 56 MB SVG, **9.5 MB package**): **1.54 s** (SVG 600 ms + package 935 ms); every other corpus file ≤ 210 ms | **fails on vector-dense documents** (XARA-US-0023 round 2, 2026-09-23); table in "`.xarast` save end to end"; XARA-T-0101 / XARA-T-0090 |
| Re-save a 300 MB photo `.xarast` with unchanged resources | ≤ 1 s | **104 ms** | passes: raw copies, no rehash, no recompression |
| Open a 20 MB `.xarast`: signature + manifest + thumbnail | ≤ 15 ms | **0.046 ms** | passes |
| BLAKE3 throughput, one core | ≥ 1 GB/s | **5.4–5.9 GiB/s** | passes |
| Open a 5 MB `.xar` to first paint | ≤ 500 ms | not yet | XARA-T-0009; the import alone is already 644 ms for 7.4 MB |
| Cold start to window | ≤ 400 ms | not yet | XARA-T-0010 |

### Document model

`cargo bench -p xarast-doc --bench doc`, two runs. The figures are in the
table above.

**The undo budget hid a regression in dispatch.** The old
`undo/single_node_edit` bench timed dispatch and undo together, and that
figure went from 0.27 µs on the container to 1.13 ms here. The undo itself is
0.28 µs. The whole millisecond is `Tx::commit` → `keep_one_active_layer`,
which walks the **whole tree** with `preorder` on every commit to find the
spreads. That makes every edit O(document size). The bench is now split into
`dispatch/single_node_edit` and `undo/single_node_edit`, so this cannot hide
again. **Fixed 2026-09-23 (XARA-T-0030):** the commit now checks only the
spreads the transaction touched. `dispatch/single_node_edit` went from
1.96 ms (load ~24) to **101 ns** (100.97–101.23 ns, load ~5). Undo is
61 ns and redo 102 ns in the same run.

Full `doc` bench after this round (2026-09-23, load 5): walk_render 0.86 ms, preorder 1.05 ms, get 5.1 ns, resolve warm/cold 20 ns / 0.87 µs, builder 17.9 ms, digest 3.4 ms, snapshot 9.0 ms, dispatch 107 ns, undo 61 ns, redo 103 ns, compute_bounds cold 8.2 ms.

**`snapshot()` fixed the same day.** The HAMT of `Arc<NodeData>` cost a
HAMT insert plus an allocation per node on top of the payload clone. A
side-by-side scratch measurement at load 19–25 split the cost as follows:
the payload clones alone 11–18 ms, into a `std` map 21–24 ms, into `imbl`
28–42 ms, and the old `snapshot()` 26–67 ms. The dense `SecondaryMap`
took 7–13 ms including the drop. `snapshot/100k` now measures
**9.7 ms** (9.63–9.83 ms, load 31).

### Render

`cargo bench -p xarast-render --bench render` (production path: our display
list and compositor over `vello_cpu`, `CpuConfig::interactive()`, light
`bulk` scene):

| | Before (2026-09-23 morning) | After (XARA-US-0016) |
|---|---|---|
| Full frame 1920 × 1080, 20 000 objects | 9.6 ms (8.7–10.1 re-measured) | **5.2 ms** |
| Full frame 1920 × 1080, 100 000 objects | **44.7 ms** (45–49 re-measured) | **19.0 ms** |
| Full frame 960 × 540, 20 000 objects | 22.7 ms (24–52, noisy) | **4.0 ms** |
| Incremental 64 × 64 dirty rect | 0.12–0.15 ms | 0.11 ms |
| `DisplayList::build`, 20 000 | 1.11 ms (55 ns/cmd) | **0.34 ms** (17 ns/cmd) |
| `DisplayList::build`, 100 000 | **19.5–20.8 ms (208 ns/cmd)** | **1.91 ms** (19 ns/cmd) |
| `strip/build_1920x40_100000` (new) | ≈ 15–19 ms (app bench) | **0.26 ms** |
| `strip/render_1920x40_100000` (new) | 9.4 ms | **1.17 ms** |
| Cache sweep, groups of 8 / 32 / 64 / 128 / 512 | 2.44 / 2.59 / 3.15 / 3.55 / 6.67 ms | not re-run |

Criterion medians. The machine was shared with other agents' builds
(load 4–15). Spreads within one run were under 3 %. Between runs at
different loads they reached 15 %, and the "before" full frames varied
more, so the ranges are quoted.

**Why `DisplayList::build` was superlinear (profiled 2026-09-23).** Each
`DrawCmd` was 384 bytes: it cloned the path, the paint, the stroke style
and the transparency. At 100 000 commands the vector was 38 MB, which is
above glibc's 32 MB mmap ceiling. So every build page-faulted a fresh
mapping, and dropping the list walked 200 000 cold `Arc` counters. At 20k
it fitted in the heap and in cache. Ablations on a 100k scene: a 40-byte
record per op, fresh vector, 3.1 ms; the same plus one `PathRef` clone,
5.7 ms; the real build, 21 ms. Rounding was the next cost:
`DeviceRect::enclosing` made eight libm `floor`/`ceil` calls per command,
because the baseline x86-64 target has no SSE4.1. That was 16 ns of the
remaining 26. The fix, in `render.md`: commands index into the scene's
shared ops, and the rounding is done with exact integer casts.

**Where the full frame went.** Single-threaded, 100k: 274 ms, of which
composite 102 ms, `fill_path` 54 ms, **flatten 47 ms** (of polygons, which
flattening reproduces unchanged) and `vello` render 30 ms. The frame is
parallel over bands, and 1 MiB bands are only 8 per 1080p frame. Fixes:
polylines skip flattening, fully covered opaque pixels are written
directly, the unused per-band binning is gone (2 ms), and interactive
bands are 512 KiB (16 per frame).

The W0 spike and the G1/G2 verdicts are in `docs/memory/render.md`.

### Viewport through the Draft → Final scheduler

`cargo bench -p xarast-app -- viewport` (XARA-US-0006, 2026-09-23). The
synthetic document at `SynthSpec { nodes: 250_000, .. }`: **105 852
objects, each filled and stroked, so 224 218 primitives**, 1920 × 1080,
CPU backend in its interactive configuration (all 24 threads). Each figure
is one canvas frame from the intent to the collected pixels: `apply`,
`Canvas::note`, `Canvas::pump`, the render thread, `take_latest`. Two
views: **fit** (the whole page; the drawing fills the height and the sides
are pasteboard) and **zoomed** (3× that, every pixel over the drawing, so
every exposed strip has ink: the worst case for reuse). Two runs at load
7–15 agree to within 3 %; a third taken while the load rose to 25 read
up to 60 % higher and is discarded.

| Frame | fit | zoomed | What the render thread did |
|---|---|---|---|
| `pan_draft`: 9 px right, 5 px up | **5.3–6.6 ms** | **51–57 ms** | scroll + two strips. Zoomed: 35 ms of `DisplayList::build` (two scans of all 224k ops) + 15 ms raster |
| `zoom_draft`: one notch in or out | **1.5–1.6 ms** | **1.5–1.6 ms** | nearest resample of the kept frame; backdrop in the uncovered border |
| `final_after_idle`: the upgrade | 284–287 ms (now 104 ms) | 340–363 ms (now 102 ms) | four full-height columns: ~105 ms of list builds + 170–235 ms raster |
| `full_draft`: no reuse, for reference | 231–235 ms | 262–270 ms | one full Draft frame |

**Verdict against ≤ 16 ms.** Zoom passes everywhere. Pan passes at fit
page and **fails by ~3.4× when the view is full of ink**. Reuse turns a
230–270 ms full Draft into 54 ms, but what is left is not in `xarast-app`:

- `DisplayList::build` visits every scene op for any dirty rect, 15–19 ms
  per strip at 224k primitives however few commands survive
  (XARA-T-0033). A diagonal pan has two strips.
- The CPU backend parallelises over horizontal bands, so a short, wide
  strip runs on one core (XARA-T-0034). Rasterising a zoom-out's border
  this way cost ~270 ms, which is why the Draft now leaves it to the Final.

The Final is off the interactive path, but new input waits for the column
in flight: about a quarter of 285–360 ms. It is 1.2–1.4× a full Draft
because four column builds cost more than one full build.

Also measured while building it (probe, not in the bench): a full scene
walk of this document is ~100 ms, and a walk culled to a 9 px strip is
30–130 ms, so neither re-walking at Draft quality nor walking strips is a
way round the list-build scan.

### `.xar` import

`XARAST_XAR_CORPUS=… cargo bench -p xarast-xar --bench import`. The median
is over 7 warm runs per file, with the bytes already in memory. Two runs, at
load 6 and 19, agree to 0.1 %.

| File | Bytes | Nodes | Parse | Full import |
|---|---|---|---|---|
| `testfiles/ProbeX16.xar` | 7 369 847 | 518 346 | 129–141 ms | **644 ms** |
| `Designs/Spitfire.xar` | 646 635 | 5 971 | 1.9–2.1 ms | 4.3–4.9 ms |
| `testfiles/20000GradFilledShapes50PCtransparent.xar` | 334 528 | 120 006 | 17.6–17.9 ms | 96–103 ms |
| `testfiles/20000GradFilledShapes.xar` | 331 360 | 80 006 | 11.2–13.0 ms | 57–59 ms |
| **All 59 files** | 12 252 824 | 830 533 | 180–200 ms | 870–890 ms |

After XARA-T-0031 (2026-09-23, load 8, three runs): `ProbeX16.xar`
**329–343 ms** (parse 120–147 ms), `20000GradFilledShapes50PCtransparent`
53 ms, `20000GradFilledShapes` 35 ms, and **all 59 files 458–473 ms**
(parse 168–201 ms). Where the time went, and what is left, is in
`docs/memory/xar-import.md` finding 13. This includes the new
quick-shape outline generation (~12 ms on ProbeX16's 34 672 shapes).

Per file: median 0.10 ms, p90 4.5–5.1 ms. The parse is about 20 % of an
import. The rest is mapping into the document model, at roughly 1.2 µs per
node, so that is where to look first. `ProbeX16` is the only corpus file
above 5 MB, and its import alone exceeds the 500 ms open-to-first-paint
budget.

### `.xarast` container

`cargo bench -p xarast-format --bench container`, pinned with
`taskset -c 4-7`, 2026-09-23. Criterion medians; the spread across
samples is under 0.5 %. Two runs: the first with DEFLATE on `zlib-rs`
(load ~4, two fuzzers running), the second on `miniz_oxide` (load ~8),
which is what ships — see "DEFLATE backend" below.

The 20 MB document is synthetic and generated by the bench: 12 MiB of
SVG-like path text plus eight 1 MiB incompressible "JPEG" resources, a
thumbnail, meta. It deflates to a 9.07 MB package, i.e. the SVG part
compresses about 12:1 — at the optimistic end of the 6–12:1 of
`research/06 §4.3`, so a real `document.svg` will cost somewhat more DEFLATE
time per byte. The SVG serialiser (W3) is not in these numbers.

| Bench | What it does | `miniz_oxide` (shipped) | `zlib-rs` |
|---|---|---|---|
| `save_20mb` | `PackageWriter` → `Vec`: BLAKE3 of meta/document/thumbnail, compression policy, DEFLATE level 6, manifest, ZIP | **273 ms** (73 MiB/s of input) | 92.1 ms (217 MiB/s) |
| `save_20mb_atomic` | the same through `write_atomic` to a file on the local disk: temporary, fsync, rename, directory fsync | **285 ms** | 102.4 ms |
| `open_20mb` | `XarastReader::open` (sniff, end record, central directory, manifest parse, consistency) + `thumbnail()` | **46.5 µs** | 47.9 µs |
| `resave_300mb_raw` | open a 310 MB package (30 × 10 MiB resources), `from_package`, `carry_from`, `finish_with_source` into a `Vec` | **104 ms** (1.50 GiB/s: a `memcpy` of compressed bytes) | 100.9 ms |
| `blake3_64mb` | `Digest::of`, single-threaded | 10.6 ms (5.88 GiB/s) | 11.5 ms (5.43 GiB/s) |

Where the save time goes (not profiled, inferred): the resources are
STORED and already hashed in the index, BLAKE3 of 12 MiB is ~2 ms, so the
rest is DEFLATE of the SVG — which is why the backend alone makes it 3×.

**DEFLATE backend.** `zip` was first wired to `zlib-rs`. flate2 picks one
backend for the whole build, so that also switched the `.xar` reader's
tests in workspace builds, and `crates/xarast-xar/tests/fuzz_seeds.rs`
failed because a committed seed (`fuzz_xar_import/two-blocks-streamed-record.bin`)
is compressed bytes. It would equally make a "deterministic" `.xarast`
differ between binaries that happen to enable different backends. The
workspace now has exactly one backend, `miniz_oxide`, and
`deterministic_bytes_are_pinned` fails if it moves. Going to `zlib-rs`
everywhere is a one-line change plus regenerating that one `.xar` seed; it
buys ~180 ms per 20 MB save and is filed as XARA-T-0090. Until the SVG
serialiser exists there is no pressure: 273 ms is a quarter of the budget.

### `.xarast` save end to end (W3, 2026-09-23)

`xarast-cli convert` in release over the 59-file corpus into a scratch
directory (never the repository): `.xar` import, `write_svg`,
`meta.xml`, `PackageWriter` (DEFLATE 6, `miniz_oxide`) through
`write_atomic` with its fsyncs. Development machine, no pinning, load
from other agents; single runs, so read ±10 %.

| Document | Nodes | `document.svg` | Package | Import | SVG | Package write |
|---|---|---|---|---|---|---|
| `ProbeX16.xar` | 518 346 | 56.2 MB | 9.46 MB | 316 ms | **600 ms** | **935 ms** |
| `20000GradFilledShapes50PCtransparent.xar` | 120 006 | 13.3 MB | 0.60 MB | 42 ms | 140 ms | 66 ms |
| `10000GradFilledShapes.xar` | 50 011 | 11.1 MB | 0.74 MB | 19 ms | 100 ms | 79 ms |
| `amurdove.xar` | 8 799 | 1.40 MB | 0.35 MB | 3.3 ms | 13.7 ms | 36.5 ms |
| `Spitfire.xar` (one JPEG) | 5 971 | 0.83 MB | 0.65 MB | 3.1 ms | 10.9 ms | 37.3 ms |
| all 59 files | — | 103 MB | 14.8 MB | 415 ms | 1 076 ms | 1 592 ms |

Criterion (`cargo bench -p xarast-format --bench container -- svg`),
synthetic 100 000-node document: `svg/write_100k` **34.2 ms**,
`svg/save_100k` (SVG + meta + container into a `Vec`) **114.9 ms**.

Reading it: the budget ("save a 20 MB `.xarast` ≤ 1 s") holds for every
real file except ProbeX16, whose package is only 9.5 MB but whose SVG is
56 MB — ~108 bytes per node, because every ink element carries its full
resolved paint (passes 4–5, hoisting and CSS classes, are not done) — and
DEFLATE of those 56 MB with `miniz_oxide` is ~60 MB/s. Both halves have a
filed fix: XARA-T-0101 (passes 4–5, the spec expects ~35 % less SVG) and
XARA-T-0090 (`zlib-rs`, ~3× faster DEFLATE). Serialisation itself is
~1.2 µs per node.

## Phase 2 — document model (development container, historical)

`cargo bench -p xarast-doc --bench doc -- --quick`

| Operation | Budget | Measured | Verdict |
|---|---|---|---|
| Undo of a single-node edit | ≤ 1 ms | 0.27 µs | well under |
| Redo of a single-node edit | ≤ 1 ms | 0.17 µs | well under |
| `walk_render` over 100 000 nodes | ≤ 2.0 ms | 2.01 ms | at budget (50 M nodes/s) |
| `preorder` over 100 000 nodes | ≤ 1.5 ms | 2.09 ms | 1.4× over |
| Random `Tree::get` by `NodeId` | ≤ 5 ns | 6.6 ns | 1.3× over |
| `AttrStack::push` + `pop_scope` | ≤ 20 ns | 17.5 ns | under |
| `AttrResolver::resolve`, warm | ≤ 30 ns | 30.4 ns | at budget |
| `AttrResolver::resolve`, cold | ≤ 2 µs | 1.08 µs | under |
| `DocumentBuilder`, 100 000 nodes | ≤ 150 ms | 47.4 ms | under |
| `Document::canonical_digest()` | ≤ 30 ms | 6.2 ms | under |
| `Document::snapshot()` | ≤ 25 ms | 58.5 ms | **2.3× over** |
| Resident bytes per node | ≤ 160 B | 221 B (incl. payloads) | not comparable |
| `compute_bounds`, 100 000 nodes, cold | ≤ 12 ms | 8.95 ms | under |

### The arena versus a persistent store

`cargo run --release -p xarast-doc --example arena_vs_persistent`

| M | Arena | `imbl` HAMT | |
|---|---|---|---|
| A full render traversal | 1.99 ms (50.3 M nodes/s) | 22.3 ms (4.5 M nodes/s) | HAMT 11.2× slower |
| B 10 M random lookups | 7.06 ns each | 103.3 ns each | HAMT 14.6× slower |
| C edit + undo | 0.21 µs | 1.52 µs | HAMT 7.2× slower |
| D resident bytes | 22.13 MB (221 B/node) | 30.89 MB (308 B/node) | HAMT 1.40× |

The arena wins against the pre-registered decision rule. Full reasoning in
[`document-model.md`](document-model.md).

## Phase 4 — render engine (development container, historical)

`cargo bench -p xarast-render`, on the same development container the W0 spike
used: **Intel Xeon @ 2.10 GHz, 4 vCPU, 15 GiB, no GPU**. The phase's budget
table assumes eight cores and an integrated GPU, so the overruns below are
partly the machine. `docs/memory/render.md` has the full spike table and the
gate verdicts.

| Budget | Target | Measured | Verdict |
|---|---|---|---|
| CPU full frame, 1920 × 1080, Final, 8 cores | ≤ 25 ms at 100 000 objects | **24.6 ms at 20 000 objects** on 4 cores | ~5× over, machine-adjusted ~1.5× over |
| CPU full frame, 960 × 540, 20 000 objects | — | 38.4 ms | slower than 1080p: four times the overdraw |
| Incremental 64 × 64 dirty rect | ≤ 0.3 ms | **0.258 ms** | **under** |
| Display-list build, warm scene | ≤ 3 ms at 100 000 nodes | 2.12 ms at 20 000 nodes (≈ 10.6 ms extrapolated) | **3.5× over**; `DrawCmd` is large |
| Ramp build, 2048 entries, 8 stops, with profile | ≤ 40 µs | 86.6 µs | 2.2× over (was 126 µs before the RGB fast path) |
| Ramp cache hit | — | 0.52 ns | noise |
| Blend LUT set, 12 families | ≤ 15 ms once at startup | **0.56 ms** | **27× under** |
| Resident LUT memory | ≤ 768 KiB | **768 KiB exactly** | at budget |
| GPU full frame | ≤ 8 ms | **unmeasured** | no adapter on this machine |

Cache-admission sweep, 512 × 512, one group of N primitives per frame — the
measurement behind the threshold of 64:

| Group size | 8 | 32 | 64 | 128 | 512 |
|---|---|---|---|---|---|
| Frame | 4.41 ms | 4.52 ms | 4.82 ms | 5.69 ms | 10.08 ms |

### What the spike says about where the time goes

Recording a scene into the rasteriser costs more than filling the pixels:
273 ms of a 442 ms 100 000-object frame. `vello_cpu` has no scene-reuse API,
so that cost recurs every frame, which is the argument for a per-node cache
that holds **pixels** rather than encodings.

A naive bounding-box scan over 100 000 objects costs 36.5 ms — forty times the
0.81 ms it takes to render a 64 × 64 dirty rect out of them. The display list
therefore carries precomputed bounds and the tile planner bins by them; an
incremental redraw that rediscovers its own geometry is not incremental.

## Phase 5 — user interface (`xarast-ui`, development container)

Measured by `cargo run -p xarast-ui --release --example density` and kept
honest by `cargo bench -p xarast-ui`. Same container as Phase 4: 4 slow
cores, **no GPU and no compositor**, so the GPU and presentation rows are
unmeasured rather than estimated. Full context in `docs/memory/ui.md`.

| Budget | Target | Measured | Note |
|---|---|---|---|
| `build_ui_frame()`, density probe (1,789 controls, 2560×1440) | ≤ 3 ms p50 | **1.63–1.65 ms p50, 2.19–3.40 ms p99** | passes; the p99 spread is this container under three concurrent builds |
| UI build + tessellation, same probe | ≤ 8 ms | **4.71–6.68 ms** (p99 + p99) | events and submit not included |
| egui GPU pass | ≤ 1.5 ms | **unmeasured** | needs an adapter |
| 5,000-row virtualised tree, 600 scrolled frames | no frame over 16 ms | **worst 2.84–2.99 ms** (CPU only) | presentation unmeasured |
| One slider dragged vs. idle frame | within 1 ms | **+0.00 to +0.05 ms** | partial update is a non-issue |
| Resident growth, all panels open | ≤ 50 MB | **9.3 MB** | |
| Frame build at 1× / 1.25× / 1.5× | no scale cliff | **1.77 / 1.79 / 1.78 ms** | rows are 20/25/30 device px |
| AccessKit tree, full probe | tree, fields, toggles present | **2,415 nodes, 1,752 labelled** | list roles are published by us, not by egui |

## Corpus render at 100 % (`xarast-cli render`)

```text
cargo build --release -p xarast-cli
xarast-cli render $XARAST_XAR_CORPUS/{testfiles,Designs,Templates,TextDesigns} \
    --out-dir <scratch>          # never into the repository
```

These are the 59 corpus files on the deterministic CPU config, `Final` quality,
96 dpi, each framed on its own drawing. Measured 2026-09-23 on a different
machine from the Phase 4/5 rows: **Intel Core Ultra 9 285, 24 threads, 93 GiB,
no GPU**. Another agent was building on the same machine at the time, so treat
the numbers as ±20 %.

| Measure | Value |
|---|---|
| Wall time, whole run (one process) | **29.2 s**, peak RSS 505 MiB |
| Open (read + import), all 59 files | 0.94–0.98 s; slowest is `ProbeX16` at 0.65 s |
| Render, all 59 files | 26.9–33.4 s |
| PNG encode + write, all 59 files | 0.69–0.79 s |
| Render per file, median | **≈ 0.7 ms** |
| Render per file, p90 | ≈ 135–180 ms |
| Render, the three `*GradFilledShapes*` files | **9.8 / 10.2 / 10.9 s** (766×739 and 766×880 px, 10 000–20 000 gradient fills); **2.6 / 2.2 / 4.5 s** after XARA-T-0014 |
| Render, the other 56 files together | ≈ 2.5 s; next-slowest are `ProbeX16` 1.07 s and `SimpleSphere` 0.66 s |

Ink versus blank:

- **38** files produce scene primitives and **21** do not: the 8 empty
  templates and 13 text-only `TextDesigns`, since text is Phase 9.
- **35** files paint pixels and **24** are blank. The extra three
  (`RedStar`, `Test00`, `TestBitmapFill`) are quick shapes with zero-area
  bounds, which the display list culls (XARA-T-0013).

`smoke-open` over the same four directories reports "59 opened
(36 complete, 38 with primitives)". In total, 7 777 text nodes and 25 images
are still pending.

**The gradient files are the finding.** 10 000 gradient-filled shapes take
10 s at 0.57 Mpx. Phase 4 measured 20 000 *flat* fills at 24.6 ms at 1080p,
so gradients cost roughly 400× more per frame. `--quality draft` only brings
the first file down to 6.5 s. At 192×192 it takes 0.45 s, so the cost scales
with pixels × gradient objects. Tracked as XARA-T-0014.

**Why (profiled 2026-09-23): not per-object waste but pixels.** Each shape
covers about 14 000 px, against about 100 px for a `bulk` shape, so the
10k file composites **1.4 × 10⁸ pixels**. The "400×" compares unequal
pixel counts. Per pixel, sampling the paint and the graduated
transparency cost about 21 ns and the blend 9 ns. Each sample used to
invert the gradient's 3×3 mapping. The transparency also sat in document
space (a bug, see `render.md`), so these files had rendered with *flat*
transparency until then. Fixes: samplers ready the inverse, table and
image once per primitive. A linear gradient in an affine frame computes
only `u`. `rem_euclid` and `round` stop calling libm. Opaque sampled
pixels are written directly. The deterministic configuration uses every
core, which is byte-safe because bands merge by index. After:
**2.6 / 2.2 / 4.5 s**. What is left: about 30 ns per pixel across only
three bands, because a 766 px wide image at 1 MiB per band is three bands
tall. The next steps are row-wise (SIMD) paint and blend, and more bands
for export; see XARA-T-0038.

### GPU tile compositing (XARA-US-0011)

`cargo bench -p xarast-render --features gpu --bench tiles`, 2026-09-23,
four runs at load 5–25, medians of 50–300 frames; ranges span the runs.
`WGPU_ADAPTER_NAME=<substring>` selects one adapter. GPU rows are wall
clock to `poll(wait)`.

| | Intel Arrow Lake iGPU | RTX 4000 SFF Ada |
|---|---|---|
| empty submit + wait | 0.05–0.37 ms | 0.05–0.19 ms |
| upload a whole 1080p frame (today) | 0.54–1.6 ms | 0.61–0.89 ms |
| pan, 40 resident tiles | 0.47–1.4 ms | 0.08–0.21 ms |
| pan crossing a tile row (8 uploads + composite) | 0.71–2.2 ms | 0.27–0.51 ms |
| Draft zoom 1.3× / 0.5× | 0.37–1.8 / 0.71–2.0 ms | 0.08–0.15 / 0.15–0.47 ms |
| upload a whole 4K frame (today) | **23–27 ms** | 2.6–3.5 ms |
| 4K pan, 144 resident tiles | 1.5–3.0 ms | 0.30–0.54 ms |
| upload 64 MiB of A8 | 27–33 ms | 5.4–6.9 ms |

CPU side of the same bench: `scroll_surface` 1080p 0.19–0.73 ms;
`compose_cpu` 1080p 1.3–3.1 ms; one 256² tile of a 900k-op scene 5–10 ms
and a 1920 × 256 row 12–18 ms, of which the culled `DisplayList::build`
alone is 6.5–10 ms (linear in the scene's ops). The decision these feed
is in `render.md`, "The GPU decision".

**The iGPU's large-upload cliff.** 8 MB uploads in about 0.6 ms, 33 MB in
24 ms: four times the bytes, fifteen to fifty times the time. The RTX scales
linearly. Unexplained; XARA-T-0052.

### Pan and zoom end to end (XARA-T-0050, XARA-T-0008)

Two probes, 2026-09-23, load average 1–3.5, 300 samples each after 10
warm-up frames, one scripted step a frame: pan 23.4 × 7.7 px (fractional,
reversing every 60 steps), zoom 1.05× about the centre (ten in, ten out).
Documents: the synthetic 250 000-node document (~105 000 paths, filled and
stroked; `--synthetic 250000`) and `testfiles/ProbeX16.xar`.

**Live window** — `xarast --probe pan|zoom [--synthetic 250000 | file]`
on the COSMIC session, no vsync, the window closing itself. The sample is
from the intent being applied to `Queue::present` returning, and to the
GPU going idle after it (`device.poll(wait)`), which includes the
interface frame, the tile uploads of whatever the render thread delivered
and the composite. COSMIC tiles the window, so the canvas is 1796 × 1338
(2.4 MP, more than 1080p) whatever `--size` asks.

| p50 / p99 ms, input → presented (→ GPU idle) | synthetic pan | synthetic zoom | ProbeX16 pan | ProbeX16 zoom |
|---|---|---|---|---|
| Intel iGPU, GPU tiles | 0.50 / 1.31 (2.24 / 5.52) | 0.37 / 0.54 (2.13 / 2.75) | 0.37 / 1.06 (1.56 / 4.68) | 0.33 / 0.53 (2.05 / 8.72) |
| Intel iGPU, CPU tier | 3.50 / 4.15 (5.00 / 9.47) | 3.46 / 3.96 (5.13 / 10.66) | 3.36 / 4.27 (5.14 / 12.26) | 2.52 / 3.40 (3.58 / 10.07) |
| RTX 4000 Ada, GPU tiles | 0.44 / 3.19 (0.79 / 3.38) | 0.32 / 2.48 (0.69 / 2.49) | 0.31 / 2.64 (0.76 / 2.79) | 0.30 / 2.52 (0.69 / 2.52) |
| RTX 4000 Ada, CPU tier | 3.28 / 5.02 (3.74 / 5.30) | 2.37 / 3.86 (2.90 / 3.94) | 3.11 / 4.15 (3.63 / 4.76) | 2.36 / 3.20 (2.89 / 3.60) |

Worst single sample of the iGPU GPU-tiles runs: 21.8 ms to GPU idle (one,
synthetic pan); every p99 is inside 16 ms.

**Offscreen at any size** — `cargo run --release -p xarast-shell
--example canvas_probe -- --size WxH --tier gpu|cpu --kind pan|zoom`,
input at 120 Hz. Same session, scheduler, render thread, planner and tile
store, composited into a canvas-sized texture; no interface, no
swapchain. The sample is intent → GPU idle.

| p50 / p99 ms | 1080p pan | 1080p zoom | 4K pan | 4K zoom |
|---|---|---|---|---|
| Intel, GPU tiles, synthetic | 1.79 / 3.98 | 1.58 / 4.15 | 3.71 / 9.62 | 1.98 / 5.83 |
| Intel, GPU tiles, ProbeX16 | 1.94 / 5.45 | 1.59 / 3.16 | 3.44 / 7.69 | 2.86 / 6.92 |
| Intel, CPU tier, synthetic | 3.23 / 7.23 | 2.83 / 5.76 | **34.8 / 40.6** | **31.1 / 35.4** |
| RTX, GPU tiles, synthetic | 0.49 / 1.48 | 0.43 / 0.63 | 0.79 / 4.57 | 0.56 / 0.94 |
| RTX, CPU tier, synthetic | 2.90 / 4.38 | 2.66 / 3.19 | 12.7 / 17.8 | 8.79 / 11.3 |

Reading them:

- **The budget is met on the integrated GPU at 1080p and at 4K** through
  the GPU tile tier; XARA-T-0008's GPU half is closed with this.
- The CPU tier (`XARAST_RENDERER=cpu`, and the fallback) is fine at 1080p
  and fails at 4K on the iGPU: its whole-canvas upload is the 33 MB cliff
  of XARA-T-0052. That is the fallback's cost, not the default path's.
- A pan uploads about 320 KB a step (95 MB over 300 steps at 1080p): the
  exposed strips, plus the partial tiles at the trailing edge that cannot
  grow their valid area into a rectangle and start afresh.
- A zoom uploads nothing until the gesture ends: the render thread skips
  Draft zooms (`FrameJob::cpu_rescale = false`) and the GPU resamples the
  resident level. The Final that follows is a whole-frame upload, at rest
  (at 4K on the iGPU that single frame pays the T-0052 cliff once).
- Max outliers (15–25 ms offscreen at 4K, once per run) coincide with the
  first frames after a direction change, when the render thread's strips
  land in a burst; p99 stays inside budget.

## Things that were slow, and why

Worth remembering, because each was a factor of several and each has a shape
that will recur:

- **`Document::snapshot()` on the edit path.** Checkpointing every 64
  transactions cost about **1.5 ms amortised per edit** and blew the 1 ms undo
  budget on its own. Automatic checkpointing is now off by default. A
  persistent snapshot is cheap to *hold* and expensive to *build*; build it
  incrementally, off the edit path.
- **Taking a resolved-attribute snapshot per node.** `ResolvedAttrs` is two
  `Arc` clones to copy but a fresh allocation to make, and a whole-document
  bounds pass made one per node: 53 ms for 100 000 nodes. Asking the
  `AttrStack` for just the stroke extent took it to 17.8 ms.
- **`HashMap` where a `SecondaryMap` belongs.** The same bounds pass went from
  17.8 ms to 8.95 ms by keying its scratch map on the slot index instead of
  hashing the `NodeId`. Anything keyed by `NodeId` over a whole document should
  be a `SecondaryMap`.

## Open TODOs

- [x] Establish a reference machine and pin the budget table to it — done
      2026-09-23 (XARA-US-0010); see [Reference machine](#reference-machine).
- [x] Re-measure `preorder` and `Tree::get` on it — `preorder` passes
      (1.07 ms), `Tree::get` is at budget (5.1 ns including the bench's own
      index arithmetic). It was the container.
- [x] Pan/zoom on the CPU through the scheduler: `cargo bench -p xarast-app
      -- viewport` (XARA-US-0006). Zoom passes, pan fails 3.4× when zoomed
      into the drawing; see the section above.
- [x] Pan/zoom on the integrated GPU (XARA-T-0008) — passes end to end
      through the GPU tile tier, 1080p and 4K (XARA-T-0050, see "Pan and
      zoom end to end").
- [ ] Open-to-first-paint (XARA-T-0009) and cold start (XARA-T-0010).
- [ ] `DisplayList::build` culled builds (XARA-T-0033) and single-core
      short strips (XARA-T-0034): the two things between pan and 16 ms.
- [ ] `Tx::commit` walks the whole tree on every edit
      (`keep_one_active_layer`): about 1 ms per command at 100k nodes.
- [ ] `.xar` import maps at about 1.2 µs per node: `ProbeX16.xar` takes 644 ms
      against 350 ms. Profile the model-mapping half.
- [x] `DisplayList::build` at 100k: 20.8 ms against 3 ms — now 1.9 ms
      (XARA-US-0016, 2026-09-23). The CPU full frame at 100k passes too
      (19 ms against 25).
- [ ] Gradient-heavy export: about 30 ns per composited pixel and three
      bands on a 766 px image (XARA-T-0038).
- [ ] `Document::snapshot()`: 43–80 ms against 25 ms, still.
- [ ] Wire the budgets into CI so that a regression fails the build, rather
      than being noticed later. CI runners are not the reference machine, so
      the CI gate needs either a self-hosted runner on it or budgets scaled by
      a calibration bench.
- [ ] Measure bytes per node excluding payloads, so the 160 B budget can
      actually be judged.
