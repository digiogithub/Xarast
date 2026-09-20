# Xarast — architecture

> Keystone document. It fixes the crate layout, the data flow and the decisions
> that the research documents left open or contradicted each other on.
> Where this document disagrees with `docs/research/*`, **this one wins**.

---

## 1. Shape of the system

```
                   ┌───────────────────────────────────────────────┐
   .xar  ──────────▶ xarast-xar        (read-only legacy importer)  │
   .xarast ◀───────▶ xarast-format     (ZIP + SVG native container) │
   .svg/.png/… ◀───▶ xarast-io         (import/export filters)      │
                   └───────────────────┬───────────────────────────┘
                                       │ builds / serialises
                                       ▼
        ┌───────────────────────────────────────────────────────────┐
        │  xarast-doc      document model: node arena, attributes,   │
        │                  layers, pages, resources, undo log        │
        └───────────────┬───────────────────────────┬───────────────┘
                        │ read-only traversal       │ mutation via commands
                        ▼                           │
        ┌───────────────────────────────┐           │
        │  xarast-render                │           │
        │  scene → display list →       │           │
        │  tiles → GPU | CPU backend    │           │
        └───────────────┬───────────────┘           │
                        │ pixels                    │
                        ▼                           ▼
        ┌───────────────────────────────────────────────────────────┐
        │  xarast-app      tools, selection, viewport, command bus,  │
        │                  preferences — UI-toolkit agnostic         │
        └───────────────┬───────────────────────────┬───────────────┘
                        ▼                           ▼
              xarast-ui (panels, canvas)      xarast-cli (headless)
                        │
              xarast-shell (winit + wgpu + platform integration)
```

Two consumers sit on the same core: the GUI and a headless CLI. The CLI is not
a side project — it is how rendering is tested in CI, how the corpus is
validated, and how `.xar → .xarast → png` conversions are benchmarked.

---

## 2. Crates

| Crate | Responsibility | Depends on |
|---|---|---|
| `xarast-geom` | Points, rects, matrices, paths, flattening, stroke-to-path, boolean ops, millipoint fixed-point type | `kurbo`, `lyon_algorithms` |
| `xarast-color` | Colour models (RGB/CMYK/HSV/grey), named and indexed colours, tints/shades/links, conversion | — |
| `xarast-doc` | Node arena, attribute model, layers/pages/spreads, resource table, command/undo log | `xarast-geom`, `xarast-color` |
| `xarast-text` | Font database, shaping, paragraph layout, text on a path | `parley`, `swash`/`skrifa`, `fontdb` |
| `xarast-image` | Image decode/encode, resampling, colour management, bitmap resources | `image`, `zune-*`, `jpeg-decoder` |
| `xarast-render` | Scene, display list, tiling, GPU and CPU backends, compositor, blend modes | `xarast-geom`, `vello`, `vello_cpu`, `wgpu` |
| `xarast-xar` | `.xar` reader: record layer, decompression, tree builder, model mapping | `xarast-doc`, `flate2` |
| `xarast-format` | `.xarast` container: ZIP, SVG profile read/write, resource dedup, round-trip preservation | `xarast-doc`, `zip`, `quick-xml` |
| `xarast-io` | Import/export filters: SVG, PNG, JPEG, WebP, PDF | `xarast-doc`, `xarast-image` |
| `xarast-app` | Tools, selection, viewport, command bus, preferences, document session | all of the above |
| `xarast-ui` | Panels, galleries, dialogs, canvas widget, on-canvas handle overlays | `xarast-app`, `egui` |
| `xarast-shell` | Window, event loop, GPU surface, tablet input, portals, clipboard, DnD | `winit`, `wgpu`, `accesskit` |
| `xarast-cli` | Headless convert/render/inspect; the test and benchmark driver | `xarast-app` |

| `xarast-testkit` | Shared test fixtures: the `.xar` corpus loader, golden-image harness, comparison helpers. A dev-dependency only, never shipped | `xarast-doc` |

