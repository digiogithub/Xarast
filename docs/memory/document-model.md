# document-model

Memory note for the **document model** subsystem — the crate `xarast-doc`.
Full research in [`../research/02-document-model.md`](../research/02-document-model.md).
Arbitrated decisions in [`../10-architecture.md`](../10-architecture.md) §3.1, §3.2, §3.5b, §3.6.
Phase specification: [`../phases/phase-02-document-model.md`](../phases/phase-02-document-model.md).

> Crate name: the research note called it `xarast-model`; the architecture
> document settles on **`xarast-doc`**. Use `xarast-doc`.

## Current state

**Phase 2 is implemented.** `xarast-doc` is a working document model with no
graphics and no UI dependency; it builds, tests, fuzz-shaped property-tests and
benchmarks with no GPU and no windowing system present.

Modules, all of them in `crates/xarast-doc/src/`:

| Module | Holds |
|---|---|
| `tree` | `NodeId`, `Tag`, `Links`, `NodeFlags`, `NodeData`, `Tree`, `Attach`, `TreeError` |
| `walk` | `Children`, `Ancestors`, `Preorder`, `Postorder`, `RenderWalk`, `WalkEvent`, `Descend` |
| `kind` | `NodeKind` and its payload structs |
| `bounds` | `Epoch`, `BoundsCache`, `compute_bounds`, `compute_bounds_with` |
| `attr` | `AttrSlot` (46), `AttrValue`, `AttrNode`, `MultiAttr`, `Quality`, `DefaultAttrs` |
| `attr::stack` | `AttrStack`, `ResolvedAttrs` |
| `attr::resolve` | `AttrResolver`, `resolve_uncached` |
| `attr::tags` | the `.xar` tag → slot reconciliation table and its tests |
| `fill` | `FillGeometry<S>`, `Ramp`, `RampStop`, `Perspective`, `Tiling`, `RampMapping`, `Paint`, `TranspPaint` |
| `structure` | `DocumentNode`, `SpreadNode`, `PageNode`, `LayerNode`, `GridNode`, `FrameProps`, `AnimProps` |
| `text` | `TextStoryNode`, `TextLineNode`, `TextItem`, `TextLayout`, `Justification`, `LineSpacing`, `Script`, `TabStop` |
| `live` | `LiveNode`, `LiveRole`, `LiveKind`, `RegenState` and the seven parameter structs |
| `resources` | `DocumentResources`, `BitmapId`/`DashId`/`ArrowId`, `BitmapResource`, `collect_unused` |
| `history` | `Action`, `Tx`, `Transaction`, `History`, `CoalesceKey`, `Command`, `CommandBus`, `EditError` |
| `snapshot` | `Snapshot` over `imbl`, plus `Document::snapshot`/`restore` |
| `document` | `Document`, `DocumentMeta`, `DumpOptions`, `canonical_digest`, `dump`, `update_bounds` |
| `digest` | `CanonicalHasher`, the `Canon` trait and its impls |
| `builder` | `DocumentBuilder`, `BuildLimits`, `Diagnostic`, `DiagCode`, `Severity`, `BuildError` |
| `validate` | `ValidationReport`, `Invariant`, `validate_tree`, `validate_document` |
| `synth` | `SynthSpec`, `synthetic_document` — the 100 000-node benchmark fixture |

`NodeKind` variants implemented: `Document`, `Chapter`, `Spread`, `Page`,
`Layer`, `Grid`, `Path`, `Shape`, `QuickShape`, `Bitmap`, `Guideline`, `Group`,
`Live`, `ClipView`, `TextStory`, `TextLine`, `TextItem`, `Attr`, `Opaque`.
`Live` and the text variants are **structure and round-trip only**:
`regenerate()` is Phase 13 and shaping is Phase 9. `QuickShape` stores its
parameters and an optional imported path; generating the path is Phase 7.

`ATTR_SLOT_COUNT` is **46**, unchanged by the reconciliation.

Tests: 82 crate-internal (`src/tests/`), 5 size gates, 5 public-API
integration, 1 compile-fail doctest. Benchmarks: `benches/doc.rs` (the budget
table) and `examples/arena_vs_persistent.rs` (the §3.1 benchmark).

