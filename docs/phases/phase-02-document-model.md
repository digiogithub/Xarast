# Phase 2 — Document model

> After this phase there is a document: a node arena with stable ids, lexically
> scoped attributes, layers and pages, a resource table, and an undo log that is
> complete by construction because nothing can mutate the arena except through
> it.

## Goal

Ship `xarast-doc`, the crate that every importer emits into, the scene builder
traverses, and every tool mutates. (The renderer itself never sees the arena:
`10-architecture.md §2` keeps `xarast-render` free of a `xarast-doc` dependency
and puts scene building in `xarast-app`. This crate's traversal API is what that
scene builder is written against.) It implements the arena decision of
`docs/10-architecture.md §3.1` and the attribute-scope decision of `§3.6`, and
it settles the arena-versus-persistent-store question with a benchmark rather
than an argument.

Four properties are the point of the phase:

1. **Node identity is stable and cheap.** A generational key survives deletion,
   undo and reattachment, and resolving one is an index plus a generation check.
2. **Attributes keep their lexical scope.** An attribute node applies to its
   following siblings and their subtrees. This is what makes group/ungroup
   preserve appearance and what keeps both `.xar` and SVG round-trippable.
3. **Undo is complete by construction.** The arena's mutating API is crate
   private. The only way to change a document is a `Command`, and a `Command`
   produces its inverse before it applies.
4. **Importers cannot build an invalid document.** They emit into
   `DocumentBuilder`, which validates; they never see the arena.

## Scope

### In scope

- `Tree`: `SlotMap<NodeId, NodeData>`, `Tag`, sibling-list links, flags,
  attach/detach/move/destroy, `validate()`.
- `NodeKind`: the sum type replacing the original's class hierarchy, with the
  variants Phases 2–5 need and the rest stubbed as `#[non_exhaustive]`-adjacent
  placeholders.
- Traversals: preorder, postorder, children, ancestors, and `walk_render` with
  scope events and pruning.
- The attribute model: `AttrSlot`, `AttrValue`, `AttrNode`, `AttrStack`,
  `ResolvedAttrs`, `AttrResolver`, `DefaultAttrs`.
- `FillGeometry<S: Stop>` and `Paint`: one generic fill type over colour and
  transparency payloads.
- Structural nodes: document, chapter, spread, page, layer, grid, and the
  canonical tree shape.
- `DocumentResources`: colour table, bitmap table, dash and arrow tables, with
  `Arc` payloads, content-hash deduplication and orphan collection.
- `Action`, `Tx`, `Command`, `CommandBus`, `History` with a **byte** budget,
  coalescing, and periodic `imbl` checkpoints.
- `DocumentBuilder`: the only public construction path, used by every importer.
- `Document::canonical_digest()`, the structural equality primitive that the
  undo round-trip test depends on.
- **The arena benchmark** that confirms or overturns `10-architecture.md §3.1`.

### Explicitly out of scope (and which phase owns it)

| Out of scope | Owner |
|---|---|
| Reading a `.xar` byte | Phase 3 |
| Writing `.xarast` | Phase 6 |
| Rendering, display lists, raster caches | Phase 4 |
| Live objects (blend, mould, contour, shadow, bevel, ClipView) — the `NodeKind::Live` variant exists and round-trips, but `regenerate()` does not | Phase 13 |
| Text layout. `TextStory`/`TextLine`/`TextItem` exist as tree structure; shaping and measurement do not | Phase 9 |
| Bitmap decoding. `BitmapResource` holds bytes and metadata; `xarast-image` decodes them | Phase 10 |
| Selection, tools, the edit session (`EditState` lives in `xarast-app`, deliberately outside the document) | Phase 5 / Phase 7 |
| Persistent history across save/reload | Phase 6 |
| `AttrResolver` cache *invalidation tuning*; a correct but conservative invalidation ships here | Phase 4 |
| QuickShape geometry generation (the node stores its parameters; turning them into a path) | Phase 7 |

## Prerequisites

- **Phase 0** closed: workspace, CI, `xarast-testkit`, `criterion` and
  `proptest` available.
- **Phase 1's public API agreed**, not necessarily finished. `xarast-doc`
  depends on `xarast-geom` (`Mp`, `Point`, `Rect`, `Matrix`, `Path`,
  `BiasGain`) and `xarast-color` (`Colour`, `Transparency`, `ColourTable`,
  `Stop`, `FillEffect`). The two phases run in parallel
  (`docs/phases/00-roadmap.md`), so the signatures in
  `docs/phases/phase-01-geometry-and-colour.md §Public API` are the contract; if
  Phase 1 needs to change one, it changes there first.

## Workstreams

### W2.1 — Arena and tree

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 2.1.1 | `NodeId` (generational), `Tag`, `Links`, `NodeFlags`, `NodeData` | `xarast-doc` | M | — |
| 2.1.2 | `Tree`: `create`, `attach`, `detach`, `move_node`, `destroy_subtree`, `anchor_of` | `xarast-doc` | L | 2.1.1 |
| 2.1.3 | `by_tag` index and `Tag` allocation | `xarast-doc` | S | 2.1.1 |
| 2.1.4 | `Tree::validate()` covering every invariant | `xarast-doc` | M | 2.1.2 |
| 2.1.5 | `size_of::<NodeData>() <= 64` test and the `Box`-the-large-variants discipline | `xarast-doc` | S | 2.1.1 |
| 2.1.6 | `proptest` suite over random attach/detach/move sequences | `xarast-doc` | M | 2.1.4 |

`NodeData` is `{ tag: Tag, links: Links, flags: NodeFlags, bounds: BoundsCache,
kind: NodeKind }`. `Links` is `{ parent, prev, next, first_child, last_child }`,
each `Option<NodeId>` — which costs nothing extra, because `slotmap`'s key has a
niche, so `Option<NodeId>` is the same size as `NodeId`.

`last_child` is the one field the original does not have (it walks to find it).
We keep it because the importer appends children in bulk and appending must be
O(1); `research/02 §10.3` makes the same call.

**Why a linked sibling list and not `Vec<NodeId>`.** Insert, delete, move and
reorder are the dominant operations in a vector editor and are O(1) on a linked
list, O(n) with a memmove on a `Vec`. Traversal is a pointer hop per node
instead of a contiguous scan, which is the cost we pay; if profiling later shows
traversal dominating, the answer is an arena laid out in paint order, not a
different logical structure.

**Deletion is `detach`, not destruction.** There is no `NodeHidden` and no
hidden-reference counter. A deleted node keeps its `NodeId` and its subtree,
gains `NodeFlags::DETACHED`, and is retained by the `Transaction` that deleted
it. Real destruction happens only when the history evicts that transaction. This
single change removes an entire subsystem from the original design
(`research/02 §10.3` lists the nine functions it replaces).

**The `size_of::<NodeData>() <= 64` test is a real gate, not a wish.** A
document with text is mostly `TextItem` nodes; if `NodeKind`'s size is set by
its largest variant, every character pays for a `SpreadNode`. Large variants are
`Box`ed. When the test fails, the fix is to box a variant, never to raise the
limit.

### W2.2 — `NodeKind`

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 2.2.1 | The `NodeKind` enum and its payload structs | `xarast-doc` | L | 2.1.1 |
| 2.2.2 | Derived predicates: `is_ink`, `is_paper`, `is_attr`, `is_compound`, `needs_parent` | `xarast-doc` | S | 2.2.1 |
| 2.2.3 | `compute_bounds` as a single `match` | `xarast-doc` | M | 2.2.1 |
| 2.2.4 | `BoundsCache` with per-node epoch and upward propagation with early cut-off | `xarast-doc` | M | 2.2.3 |