**Where the document meets the renderer.** The crate table deliberately gives
`xarast-render` no dependency on `xarast-doc`: the renderer consumes a scene,
not a document. Walking the arena to build that scene therefore lives in
`xarast-app`, which already depends on both. This keeps the renderer testable
from hand-built scenes with no document present, and it is why the display list
is the contract between the two rather than the node tree.

**Rule:** nothing below `xarast-app` may depend on a UI toolkit. `xarast-doc`
and `xarast-render` must build and run with no windowing system present —
that is what makes CI rendering tests possible.

---

## 3. Decisions arbitrated here

### 3.1 Live store: arena, not a persistent tree

`research/05` proposed that the live document *be* a persistent (HAMT)
structure; `research/02` argued for a generational arena with persistent
snapshots layered on top. **The arena wins.**

Render traversal, hit-testing and layout touch nodes millions of times per
second by id. A `SlotMap` lookup is an index plus a generation check; a HAMT is
two to five pointer hops with the cache misses to match. Undo does not need the
live store to be persistent: an inverse-action log with periodic snapshots
delivers the same user-visible behaviour (O(1) undo, history branches, undo
after reopening) *and* lets us budget history in bytes rather than steps, which
is what actually scales on real documents.

- Live store: `SlotMap<NodeId, NodeData>`, `NodeData { parent, first_child, next_sibling, prev_sibling, flags, kind: NodeKind, .. }`.
- Heavy payloads (`long paths, bitmaps, gradient ramps, font data`) behind `Arc<T>`, copy-on-write.
- Checkpoints: `imbl::HashMap<NodeId, Arc<NodeData>>` snapshots for autosave and
  session-persistent history, built *over* the arena.

**Measured in Phase 2, and the arena stands.** The benchmark walks a
100,000-node synthetic document in both representations
(`crates/xarast-doc/examples/arena_vs_persistent.rs`), against the decision
rule pre-registered in `docs/phases/phase-02-document-model.md` §W2.11. The
HAMT lost every condition it had to win: 11× slower on traversal, 15× slower
on lookup, and 7× *slower* on edit-then-undo rather than 5× faster; it was
only inside the memory allowance. The numbers are in
`docs/memory/document-model.md`. Question 1 of §7 is closed.

### 3.2 Node typing: enum, not trait objects

`enum NodeKind` with exhaustive `match`, not `Box<dyn Node>`. The original's
~60 virtual `IsXxx()` predicates are exactly what we are trying not to inherit:
they cannot be exhaustively checked, they put `dyn` dispatch on the hottest
path, and they make `Clone` and serialisation painful. When a new node type
forces us to touch every `match`, that is the type system doing its job.

### 3.3 Render: our engine, vello as the coverage rasteriser

No third-party library becomes "the engine". None of them has Xara's blend
modes, its conical and diamond gradients, or its non-linear ramp profiles. What
*is* worth reusing — and expensive to write — is a high-quality antialiased
coverage rasteriser. `vello`'s sparse-strip representation maps closely onto
the same concept CDraw used, and leaves the compositor ours.

- One scene/display-list frontend, two backends: `vello` on `wgpu` (GPU) and
  `vello_cpu` (CPU).
- The CPU backend is not a fallback afterthought. It is the **deterministic**
  path: export and golden-image tests always go through it, and comparing the
  two backends pixel by pixel is itself a test.
- Xara's exotic blend modes are custom WGSL compositing passes on GPU, mirrored
  by a CPU compositor.
- `skia-safe` is rejected: 30–60 MB and a C++ toolchain would recreate the very
  `libCDraw.a` problem we are solving.
- `tiny-skia` stays as a validation reference in tests, never a production
  dependency.

### 3.4 Coordinates: millipoints at the boundary, floats inside

Xara's integer millipoints are kept for the document model and both file
formats: they give determinism, exact equality, perfect `.xar` round-trip and
no NaN. The render pipeline converts to `f32`/`f64` once, at the scene-building
boundary. Geometry operations that need continuous maths (boolean ops,
offsetting, flattening) work in `f64` and quantise on the way back.

