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

- [ ] Establish a reference machine and pin the budget table to it.
- [ ] Wire the budgets into CI so that a regression fails the build, rather
      than being noticed later.
- [ ] Measure bytes per node excluding payloads, so the 160 B budget can
      actually be judged.
- [ ] Re-measure `preorder` and `Tree::get` once there is a reference machine;
      both are within 40 % of their budgets and may simply be this machine.