**First real consumer, Phase 3.** The `.xar` importer
(`crates/xarast-xar/src/import.rs`) now builds documents from all 59 corpus
files — 830 533 nodes, zero `validate()` errors — through `DocumentBuilder`
and nothing else. It needed exactly one change to this crate:
`BitmapResource::content_hash` (decision 27 below). It did **not** need the
arena, `Tree`'s mutating methods, or an "amend the node I just made"
builder method: where `.xar` describes a node from its children, the
importer reads ahead instead. Keep it that way. What the importer found
that this crate should grow is listed in `xar-import.md` finding 10 — a
three-point linear fill, an "extra" tiling — and neither is urgent.

## The arena benchmark — verdict

Run: `cargo run --release -p xarast-doc --example arena_vs_persistent -- time`
(and `mem-arena` / `mem-hamt` for D, each in a process of its own because
resident memory does not reliably come back down after a free).

Fixture: the synthetic 100 002-node document of `xarast_doc::synth` — one
spread, six layers, groups nested to depth six, eight children per group, 40 %
attribute nodes.

| M | What | Arena | HAMT (`imbl`) | Ratio |
|---|---|---|---|---|
| A | full render traversal | **1.99 ms** (50.3 M nodes/s) | 22.3 ms (4.5 M nodes/s) | HAMT **11.2×** slower |
| B | 10 000 000 random lookups by `NodeId` | **70.6 ms** (7.06 ns each) | 1 033 ms (103.3 ns each) | HAMT **14.6×** slower |
| C | one attribute edit, then undo | **0.21 µs** | 1.52 µs | HAMT **7.2×** slower |
| D | resident bytes, whole document | **22.13 MB** (221 B/node) | 30.89 MB (308 B/node) | HAMT **1.40×** |

Against the rule pre-registered in `phase-02 §W2.11` — *the arena stands unless
all three hold: the HAMT within 20 % on A and B; the HAMT more than 5× faster
on C; the HAMT's memory within 1.5× on D* —

- A: **fails** (11× slower, not within 20 %).
- B: **fails** (15× slower, not within 20 %).
- C: **fails**, and in the opposite direction: the HAMT is 7× *slower*, not
  5× faster. Its own undo really is a pointer swap, but the edit costs a
  `HashMap` clone plus a node rewrite, and the arena's whole edit-and-undo
  cycle is two `SlotMap` writes and an inverse it already had.
- D: passes (1.40× ≤ 1.5×), and it is the only one.

**Verdict: the arena wins, decisively, and the question is closed.**
`10-architecture.md` §3.1 and §7 question 1 are updated to match, and the stale
"Phase 1 ships a benchmark" sentence in §3.1 is corrected in the same change.
Nothing about the result was surprising except how large the margins were; the
benchmark was worth running mainly because C was the condition that could
plausibly have gone the other way, and it did not come close.

Note the second-order finding this produced: a persistent snapshot is
expensive to *build* (see the checkpoint budget below), which is why
checkpointing is off by default.

## Measured budgets

`cargo bench -p xarast-doc --bench doc -- --quick`, release, on the development
machine. The phase's budget table was written for a "reference machine" we do
not have, so read the overruns as "this is where the cost is", not as failures
of a controlled measurement.

| Operation | Budget | Measured | |
|---|---|---|---|
| Undo of a single-node edit | ≤ 1 ms | **0.27 µs** | ✅ 3 700× under |
| Redo of a single-node edit | ≤ 1 ms | **0.17 µs** | ✅ |
| `walk_render` over 100 000 nodes | ≤ 2.0 ms | **2.01 ms** | ≈ at budget |
| `preorder` over 100 000 nodes | ≤ 1.5 ms | **2.09 ms** | ⚠ 1.4× over |
| Random `Tree::get` | ≤ 5 ns | **6.6 ns** | ⚠ 1.3× over |
| `AttrStack::push` + `pop_scope` | ≤ 20 ns | **17.5 ns** | ✅ |
| `AttrResolver::resolve`, warm | ≤ 30 ns | **30.4 ns** | ≈ at budget |
| `AttrResolver::resolve`, cold | ≤ 2 µs | **1.08 µs** | ✅ |
| `DocumentBuilder`, 100 000 nodes | ≤ 150 ms | **47.4 ms** | ✅ |
| `canonical_digest()`, 100 000 nodes | ≤ 30 ms | **6.2 ms** | ✅ |
| `Document::snapshot()`, 100 000 nodes | ≤ 25 ms | **58.5 ms** | ❌ 2.3× over |
| Resident bytes per node | ≤ 160 B | **221 B** | ❌ but see below |
| `compute_bounds` over 100 000 nodes, cold | ≤ 12 ms | **8.95 ms** | ✅ |