`10-architecture.md §3.2` fixes this: an `enum` with exhaustive `match`, not
`Box<dyn Node>`. The consequence to embrace rather than work around is that
adding a node type makes every `match` fail to compile — which is the type
system listing the places that need a decision.

The discipline that makes the enum pay off: **one function per behaviour, not
one method per variant.** `compute_bounds`, `describe`, `serialise` and
`hit_test` are each a single free function with one `match`, so the whole
behaviour of an aspect is readable on one screen. That is precisely what the
original's inheritance prevented.

`BoundsCache` uses a per-node epoch with upward propagation and early cut-off:
invalidating a node marks its ancestors invalid and stops as soon as it meets an
ancestor already invalid. A single global epoch was tried on paper and rejected
(`research/02` dead ends): it invalidates everything on every edit, which is
worse than the original's targeted invalidation.

### W2.3 — Traversal

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 2.3.1 | `children`, `ancestors`, `preorder`, `postorder` iterators | `xarast-doc` | M | 2.1.2 |
| 2.3.2 | `walk_render` emitting `WalkEvent` | `xarast-doc` | M | 2.3.1 |
| 2.3.3 | `Descend` pruning control | `xarast-doc` | M | 2.3.2 |
| 2.3.4 | Traversal benchmarks | `xarast-doc` | S | 2.3.1 |

