# perf

Memory note for **performance**: budgets, the measurements behind them, and
the regressions to watch for.

## Current state

Phase 0 set the budgets in the phase documents. Phase 2 is the first phase to
measure against them at scale, with `criterion` over a synthetic 100 000-node
document (`xarast_doc::synth`).

**There is no reference machine yet.** Every number below was taken on the
development machine in `release`, with `cargo bench -- --quick`. Read a 30 %
overrun as "this is where the cost is", not as a failed gate. Establishing a
reference machine, and wiring the budgets into CI so that a regression fails
the build, is still owed — the phase documents assume it.

## Phase 2 — document model

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

## Phase 4 — render engine

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

## Phase 5 — user interface (`xarast-ui`)

Measured by `cargo run -p xarast-ui --release --example density` and kept
honest by `cargo bench -p xarast-ui`. Same container as Phase 4: 4 slow
cores, **no GPU and no compositor**, so the GPU and presentation rows are
unmeasured rather than estimated. Full context in `docs/memory/ui.md`.

| Budget | Target | Measured | Note |
|---|---|---|---|
| `build_ui_frame()`, density probe (1,789 controls, 2560×1440) | ≤ 3 ms p50 | **1.65 ms p50, 3.40 ms p99** | passes |
| UI build + tessellation, same probe | ≤ 8 ms | **6.68 ms** (p99 + p99) | events and submit not included |
| egui GPU pass | ≤ 1.5 ms | **unmeasured** | needs an adapter |
| 5,000-row virtualised tree, 600 scrolled frames | no frame over 16 ms | **worst 2.99 ms** (CPU only) | presentation unmeasured |
| One slider dragged vs. idle frame | within 1 ms | **+0.05 ms** | partial update is a non-issue |
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

- [ ] Establish a reference machine and pin the budget table to it. Phase 4
      needed one badly: G1 and G2 of its rasteriser spike are both unsettled
      because the container has 4 slow cores and no GPU at all.
- [ ] Wire the budgets into CI so that a regression fails the build, rather
      than being noticed later.
- [ ] Measure bytes per node excluding payloads, so the 160 B budget can
      actually be judged.
- [ ] Re-measure `preorder` and `Tree::get` once there is a reference machine;
      both are within 40 % of their budgets and may simply be this machine.