Three of these need a word.

- **`snapshot()` is 2.3× over.** Building a HAMT of 100 000 `Arc<NodeData>`
  clones is simply that expensive, and each clone deep-copies a boxed payload.
  The consequence was concrete: checkpointing every 64 transactions put about
  **1.5 ms of amortised cost on every single edit** and blew the 1 ms undo
  budget on its own. So automatic checkpointing is now **off by default**
  (`History::set_checkpoint_cadence`), and Phase 6 should drive checkpoints
  from the autosave timer and make them incremental — sharing structure with
  the previous snapshot instead of rebuilding — which is the one thing a HAMT
  is actually good at.
- **221 B/node is not comparable to the 160 B budget.** The budget says
  "excluding payloads"; the measurement is whole-process resident memory and
  therefore includes every `Arc<Path>`, every boxed `NodeKind` payload and the
  allocator's own overhead. `NodeData` itself is exactly 64 bytes. A
  payload-free measurement is still owed.
- **`preorder` and `Tree::get` are within 40 % of budgets set for another
  machine.** Not worth optimising until there is a reference machine to
  measure against; recorded so that a later regression is visible.

`compute_bounds` was 53 ms when first written, and 8.95 ms after two changes
worth remembering: it took a `ResolvedAttrs` snapshot per node (46 `Arc` clones
each — the fix is `AttrStack::stroke_extent()`, which is all a bounding box
actually needs), and it accumulated into a `HashMap` where a
`SecondaryMap<NodeId, Rect>` does.

## Decisions taken (and why)

The decisions carried over from the research and re-confirmed in
implementation:

1. **Arena `slotmap::SlotMap<NodeId, NodeData>` + `enum NodeKind`** — not
   inheritance, not trait objects, not ECS. Confirmed by measurement, above.
2. **Sibling linked list** (`parent/prev/next/first_child/last_child`) rather
   than `Vec<NodeId>`. We keep `last_child`, which the original lacks, because
   the importer appends in bulk.
3. **`NodeHidden` is gone.** Delete = `detach` + a `DETACHED` flag; the node
   stays alive in the arena, retained by the history `Transaction`, and is
   really destroyed only when the history evicts that transaction.
4. **Attributes remain nodes** (`NodeKind::Attr`), with `AttrResolver` as a
   derived cache on top.
5. **`AttrStack`** is a dense table indexed by `AttrSlot`, an unwind log of
   `(slot, previous)` and a vector of scope marks. About sixty lines.
6. **One generic `FillGeometry<S: Stop>`**; `Perspective` is an `Option`.
7. **Undo is an inverse-action log**, not a persistent live store.
8. **Selection lives outside the document**, in `xarast-app`. There is no
   `SELECTED` flag in `NodeFlags` and no `EditState` in this crate.
9. **Control-point selection lives outside `PathData`.** `Path` stays
   comparable with `==`.
10. **Caches are keyed on state**, not on a boolean.
11. **`xarast-doc` has no graphics dependency.**

New in Phase 2:

12. **`Tree`'s mutating API is `pub(crate)`, and that is the mechanism, not a
    convention.** `create`, `attach`, `detach`, `destroy_subtree`, `get_mut`,
    `set_tag`, `set_bounds` and `invalidate_bounds` are all crate private. The
    entire public mutation surface is `DocumentBuilder` during construction and
    `CommandBus` afterwards, and both record inverses. A `compile_fail,E0624`
    doctest in `lib.rs` proves a downstream crate cannot call `tree.detach()` —
    a doctest rather than `trybuild`, because it needs no extra dependency and
    does not depend on matching a compiler's exact stderr. **Do not widen this
    API.** Nothing would fail until a user lost work.
