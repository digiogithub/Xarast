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
| Dispatch (commit) of that edit | — | **1.04 ms** | regression, see below |
| `walk_render`, 100 000 nodes | ≤ 2.0 ms | 0.87–0.92 ms | passes |
| `preorder`, 100 000 nodes | ≤ 1.5 ms | 1.07–1.08 ms | passes (2.09 ms on the container) |
| Random `Tree::get` | ≤ 5 ns | 5.10–5.67 ns | at budget; the figure includes the bench's own index arithmetic |
| `AttrStack::push` + `pop_scope` | ≤ 20 ns | 12.1–12.9 ns | passes |
| `AttrResolver::resolve`, warm / cold | ≤ 30 ns / ≤ 2 µs | 18.9 ns / 0.87–0.92 µs | passes |
| `DocumentBuilder`, 100 000 nodes | ≤ 150 ms | 40–46 ms | passes |
| `Document::canonical_digest()` | ≤ 30 ms | 4.1–5.7 ms | passes |
| `Document::snapshot()` | ≤ 25 ms | **43–80 ms** | **1.7–3.2× over** |
| `compute_bounds`, 100 000 nodes, cold | ≤ 12 ms | 7.6–8.1 ms | passes |
| W0 G1: `vello_cpu`, `bulk` 1080p, 1 thread | ≤ 120 ms | **159–164 ms** | **fails** |
| W0 G1: `vello_cpu`, `bulk` 1080p, 8 P-cores | ≤ 25 ms | **53–58 ms** | **fails** |
| W0 G2: `vello`, `bulk` 1080p, integrated GPU | ≤ 8 ms | **72–86 ms** | **fails, 9–11×** |
| W0 G2: same, discrete GPU (information only) | ≤ 8 ms | 7.8–8.0 ms | at the line |
| CPU full frame, production path, 1080p, 100 000 objects | ≤ 25 ms | **44.7 ms** | **1.8× over** |
| Incremental 64 × 64 dirty rect | ≤ 0.3 ms | 0.12–0.13 ms | passes |
| `DisplayList::build`, 100 000 commands | ≤ 3 ms | **20.8 ms** | **6.9× over** |
| Ramp build, 2048 entries, 8 stops, with profile | ≤ 40 µs | 34.5 µs | passes (86.6 µs on the container) |
| Blend LUT set, 12 families | ≤ 15 ms | 0.28 ms | passes |
| `.xar` full import, `ProbeX16.xar` (7.4 MB) | ≤ 350 ms | **644 ms** | **1.8× over** |
| `.xar` full import, whole corpus (59 files, 12.3 MB) | ≤ 3 s | 0.87–0.89 s | passes |
| Pan/zoom, 100k objects, integrated GPU | ≤ 16 ms | not yet | XARA-T-0008, needs the viewer |
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
again. Not fixed in this round. The fix is to track spreads, or to check only
the spreads the transaction touched.

### Render

`cargo bench -p xarast-render --bench render` (production path: our display
list and compositor over `vello_cpu`, `CpuConfig::interactive()`, light
`bulk` scene):

| | Measured |
|---|---|
| Full frame 1920 × 1080, 20 000 / 100 000 objects | 9.6 ms / **44.7 ms** |
| Full frame 960 × 540, 20 000 objects | 22.7 ms |
| Incremental 64 × 64 dirty rect | 0.12–0.13 ms |
| `DisplayList::build`, 20 000 / 100 000 | 1.11 ms (55 ns/cmd) / **20.8 ms (208 ns/cmd)** |
| Cache sweep, groups of 8 / 32 / 64 / 128 / 512 | 2.44 / 2.59 / 3.15 / 3.55 / 6.67 ms |

**`DisplayList::build` is superlinear.** Five times the commands cost 18.8
times the time, so the old estimate of ~10.6 ms at 100k, extrapolated
linearly from 20k, was wrong by half. The loop is linear, so the cause is
probably memory: the command vector and the `Arc` path clones fall out of
cache, and every build allocates a large, fresh vector. That is not yet
profiled. Story XARA-US-0016 owns it.

The W0 spike and the G1/G2 verdicts are in `docs/memory/render.md`.

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

Per file: median 0.10 ms, p90 4.5–5.1 ms. The parse is about 20 % of an
import. The rest is mapping into the document model, at roughly 1.2 µs per
node, so that is where to look first. `ProbeX16` is the only corpus file
above 5 MB, and its import alone exceeds the 500 ms open-to-first-paint
budget.

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
- [ ] Measure pan/zoom (XARA-T-0008), open-to-first-paint (XARA-T-0009) and
      cold start (XARA-T-0010) once the viewer is wired.
- [ ] `Tx::commit` walks the whole tree on every edit
      (`keep_one_active_layer`): about 1 ms per command at 100k nodes.
- [ ] `.xar` import maps at about 1.2 µs per node: `ProbeX16.xar` takes 644 ms
      against 350 ms. Profile the model-mapping half.
- [ ] `DisplayList::build` at 100k: 20.8 ms against 3 ms (XARA-US-0016).
- [ ] `Document::snapshot()`: 43–80 ms against 25 ms, still.
- [ ] Wire the budgets into CI so that a regression fails the build, rather
      than being noticed later. CI runners are not the reference machine, so
      the CI gate needs either a self-hosted runner on it or budgets scaled by
      a calibration bench.
- [ ] Measure bytes per node excluding payloads, so the 160 B budget can
      actually be judged.