### 3.5 `.xar` is read-only — a correction

`research/04` lists `.xar` export in the MVP. That contradicts
`docs/00-vision-and-scope.md`, which makes writing `.xar` an explicit non-goal.
**The non-goal stands.** Writing a format we only understand by observation
invites silently corrupt files, and the interoperability need is served by SVG
and PDF export. Reconsider only if users actually ask for it.

### 3.5b Session state lives in `xarast-app`, not `xarast-doc`

Selection, the text caret, the insertion point, tool state and viewport all
live in an `EditState` owned by `xarast-app`. They are session state: never
serialised, never undone, never part of the document's identity. Two documents
with different selections are the same document.

Putting them in the arena — as the original does, with a `Selected` bit in the
node flags and a caret node in the tree — contaminates undo, serialisation,
copying and every traversal. Phase 5's draft placed `EditState` in
`xarast-doc`; it belongs in `xarast-app`, which is also where the crate table
already puts selection and viewport.

### 3.6 Attribute model: lexical scope, kept

The original's attribute scoping — an attribute node applies to its following
siblings within a child list — is not an accident of 1990s design. It is what
makes grouping and ungrouping preserve appearance, and what keeps files
compact. We keep the semantics, with an explicit push/restore attribute stack
during traversal, and drop the ~40 redundant parallel classes for colour and
transparency in favour of one generic `FillGeometry` over its payload.

---

## 4. Data flow

**Opening a document.** Filter → `DocumentBuilder` → arena. Importers never
touch the arena directly; they emit a build script the model validates. An
importer cannot create an inconsistent document.

**Editing.** Every mutation is a `Command` on the bus. A command produces its
inverse before applying, which is what the undo log stores. Tools never mutate
the arena directly — that rule is what makes undo complete by construction.

**Rendering.** The arena is walked once per dirty region into an immutable
display list; the display list is tiled; tiles are rasterised in parallel. Per
node render caches are keyed by `(node, resolution, variant)` so that dragging
does not regenerate untouched subtrees.

**Saving.** Arena → SVG profile serialiser → container writer. Unknown data
preserved from a previous load is re-emitted verbatim.

---

## 5. Threading

| Thread | Work |
|---|---|
| Main | Window, input, UI, command dispatch |
| Render | Scene build and rasterisation; owns the GPU queue |
| Pool (`rayon`) | Tile rasterisation, image decode, boolean ops, text shaping |
| I/O | Load, save, autosave, thumbnailing |

The document is owned by the main thread. The render thread gets an immutable
snapshot of the display list, never the arena. This keeps `xarast-doc` free of
locks entirely.

---

## 6. Testing strategy

| Layer | How it is tested |
|---|---|
| `.xar` parser | The 59-file corpus must parse with zero errors; `cargo-fuzz` on the record layer; no panics on corrupt input |
| Document model | Property tests on tree invariants; undo/redo round-trip returns an identical document |
| Render | Golden images per feature; GPU vs CPU backend pixel comparison; perceptual diff threshold |
| `.xarast` | Round-trip equality including unknown-data preservation; the produced SVG validates |
| Performance | `criterion` benchmarks with budgets that fail CI on regression |

---

## 7. Open questions

| # | Question | Decided by |
|---|---|---|
| 1 | ~~Arena vs persistent store — confirm by measurement~~ **Closed in Phase 2: the arena wins on every measure but memory, where it also wins.** | Phase 2 benchmark |
| 2 | `egui` immediate mode at professional panel density | Phase 5 UI spike |
| 3 | `winit 0.31-beta` pinning for tablet pressure — is the beta stable enough | Phase 5 |
| 4 | Whether text on a path needs our own layout pass over `parley` | Phase 9 |
| 5 | Is a perceptual diff or exact match the right golden-image gate | Phase 4 |

Questions 2 to 5 were originally filed against the wrong phases; the phase
documents that actually answer them are the ones listed above. Question 1 moved
from phase 1 to phase 2 because the document model, not the geometry crate, is
what the benchmark measures, and Phase 2 answered it.