13. **`BoundsCache` is a side table on `Tree`, not a field of `NodeData`.**
    This is the one real deviation from the phase document's struct layout and
    it is forced: five `Option<NodeId>` links cost 40 bytes, `NodeKind` costs
    16, the tag and flags cost 6 — which is 62, and `NodeData` is gated at 64.
    A bounds cache is 20 bytes more. It is derived data, traversal never reads
    it, and in a side table it does not pollute the cache line that traversal
    does read. Access is `Tree::bounds(id)`.
14. **Every `NodeKind` payload larger than a pointer is boxed**, which is what
    keeps `NodeKind` at 16 bytes and `NodeData` at exactly 64. `TextItem` is
    deliberately 8 bytes and stays inline: a text document is mostly those, and
    its glyph metrics are Phase 9's derived cache, not node data.
15. **The default attribute block is `DefaultAttrs`, a dense table — not 46
    attribute nodes under the document root.** The original materialises them;
    doing the same here would duplicate the table, make every resolution scan
    forty-odd nodes before it starts, and put them in the digest and every
    dump. `TAG_CURRENTATTRIBUTES` maps onto `DocumentBuilder::default_attribute`.
16. **An attribute's scope is "its following siblings and their subtrees, **and
    its parent's own ink**".** The second half is not an extra rule: a parent
    paints after its children, which is exactly what makes a `.xar` path's
    fill — stored by the format as the path's *child* — apply to the path.
    `RenderWalk` emits scope events only for nodes that have children, so a
    consumer paints an ink node at `LeaveScope` when it has children and at
    `Visit` when it has none; the two cases never overlap. A property test
    checks the walk, the uncached resolver and the cache all agree.
17. **`Action::inverse(&self, doc: &Document)` takes the document**, so it
    physically cannot read post-apply state.
18. **A transform's inverse is exact or it is a snapshot.** A pure translation
    in millipoints inverts exactly, so its inverse is another `Transform`.
    Anything else would go through a float inverse and requantise, so the
    inverse is a `SetKind` carrying the pre-transform payload. Cheap where it
    can be, exact always — which is what makes the byte-identical undo test
    pass rather than nearly pass.
19. **Undo and redo share one function.** `apply_reversed` applies a list in
    reverse while collecting the inverses in the same convention, so redo is
    undo applied to the inverse list. One implementation, one thing to get
    right.
20. **The history budget is in bytes**, `History::DEFAULT_BUDGET` = 128 MiB.
    Eviction is oldest-first and is what finally destroys the nodes a
    transaction retained.
21. **Coalescing is gesture based** by default: `CommandBus::begin_gesture` /
    `end_gesture`, because it does not depend on wall-clock timing and is
    therefore testable. A `CoalesceKey` a command supplies itself overrides it,
    which is the escape hatch for keyboard nudges.
22. **Automatic checkpointing is off by default.** Measured, see above.
23. **"One spread, one active layer" is maintained by commands, not by the
    arena.** `Tx::keep_one_active_layer` runs inside the same transaction, so
    undo puts the old active layer back too. The arena deliberately does not
    fix it up for itself: an automatic change the undo log never saw is exactly
    how undo stops being exact.
24. **`DocumentBuilder::finish` has exactly two outcomes**: `Err(BuildError)`,
    or a document whose `validate()` has no errors. Illegal nestings are
    created detached and swept; dangling bitmap references are dropped;
    controllers without a source get one; spreads get an active layer. Each
    repair is a `Diagnostic`. `BuildError::Inconsistent` exists so that a
    failure of this guarantee is loud rather than hidden — it would be a bug in
    the builder, never in the input.
25. **Build limits live in `BuildLimits`, not in importers**: depth 256, nodes
    20 000 000, bytes 2 GiB, points per path 16 000 000.
26. **`DiagCode` is the shared vocabulary of every importer**, extended from
    the phase list with `IllegalNesting` and `Repaired`.
