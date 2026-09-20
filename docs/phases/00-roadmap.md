# Xarast — execution roadmap

> Master plan. Each phase has its own document with detailed tasks, acceptance
> criteria and risks. This one defines the phases, their order, what can run in
> parallel, and the gate that closes each.

---

## Principles behind the ordering

1. **Ship an AppImage from day one.** Linux/Wayland packaging is the stated
   priority, so it is Phase 0, not the last phase. The very first CI run
   produces a downloadable AppImage that opens an empty window. Everything
   after that is filling it in. Packaging problems surface when they are cheap
   to fix, and there is always something to hand someone.
2. **Risk first.** The rasteriser is the one component that can sink the
   product (`research/04 §2.4`). It gets prototyped and measured early, before
   anything is built on top of it.
3. **Read before write.** Importing `.xar` comes before the native format:
   it validates the document model against 59 real files instead of against our
   own assumptions.
4. **Every phase ends in something runnable.** No phase closes on a library
   nobody can exercise.

---

## Phase list

| # | Phase | Delivers | Depends on | Parallel with |
|---|---|---|---|---|
| **0** | Foundations & walking skeleton | Workspace, CI, licence gate, empty window shipped as an AppImage | — | — |
| **1** | Geometry & colour core | `xarast-geom`, `xarast-color`, millipoint arithmetic, path ops | 0 | 2 |
| **2** | Document model | `xarast-doc`: arena, attributes, layers, command bus, undo | 0 | 1 |
| **3** | `.xar` importer | `xarast-xar` + `xar-dump`; the 59-file corpus parses clean | 1, 2 | 4 |
| **4** | Render engine | `xarast-render`: scene, display list, GPU + CPU backends, gradients, transparency | 1 | 3 |
| **5** | Shell & UI skeleton | `xarast-shell` + `xarast-ui`: canvas, viewport, layer and colour panels → **first usable viewer** | 3, 4 | — |
| **6** | Native `.xarast` format | `xarast-format`: read, write, round-trip, resource dedup | 2, 3 | 7 |
| **7** | Tools & editing | Selector, transforms, rectangle, ellipse, node editing, structure ops | 5 | 6 |
| **8** | Colour, fills & transparency | Interactive on-canvas fill and transparency handles, colour editor, palette | 7 | 9 |
| **9** | Text | `xarast-text`: shaping, layout, text tool, convert to shapes | 5 | 8 |
| **10** | Bitmaps & photo | Import, bitmap resources, gallery, non-destructive adjustments | 4 | 9 |
| **11** | Export filters | SVG, PNG, JPEG, WebP, PDF | 4, 6 | 10 |
| **12** | **v0.1 release** | Performance budgets, accessibility, i18n scaffolding, docs, signed AppImage | 5–11 | — |
| **13** | Live effects | Shadow, feather, bevel, contour, blend, mould, fractal fills | 12 | — |
| **14** | Windows & macOS | Platform shells, packaging, notarisation | 12 | 13 |
| **15** | **v1.0** | Wider import, print/PDF fidelity, stability | 13, 14 | — |

---

## Dependency graph

```
        ┌── 1 geometry ──┬───────────────┐
  0 ────┤                │               ├── 4 render ──┐
        └── 2 model ─────┴── 3 xar ──────┤              ├── 5 shell/UI ──┬── 7 tools ── 8 fills
                                         │              │                ├── 9 text
                                         └── 6 xarast ──┘                └── 10 bitmaps
                                                                                  │
                                              11 export ───────────────────────────┤
                                                                                  ▼
                                                                          12 v0.1 release
                                                                                  │
                                                                     ┌────────────┴────────────┐
                                                               13 live effects          14 win/mac
                                                                     └────────────┬────────────┘
                                                                                  ▼
                                                                              15 v1.0
```

## Parallelisation

Phases marked parallel are worked by separate agents against separate crates.
The crate boundaries in `docs/10-architecture.md` are also the concurrency
boundaries: two agents never edit the same crate at the same time. Shared
contracts (trait signatures between crates) are agreed in the architecture
document *before* the parallel work starts, so neither side blocks on the other.

## Milestones

| Milestone | Phases | What it means |
|---|---|---|
| **M1 — It runs** | 0 | An AppImage exists and opens |
| **M2 — It reads** | 1–3 | `xar-dump` parses the whole corpus |
| **M3 — It draws** | 4–5 | Real Xara documents render on screen, pan and zoom |
| **M4 — It edits** | 6–8 | Open, change, save, reopen without loss |
| **M5 — v0.1** | 9–12 | A usable editor, released as an AppImage |
| **M6 — v1.0** | 13–15 | Parity on what matters, on three platforms |

---

## Phase gates

A phase is closed only when **all** of the following hold. No exceptions, no
"we will come back to it".

1. `cargo test --workspace` passes.
2. `cargo clippy --workspace -- -D warnings` is clean.
3. `cargo deny check licenses` passes.
4. The phase's own acceptance criteria (in its document) are met and
   demonstrable.
5. Performance budgets for the phase are met and recorded.
6. The relevant `docs/memory/*.md` note is updated with decisions, dead ends
   and invariants.

---

## Performance budgets

Carried from Phase 0 and checked in CI from the moment each becomes meaningful.
A regression fails the build.

| Budget | Target | Measured from |
|---|---|---|
| Pan/zoom frame time, 100k objects, integrated GPU | ≤ 16 ms | Phase 5 |
| Open a 5 MB `.xar` | ≤ 500 ms to first paint | Phase 5 |
| Undo/redo of a single edit | ≤ 1 ms | Phase 2 |
| Save a 20 MB `.xarast` | ≤ 1 s | Phase 6 |
| AppImage size | ≤ 80 MB | Phase 0 |
| Cold start to window | ≤ 400 ms | Phase 0 |
