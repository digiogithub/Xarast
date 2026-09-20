# document-model

Memory note for the **document model** subsystem.
Full research in [`../research/02-document-model.md`](../research/02-document-model.md).
Arbitrated decisions in [`../10-architecture.md`](../10-architecture.md) §3.1, §3.2, §3.6.

> Crate name: the research note called it `xarast-model`; the architecture
> document settles on **`xarast-doc`**. Use `xarast-doc`.

## Current state

- Research on the Xara LX model is **complete**: node hierarchy, the
  document/chapter/spread/page/layer tree, the attribute system, fills, live
  composites, text, bitmaps, selection and undo.
- **No Rust written yet.** The design proposal (§10 of the research document)
  is ready to implement in phase 2.

## Decisions taken (and why)

1. **Arena `slotmap::SlotMap<NodeId, NodeData>` + `enum NodeKind`** — not
   inheritance, not trait objects, not ECS. The document is a hierarchical tree
   with strict paint order; ECS fits neither the lexical attribute scope nor
   the ordering. The original's ~60 virtual `IsXxx()` predicates
   (`node.h:460-504`) are evidence that its hierarchy was standing in for sum
   types.
2. **Sibling linked list** (`parent/prev/next/first_child/last_child`) rather
   than `Vec<NodeId>`: insert, delete, move and reorder dominate in a vector
   editor and are O(1) this way. We add `last_child`, which Xara lacks (it
   walks), because the importer appends in bulk.
3. **`NodeHidden` is gone.** Delete = `detach` + a `DETACHED` flag; the node
   stays alive in the arena, retained by the history `Transaction`. This
   removes `HiddenRefCnt`, `FindNextNonHidden`, `IsOrHidesAnAttribute`,
   `HidingNode`/`ShowingNode`/`ComplexHide` and
   `KernelBitmapRef::RemoveFromTree`/`AddtoTree`.
4. **Attributes REMAIN NODES** (`NodeKind::Attr`). Debated and settled: it is
   the only representation that preserves lexical scope — "this attribute
   affects the following siblings and their subtrees" — which is exactly the
   semantics of both `.xar` **and** SVG (`<g>` with presentation properties). A
   per-node attribute map would break round-tripping. An `AttrResolver` with a
   cache sits on top and gives O(1) queries.
5. **`AttrStack` mirrors `CurrentAttrs` + `RenderStack`**: a dense table indexed
   by `AttrSlot`, an undo log of `(slot, previous_value)`, and level marks.
   `push_scope()`/`pop_scope()` on descending into and leaving a child list.
   Roughly 60 lines replace `rndrgn.cpp:7000-7150` plus `rndstack.cpp`.
6. **One generic `FillGeometry<S: Stop>`** instead of the colour/transparency
   duplication (~40 classes in `fillattr2.h` plus 20 in `fillval.h`).
   `Perspective` is an `Option<…>`, not two points and a `BOOL IsPersp`.
7. **Undo is an inverse-action log** (Xara's model), not a persistent structure
   as the live store. Settled in architecture §3.1.
8. **Selection lives outside the nodes**: an `IndexSet<NodeId>` in `EditState`,
   not a bit in `NodeFlags`. Likewise the text caret (`CaretNode` outside the
   tree) and the insertion point (`InsertionNode` outside the tree).
9. **Control-point selection lives outside `PathData`**, in an overlay
   `HashMap<NodeId, BitVec>`, so geometry stays comparable with `==` and
   selecting a point does not break `Arc<PathData>` copy-on-write.
10. **Caches unified under a state-derived key**: `BoundsCache` with a per-node
    epoch propagating upward with early cut-off; `RasterKey` carrying a
    `state_hash` instead of the `m_Last*` fields of `NodeShadow`/`NodeBevel`.
11. **`xarast-doc` has no graphics dependencies.** Required for testing without
    a GPU and for the importer fuzzer in CI.

## Invariants that must not be broken

1. The tree is acyclic; `next`/`prev` links are reciprocal; every child points
   at the same parent; `first_child` has no `prev` and `last_child` has no
   `next`.
2. `DETACHED` is transitive downward — nothing under a detached node is
   reachable from the root.
3. Every `LiveRole::Generated` node has a `LiveRole::Controller` ancestor of the
   same `LiveKind`, and every controller has exactly one `Source` subtree.
4. Attributes come before the first ink node within a child list. This is
   *desirable, not mandatory*: **the importer must accept files that violate
   it** — real `.xar` files do.
5. One spread has exactly one active layer.
6. `Tag` is unique and stable per document; `by_tag` is bijective with the live
   nodes.
7. If a node's bounding box is invalid, so is every ancestor's.
8. Every referenced `BitmapId`/`PaletteId`/`BrushId` exists in
   `DocumentResources`.
9. The selection contains only nodes reachable from the root.
10. `TextItem` only under `TextLine`; `TextLine` only under `TextStory`.
11. Coordinates are always millipoint `i32` in the model, never `f64`. The only
    `f32` allowed are colour values and the `0..1` position of ramp stops.

## Dead ends (do not retry)

- **Translating the hierarchy with `Box<dyn Node>` + `Any`**: reproduces the
  original's constant downcasting and makes exhaustive `match` impossible.
- **`Rc<RefCell<Node>>`**: `BorrowMut` panics in traversals that go both down
  and up (rendering returns to the parent after the children), parent/child
  cycles, and not `Send`.
- **A per-node attribute map with no attribute nodes**: loses list scope and
  breaks round-tripping with both `.xar` and SVG. Rejected after analysis.
- **A global `Epoch` for bounds invalidation**: invalidates everything on every
  edit, worse than Xara's targeted `InvalidateBoundingRect`. Use a per-node
  epoch with upward propagation and early cut-off.
- **Putting the text caret, the selection or the insertion point in the tree**
  (as Xara does with `CaretNode`, `NodeFlags::Selected` and `InsertionNode`):
  it contaminates undo, serialisation, copying and traversal.

## Open TODOs

- [ ] Benchmark arena vs `imbl` over a 100,000-node traversal. Architecture
      §3.1 chose the arena provisionally; this measurement confirms or
      overturns it. Record the result here.
- [ ] Settle the exact `AttrSlot` set against the 211 tags in
      `Kernel/cxftags.h` (see `research/01-xar-format.md`).
- [ ] Decide whether blend intermediate steps are materialised as nodes or
      generated at render time. Xara does the latter; it affects hit-testing.
- [ ] Define `ProceduralSource` (fractal/noise) and its cache hash.
- [ ] `Tree::validate()` plus `proptest` property tests over attach/detach/move
      sequences — before the importer is written.
- [ ] Size test: `size_of::<NodeData>() <= 64`.
- [ ] Decide the `MouldGeometry` model (trait vs enum) when live effects land.