27. **Deduplication is by SHA-256 of the decoded payload *and the encoded
    original***; liveness is `collect_unused`, a sweep over everything alive
    in the arena — which automatically includes what the history retains, so
    an undone deletion still finds its resources.
    *Changed in Phase 3.* Hashing only the decoded payload was wrong in two
    ways. The immediate one: the `.xar` importer stores the encoded bytes and
    leaves `pixels` empty until Phase 10 decodes them, so every bitmap in a
    file hashed identically and `insert_bitmap` returned one id for all of
    them. The deeper one: two bitmaps with the same pixels but different
    encodings really are two resources, because `original` is emitted
    verbatim on save and collapsing them throws one encoding away.
28. **`canonical_digest()` excludes `NodeId` *and* `Tag`.** Both are allocation
    ordered. Resources are hashed in content order so that the order they were
    inserted in does not change the digest. Colour and bitmap *references* are
    hashed by table slot, which is stable within a document but not across two
    documents built by different routes — good enough for what the digest is
    for (undo round-trip, round-trip tests, "did this refactor change
    anything"), and worth knowing before someone compares digests across
    importers.
29. **`Document::restore` rebuilds the tree and remaps every `NodeId`**, but
    carries `Tag` across unchanged. `slotmap` cannot reinsert at a given key,
    and `NodeId` is documented as never serialised, so this is the honest
    implementation. It invalidates any outstanding `NodeId` and any history.
30. **The gradient profile and ramp mapping live on `Ramp`**, and the bitmap
    and procedural fills carry their own `profile` field. The phase document's
    variant list omitted them; the format carries bias/gain on every gradient,
    so they had to go somewhere.
31. **`imbl` is MPL-2.0 and is used unmodified**, for checkpoints and for the
    benchmark. `deny.toml` carries the exception with its justification.

## The `.xar` attribute tag reconciliation

Done tag by tag against `research/01 §8` and `§4.12`. The result **did not
change the slot count**: 46, as `research/02 §10.6` proposed. The machine-
readable table is `crates/xarast-doc/src/attr/tags.rs`
(`XAR_ATTRIBUTE_TAGS`, `SLOTS_WITHOUT_ATTRIBUTE_TAG`), and three tests assert
that no tag appears twice, that every slot is either reached by a tag or listed
as tagless, and that the count is still 46. Phase 3 reads that table; it does
not invent its own.

| `.xar` tags | Slot |
|---|---|
| 151, 193–195 line colour | `StrokeColour` |
| 173 | `StrokeTransp` |
| 150, 190–192, 153–159, 200, 202, 204, 4010, 4075–4078, 4088, 4121, 4122 | `FillGeometry` |
| 166–172, 201, 203, 205, 4011, 4123 | `TranspFillGeometry` |
| 163–165, 206 | `FillMapping` |
| 180–182, 207 | `TranspFillMapping` |
| 160–162 | `FillEffect` |
| 152 | `LineWidth` |
| 178 | `WindingRule` |
| 176 | `JoinType` |
| 179 | `Quality` |
| 183, 184, 188 | `DashPattern` |
| 174 **and 175** | `StartCap` |
| 185 / 186 | `StartArrow` / `EndArrow` |
| 177 | `MitreLimit` |
| 4086 | `Feather` |
| 3500–3505 | `OverprintLine`, `OverprintFill`, `PrintOnAllPlates` |
| 2900, 2901 | `TxtLineSpace` |
| 2902–2905 | `TxtJustification` |
| 2906 / 2907 | `TxtFontSize` / `TxtFontTypeface` |
| 2908–2913 | `TxtBold`, `TxtItalic`, `TxtUnderline` |
| 2914–2917 | `TxtScript` |
| 2918 / 2919 / 2920 | `TxtTracking` / `TxtAspectRatio` / `TxtBaseline` |
| 4201 / 4202 / 4203 / 4204 | `TxtLeftMargin` / `TxtFirstIndent` / `TxtRightMargin` / `TxtRuler` |
| 189 `USERVALUE` | **multi-applicable** — except with the web-address key, which is `WebAddress` |

Slots no attribute tag reaches, and where they come from instead:
`WebAddress` (tag 189 with the web-address key); `StrokeType`,
`VariableWidth`, `BrushType` (§4.10, brushes and strokes); `BevelIndent`,
`BevelType`, `BevelContrast`, `BevelLightAngle`, `BevelLightTilt`,
`ClipRegion`, `ClipView` (§4.8, containers and effects). Phase 3 wires them up.

Two things Phase 3 was asked to check against the corpus, **now answered**
(`the_two_attribute_questions_phase_two_left_open` in
`crates/xarast-xar/tests/corpus.rs`):

- **`ENDCAP` never disagrees with `STARTCAP`.** 26 257 start/end pairs in the
  corpus, zero of them different. One cap slot is enough, and the test fails
  if a future file proves otherwise rather than silently losing the end cap.
- **The overprint pair 3500–3505 cannot be answered from this corpus**: not
  one of those records occurs in any of the 59 files. Leave it open until a
  file that uses them turns up; nothing renders them yet.

## Invariants that must not be broken

The twelve from `research/02 §10.16`, each with a matching `Invariant` variant
and a unit test that corrupts exactly it:

1. The tree is acyclic. → `Cycle`
2. `next`/`prev` are reciprocal; every child points at the same parent;
   `first_child` has no `prev` and `last_child` has no `next`.
   → `BrokenSiblingLink`, `ChildParentMismatch`
3. `DETACHED` is transitive downward: nothing reachable from the root carries
   the flag. → `DetachedReachable`
4. Every `LiveRole::Generated` node has a `Controller` ancestor of the same
   `LiveKind`. → `GeneratedWithoutController`
5. Attribute nodes come before the first ink node in a child list.
   **Warning, never an error**: real `.xar` files violate it, and
   `DocumentBuilder::finish` accepts them. → `AttrAfterInk`
6. One spread, exactly one active layer. → `NoActiveLayer`
7. `Tag` is unique; `by_tag` is bijective with the arena. → `DuplicateTag`
8. An invalid bounding box propagates upward. → `StaleBounds`
9. Every referenced resource exists. → `MissingResource`
    (`Document::validate` only; `Tree::validate` cannot see the tables)
10. The selection contains only reachable nodes — **not this crate's problem**,
    because the selection is not in this crate. Recorded so the numbering
    matches the research.
11. `TextItem` only under `TextLine`; `TextLine` only under `TextStory`.
    → `TextItemOutsideLine`
12. Every controller has exactly one source subtree.
    → `ControllerWithoutSource`

Plus, from this phase:

13. **`size_of::<NodeData>() <= 64`**, and it is exactly 64 today. When the
    gate fails the fix is to box a variant, never to raise the limit.
14. **`size_of::<Option<NodeId>>() == size_of::<NodeId>()`** — the `Links`
    design assumes the niche.
15. **Mutation only through `DocumentBuilder` and `CommandBus`.**
16. **`Action::inverse` is computed before `apply`.**
17. **Selection, the caret and the insertion point never enter the tree.**
18. **Coordinates are `Mp` everywhere in the model.** The only floats are
    colour components, ramp positions, matrix scale/shear and the handful of
    angle and ratio parameters on live effects.
19. **Invariant 6 is a command-level invariant, not an arena-level one.**
    Raw tree surgery may break it; a `Tx` may not leave it broken.

## Fixed defects worth remembering

**Invariant repair must run at commit, not per operation.** `Tx::attach`,
`delete`, `move_node` and `set_kind` each used to call
`keep_one_active_layer` immediately. That looks safer and is a trap: moving
the active layer takes two mutations, and the intermediate state necessarily
has zero or two active layers. The per-operation repair saw that intermediate
state, picked the first layer, and silently reverted the caller — so **no pair
of operations could ever move the active layer**, and the failure was silent.
Found by the phase 5 app-core work, which had to route around it.

The repair now runs once in `Tx::commit`, still inside the transaction, so its
own actions are recorded and undo stays exact. Pinned by
`tests::undo::the_active_layer_can_be_moved_within_one_transaction`, which was
confirmed to fail against the old behaviour before being committed.

The general rule this establishes: **a transaction is allowed to pass through
an invalid intermediate state — that is what transactions are for.** Repairs
and invariant checks belong at the boundary, never between two mutations that
are meant to be one change.

## Dead ends (do not retry)

- **`Box<dyn Node>` + `Any`.** Reproduces the original's constant downcasting
  and makes exhaustive `match` impossible.
- **`Rc<RefCell<Node>>`.** `BorrowMut` panics in traversals that go both down
  and up, allows parent/child cycles, and is not `Send`.
- **A per-node attribute map with no attribute nodes.** Loses list scope and
  breaks round-tripping with both `.xar` and SVG.
- **A global `Epoch` for bounds invalidation.** Invalidates everything on every
  edit. Use the per-node flag with upward propagation and early cut-off.
- **Putting the caret, the selection or the insertion point in the tree.**
- **A persistent HAMT as the *live* store.** Measured in this phase: 11× on
  traversal, 15× on lookup, 7× on edit-and-undo, all against it. Closed.
- **`BoundsCache` as a field of `NodeData`.** It does not fit under the 64-byte
  gate alongside five links and a boxed kind, and traversal does not want it in
  the cache line anyway.
- **Automatic checkpointing on the edit path.** Measured at ~1.5 ms amortised
  per edit at a cadence of 64. Checkpoints belong to autosave.
- **Taking a `ResolvedAttrs` snapshot per node in a whole-document pass.** 46
  `Arc` clones per node; `AttrStack::stroke_extent()` is what a bounding box
  needs.
- **Letting `Tree::detach` fix the active layer for itself.** An automatic
  change the undo log never saw breaks byte-identical undo. Policy belongs to
  commands.

## Open TODOs

- [x] Benchmark arena vs `imbl` over a 100 000-node traversal. **Done; the
      arena wins; `10-architecture.md` §3.1 and §7 updated.**
- [x] Settle the exact `AttrSlot` set against the `.xar` tags. **Done; 46,
      unchanged; the table is `attr/tags.rs`.**
- [x] `Tree::validate()` plus `proptest` property tests over attach/detach/move
      sequences. **Done, before the importer.**
- [x] Size test `size_of::<NodeData>() <= 64`. **Done; it is exactly 64.**
- [x] Confirm whether `ENDCAP` (175) ever disagrees with `STARTCAP` (174)
      against the corpus. **Done: 26 257 pairs, zero disagreements.**
- [ ] The 3500–3505 overprint on/off pairing: **unanswerable from this
      corpus**, which contains none of those records.
- [ ] Decide whether blend intermediate steps are materialised as nodes or
      generated at render time. The original does the latter; it affects
      hit-testing (Phase 13).
- [ ] `ProceduralSource` and its cache hash — the struct exists, the hash does
      not (Phase 13 / Phase 10).
- [ ] `MouldGeometry` as trait or enum, when live effects land (Phase 13).
- [ ] Incremental checkpoints: share structure with the previous snapshot
      instead of rebuilding, and drive them from the autosave timer (Phase 6).
      Until then `History::set_checkpoint_cadence` is opt-in.
- [ ] `AttrResolver` invalidation tuning. Phase 2 ships the conservative
      policy: **any** change drops the whole cache. Correct, and slow on a
      large document mid-drag (Phase 4).
- [ ] A payload-free "bytes per node" measurement, to judge the 160 B budget
      properly.
- [ ] Persistent history across save and reload (Phase 6).
- [ ] `ndoptmz.cpp`'s normalisation — `make_self_contained`, `strip_redundant`,
      `factor_out`, `localise` — at the four boundaries `research/02 §10.6`
      identifies. Not needed until copy/paste and grouping exist (Phase 7) and
      saving does (Phase 6).
- [ ] A `cargo-fuzz` target for `DocumentBuilder`. The property test in
      `src/tests/props.rs` covers the same invariants over random build
      scripts; a real fuzz target should exist before Phase 3 fuzzes the `.xar`
      parser, so that that work tests the parser rather than rediscovering
      builder bugs.
- [ ] **`Tx::commit` is O(document size).** `keep_one_active_layer` runs
      `preorder` over the whole tree on every commit to find the spreads. On
      the reference machine that makes a single-node edit cost **1.04 ms** to
      dispatch at 100 000 nodes, while the undo itself costs 0.28 µs. It was
      found on 2026-09-23 when the `undo/single_node_edit` bench, which timed
      dispatch and undo together, jumped from 0.27 µs to 1.13 ms. The bench is
      now split (`dispatch/` and `undo/`). The fix is to keep a spread index,
      or to check only the spreads whose layers the transaction touched.
      `docs/memory/perf.md` has the numbers.