`walk_render` is the primitive the renderer, hit-test and text layout all use.
It emits `EnterScope { parent }` before a child list, `Visit { node }` per node,
and `LeaveScope { parent }` after — which is exactly what the consumer needs to
keep the attribute stack correct with no bookkeeping of its own. Postorder is
the ink paint order (children before the parent's own ink), matching the
original's depth-first order.

`Descend` gives the caller pruning: `Skip` (do not enter), `SelfOnly`,
`SelfAndChildren`, `JumpTo(NodeId)` (a render cache hit, resume at a later
node) and `RunTo(NodeId)` (advance while maintaining the attribute stack). These
map one-to-one onto the original's `SubtreeRenderState` and exist because
Phase 4 needs them; building them now costs an afternoon and retrofitting them
later costs a rewrite of the traversal.

### W2.4 — Attributes

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 2.4.1 | `AttrSlot` enum and `ATTR_SLOT_COUNT` | `xarast-doc` | M | — |
| 2.4.2 | `AttrValue` and its classification methods | `xarast-doc` | L | 2.4.1, W2.5 |
| 2.4.3 | `AttrStack`: dense table, undo log, scope marks | `xarast-doc` | M | 2.4.2 |
| 2.4.4 | `ResolvedAttrs` snapshot | `xarast-doc` | S | 2.4.3 |
| 2.4.5 | `AttrResolver` with cache and invalidation | `xarast-doc` | M | 2.4.4 |
| 2.4.6 | `DefaultAttrs` and the document's default attribute block | `xarast-doc` | M | 2.4.2 |
| 2.4.7 | Multi-applicable attributes (`MultiAttr`) | `xarast-doc` | S | 2.4.2 |

**Attributes are nodes.** This was the contested decision and it is settled
(`10-architecture.md §3.6`, `research/02 §10.6`). Three representations were
weighed: attribute-as-node (faithful, exact round-trip, scope preserved, but
resolution needs a walk); a per-node attribute map (O(1) lookup but **loses list
scope**, so a loose attribute on a layer that affects its following siblings
cannot be represented, and re-export inflates the file); and the hybrid. We take
the **hybrid**: attribute nodes as the source of truth, plus `AttrResolver`,
a cache of `ResolvedAttrs` per ink node.

The scope machinery is small and must stay small:

- `AttrStack` holds a **dense array indexed by `AttrSlot`** of the currently
  effective value, a `Vec<(slot, previous)>` undo log, and a `Vec<ScopeMark>` of
  `(undo_len, multi_len)` pairs.
- `push(value)` replaces the slot and pushes the old value onto the undo log.
- `push_scope()` records the marks; `pop_scope()` unwinds the undo log back to
  the mark. Both are O(1) plus the unwind, which is proportional to the number
  of attributes actually set in that scope.
- Multi-applicable attributes (object names, user key/value pairs) do not own a
  slot; they accumulate in a `Vec` truncated by `pop_scope`.

That is roughly sixty lines, and it replaces the original's render stack, its
current-attribute table and its attribute-node dispatch entirely.

**The slot set is not yet final.** `research/02 §10.6` lists 46 slots. The
memory note's open TODO is to reconcile that list against the 211 `.xar` tags
before Phase 3 depends on it. This phase does the reconciliation: every `.xar`
attribute tag in `research/01 §8` must map to exactly one `AttrSlot` or be
explicitly listed as multi-applicable or ignorable, and the mapping table goes
in `docs/memory/document-model.md`. If the count changes from 46, it changes
here, once, before the importer is written against it.

**Invariant 5 is deliberately weak.** "Attribute nodes come before the first ink
node in a child list" is *desirable*, not required: real `.xar` files violate it
(`research/02 §10.16`). `validate()` reports it as a warning, never an error,
and `DocumentBuilder` must accept it.

### W2.5 — Fills and paints

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 2.5.1 | `FillGeometry<S: Stop>` with all shape variants | `xarast-doc` | L | Phase 1 `Stop` |
| 2.5.2 | `Ramp<S>`, `RampStop<S>`, ordering invariant | `xarast-doc` | S | 2.5.1 |
| 2.5.3 | `Perspective`, `Tiling`, `RampMapping` | `xarast-doc` | S | 2.5.1 |
| 2.5.4 | `Paint` = `FillGeometry<Colour>`, `TranspPaint` = `FillGeometry<Transparency>` | `xarast-doc` | S | 2.5.1 |
| 2.5.5 | Control-point accessors, `has_control_points`, `transform` | `xarast-doc` | M | 2.5.1 |

One generic type replaces roughly sixty parallel classes in the original
(`research/02 §10.7`): `FillGeometry<S: Stop>` with variants `Flat`, `Linear`,
`Radial`, `Conical`, `Diamond`, `ThreeColour`, `FourColour`, `Bitmap`,
`Fractal`, `Noise`. Instantiated at `Colour` it is a colour fill; instantiated
at `Transparency` it is a transparency fill. Xara has had gradients of blend
mode since 1995 and this is how we get them without writing everything twice.

`Perspective` is an `Option<…>`, not "two extra points plus a boolean". Ramp
stops are the **intermediate** stops only — the endpoints live in `from` and
`to`, matching the format and the original (`research/02 §5.6`) — and the vector
is kept sorted by position, which `Ramp::insert` maintains and `validate()`
checks. `ThreeColour` and `FourColour` carry no ramp, by construction rather
than by convention.

### W2.6 — Structure: spreads, pages, layers

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 2.6.1 | `DocumentNode`, `SpreadNode`, `PageNode`, `LayerNode`, `GridNode` | `xarast-doc` | M | 2.2.1 |
| 2.6.2 | Canonical tree construction (`Document::new_empty`) | `xarast-doc` | M | 2.6.1 |
| 2.6.3 | Layer flags, active-layer invariant, guide layers, page-background layers | `xarast-doc` | M | 2.6.1 |
| 2.6.4 | Spread coordinate origin and page geometry | `xarast-doc` | S | 2.6.1 |
| 2.6.5 | `DocumentMeta` | `xarast-doc` | S | — |

The canonical shape (`research/02 §3.1`) is
document → chapter → spread → (page, grid, layers) → objects, with the default
attribute block hanging directly under the document node. `Document::new_empty`
builds it, and every importer starts from it rather than inventing a shape.

Layer flags come straight from `TAG_LAYERDETAILS`
(`research/01 §4.3`): visible, locked, printable, active, page background,
background. `SpreadNode` carries width, height, pasteboard margin, bleed, the
double-page-spread flag and the page-shadow flag. On the double-page flag,
`research/01 §11` item 3 records that the original's import handler reads bit 0
while its debug printer reads bit 2 — we follow the **handler**, bit 0, and
treat bit 2 as unknown.

Coordinate origin: `research/01 §5.3` says record coordinates are relative to
the `lo` corner of the rectangle enclosing all pages of the spread. That
translation is applied by the **importer** (Phase 3), not stored here; the
document model holds absolute document coordinates with Y up. `SpreadNode`
exposes `coord_origin()` so the importer has one place to ask.

### W2.7 — Resources

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 2.7.1 | `DocumentResources` with the colour, bitmap, dash and arrow tables | `xarast-doc` | M | — |
| 2.7.2 | `BitmapResource` with `Arc` payloads and original-bytes preservation | `xarast-doc` | M | 2.7.1 |
| 2.7.3 | Content-hash deduplication | `xarast-doc` | M | 2.7.2 |
| 2.7.4 | `collect_unused` orphan sweep | `xarast-doc` | M | 2.7.1, W2.8 |

Every heavy payload sits behind an `Arc` and is copy-on-write via
`Arc::make_mut`: path data, bitmap pixels, colour ramps, dash patterns, names.
Cloning a node for undo, for the clipboard or for a blend step then copies a
handful of pointers.

`BitmapResource::original` keeps the **encoded source bytes** when we have them.
This is not an optimisation, it is a fidelity requirement: re-encoding an
embedded JPEG on save loses quality every round trip. If the bytes are there,
the writer emits them verbatim.

Deduplication is by SHA-256 of the decoded payload, held in a
`HashMap<[u8;32], BitmapId>`. Reference counting is `Arc`'s job; **liveness** is
a sweep, run on save and on history eviction, not on every edit — because a node
detached by undo still holds its resources and must keep them.

### W2.8 — Commands, actions, undo

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 2.8.1 | `Action` with `apply`, `inverse`, `size_hint` | `xarast-doc` | L | 2.1.2 |
| 2.8.2 | `Tx` builder with rollback on drop | `xarast-doc` | M | 2.8.1 |
| 2.8.3 | `Transaction` with retained nodes and a byte cost | `xarast-doc` | M | 2.8.2 |
| 2.8.4 | `History`: undo, redo, byte budget, eviction, coalescing | `xarast-doc` | L | 2.8.3 |
| 2.8.5 | `Command` trait and `CommandBus` | `xarast-doc` | M | 2.8.4 |
| 2.8.6 | `Snapshot` checkpoints over `imbl` | `xarast-doc` | M | 2.8.4 |
| 2.8.7 | Undo round-trip property test | `xarast-doc` | M | 2.8.5 |

**The inverse is computed before the action applies** (`10-architecture.md §4`),
which is why `Action::inverse(&self, doc: &Document) -> Action` takes the
document: a `SetKind` inverse needs the old kind, and after applying it is gone.

**`Tx` rolls back on drop.** If a command fails halfway with `?`, the `Drop` impl
undoes the actions already applied. That removes the original's manual
fail-and-execute dance and makes a half-applied command structurally impossible.

**The budget is in bytes, not steps.** `Action::size_hint()` estimates the
retained cost — for `Detach`, the whole retained subtree; for `SetAttr`, the
value. `History` evicts oldest-first when over budget, and eviction is what
finally destroys the nodes that transaction retained. A step count is the wrong
unit: a hundred nudges cost nothing and one "delete the 40 MB bitmap layer"
costs everything.

**Coalescing.** `Command::coalesce_key()` returns `Option<CoalesceKey>`; two
consecutive transactions with an equal key and within a time window merge into
one. This is how a fifty-event drag becomes one undo step. The coalescing window
and whether it is time-based or gesture-based is **to be determined in this
phase**, decided by trying both against the drag interaction in the Phase 5
prototype — until then, gesture-based (the tool explicitly begins and ends the
coalescing group) is the default, because it does not depend on wall-clock
timing and is therefore testable.

**Checkpoints.** Every `N` transactions the history records an
`imbl::HashMap<NodeId, Arc<NodeData>>` snapshot. This is the autosave source and
the future persistent-history source (Phase 6). It is a *snapshot over* the
arena, never the live store — that distinction is the whole of decision §3.1.

**The completeness test.** Generate a random sequence of 1 000 commands against
a seeded document, record `canonical_digest()` before and after each, then undo
everything and assert the digest matches the original; then redo everything and
assert it matches the post-sequence digest. This is the test that catches a
wrong inverse, and a wrong inverse is the failure mode that corrupts documents
silently.

### W2.9 — `DocumentBuilder`

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 2.9.1 | `DocumentBuilder` scope stack and node emission | `xarast-doc` | L | W2.1–W2.7 |
| 2.9.2 | Definition tables (colours, bitmaps, dashes, arrows) and reference resolution | `xarast-doc` | M | 2.9.1 |
| 2.9.3 | `Diagnostic` and the severity policy | `xarast-doc` | S | 2.9.1 |
| 2.9.4 | Resource limits: depth, node count, total bytes | `xarast-doc` | M | 2.9.1 |
| 2.9.5 | `Tree`'s mutating API made crate private | `xarast-doc` | S | 2.9.1 |

`10-architecture.md §4`: *importers never touch the arena directly; they emit a
build script the model validates; an importer cannot create an inconsistent
document.* Making that true requires one unglamorous step — 2.9.5 — that is easy
to skip and that the whole guarantee rests on. `Tree::attach` and friends are
`pub(crate)`. The public mutation surface is exactly `DocumentBuilder` (during
construction) and `CommandBus` (afterwards).

The builder mirrors the importer's shape: `push_scope()`/`pop_scope()` are what
`TAG_DOWN`/`TAG_UP` become, `node()` and `attribute()` append at the current
level, and `define_*` registers a referenceable definition. Unbalanced scopes
are tolerated and reported — truncated files are real — and `finish()` closes
any scopes still open.

**Resource limits belong here, not in the importer**, because every importer
needs them and only one place should decide: maximum tree depth (default 256;
the deepest corpus file is 13 levels, `research/01 §12.2`), maximum node count
(default 20 000 000), maximum total retained bytes (default 2 GiB), maximum
points in a single path (default 16 000 000). Exceeding a limit is a hard error,
not a diagnostic. These are the numbers that make "bounded allocation on hostile
input" (Phase 3's fuzz invariant) achievable at all.

### W2.10 — Validation and property testing

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 2.10.1 | `Tree::validate()` and `Document::validate()` covering all 12 invariants | `xarast-doc` | M | W2.1–W2.7 |
| 2.10.2 | `Document::canonical_digest()` | `xarast-doc` | M | 2.2.1 |
| 2.10.3 | `proptest` strategies that generate arbitrary valid documents | `xarast-doc` | L | 2.10.1 |
| 2.10.4 | `insta` snapshots of the tree dump format | `xarast-doc` | S | 2.10.2 |

`canonical_digest()` is a SHA-256 over a canonical visit of the document:
structure, kinds, payloads and resources, excluding caches, `NodeId` values
(which are allocation-order dependent) and anything session-scoped. It is the
primitive behind the undo round-trip test, the `.xarast` round-trip test
(Phase 6) and the "did this refactor change anything" check.

### W2.11 — The arena benchmark

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 2.11.1 | Synthetic 100 000-node document generator | `xarast-doc` | M | W2.1 |
| 2.11.2 | `imbl`-backed reference implementation of the same tree API | `xarast-doc` (bench only) | L | 2.11.1 |
| 2.11.3 | The four measurements, with the decision rule pre-registered | `xarast-doc` | M | 2.11.2 |
| 2.11.4 | Record the outcome in `docs/memory/document-model.md` and, if it overturns, amend `10-architecture.md §3.1` | — | S | 2.11.3 |

`10-architecture.md §7` question 1 assigns this benchmark to Phase 2; the prose
of `§3.1` still says "Phase 1" and is stale. The benchmark measures a document
model that does not exist before this phase, so Phase 2 owns it. Fix the
sentence in `§3.1` in the same commit, and record the correction in the memory
note.

The benchmark compares the shipping `SlotMap` arena against an `imbl::HashMap<NodeId,
Arc<NodeData>>` implementing the same tree operations, on a synthetic document
of 100 000 nodes with a realistic shape (a spread, six layers, groups nested to
depth six, an average of eight children per group, 40 % attribute nodes — the
proportions taken from the corpus statistics in `research/01 §12.2`).

Four measurements:

| M | What | Why it matters |
|---|---|---|
| A | Full `walk_render` traversal, nodes per second | The render hot path |
| B | 10 000 000 random lookups by `NodeId` | Hit-test and resolution |
| C | Single-node attribute edit, then undo, latency | The 1 ms budget |
| D | Resident bytes for the whole document | Large-document viability |

**The decision rule is pre-registered so the result cannot be rationalised after
the fact.** The arena stands unless *all three* hold: the HAMT is within 20 % of
the arena on A and B; the HAMT beats the arena by more than 5× on C; and the
HAMT's memory in D is within 1.5× of the arena's. Anything else and the arena
wins and the question is closed. Either way the numbers, not a summary, go into
`docs/memory/document-model.md`.

## Public API introduced

```rust
// ═══ xarast-doc ══════════════════════════════════════════════════════════════
// Note: `research/02` calls this crate `xarast-model`. `docs/10-architecture.md`
// names it `xarast-doc`, and the architecture document wins.

// ─── Identity ────────────────────────────────────────────────────────────────

slotmap::new_key_type! {
    /// Generational arena key. Stable across detach, undo and reattach; never
    /// serialised.
    pub struct NodeId;
}

/// Persistent identifier, stable across save and reload. This is what gets
/// serialised; `NodeId` does not.
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Tag(pub u32);

// ─── Tree ────────────────────────────────────────────────────────────────────

#[derive(Copy, Clone, Default, Debug, PartialEq, Eq)]
pub struct Links {
    pub parent: Option<NodeId>,
    pub prev: Option<NodeId>,
    pub next: Option<NodeId>,
    pub first_child: Option<NodeId>,
    pub last_child: Option<NodeId>,
}

bitflags::bitflags! {
    #[derive(Copy, Clone, Default, PartialEq, Eq, Debug)]
    pub struct NodeFlags: u16 {
        const LOCKED    = 1 << 0;
        const MARKED    = 1 << 1;   // transient traversal marking
        const MAGNETIC  = 1 << 2;   // participates in snapping
        /// Unlinked from the tree but alive in the arena. Replaces `NodeHidden`.
        const DETACHED  = 1 << 3;
    }
}
// There is deliberately no SELECTED flag: selection lives in `xarast-app`.

#[derive(Clone, Debug)]
pub struct NodeData {
    pub tag: Tag,
    pub links: Links,
    pub flags: NodeFlags,
    pub bounds: BoundsCache,
    pub kind: NodeKind,
}
// Gated by a test: size_of::<NodeData>() <= 64.

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Attach { Prev, Next, FirstChild, LastChild }

#[derive(Debug, thiserror::Error)]
pub enum TreeError {
    #[error("node {0:?} is not in the arena")]            NoSuchNode(NodeId),
    #[error("node {0:?} is already attached")]            AlreadyAttached(NodeId),
    #[error("attaching {child:?} under {anchor:?} would create a cycle")]
    WouldCycle { child: NodeId, anchor: NodeId },
    #[error("{kind} may not be a child of {parent_kind}")]
    IllegalParent { kind: &'static str, parent_kind: &'static str },
    #[error("tree depth limit {limit} exceeded")]         TooDeep { limit: usize },
}

#[derive(Debug)]
pub struct Tree { /* SlotMap<NodeId, NodeData>, root, by_tag, next_tag */ }

impl Tree {
    pub fn root(&self) -> NodeId;
    pub fn get(&self, id: NodeId) -> Option<&NodeData>;
    pub fn contains(&self, id: NodeId) -> bool;
    pub fn node_count(&self) -> usize;
    pub fn by_tag(&self, tag: Tag) -> Option<NodeId>;
    pub fn anchor_of(&self, id: NodeId) -> Option<(NodeId, Attach)>;
    pub fn depth_of(&self, id: NodeId) -> usize;
    pub fn is_ancestor(&self, ancestor: NodeId, of: NodeId) -> bool;

    // ── Mutation is pub(crate). The public paths are `DocumentBuilder` and
    //    `CommandBus`; this is what makes undo complete by construction. ──
    pub(crate) fn create(&mut self, kind: NodeKind) -> NodeId;
    pub(crate) fn attach(&mut self, id: NodeId, anchor: NodeId, how: Attach)
        -> Result<(), TreeError>;
    pub(crate) fn detach(&mut self, id: NodeId) -> Result<(), TreeError>;
    pub(crate) fn destroy_subtree(&mut self, id: NodeId) -> usize;
    pub(crate) fn get_mut(&mut self, id: NodeId) -> Option<&mut NodeData>;

    // ── Traversal ──
    pub fn children(&self, id: NodeId) -> Children<'_>;
    pub fn ancestors(&self, id: NodeId) -> Ancestors<'_>;
    pub fn preorder(&self, root: NodeId) -> Preorder<'_>;
    /// Ink paint order: children before the parent's own ink.
    pub fn postorder(&self, root: NodeId) -> Postorder<'_>;
    pub fn walk_render(&self, root: NodeId) -> RenderWalk<'_>;

    pub fn validate(&self) -> ValidationReport;
}

/// Scope events, so the consumer keeps the attribute stack correct for free.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum WalkEvent {
    EnterScope { parent: NodeId },
    Visit { node: NodeId },
    LeaveScope { parent: NodeId },
}

/// Pruning control, mirroring the original's subtree render states.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Descend { Skip, SelfOnly, SelfAndChildren, JumpTo(NodeId), RunTo(NodeId) }

impl RenderWalk<'_> {
    /// Applies to the node most recently returned by `next()`.
    pub fn control(&mut self, d: Descend);
}

#[derive(Clone, Debug, Default)]
pub struct ValidationReport { pub errors: Vec<Invariant>, pub warnings: Vec<Invariant> }

#[derive(Clone, Debug, PartialEq)]
pub enum Invariant {
    Cycle { at: NodeId },
    BrokenSiblingLink { a: NodeId, b: NodeId },
    ChildParentMismatch { child: NodeId, claims: Option<NodeId>, actual: NodeId },
    DetachedReachable { node: NodeId },
    GeneratedWithoutController { node: NodeId },
    /// Warning only: real `.xar` files violate it.
    AttrAfterInk { attr: NodeId },
    NoActiveLayer { spread: NodeId },
    DuplicateTag { tag: Tag },
    StaleBounds { node: NodeId },
    MissingResource { node: NodeId, resource: ResourceRef },
    TextItemOutsideLine { node: NodeId },
    ControllerWithoutSource { node: NodeId },
}

// ─── Node kinds ──────────────────────────────────────────────────────────────

/// Large variants are `Box`ed so that `NodeData` stays within a cache line.
#[derive(Clone, Debug)]
pub enum NodeKind {
    // structure
    Document(Box<DocumentNode>),
    Chapter,
    Spread(Box<SpreadNode>),
    Page(PageNode),
    Layer(Box<LayerNode>),
    Grid(GridNode),
    // geometry
    Path(PathNode),
    Shape(ShapeNode),
    QuickShape(Box<QuickShape>),
    Bitmap(BitmapNode),
    Guideline(GuidelineNode),
    // grouping and live objects
    Group(GroupNode),
    Live(Box<LiveNode>),     // structure + round-trip only until Phase 13
    ClipView(ClipViewNode),
    // text — structure only until Phase 9
    TextStory(Box<TextStoryNode>),
    TextLine(Box<TextLineNode>),
    TextItem(TextItem),
    // attributes
    Attr(Box<AttrNode>),
    /// Data from a newer producer we do not model, kept verbatim so that
    /// `.xarast` round-trip preserves it. Never rendered.
    Opaque(Box<OpaqueNode>),
}

impl NodeKind {
    /// Painted AFTER its children.
    pub fn is_ink(&self) -> bool;
    /// Painted BEFORE its children.
    pub fn is_paper(&self) -> bool;
    pub fn is_attr(&self) -> bool;
    pub fn is_compound(&self) -> bool;
    /// Cannot exist outside its controller.
    pub fn needs_parent(&self) -> bool;
    pub fn type_name(&self) -> &'static str;
}

#[derive(Clone, Debug)]
pub struct PathNode { pub data: std::sync::Arc<xarast_geom::Path>, pub filled: bool, pub stroked: bool }
impl PathNode { pub fn edit(&mut self) -> &mut xarast_geom::Path; }   // Arc::make_mut

#[derive(Clone, Debug)]
pub struct LayerNode {
    pub name: std::sync::Arc<str>,
    pub visible: bool,
    pub locked: bool,
    pub printable: bool,
    pub active: bool,
    pub page_background: bool,
    pub background: bool,
    pub guide: bool,
    pub guide_colour: Option<xarast_color::ColourId>,
    pub frame: Option<FrameProps>,
}

#[derive(Clone, Debug)]
pub struct SpreadNode {
    pub page_size: xarast_geom::Rect,
    pub margin: xarast_geom::Mp,
    pub bleed: xarast_geom::Mp,
    /// Bit 0 of the `.xar` spread flags; bit 2 is treated as unknown.
    pub double_page: bool,
    pub show_shadow: bool,
    pub anim: Option<Box<AnimProps>>,
}
impl SpreadNode {
    /// The `lo` corner of the rectangle enclosing every page of the spread:
    /// what `.xar` record coordinates are relative to.
    pub fn coord_origin(&self) -> xarast_geom::Point;
}

/// Cheap bounds cache: per-node epoch, invalidation propagates up and stops at
/// the first already-invalid ancestor.
#[derive(Copy, Clone, Debug, Default)]
pub struct BoundsCache { /* rect + epoch + valid flag */ }
impl BoundsCache {
    pub fn get(&self) -> Option<xarast_geom::Rect>;
    pub fn invalid() -> BoundsCache;
}

pub fn compute_bounds(tree: &Tree, id: NodeId, attrs: &ResolvedAttrs) -> xarast_geom::Rect;

// ─── Attributes ──────────────────────────────────────────────────────────────

/// One slot per attribute that can be overridden. The exact set is reconciled
/// against the 211 `.xar` tags in this phase; see the memory note.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
#[repr(u16)]
pub enum AttrSlot { /* LineWidth, StrokeColour, Fill, ... */ }
pub const ATTR_SLOT_COUNT: usize = /* fixed in this phase */ 46;

#[derive(Clone, PartialEq, Debug)]
pub enum AttrValue { /* one variant per attribute; see research/02 §10.6 */ }

impl AttrValue {
    /// `None` means multi-applicable: it accumulates instead of replacing.
    pub fn slot(&self) -> Option<AttrSlot>;
    /// Does it enlarge the object's bounding box (stroke width, feather)?
    pub fn affects_bounds(&self) -> bool;
    /// Does it force rendering through an offscreen buffer (feather, clip)?
    pub fn is_effect(&self) -> bool;
    /// Are its coordinates in object space, so that transforming the object
    /// must transform them too?
    pub fn linked_to_geometry(&self) -> bool;
    /// Interpolation for blends. `None` when the attribute cannot interpolate.
    pub fn blend(&self, other: &AttrValue, t: f64) -> Option<AttrValue>;
}

#[derive(Clone, Debug)]
pub struct AttrNode { pub value: AttrValue }

/// Dense current-value table plus an unwind log. The whole of the original's
/// render stack and current-attribute machinery, in about sixty lines.
#[derive(Clone, Debug)]
pub struct AttrStack { /* ... */ }

impl AttrStack {
    pub fn with_defaults(defaults: &DefaultAttrs) -> AttrStack;
    pub fn get(&self, slot: AttrSlot) -> &AttrValue;
    pub fn multi(&self) -> &[std::sync::Arc<AttrValue>];
    pub fn push(&mut self, value: std::sync::Arc<AttrValue>);
    /// Call on `WalkEvent::EnterScope`.
    pub fn push_scope(&mut self);
    /// Call on `WalkEvent::LeaveScope`.
    pub fn pop_scope(&mut self);
    pub fn snapshot(&self) -> ResolvedAttrs;
}

/// Immutable resolved state. Cheap to clone: `ATTR_SLOT_COUNT` `Arc`s.
#[derive(Clone, Debug)]
pub struct ResolvedAttrs { /* ... */ }
impl ResolvedAttrs {
    pub fn get(&self, slot: AttrSlot) -> &AttrValue;
    pub fn stroke_extent(&self) -> xarast_geom::Mp;
}

#[derive(Debug, Default)]
pub struct AttrResolver { /* cache keyed by NodeId, invalidated by epoch */ }
impl AttrResolver {
    pub fn resolve(&mut self, tree: &Tree, id: NodeId, defaults: &DefaultAttrs) -> &ResolvedAttrs;
    pub fn invalidate_subtree(&mut self, tree: &Tree, id: NodeId);
    pub fn invalidate_all(&mut self);
}

#[derive(Clone, Debug)]
pub struct DefaultAttrs { /* one value per slot */ }
impl DefaultAttrs { pub fn xara_compatible() -> DefaultAttrs; }

// ─── Fills ───────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Debug)]
pub struct RampStop<S: xarast_color::Stop> { pub pos: f32, pub value: S }

/// Intermediate stops only, kept sorted by position; the endpoints live in the
/// geometry's `from`/`to`.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Ramp<S: xarast_color::Stop> { /* ... */ }
impl<S: xarast_color::Stop> Ramp<S> {
    pub fn insert(&mut self, stop: RampStop<S>);
    pub fn stops(&self) -> &[RampStop<S>];
    pub fn sample(&self, from: &S, to: &S, t: f32, effect: xarast_color::FillEffect) -> S;
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum Tiling { #[default] None, Simple, Repeat, RepeatInverted }

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum RampMapping { #[default] Linear, Sin }

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Perspective { pub p2: xarast_geom::Point, pub p3: xarast_geom::Point }

/// One generic type in place of roughly sixty parallel classes.
#[derive(Clone, PartialEq, Debug)]
pub enum FillGeometry<S: xarast_color::Stop> {
    Flat { value: S },
    Linear { start: xarast_geom::Point, end: xarast_geom::Point,
             persp: Option<Perspective>, from: S, to: S, ramp: Ramp<S> },
    Radial { centre: xarast_geom::Point, major: xarast_geom::Point, minor: xarast_geom::Point,
             aspect_locked: bool, persp: Option<Perspective>, from: S, to: S, ramp: Ramp<S> },
    Conical { centre: xarast_geom::Point, zero_dir: xarast_geom::Point,
              from: S, to: S, ramp: Ramp<S> },
    Diamond { centre: xarast_geom::Point, corner1: xarast_geom::Point,
              corner2: xarast_geom::Point, persp: Option<Perspective>,
              from: S, to: S, ramp: Ramp<S> },
    /// Barycentric; carries no ramp, by construction.
    ThreeColour { origin: xarast_geom::Point, axis1: xarast_geom::Point,
                  axis2: xarast_geom::Point, c0: S, c1: S, c2: S },
    FourColour { origin: xarast_geom::Point, axis1: xarast_geom::Point,
                 axis2: xarast_geom::Point, axis3: xarast_geom::Point,
                 c0: S, c1: S, c2: S, c3: S },
    Bitmap { image: BitmapId, origin: xarast_geom::Point, axis_x: xarast_geom::Point,
             axis_y: xarast_geom::Point, persp: Option<Perspective>,
             tiling: Tiling, dpi: u32, contone: Option<(S, S)> },
    Fractal { params: Box<ProceduralParams>, from: S, to: S },
    Noise { params: Box<ProceduralParams>, from: S, to: S },
}

impl<S: xarast_color::Stop> FillGeometry<S> {
    pub fn profile(&self) -> xarast_geom::BiasGain;
    pub fn set_profile(&mut self, p: xarast_geom::BiasGain);
    pub fn mapping(&self) -> RampMapping;
    pub fn has_control_points(&self) -> bool;
    pub fn control_points(&self) -> smallvec::SmallVec<[xarast_geom::Point; 4]>;
    pub fn transform(&mut self, m: xarast_geom::Matrix);
    pub fn sample(&self, at: xarast_geom::Point, effect: xarast_color::FillEffect) -> S;
}

pub type Paint = FillGeometry<xarast_color::Colour>;
pub type TranspPaint = FillGeometry<xarast_color::Transparency>;

// ─── Resources ───────────────────────────────────────────────────────────────

slotmap::new_key_type! {
    pub struct BitmapId;
    pub struct DashId;
    pub struct ArrowId;
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum ResourceRef {
    Colour(xarast_color::ColourId),
    Bitmap(BitmapId),
    Dash(DashId),
    Arrow(ArrowId),
}

#[derive(Clone, Debug)]
pub struct BitmapResource {
    pub name: std::sync::Arc<str>,
    pub info: BitmapInfo,
    pub pixels: std::sync::Arc<BitmapData>,
    /// The encoded source bytes, when we have them. Re-emitted verbatim on
    /// save, so an embedded JPEG never loses quality on a round trip.
    pub original: Option<std::sync::Arc<OriginalEncoded>>,
    pub procedural: Option<ProceduralSource>,
    pub transparent_index: Option<u8>,
}

#[derive(Debug, Default)]
pub struct DocumentResources {
    pub colours: xarast_color::ColourTable,
    /* bitmaps, dashes, arrows, and the content-hash index */
}

impl DocumentResources {
    pub fn insert_bitmap(&mut self, res: BitmapResource) -> BitmapId;   // dedups by SHA-256
    pub fn bitmap(&self, id: BitmapId) -> Option<&BitmapResource>;
    pub fn insert_dash(&mut self, d: xarast_geom::DashPattern) -> DashId;
    pub fn contains(&self, r: ResourceRef) -> bool;
}

/// Sweeps resources unreachable from the tree AND from every node the history
/// retains. Run on save and on history eviction, never per edit.
pub fn collect_unused(doc: &mut Document) -> usize;

// ─── Actions, transactions, history ──────────────────────────────────────────

#[derive(Clone, Debug)]
pub enum Action {
    Attach { node: NodeId, anchor: NodeId, how: Attach },
    Detach { node: NodeId, prev_anchor: NodeId, prev_how: Attach },
    SetKind { node: NodeId, new: Box<NodeKind> },
    SetFlags { node: NodeId, new: NodeFlags },
    Transform { node: NodeId, matrix: xarast_geom::Matrix },
    SetAttr { node: NodeId, new: std::sync::Arc<AttrValue> },
    SetResource { id: ResourceRef, new: Option<std::sync::Arc<BitmapData>> },
    Batch(Vec<Action>),
}

impl Action {
    pub(crate) fn apply(&self, doc: &mut Document) -> Result<(), EditError>;
    /// Computed BEFORE `apply`, which is why it needs the document.
    pub fn inverse(&self, doc: &Document) -> Action;
    /// Estimated retained bytes, for the history budget.
    pub fn size_hint(&self, doc: &Document) -> usize;
}

#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error(transparent)] Tree(#[from] TreeError),
    #[error("the operation is not permitted on {0:?} (locked, or a generated node)")]
    NotPermitted(NodeId),
    #[error("resource {0:?} does not exist")] MissingResource(ResourceRef),
    #[error("limit exceeded: {0}")] LimitExceeded(&'static str),
}

/// Applies as it goes and records inverses. Dropping without `commit` rolls
/// back, so a command that fails halfway cannot leave a half-edited document.
#[derive(Debug)]
pub struct Tx<'d> { /* ... */ }

impl<'d> Tx<'d> {
    pub fn create(&mut self, kind: NodeKind) -> Result<NodeId, EditError>;
    pub fn attach(&mut self, node: NodeId, anchor: NodeId, how: Attach) -> Result<(), EditError>;
    pub fn delete(&mut self, node: NodeId) -> Result<(), EditError>;
    pub fn move_node(&mut self, node: NodeId, anchor: NodeId, how: Attach) -> Result<(), EditError>;
    pub fn set_kind(&mut self, node: NodeId, kind: NodeKind) -> Result<(), EditError>;
    pub fn set_attr(&mut self, node: NodeId, value: AttrValue) -> Result<(), EditError>;
    pub fn transform(&mut self, node: NodeId, m: xarast_geom::Matrix) -> Result<(), EditError>;
    pub fn doc(&self) -> &Document;
    pub fn commit(self, label: &'static str) -> Transaction;
    pub fn rollback(self);
}

#[derive(Debug)]
pub struct Transaction {
    pub label: &'static str,
    pub inverses: Vec<Action>,
    /// Detached nodes this transaction keeps alive.
    pub retained: Vec<NodeId>,
    pub bytes: usize,
    pub coalesce: Option<CoalesceKey>,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct CoalesceKey { pub gesture: u64, pub kind: &'static str }

#[derive(Debug)]
pub struct History { /* past, future, bytes, budget, checkpoints */ }

impl History {
    /// 128 MiB. Budgeting in bytes, not steps, is the point: a hundred nudges
    /// cost nothing and one large-bitmap delete costs everything.
    pub const DEFAULT_BUDGET: usize = 128 << 20;

    pub fn with_budget(bytes: usize) -> History;
    pub fn undo(&mut self, doc: &mut Document) -> Option<&'static str>;
    pub fn redo(&mut self, doc: &mut Document) -> Option<&'static str>;
    pub fn can_undo(&self) -> bool;
    pub fn can_redo(&self) -> bool;
    pub fn labels(&self) -> (Vec<&'static str>, Vec<&'static str>);
    pub fn bytes_used(&self) -> usize;
    pub fn clear(&mut self, doc: &mut Document);
}

/// A user-level operation. Tools implement this; tools never touch the arena.
pub trait Command: core::fmt::Debug {
    fn label(&self) -> &'static str;
    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError>;
    /// Consecutive commands with an equal key merge into one undo step.
    fn coalesce_key(&self) -> Option<CoalesceKey> { None }
}

#[derive(Debug, Default)]
pub struct CommandBus { /* history + gesture counter */ }

impl CommandBus {
    pub fn dispatch(&mut self, doc: &mut Document, cmd: &dyn Command)
        -> Result<&'static str, EditError>;
    pub fn begin_gesture(&mut self) -> u64;
    pub fn end_gesture(&mut self, gesture: u64);
    pub fn history(&self) -> &History;
    pub fn history_mut(&mut self) -> &mut History;
}

/// A snapshot OVER the arena, for autosave and future persistent history.
/// Never the live store — that is the whole of architecture decision §3.1.
#[derive(Clone, Debug)]
pub struct Snapshot { /* imbl::HashMap<NodeId, Arc<NodeData>> + root + resources */ }

// ─── Document ────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct Document {
    pub tree: Tree,
    pub resources: DocumentResources,
    pub defaults: DefaultAttrs,
    pub meta: DocumentMeta,
    pub attrs: AttrResolver,
}

impl Document {
    pub fn new_empty() -> Document;                 // the canonical tree
    pub fn validate(&self) -> ValidationReport;
    /// SHA-256 over a canonical visit: structure, kinds, payloads, resources.
    /// Excludes caches, `NodeId` values and anything session-scoped.
    pub fn canonical_digest(&self) -> [u8; 32];
    pub fn snapshot(&self) -> Snapshot;
    pub fn restore(&mut self, s: &Snapshot);
    pub fn active_spread(&self) -> NodeId;
    pub fn active_layer(&self, spread: NodeId) -> Option<NodeId>;
    /// Human-readable tree dump, used by `xar-dump` and by `insta` snapshots.
    pub fn dump(&self, opts: DumpOptions) -> String;
}

#[derive(Clone, Debug, Default)]
pub struct DocumentMeta {
    pub title: Option<String>,
    pub comment: Option<String>,
    pub created: Option<i64>,
    pub modified: Option<i64>,
    pub producer: Option<String>,
    pub producer_version: Option<String>,
    pub producer_build: Option<String>,
}

// ─── DocumentBuilder ─────────────────────────────────────────────────────────

/// The only public construction path. Importers emit into this; they never see
/// the arena, and therefore cannot build an inconsistent document.
#[derive(Debug)]
pub struct DocumentBuilder { /* ... */ }

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct BuildId(NodeId);

#[derive(Clone, Debug)]
pub struct BuildLimits {
    pub max_depth: usize,          // 256 (deepest corpus file: 13)
    pub max_nodes: usize,          // 20_000_000
    pub max_bytes: usize,          // 2 GiB
    pub max_points_per_path: usize,// 16_000_000
}
impl Default for BuildLimits { /* the values above */ }

#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: DiagCode,
    pub message: String,
    /// Importer-specific location: a `.xar` record number, an XML line, ...
    pub location: Option<u64>,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Severity { Info, Warning, Error }

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum DiagCode {
    UnknownTag, UnknownStructuralTag, EssentialTagMissing, AtomicSubtreeDropped,
    CoordinateClamped, TruncatedRecord, ChecksumMismatch, UnbalancedScope,
    DanglingReference, ColourCycle, UnsupportedFeature, LimitExceeded,
}

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("limit exceeded: {0}")]      Limit(&'static str),
    #[error(transparent)]                Tree(#[from] TreeError),
    #[error("nothing was built")]        Empty,
}

impl DocumentBuilder {
    pub fn new(limits: BuildLimits) -> DocumentBuilder;
    pub fn meta(&mut self, meta: DocumentMeta);

    /// `TAG_DOWN` becomes this.
    pub fn push_scope(&mut self) -> Result<(), BuildError>;
    /// `TAG_UP` becomes this. Tolerates an unbalanced close and reports it.
    pub fn pop_scope(&mut self);

    pub fn node(&mut self, kind: NodeKind) -> Result<BuildId, BuildError>;
    pub fn attribute(&mut self, value: AttrValue) -> Result<BuildId, BuildError>;

    pub fn define_colour(&mut self, def: xarast_color::ColourDef) -> xarast_color::ColourId;
    pub fn define_bitmap(&mut self, res: BitmapResource) -> BitmapId;
    pub fn define_dash(&mut self, d: xarast_geom::DashPattern) -> DashId;

    pub fn diagnostic(&mut self, d: Diagnostic);
    pub fn diagnostics(&self) -> &[Diagnostic];

    pub fn depth(&self) -> usize;
    pub fn node_count(&self) -> usize;

    /// Closes any open scopes, runs `validate()`, and returns the document with
    /// every diagnostic gathered along the way.
    pub fn finish(self) -> Result<(Document, Vec<Diagnostic>), BuildError>;
}
```

## Acceptance criteria

1. `cargo nextest run -p xarast-doc` passes and
   `cargo clippy -p xarast-doc --all-targets -- -D warnings` is clean.
2. `std::mem::size_of::<NodeData>() <= 64`, asserted by a test whose failure
   message prints the actual size and the largest `NodeKind` variant.
3. `std::mem::size_of::<Option<NodeId>>() == std::mem::size_of::<NodeId>()`,
   confirming the niche optimisation the `Links` design assumes.
4. `Tree::validate()` detects every one of the twelve invariants: twelve unit
   tests, each corrupting exactly one and asserting exactly that `Invariant`
   variant is reported, plus one test asserting a healthy tree reports nothing.
5. `AttrAfterInk` is reported as a **warning** and never as an error, and a
   document built with attributes after ink nodes passes `finish()`.
6. `proptest`, ≥ 50 000 cases: random sequences of up to 200 attach/detach/
   move/create/destroy operations leave `validate()` with zero errors after
   every step.
7. **Undo round-trip**: `proptest`, ≥ 10 000 cases, sequences of up to 200
   commands. After undoing all of them, `canonical_digest()` equals the initial
   digest; after redoing all of them, it equals the post-sequence digest. Byte
   equality, not approximate.
8. `Tx` rollback: a command that returns `Err` after applying three actions
   leaves `canonical_digest()` unchanged. Tested for each `Action` variant.
9. History byte budget: with a 1 MiB budget, committing transactions totalling
   10 MiB leaves `bytes_used() <= 1 MiB`, the oldest transactions are gone, and
   the nodes they retained have been destroyed
   (`Tree::contains(id) == false`).
10. Coalescing: 50 transform commands inside one gesture produce exactly one
    undo step, and undoing it restores the pre-gesture digest.
11. `AttrStack` scope correctness: for every corpus-shaped synthetic tree,
    walking with `walk_render` and comparing `AttrStack::snapshot()` at each ink
    node against an independently computed "walk up the ancestors" resolution
    gives identical results, for 10 000 generated trees.
12. `AttrResolver` agrees with `AttrStack` on the same trees, and
    `invalidate_subtree` after a random attribute edit makes it agree again.
13. Every `.xar` attribute tag listed in `research/01 §8` maps to exactly one
    `AttrSlot`, or is explicitly recorded as multi-applicable or ignorable. The
    mapping is a checked-in table and a test asserts it is total and injective
    over the slots.
14. `Document::new_empty()` produces the canonical tree of `research/02 §3.1`,
    asserted by an `insta` snapshot of `dump()`.
15. Resource deduplication: inserting the same 4 MB bitmap twice yields one
    `BitmapId` and one allocation, asserted by `Arc::strong_count` and by the
    table length.
16. `collect_unused` removes a resource referenced only by a node that has been
    deleted *and* whose transaction has been evicted, and does **not** remove
    one still referenced by a retained node.
17. `DocumentBuilder` cannot produce an invalid document: 100 000 `proptest`
    cases of random build scripts — including unbalanced scopes, attributes at
    the root, nodes under illegal parents, and references to undefined
    resources — either return `Err(BuildError)` or return a document whose
    `validate()` has zero errors. Never a third outcome.
18. `Tree`'s mutating methods are not reachable from outside the crate: a
    `trybuild` compile-fail test proves `tree.attach(..)` does not compile in a
    downstream crate.
19. Build limits are enforced: a build script exceeding each of the four limits
    fails with `BuildError::Limit` naming that limit, and peak RSS during the
    attempt stays under 2× the limit.
20. The arena benchmark runs, produces measurements A–D for both
    representations, and the pre-registered decision rule is applied; the
    verdict and the four numbers are written into
    `docs/memory/document-model.md`. The stale "Phase 1" sentence in
    `docs/10-architecture.md §3.1` is corrected in the same commit, and if the
    rule overturns the arena, `§3.1` and `§7` are rewritten with it.
21. All Phase 2 performance budgets below are met and recorded.

## Performance budgets

Measured with `criterion` on the reference machine, `release` profile, against
the synthetic 100 000-node document described in W2.11.

| Operation | Budget | Note |
|---|---|---|
| Undo of a single-node edit | **≤ 1 ms** | From `docs/phases/00-roadmap.md`; the only budget this phase inherits |
| Redo of a single-node edit | ≤ 1 ms | Symmetric |
| `walk_render` over 100 000 nodes | ≤ 2.0 ms | ≥ 50 M nodes/s; this is the render hot path |
| `preorder` over 100 000 nodes | ≤ 1.5 ms | |
| Random `Tree::get` by `NodeId` | ≤ 5 ns amortised | ≥ 200 M lookups/s |
| `AttrStack::push` / `pop_scope` | ≤ 20 ns per operation | |
| `AttrResolver::resolve`, warm cache | ≤ 30 ns | |
| `AttrResolver::resolve`, cold | ≤ 2 µs at depth 10 | |
| `DocumentBuilder` building 100 000 nodes | ≤ 150 ms | Bounds the Phase 3 `.xar` open budget |
| `Document::canonical_digest()` over 100 000 nodes | ≤ 30 ms | Used by tests, not by the UI |
| `Document::snapshot()` over 100 000 nodes | ≤ 25 ms | Autosave cadence depends on it |
| Resident bytes per node, whole document | ≤ 160 B | 100 000 nodes ≈ 16 MB, excluding payloads |
| `compute_bounds` over 100 000 nodes, cold | ≤ 12 ms | |

## Risks and mitigations

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| 1 | A wrong `Action::inverse` corrupts documents silently — the single worst failure mode in this phase | Medium | **Severe** | Criterion 7's digest round-trip over 10 000 random command sequences; one test per `Action` variant; `inverse` takes `&Document` so it physically cannot read post-apply state |
| 2 | `NodeData` grows past 64 bytes as variants are added, and text documents blow the cache | **High** | Medium | Criterion 2 is a gate. The fix is always to `Box` a variant |
| 3 | `AttrResolver` cache invalidation is subtly wrong, so rendering uses stale attributes | Medium | High | Criterion 12 cross-checks it against the uncached walk on every generated tree. A correct-but-conservative invalidation (drop the whole cache on any structural change) ships first; tuning is Phase 4 |
| 4 | The `AttrSlot` set turns out wrong once the importer meets real tags, forcing a churn through every consumer | Medium | High | Criterion 13 does the reconciliation **in this phase**, before Phase 3 writes a line against it |
| 5 | The benchmark result is ambiguous and gets argued rather than decided | Medium | Medium | The decision rule is pre-registered in W2.11, with numeric thresholds, before the benchmark is written |
| 6 | Retained nodes leak: history keeps subtrees alive and memory grows without bound | Medium | High | Byte budget with eviction (criterion 9); `bytes_used()` exposed in the UI's diagnostics from Phase 5 |
| 7 | Someone adds a public mutating method to `Tree` "just for this one case" and undo silently stops being complete | Medium | **Severe** | Criterion 18's compile-fail test; the rule is stated at the top of `tree.rs` |
| 8 | Boxing everything to satisfy criterion 2 turns hot traversals into pointer chases | Low | Medium | Only variants above the threshold are boxed, and the traversal benchmarks (criterion 21) would catch it |
| 9 | `imbl` is MPL-2.0; using it for checkpoints needs a `deny.toml` exception | Certain | Low | Add the exception with a written justification when the checkpoint code lands, as `docs/11-licensing-and-clean-room.md §4` requires. Used unmodified, MPL-2.0 is acceptable |
| 10 | Gesture-based coalescing turns out wrong for keyboard nudges, which have no gesture boundary | Medium | Low | Nudge commands supply an explicit key with a sequence number; the time-window variant stays available behind the same `coalesce_key` API |

## Test plan

**Unit.** Every `Tree` mutation against every anchor position; `anchor_of`
round-trips; `Tag` uniqueness and `by_tag` bijectivity; each `NodeKind`
predicate for each variant; `Ramp::insert` ordering; `FillGeometry`
control-point extraction and transformation for every variant; `Action::inverse`
for every variant; `History` eviction at the budget boundary.

**Property (`proptest`).** Tree invariants under random operation sequences
(criterion 6). Undo/redo digest round-trip (criterion 7). `AttrStack` versus
ancestor-walk agreement (criterion 11). `DocumentBuilder` never produces an
invalid document (criterion 17). `canonical_digest` is invariant under
operations that should not change it — reordering independent resource
insertions, re-running `collect_unused`, taking and restoring a snapshot.

**Snapshot (`insta`).** `Document::new_empty().dump()`; the dump of a
hand-built document exercising every `NodeKind`; the `AttrSlot` mapping table.
Text snapshots make a model change a readable diff during review, which is worth
far more here than in most crates.

**Benchmarks.** Everything in the budget table, plus the four arena-comparison
measurements.

**Fuzz.** `fuzz_document_builder`: `arbitrary` generates a build script
(a sequence of `push_scope`/`pop_scope`/`node`/`attribute`/`define_*`), and the
invariants are no panic, bounded allocation within `BuildLimits`, and
`validate()` clean on any `Ok` result. This target exists now so that Phase 3's
`.xar` fuzzing tests the *parser* rather than rediscovering builder bugs.

**Manual, recorded in the memory note.** Walk the `AttrSlot` table against
`research/01 §8` by hand once, tag by tag. It is tedious, it is the reconciliation
criterion 13 automates afterwards, and doing it by hand once is how the
automation gets the right answer to check against.

## Memory note

Update **`docs/memory/document-model.md`** — it already exists and already
records the design; this phase turns proposals into decisions. It must also be
**translated into English**, per `CLAUDE.md`'s language rule; it is currently in
Spanish, which is a standing violation.

Record:

- **Current state.** Which modules exist, the final `AttrSlot` count, the
  `NodeKind` variants implemented versus stubbed, and the measured budget table.
- **Decisions taken (and why).** The **benchmark verdict** with all four
  numbers and the pre-registered rule, closing `10-architecture.md §7`
  question 1. The correction that the benchmark belongs to Phase 2, not Phase 1.
  The crate is `xarast-doc`, not `xarast-model` as `research/02` calls it. The
  final `AttrSlot` set and the complete tag→slot mapping. `Tree` mutation being
  crate private, and why that is what makes undo complete. Byte budgeting rather
  than step budgeting, with the default. Gesture-based coalescing as the
  default. The build limits and their values. Deduplication by SHA-256.
  `DiagCode` as the shared vocabulary between all importers.
- **Invariants that must not be broken.** The twelve from `research/02 §10.16`,
  restated, with the note that number 5 (attributes before ink) is a warning and
  that real files violate it. Plus: `NodeData ≤ 64` bytes; mutation only through
  `DocumentBuilder` and `CommandBus`; `Action::inverse` computed before apply;
  selection and the text cursor never enter the tree; coordinates are `Mp`
  everywhere in the model.
- **Dead ends (do not retry).** The four already recorded — `Box<dyn Node>`,
  `Rc<RefCell<Node>>`, per-node attribute maps without attribute nodes, a global
  bounds epoch — plus anything this phase's benchmark rules out, and putting
  selection in `NodeFlags`.
- **Open TODOs.** Whether blend steps materialise as nodes or are generated at
  render time (affects hit-test; Phase 13). `ProceduralSource` and its cache
  key. `MouldGeometry` as trait or enum. The coalescing window question, if
  gesture-based proves insufficient. `AttrResolver` invalidation tuning
  (Phase 4). Persistent history across reload (Phase 6).

Also append the Phase 2 measurements to `docs/memory/perf.md`.
