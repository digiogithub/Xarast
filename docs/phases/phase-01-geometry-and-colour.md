# Phase 1 — Geometry & colour core

> After this phase every other crate has one vocabulary for coordinates, paths
> and colour — with the millipoint type's overflow behaviour pinned down as a
> correctness property rather than left to chance.

## Goal

Ship `xarast-geom` and `xarast-color`: the two leaf crates that everything else
depends on and that nothing in the project depends *upon* except `kurbo`,
`i_overlay` and the standard library.

`xarast-geom` owns the coordinate system (integer millipoints at the boundary,
`f64` inside, exactly as `docs/10-architecture.md §3.4` arbitrates), the path
representation that both the `.xar` importer and the SVG serialiser can express
without loss, and the path algebra: flattening, stroke expansion, offsetting,
boolean operations, arc length and hit-testing.

`xarast-color` owns the colour models, the conversions between them, the
built-in colour table that `.xar` negative references point at, and the derived
colour graph — tints, shades and links — from `research/02 §5.10` and
`research/01 §9.3`.

Both crates are `#![forbid(unsafe_code)]`, have no UI or GPU dependency, and
build on a machine with no display.

## Scope

### In scope

- `Mp`, the millipoint fixed-point scalar, with a **specified** overflow
  contract and a validated document extent.
- `Point`, `Vector`, `Rect`, `Matrix`, `Fixed16`, `Fixed24`, `BiasGain`.
- `Path`: parallel verb/point/flag arrays; builders; iteration by segment and by
  subpath; conversions to and from `kurbo::BezPath` and SVG path data.
- Adaptive flattening with a traceability table (source segment → output vertex
  range), because the boolean pipeline needs it.
- Stroke-to-path, dash expansion, offsetting.
- Boolean operations behind a facade, with the engine choice and its fallback
  named.
- Arc length, point-at-distance, nearest point, hit-testing for fill and stroke.
- Colour models and conversions; built-in/indexed colours; tints, shades and
  links with cycle-safe resolution; ramp interpolation effects; transparency
  values and modes.
- `criterion` benchmarks with budgets, `proptest` property suites, and the first
  fuzz target outside `.xar` (`fuzz_path_boolean`).

### Explicitly out of scope (and which phase owns it)

| Out of scope | Owner |
|---|---|
| `FillGeometry`, gradients, ramps as *document objects* (the `Stop` trait lives here; the geometry that uses it does not) | Phase 2 |
| Attribute nodes, the attribute stack, anything that knows what a document is | Phase 2 |
| Reading millipoints out of a `.xar` record | Phase 3 |
| Rasterising a path, antialiasing, tiling, blend modes | Phase 4 |
| ICC profiles and colour-managed conversion (`qcms`) | Phase 10/11 |
| Glyph outlines (they arrive as `kurbo::BezPath` from `skrifa` and convert through this crate's existing API) | Phase 9 |
| Mesh, three- and four-point gradient *evaluation* (their control points are Phase 2 data; interpolating them is Phase 4) | Phase 2 / Phase 4 |
| Arc and elliptical-arc segments as a first-class `Segment` variant | Deferred; see "Risks", item 5 |
| The arena-vs-persistent-store benchmark, which the prose of `10-architecture.md §3.1` still attributes to "Phase 1" | **Phase 2**, as `10-architecture.md §7` question 1 now states: it measures the document model, which does not exist yet |

## Prerequisites

- Phase 0 closed: workspace, lints, CI, `xarast-testkit` with the golden and
  budget helpers, `criterion` and `cargo-fuzz` scaffolding.
- No dependency on Phase 2 or Phase 3. This phase runs **in parallel with
  Phase 2** (`docs/phases/00-roadmap.md`), and the contract between them is the
  public API below, which must be agreed before either starts moving.

## Workstreams

### W1.1 — The millipoint scalar

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 1.1.1 | `Mp` type, constants, unit conversions | `xarast-geom` | S | — |
| 1.1.2 | Arithmetic with the specified overflow contract; `checked_*`, `saturating_*`, `scale`, `mul_ratio`, `div_round`, `sum` | `xarast-geom` | M | 1.1.1 |
| 1.1.3 | Document extent, `clamp_to_extent`, `is_in_extent` | `xarast-geom` | S | 1.1.2 |
| 1.1.4 | `Display`/`Debug`, parsing from a unit-bearing string | `xarast-geom` | S | 1.1.1 |
| 1.1.5 | Exhaustive-boundary property tests for every operator | `xarast-geom` | M | 1.1.2 |

**The overflow contract — this is the load-bearing decision of the phase.**

A millipoint is 1/1000 of a PostScript point. `i32` gives ±2 147 483 647 mp
≈ ±2 147 483 pt ≈ ±29 826 in ≈ ±757 m, at a resolution of 0.35 µm
(`research/01 §5.1–5.2`). Three properties are in tension: we must never panic
(a `.xar` parser that panics on hostile input is a denial of service), we must
never wrap (a wrapped coordinate produces mirrored geometry that looks
*plausible* — `research/01 §11` item 5 records exactly this failure mode), and
we must not make every arithmetic expression return `Option`.

The contract:

1. **`Add`, `Sub`, `Neg`, `AddAssign`, `SubAssign` saturate**, identically in
   debug and release. Saturation is total, deterministic, monotone and
   order-preserving, so a saturated coordinate lands at the edge of the
   representable plane rather than on the opposite side of it. `Mp::MIN` is
   `i32::MIN + 1`, not `i32::MIN`, so that `-x` is total.
2. **`Mul` between two `Mp` values is not implemented.** Millipoints times
   millipoints is not a millipoint. Scaling goes through
   `Mp::scale(self, f: f64) -> Mp` (computed in `f64`, rounded half-away-from-zero,
   saturating) or `Mp::mul_ratio(self, num: i32, den: i32) -> Mp` (computed in
   `i64`, exact until the final saturating narrowing). `Div` between two `Mp`
   values yields `f64`; `Mp::div_round(self, d: i32) -> Mp` divides by a scalar.
3. **`checked_add`, `checked_sub`, `checked_scale`, `checked_mul_ratio`** return
   `Option<Mp>`. Every I/O boundary — the `.xar` relative-path decoder above
   all — uses the checked form and turns `None` into a diagnostic rather than a
   silently clamped point.
4. **A validated document extent.** `Mp::EXTENT = ±(2^30 − 1)` millipoints
   (≈ ±14.9 km, ≈ ±10 611 in). The guarantee this buys is worth stating
   explicitly, because it is what lets the rest of the codebase stop worrying:
   *for any two values inside the extent, `a + b` and `a − b` are exactly
   representable in `i32` and therefore never saturate.* Bounding-box unions,
   midpoints, deltas and stroke inflation all fall inside that guarantee.
   `Mp::is_in_extent()` and `Mp::clamp_to_extent() -> (Mp, bool)` (the `bool`
   says whether clamping happened) are the enforcement points; the importer
   clamps and records a diagnostic, and `DocumentBuilder` (Phase 2) rejects
   out-of-extent geometry outright.
5. **`Mp::sum(iter)` accumulates in `i64`** and saturates once at the end, so a
   long polyline's running total does not saturate mid-way and then recover into
   a wrong answer.
6. **No `From<f32>` and no `to_f32()`.** `f32` has a 24-bit significand and
   loses millipoint precision above 16 777 216 mp ≈ 16 777 pt, which
   `research/01 §5.2` measures as up to 233 pt of error in a large document.
   `Mp::to_f64()` is exact and infallible; `Mp::from_f64_round(v)` rounds
   half-away-from-zero and saturates. Anything wanting `f32` must ask for it
   explicitly at the GPU boundary and is that layer's problem.

Why not the alternatives: **wrapping** produces geometry that is wrong but
believable, which is the worst possible failure; **panicking operators** turn a
corrupt file into a crash, which `docs/00-vision-and-scope.md §3.5` forbids;
**`Option` everywhere** makes the flattening and hit-test inner loops unreadable
for a case that, inside the extent, provably cannot happen.

### W1.2 — Points, rects, matrices

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 1.2.1 | `Point`, `Vector` and their operators | `xarast-geom` | S | 1.1.2 |
| 1.2.2 | `Rect` with the Y-up, `lo` = bottom-left convention; union, intersection, containment, inflation, the empty sentinel | `xarast-geom` | M | 1.2.1 |
| 1.2.3 | `Matrix`; composition, inversion, point vs vector transform, AABB transform | `xarast-geom` | M | 1.2.1 |
| 1.2.4 | `Fixed16`, `Fixed24` codecs and the quantisation policy | `xarast-geom` | S | 1.1.1 |
| 1.2.5 | `BiasGain` (Schlick bias/gain) and its LUT builder | `xarast-geom` | S | — |
| 1.2.6 | `kurbo` interop: `to_affine`, `from_affine`, `Point ↔ kurbo::Point`, `Rect ↔ kurbo::Rect` | `xarast-geom` | S | 1.2.3 |

The tricky parts:

**Y is up.** `research/01 §5.3`: Xara's origin is the bottom-left corner of the
spread's page-bounding rectangle, `Rect::lo` is bottom-left and `Rect::hi` is
top-right. Every consumer that wants Y-down (SVG, PDF, the screen) applies
`y' = page_height − y` at its own boundary, and that flip is **not** this
crate's job. Putting the flip here would mean the document model silently
carries two conventions. `Rect` therefore asserts `lo.y <= hi.y` in debug and
normalises in release.

**Points translate, vectors do not.** `Matrix::transform_point` adds `(e, f)`;
`Matrix::transform_vector` does not. This mirrors a real trap in the format:
`research/01 §5.3` records that the major and minor axes of regular shapes are
written *without* the coordinate-origin translation while everything else gets
it. Two differently named methods make that impossible to get wrong by
accident; a single `transform()` would guarantee somebody eventually does.

**FIXED16 quantisation is lossy and must happen exactly once.** `a, b, c, d`
have 16 fractional bits, so a rotation carries up to ≈4.5 arcseconds of error
(`research/01 §5.4`, and §11 item 8 warns that composing quantised matrices
accumulates it). The policy: `Matrix` stores `a..d` as `f64` and `e, f` as `Mp`;
composition, inversion and every intermediate stay in `f64`;
`Matrix::quantise_fixed16()` exists solely for the `.xar` reader to model what
the file could represent, and is **never** applied to an intermediate result.
The doc comment says so and a clippy-visible `#[doc(alias)]` points at this
paragraph.

**`Rect::EMPTY`** is `lo = (MAX, MAX)`, `hi = (MIN, MIN)`, so that
`EMPTY.union(r) == r` for every `r` with no special case. `is_empty()` tests
`lo.x > hi.x || lo.y > hi.y`.

### W1.3 — Path representation

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 1.3.1 | `Verb`, `PointFlags`, `Path`, the invariants and `Path::validate()` | `xarast-geom` | M | 1.2.1 |
| 1.3.2 | `PathBuilder` | `xarast-geom` | S | 1.3.1 |
| 1.3.3 | Segment and subpath iterators | `xarast-geom` | M | 1.3.1 |
| 1.3.4 | `kurbo::BezPath` conversion both ways | `xarast-geom` | S | 1.3.3 |
| 1.3.5 | SVG path-data writer and reader | `xarast-geom` | M | 1.3.3 |
| 1.3.6 | `bounds()` (control hull) and `tight_bounds()` (exact) | `xarast-geom` | S | 1.3.3 |

**The representation, and why it is this one.** Three parallel vectors:

```
verbs:  Vec<Verb>       MoveTo | LineTo | CubicTo | Close
points: Vec<Point>      arity:  1      | 1      | 3       | 0
flags:  Vec<PointFlags> parallel to `points`; empty means "all default"
```

This is segment-oriented (one verb per segment), which is what SVG, `kurbo` and
every rasteriser want. The `.xar` format is point-oriented (one verb byte per
point, with cubics spending three consecutive `PT_BEZIERTO` points and closure
encoded as the `PT_CLOSEFIGURE` bit on the last point of a subpath —
`research/01 §7.4`). Those two look incompatible, but they are not, because of
an identity worth writing down as an invariant:

> **`points.len()` equals the `.xar` point count exactly.** `MoveTo` and
> `LineTo` each consume one point, `CubicTo` consumes three, and `Close`
> consumes none — and in `.xar`, closure is a bit on an existing point rather
> than a point of its own. The arrays therefore line up one-to-one, in order.

That is what makes `TAG_PATH_FLAGS` (one byte per point, `research/01 §7.5`)
map straight onto our `flags` vector with no index arithmetic, and it is what
lets the `.xar` writer we will never write, and the `.xarast` writer we will,
round-trip point flags without a side table.

Invariants enforced by `Path::validate()` and by the builder's type state:

1. `points.len() == Σ arity(verb)`.
2. `flags.is_empty() || flags.len() == points.len()`.
3. The first verb is `MoveTo`; every `Close` is followed by `MoveTo` or by the
   end of the path.
4. No two consecutive `MoveTo` verbs (an empty subpath is dropped by the
   builder, not represented).
5. Every coordinate is inside `Mp::EXTENT`.

Fill and stroke intent (the `filled`/`stroked` bits that `.xar` encodes in the
path *tag*) are **not** stored here. They are attributes of the document node,
and they live in `xarast-doc` (Phase 2). `xarast-geom::Path` is pure geometry so
that `PartialEq` on it means "the same shape", which is what enables cheap
subtree comparison and resource deduplication later.

`PointFlags` are `SMOOTH`, `ROTATE` and `END_POINT`, matching
`research/01 §7.5`. `SELECTED` is deliberately **absent**: per
`research/02 §10.5`, point selection lives in an editor-side overlay, so that
selecting a control point does not invalidate the copy-on-write of the geometry
and `Path: Eq` keeps meaning what it says.

### W1.4 — Flattening

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 1.4.1 | `Tolerance` newtype and the device-pixel mapping | `xarast-geom` | S | 1.2.1 |
| 1.4.2 | `flatten()` on top of `kurbo`'s adaptive subdivision | `xarast-geom` | M | 1.3.3, 1.4.1 |
| 1.4.3 | `flatten_traced()` producing a `SegmentTrace` | `xarast-geom` | M | 1.4.2 |
| 1.4.4 | Deviation test harness (sample the curve, measure distance to the polyline) | `xarast-geom` | M | 1.4.2 |

Tolerance is expressed in **document units** (`f64` millipoints), never in
pixels, because a `Path` does not know the zoom. `Tolerance::from_device_px(px,
doc_units_per_px)` is the conversion every caller uses. Three named defaults:

- `Tolerance::RENDER` — derived per frame from 0.25 device pixels. Below about
  a quarter pixel, more subdivision buys no visible quality and costs
  proportional time.
- `Tolerance::BOOLEAN` — the input tolerance for the boolean pipeline. Starting
  value 10 mp (0.01 pt ≈ 3.5 µm). **The exact value is to be determined in this
  phase**, by sweeping it over the pathological-path regression corpus and
  picking the largest value whose worst-case area error stays under 1e-4 of the
  input area; the sweep and its result go in `docs/memory/perf.md`.
- `Tolerance::EXPORT` — 1 mp, for cases where the flattened result is the
  deliverable rather than an intermediate.

`SegmentTrace` is what makes the boolean refit step possible: for each output
vertex it records which input subpath and segment index it came from, and
whether it is an original on-curve point or a subdivision point. Without it,
step 3 of the boolean pipeline cannot tell "this whole output run came from one
untouched input cubic, restore the cubic" from "this run is a mixture, refit
it", and repeated booleans degrade curves into polylines —
`research/05 §8.4` rates that risk High/High.

### W1.5 — Stroke, dash, offset

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 1.5.1 | `Cap`, `Join`, `StrokeStyle`, `DashPattern` | `xarast-geom` | S | 1.1.1 |
| 1.5.2 | `stroke_to_path()` over `kurbo::stroke` | `xarast-geom` | M | 1.5.1, 1.3.3 |
| 1.5.3 | `dash()` over `kurbo::dash`, including the scaled-with-line-width variant | `xarast-geom` | M | 1.5.1 |
| 1.5.4 | `offset()` over `kurbo::offset`, plus self-union cleanup | `xarast-geom` | M | 1.5.2, W1.6 |
| 1.5.5 | Validation against `tiny-skia` as a test-only reference | `xarast-geom` | M | 1.5.2 |

`Cap` is `Butt | Round | Square` and `Join` is `Mitre | Round | Bevel`, matching
the `.xar` encodings 1/2/3 in both cases (`research/01 §8.2`). Start and end
caps are **separate fields**, because the format stores `TAG_STARTCAP` and
`TAG_ENDCAP` separately and `kurbo::Stroke` supports both.

**Hairlines.** `TAG_LINEWIDTH` of 0 means a hairline: a line one device pixel
wide regardless of zoom (`research/01 §8.2`). That is not expressible as a
filled path in document space, so `stroke_to_path` returns
`Err(StrokeError::Hairline)` rather than inventing a width. The renderer
(Phase 4) handles hairlines as a distinct primitive; this crate's job is to make
the case impossible to ignore.

**Offsetting self-intersects.** Naively offsetting a path with concave regions
produces loops. `offset()` therefore runs `kurbo`'s per-cubic offset, rebuilds
joins, and then passes the result through a **self-union** boolean to remove the
loops. That is why 1.5.4 depends on W1.6. Whether `cavalier_contours` does
better than this on arc-heavy input is **to be determined in this phase**, by a
one-day spike against a fixed set of ten inset/outset cases measured for
self-intersection count and Hausdorff distance to a densely-sampled reference;
if it wins, it goes behind the same `offset()` facade.

`tiny-skia` is a **test-only** dependency (`10-architecture.md §3.3` is explicit
that it never becomes a production dependency). It is used to rasterise our
stroke output and its own stroke of the same input, and compare coverage. That
catches join and cap errors that no property test would.

### W1.6 — Boolean operations

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 1.6.1 | `BoolOp`, `FillRule`, the `boolean()` facade | `xarast-geom` | S | 1.3.3 |
| 1.6.2 | Flatten → `i_overlay` with fixed `grid_size` → refit pipeline | `xarast-geom` | L | 1.4.3, 1.6.1 |
| 1.6.3 | Refit using `SegmentTrace` plus `kurbo::fit_to_bezpath_opt` for mixed runs | `xarast-geom` | L | 1.6.2 |
| 1.6.4 | Cleanup: coincident-point merge, micro-segment removal, subpath orientation | `xarast-geom` | M | 1.6.3 |
| 1.6.5 | Pathological regression corpus and `fuzz_path_boolean` | `xarast-geom` | M | 1.6.4 |
| 1.6.6 | `boolean-flo` fallback feature | `xarast-geom` | M | 1.6.1 |

**Engine chosen: `i_overlay` 9.x** (`MIT OR Apache-2.0`). It does union,
intersection, difference and xor, handles self-intersections, supports both
even-odd and non-zero, and — the reason it wins — has a **fixed-scale
`grid_size` mode that makes results bit-reproducible**. Reproducibility is not a
nicety here: golden-image tests and "undo then redo gives the same document"
both depend on the same input producing the same output byte for byte
(`research/05 §8.2`).

**Fallback named: `flo_curves` 0.8** (Apache-2.0), behind a non-default cargo
feature `boolean-flo`. It operates directly on curves, so it skips the
flatten/refit dance, at the cost of weaker robustness in degenerate cases. It
exists so that a blocking `i_overlay` bug does not stall the project; it is not
expected to ship.

**Long-term candidate: `i_curve`**, which does booleans natively on cubics and
rational elliptical arcs and is built on `i_overlay`. As of the research it was
days old with 168 total downloads. Re-evaluate when it reaches ≥1.0 with six
months of release history. Because `boolean()` is a facade over the whole
pipeline, adopting it is a contained change.

The pipeline, restated so the implementer does not have to go back to
`research/05 §8.3`:

1. `flatten_traced()` both operands at `Tolerance::BOOLEAN`, keeping the
   `SegmentTrace`.
2. Run `i_overlay` with a fixed `grid_size` derived from the tolerance, never
   from the input's magnitude — a grid that depends on the data is a grid that
   makes results depend on the data's bounding box.
3. Refit: for each output run whose trace says it came wholly from one input
   segment and preserved its endpoints, restore the original cubic verbatim. For
   mixed runs, `kurbo::fit_to_bezpath_opt` at `Tolerance::BOOLEAN / 2`.
4. Clean: merge points closer than 1 mp, drop segments shorter than 1 mp, drop
   subpaths whose area is under 1 mp², and normalise subpath orientation
   (outer counter-clockwise, holes clockwise) so that equal shapes compare
   equal.

Step 4's orientation normalisation is what makes `A ∪ ∅ == A` hold as a
structural equality and not merely as a set equality, which in turn is what
makes it a usable property test.

### W1.7 — Measurement and queries

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 1.7.1 | `arclen()`, `point_at_arclen()`, `param_at_distance()` | `xarast-geom` | M | 1.3.3 |
| 1.7.2 | `nearest_point()` | `xarast-geom` | M | 1.3.3 |
| 1.7.3 | `hit_fill()` (winding, both rules) and `hit_stroke()` | `xarast-geom` | M | 1.7.2 |
| 1.7.4 | Segment bounding-box acceleration for hit-testing large paths | `xarast-geom` | M | 1.7.3 |
| 1.7.5 | `Path::area()`, `Path::centroid()` | `xarast-geom` | S | 1.3.3 |

Arc length feeds text-on-a-path (Phase 9) and dash placement; `kurbo`'s
`ParamCurveArclen` is accurate to a requested tolerance, and
`point_at_arclen` inverts it by bisection over the cumulative table. Building
the cumulative table once per path and caching it in the caller (not here — this
crate is stateless) is the documented usage.

`hit_stroke` inflates by half the stroke width and tests distance rather than
constructing the stroke outline, because hit-testing runs on every mouse move
and stroke expansion does not. Caps and joins make this an approximation near
the ends; the error is bounded by half the stroke width and is below the
pick tolerance a user can perceive. Document the approximation; do not hide it.

Acceleration structure for 1.7.4: a flat array of per-segment bounding boxes
plus a coarse uniform grid over the path's bounds. A BVH is more elegant and
`parry2d` has one, but `parry2d` is pure Apache-2.0 and physics-shaped, and
`research/05 §8.1` already rejected it. Whether the grid is enough for a
100 000-segment path is **to be determined in this phase**, by the `hit_fill`
benchmark; if it misses the budget, the fallback is a static BVH built in
`xarast-geom` itself.

### W1.8 — `xarast-color`

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 1.8.1 | `ColourModel`, `ColourValue`, `Rgba8`, component clamping | `xarast-color` | M | — |
| 1.8.2 | Conversions between all models | `xarast-color` | M | 1.8.1 |
| 1.8.3 | `Fixed24` decoding including the inherit sentinel | `xarast-color` | S | — |
| 1.8.4 | `BuiltinColour`: the negative-reference table | `xarast-color` | S | 1.8.1 |
| 1.8.5 | `ColourDef`, `ColourKind`, `ColourTable`, cycle-safe resolution | `xarast-color` | L | 1.8.2, 1.8.4 |
| 1.8.6 | `Transparency`, `TranspMode`, the `Stop` trait | `xarast-color` | S | 1.8.1 |
| 1.8.7 | `interpolate()` with `FillEffect` (Fade, Rainbow, AltRainbow) | `xarast-color` | M | 1.8.2 |

**Component representation.** `f32` in `0.0..=1.0`, which `research/02 §10.8`
argues for and which is the one place in the model where floats replace
integers: colour-space conversion gains nothing from fixed point, and the file
format stores scaled integers that are converted at the boundary anyway.
Components are clamped on construction, so no `ColourValue` ever holds a NaN or
an out-of-range channel; `Fixed24::to_f32` is the only entry point that can see
out-of-range input, and it is where the clamping happens.

**The inherit sentinel.** `FIXED24` `0xF800_0000` is −8.0 and means "inherit
this component from the parent colour" (`research/01 §9.1`, §11 item 9). Read
literally it produces absurd colours. `Fixed24::INHERIT` and
`Fixed24::is_inherit()` make the check explicit, and `ColourDef::components` is
`[Option<f32>; 4]` where `None` *is* inherit — the type makes forgetting the
check a compile error rather than a rendering bug.

**Derived colours.** `research/01 §9.3` gives five types:

| `.xar` value | `ColourKind` | Semantics |
|---|---|---|
| 0 | `Normal` | Independent colour |
| 1 | `Spot` | Named spot ink, its own separation |
| 2 | `Tint { factor }` | Component 1 is the tint factor 0..1 of `parent` |
| 3 | `Linked { }` | Components marked inherit come from `parent`, the rest override |
| 4 | `Shade { x, y }` | Components 1 and 2 are the shade coordinates over `parent` |

Resolution walks the `parent` chain. The file format guarantees a parent is
written before its child (`research/01 §9.4`), so a single incremental pass
suffices for well-formed input — but this crate must also survive corrupt input,
so `ColourTable::resolve` is depth-limited to `MAX_PARENT_DEPTH = 16` and falls
back to the definition's cached RGB triple when the limit is hit or a cycle is
detected. `ColourTable::validate()` reports cycles explicitly for `xar-dump`.

The cached 8-bit RGB triple is not a nicety either: `research/01 §9.4` notes
that Xara itself stores it precisely so simple readers can paint without
implementing the full model, and it is the correct fallback whenever resolution
fails.

**CMYK → RGB is deliberately naive.** The original uses
`R = 1 − min(1, C + K)` and so do we (`research/01 §9.5`). This is a fidelity
decision, not an oversight: matching Xara's on-screen appearance for two decades
of existing documents matters more than being colourimetrically right, and there
are no ICC profiles in the format to be right *with*. Colour-managed conversion
arrives in Phase 10/11 as an opt-in path, and the naive one stays as the default
for `.xar` documents. Say this in the doc comment, or someone will "fix" it.

**Ramp interpolation effects** (`research/01 §8.3`, `research/02 §5.7`):
`Fade` interpolates componentwise in RGB, `Rainbow` interpolates hue the short
way round in HSV, `AltRainbow` the long way. All three take the endpoints in
whatever models they are defined in, convert to the working model, interpolate,
and convert back.

**The `Stop` trait** lives here because its two implementors do:

```rust
pub trait Stop: Clone + PartialEq + core::fmt::Debug {
    fn lerp(&self, other: &Self, t: f32, effect: FillEffect) -> Self;
}
```

`FillGeometry<S: Stop>` in `xarast-doc` (Phase 2) is generic over it, which is
how one gradient type replaces the ~60 parallel colour and transparency classes
of the original (`research/02 §10.7`).

## Public API introduced

```rust
// ═══ xarast-geom ═════════════════════════════════════════════════════════════

// ─── Scalar ──────────────────────────────────────────────────────────────────

/// A coordinate in millipoints: 1/1000 of a PostScript point.
/// See the overflow contract in `docs/phases/phase-01-geometry-and-colour.md`.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct Mp(pub i32);

impl Mp {
    pub const ZERO: Mp;
    pub const ONE_PT: Mp;                  // 1_000
    /// `i32::MIN + 1`, so that negation is total.
    pub const MIN: Mp;
    pub const MAX: Mp;
    /// The validated document extent: +-(2^30 - 1) mp ~= +-14.9 km.
    /// Within it, `a + b` and `a - b` are exact for any two values.
    pub const EXTENT_MIN: Mp;
    pub const EXTENT_MAX: Mp;

    pub const PER_PT: i32;                 // 1_000
    pub const PER_PICA: i32;               // 12_000
    pub const PER_INCH: i32;               // 72_000
    pub const PER_PX96: i32;               // 750
    pub const PER_MM: f64;                 // 2834.652715
    pub const PER_CM: f64;                 // 28346.52715

    pub const fn new(raw: i32) -> Mp;
    pub const fn raw(self) -> i32;

    // Exact, infallible. There is deliberately no `to_f32`.
    pub const fn to_f64(self) -> f64;
    /// Rounds half away from zero, saturates at `MIN`/`MAX`.
    pub fn from_f64_round(v: f64) -> Mp;

    pub fn from_pt(v: f64) -> Mp;
    pub fn from_mm(v: f64) -> Mp;
    pub fn from_inch(v: f64) -> Mp;
    pub fn from_px(v: f64, dpi: f64) -> Mp;
    pub fn to_pt(self) -> f64;
    pub fn to_mm(self) -> f64;
    pub fn to_inch(self) -> f64;
    pub fn to_px(self, dpi: f64) -> f64;

    // Saturating by contract; `Add`/`Sub`/`Neg` delegate to these.
    pub const fn saturating_add(self, rhs: Mp) -> Mp;
    pub const fn saturating_sub(self, rhs: Mp) -> Mp;
    pub const fn saturating_neg(self) -> Mp;

    pub const fn checked_add(self, rhs: Mp) -> Option<Mp>;
    pub const fn checked_sub(self, rhs: Mp) -> Option<Mp>;

    /// `self * f`, in `f64`, rounded half away from zero, saturating.
    pub fn scale(self, f: f64) -> Mp;
    pub fn checked_scale(self, f: f64) -> Option<Mp>;
    /// `self * num / den`, exact in `i64` until the final narrowing.
    pub fn mul_ratio(self, num: i32, den: i32) -> Mp;
    pub fn checked_mul_ratio(self, num: i32, den: i32) -> Option<Mp>;
    pub fn div_round(self, d: i32) -> Mp;

    /// Accumulates in `i64`, saturates once at the end.
    pub fn sum<I: IntoIterator<Item = Mp>>(iter: I) -> Mp;

    pub const fn is_in_extent(self) -> bool;
    /// Returns the clamped value and whether clamping occurred.
    pub const fn clamp_to_extent(self) -> (Mp, bool);

    pub const fn abs(self) -> Mp;
    pub const fn signum(self) -> i32;
    pub fn midpoint(self, other: Mp) -> Mp;
}

// `Mul<Mp> for Mp` is intentionally NOT implemented: mp x mp is not mp.
impl core::ops::Add for Mp { /* saturating */ }
impl core::ops::Sub for Mp { /* saturating */ }
impl core::ops::Neg for Mp { /* saturating */ }
impl core::ops::Div for Mp { type Output = f64; }
impl core::fmt::Display for Mp { /* e.g. "12.345pt" */ }

// ─── Fixed-point codecs ──────────────────────────────────────────────────────

/// 16 fractional bits. `.xar` matrix coefficients and angles (radians).
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
#[repr(transparent)]
pub struct Fixed16(pub i32);
impl Fixed16 {
    pub const fn to_f64(self) -> f64;              // raw / 65536.0
    pub fn from_f64_round(v: f64) -> Fixed16;      // saturating
}

// ─── Points, vectors, rects ──────────────────────────────────────────────────

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct Point { pub x: Mp, pub y: Mp }

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct Vector { pub dx: Mp, pub dy: Mp }

impl Point {
    pub const ORIGIN: Point;
    pub const fn new(x: Mp, y: Mp) -> Point;
    pub fn to_f64(self) -> (f64, f64);
    pub fn distance_to(self, other: Point) -> f64;
    pub const fn is_in_extent(self) -> bool;
}
impl core::ops::Sub for Point { type Output = Vector; }
impl core::ops::Add<Vector> for Point { type Output = Point; }

/// Y-up, `lo` = bottom-left, `hi` = top-right. `EMPTY` is the union identity.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Rect { pub lo: Point, pub hi: Point }

impl Rect {
    pub const EMPTY: Rect;                 // lo = (MAX, MAX), hi = (MIN, MIN)
    pub fn new(lo: Point, hi: Point) -> Rect;      // normalises
    pub const fn is_empty(self) -> bool;
    pub fn width(self) -> Mp;
    pub fn height(self) -> Mp;
    pub fn centre(self) -> Point;
    pub fn union(self, other: Rect) -> Rect;
    pub fn union_point(self, p: Point) -> Rect;
    pub fn intersection(self, other: Rect) -> Rect;
    pub fn intersects(self, other: Rect) -> bool;
    pub fn contains(self, p: Point) -> bool;
    pub fn contains_rect(self, other: Rect) -> bool;
    pub fn inflated(self, by: Mp) -> Rect;
    pub fn translated(self, by: Vector) -> Rect;
}

// ─── Matrices ────────────────────────────────────────────────────────────────

/// | x' |   | a c | | x |   | e |
/// | y' | = | b d | | y | + | f |
///
/// `a..d` are `f64` even though `.xar` stores FIXED16: quantisation happens
/// once, on write, never on an intermediate. See `quantise_fixed16`.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Matrix { pub a: f64, pub b: f64, pub c: f64, pub d: f64, pub e: Mp, pub f: Mp }

impl Matrix {
    pub const IDENTITY: Matrix;
    pub fn translate(by: Vector) -> Matrix;
    pub fn scale(sx: f64, sy: f64) -> Matrix;
    pub fn scale_about(sx: f64, sy: f64, about: Point) -> Matrix;
    pub fn rotate(radians: f64) -> Matrix;
    pub fn rotate_about(radians: f64, about: Point) -> Matrix;
    pub fn skew(ax: f64, ay: f64) -> Matrix;

    /// `self` first, then `other`.
    pub fn then(self, other: Matrix) -> Matrix;
    pub fn invert(self) -> Option<Matrix>;
    pub fn determinant(self) -> f64;

    /// Applies the linear part AND the translation.
    pub fn transform_point(self, p: Point) -> Point;
    /// Applies the linear part ONLY. Regular-shape axes are written untranslated
    /// (`research/01 §5.3`); using the wrong one here is a real, observed bug class.
    pub fn transform_vector(self, v: Vector) -> Vector;
    /// Axis-aligned bounds of the transformed rectangle.
    pub fn transform_rect(self, r: Rect) -> Rect;

    pub fn is_identity(self) -> bool;
    pub fn is_translation_only(self) -> bool;
    /// Largest singular value: how much this matrix can stretch a length.
    pub fn max_scale(self) -> f64;

    /// For the `.xar`/`.xarast` boundary only. NEVER on an intermediate result.
    pub fn quantise_fixed16(self) -> Matrix;

    pub fn to_affine(self) -> kurbo::Affine;
    pub fn from_affine(a: kurbo::Affine) -> Matrix;
}

// ─── Profiles ────────────────────────────────────────────────────────────────

/// Schlick bias/gain, both in `-1.0..=1.0`. Used by gradients, contours,
/// shadows, feather and blends (`research/02 §5.7`).
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct BiasGain { pub bias: f64, pub gain: f64 }

impl BiasGain {
    pub const IDENTITY: BiasGain;
    pub fn new(bias: f64, gain: f64) -> BiasGain;   // clamps to [-1, 1]
    /// Maps `0.0..=1.0` to `0.0..=1.0`.
    pub fn map(self, t: f64) -> f64;
    /// Precomputes a lookup table of `n` entries.
    pub fn lut(self, n: usize) -> Vec<f32>;
}

// ─── Paths ───────────────────────────────────────────────────────────────────

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Verb { MoveTo = 0, LineTo = 1, CubicTo = 2, Close = 3 }

impl Verb {
    /// Points consumed: MoveTo 1, LineTo 1, CubicTo 3, Close 0.
    pub const fn arity(self) -> usize;
}

bitflags::bitflags! {
    /// Editing metadata, one entry per point, mirroring `TAG_PATH_FLAGS`
    /// (`research/01 §7.5`). No SELECTED bit: selection is an editor overlay.
    #[derive(Copy, Clone, Default, PartialEq, Eq, Debug)]
    pub struct PointFlags: u8 {
        const SMOOTH    = 1 << 0;
        const ROTATE    = 1 << 1;
        const END_POINT = 1 << 2;
    }
}

/// Pure geometry. Fill/stroke intent belongs to the document node, not here,
/// so that `PartialEq` means "the same shape".
#[derive(Clone, PartialEq, Eq, Default, Debug)]
pub struct Path { /* verbs, points, flags */ }

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Segment {
    Line  { p0: Point, p1: Point },
    Cubic { p0: Point, p1: Point, p2: Point, p3: Point },
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct SubPathRef { pub verb_range: core::ops::Range<usize>, pub closed: bool }

#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("point count {points} does not match verb arity total {expected}")]
    ArityMismatch { points: usize, expected: usize },
    #[error("flags length {flags} != point count {points}")]
    FlagsMismatch { flags: usize, points: usize },
    #[error("path does not start with MoveTo")]
    MissingMoveTo,
    #[error("coordinate at index {index} is outside the document extent")]
    OutOfExtent { index: usize },
}

impl Path {
    pub fn new() -> Path;
    pub fn builder() -> PathBuilder;

    pub fn verbs(&self) -> &[Verb];
    pub fn points(&self) -> &[Point];
    /// Empty when every point has default flags.
    pub fn flags(&self) -> &[PointFlags];
    pub fn set_flags(&mut self, flags: Vec<PointFlags>) -> Result<(), PathError>;

    pub fn is_empty(&self) -> bool;
    pub fn segment_count(&self) -> usize;
    pub fn subpaths(&self) -> impl Iterator<Item = SubPathRef> + '_;
    pub fn segments(&self) -> impl Iterator<Item = Segment> + '_;

    /// Bounds of the control hull: cheap, conservative, never smaller than tight.
    pub fn bounds(&self) -> Rect;
    /// Exact bounds of the curve. Costs a root-solve per cubic.
    pub fn tight_bounds(&self) -> Rect;

    pub fn signed_area(&self) -> f64;       // square millipoints
    pub fn centroid(&self) -> Option<Point>;

    pub fn transformed(&self, m: Matrix) -> Path;
    pub fn reversed(&self) -> Path;
    /// Normalises subpath orientation: outer CCW, holes CW.
    pub fn normalised(&self) -> Path;

    pub fn validate(&self) -> Result<(), PathError>;

    pub fn to_bez_path(&self) -> kurbo::BezPath;
    /// Quantises to millipoints; clamps to the extent and reports whether it did.
    pub fn from_bez_path(p: &kurbo::BezPath) -> (Path, bool);

    pub fn to_svg_path_data(&self) -> String;
    pub fn from_svg_path_data(s: &str) -> Result<Path, PathError>;
}

#[derive(Default, Debug)]
pub struct PathBuilder { /* ... */ }

impl PathBuilder {
    pub fn move_to(&mut self, p: Point) -> &mut Self;
    pub fn line_to(&mut self, p: Point) -> &mut Self;
    pub fn cubic_to(&mut self, c1: Point, c2: Point, p: Point) -> &mut Self;
    /// Quadratic is elevated to cubic on insertion; there is no Quad verb.
    pub fn quad_to(&mut self, c: Point, p: Point) -> &mut Self;
    pub fn close(&mut self) -> &mut Self;
    pub fn rect(&mut self, r: Rect) -> &mut Self;
    pub fn ellipse(&mut self, centre: Point, rx: Mp, ry: Mp) -> &mut Self;
    pub fn point_flags(&mut self, f: PointFlags) -> &mut Self;   // applies to the last point
    pub fn build(self) -> Path;                                  // upholds every invariant
}

// ─── Flattening ──────────────────────────────────────────────────────────────

/// Tolerance in document units (f64 millipoints). Never pixels: a `Path` does
/// not know the zoom.
#[derive(Copy, Clone, PartialEq, PartialOrd, Debug)]
pub struct Tolerance(pub f64);

impl Tolerance {
    pub const EXPORT: Tolerance;                       // 1 mp
    /// Boolean-pipeline input tolerance. Initial value 10 mp; see the phase doc.
    pub const BOOLEAN: Tolerance;
    pub const RENDER_DEVICE_PX: f64;                   // 0.25
    pub fn from_device_px(px: f64, doc_units_per_px: f64) -> Tolerance;
}

#[derive(Clone, Debug, Default)]
pub struct Polyline { pub points: Vec<Point>, pub closed: bool }

/// Where each flattened vertex came from. Required by the boolean refit step.
#[derive(Clone, Debug, Default)]
pub struct SegmentTrace { /* per output vertex: (subpath, segment, is_original) */ }

impl SegmentTrace {
    pub fn source_of(&self, polyline: usize, vertex: usize) -> Option<(usize, usize)>;
    pub fn is_original_vertex(&self, polyline: usize, vertex: usize) -> bool;
}

pub fn flatten(path: &Path, tol: Tolerance) -> Vec<Polyline>;
pub fn flatten_traced(path: &Path, tol: Tolerance) -> (Vec<Polyline>, SegmentTrace);

// ─── Stroke, dash, offset ────────────────────────────────────────────────────

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
#[repr(u8)]
pub enum Cap { #[default] Butt = 1, Round = 2, Square = 3 }      // matches `.xar`

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
#[repr(u8)]
pub enum Join { #[default] Mitre = 1, Round = 2, Bevel = 3 }     // matches `.xar`

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
#[repr(u8)]
pub enum FillRule { #[default] NonZero = 1, Negative = 2, EvenOdd = 3, Positive = 4 }

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DashPattern {
    /// Alternating on/off lengths, in millipoints.
    pub elements: Vec<Mp>,
    pub offset: Mp,
    /// The line width the pattern was authored at, when it scales with width.
    pub reference_width: Option<Mp>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct StrokeStyle {
    /// `Mp::ZERO` means hairline: one device pixel, not expressible as a path.
    pub width: Mp,
    pub cap_start: Cap,
    pub cap_end: Cap,
    pub join: Join,
    pub mitre_limit: f64,
    pub dash: Option<DashPattern>,
}

#[derive(Debug, thiserror::Error)]
pub enum StrokeError {
    #[error("hairline strokes have no document-space outline; the renderer must handle them")]
    Hairline,
    #[error("degenerate stroke parameters: {0}")]
    Degenerate(&'static str),
}

pub fn stroke_to_path(path: &Path, style: &StrokeStyle, tol: Tolerance)
    -> Result<Path, StrokeError>;

pub fn dash(path: &Path, pattern: &DashPattern, line_width: Mp, tol: Tolerance) -> Path;

/// Positive distance grows the shape. Self-intersections are removed by a
/// self-union, which is why this depends on the boolean engine.
pub fn offset(path: &Path, distance: Mp, join: Join, tol: Tolerance) -> Path;

// ─── Boolean ─────────────────────────────────────────────────────────────────

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum BoolOp { Union, Intersection, Difference, Xor }

/// Deterministic by construction: the grid is fixed by `tol`, never by the input.
pub fn boolean(a: &Path, b: &Path, op: BoolOp, rule: FillRule, tol: Tolerance) -> Path;

/// Removes self-intersections from a single path.
pub fn self_union(a: &Path, rule: FillRule, tol: Tolerance) -> Path;

// ─── Measurement and hit-testing ─────────────────────────────────────────────

pub fn arclen(path: &Path, accuracy: f64) -> f64;
pub fn point_at_arclen(path: &Path, distance: f64, accuracy: f64) -> Option<(Point, Vector)>;

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Nearest { pub point: Point, pub distance: f64, pub subpath: usize, pub segment: usize, pub t: f64 }

pub fn nearest_point(path: &Path, p: Point, accuracy: f64) -> Option<Nearest>;

pub fn hit_fill(path: &Path, p: Point, rule: FillRule) -> bool;
/// Distance-based approximation, exact to within half the stroke width near
/// caps and joins. Deliberately does not build the stroke outline.
pub fn hit_stroke(path: &Path, p: Point, half_width: Mp, tol: Tolerance) -> bool;

/// Per-segment bounding boxes plus a coarse grid. Build once, query many times.
#[derive(Clone, Debug)]
pub struct HitIndex { /* ... */ }
impl HitIndex {
    pub fn build(path: &Path) -> HitIndex;
    pub fn hit_fill(&self, path: &Path, p: Point, rule: FillRule) -> bool;
    pub fn hit_stroke(&self, path: &Path, p: Point, half_width: Mp, tol: Tolerance) -> bool;
}

// ═══ xarast-color ════════════════════════════════════════════════════════════

/// Discriminants match the `colour_model` byte of `TAG_DEFINECOMPLEXCOLOUR`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum ColourModel { Indexed = 0, Ciet = 1, Rgbt = 2, Cmyk = 3, Hsvt = 4, Greyt = 5, WebRgbt = 6 }

/// All components clamped to `0.0..=1.0` on construction. Never NaN.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum ColourValue {
    Rgbt   { r: f32, g: f32, b: f32, t: f32 },
    Cmyk   { c: f32, m: f32, y: f32, k: f32 },
    Hsvt   { h: f32, s: f32, v: f32, t: f32 },   // h normalised 0..1, not degrees
    Greyt  { v: f32, t: f32 },
    Ciet   { x: f32, y: f32, z: f32, t: f32 },
    WebRgb { r: u8, g: u8, b: u8 },
}

impl ColourValue {
    pub const BLACK: ColourValue;
    pub const WHITE: ColourValue;
    pub fn model(self) -> ColourModel;
    pub fn to_rgbt(self) -> ColourValue;
    pub fn to_cmyk(self) -> ColourValue;
    pub fn to_hsvt(self) -> ColourValue;
    pub fn to_greyt(self) -> ColourValue;
    pub fn to_model(self, m: ColourModel) -> ColourValue;
    pub fn to_rgba8(self) -> Rgba8;
    /// Relative luminance of the sRGB approximation, for contrast decisions.
    pub fn luminance(self) -> f32;
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct Rgba8 { pub r: u8, pub g: u8, pub b: u8, pub a: u8 }

/// 24 fractional bits. Only appears in `.xar` colour components.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Fixed24(pub i32);

impl Fixed24 {
    /// `0xF800_0000` = -8.0: "inherit this component from the parent colour"
    /// (`research/01 §9.1`). Reading it literally gives absurd colours.
    pub const INHERIT: Fixed24;
    pub const fn is_inherit(self) -> bool;
    /// `None` when the value is the inherit sentinel. Clamps to `0.0..=1.0`.
    pub fn to_f32(self) -> Option<f32>;
    pub fn from_f32(v: f32) -> Fixed24;
}

/// The `.xar` predefined colours, addressed by negative reference
/// (`research/01 §6.3`).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum BuiltinColour {
    None = -1, Black = -2, White = -3, Red = -4, Green = -5, Blue = -6,
    Cyan = -7, Magenta = -8, Yellow = -9, CmykKey = -10,
}

impl BuiltinColour {
    pub fn from_ref(v: i32) -> Option<BuiltinColour>;
    /// `None` for `BuiltinColour::None`, which means "no colour", not black.
    pub fn value(self) -> Option<ColourValue>;
}

slotmap::new_key_type! { pub struct ColourId; }

#[derive(Clone, PartialEq, Debug)]
pub enum ColourKind {
    Normal,
    Spot,
    /// `.xar` type 2: component 1 is the tint factor over the parent.
    Tint { factor: f32 },
    /// `.xar` type 3: inherited components come from the parent.
    Linked,
    /// `.xar` type 4: components 1 and 2 are the shade coordinates.
    Shade { x: f32, y: f32 },
}

#[derive(Clone, PartialEq, Debug)]
pub struct ColourDef {
    pub name: Option<std::sync::Arc<str>>,
    pub model: ColourModel,
    pub kind: ColourKind,
    pub parent: Option<ColourId>,
    /// `None` means "inherit from the parent" (the FIXED24 -8.0 sentinel).
    pub components: [Option<f32>; 4],
    /// The 8-bit approximation the file carries. The fallback when resolution
    /// fails, exactly as Xara's own simple readers use it.
    pub cached_rgb: Rgba8,
    pub entry_index: u32,
}

#[derive(Clone, Default, Debug)]
pub struct ColourTable { /* SlotMap<ColourId, ColourDef> + name index */ }

#[derive(Debug, thiserror::Error)]
pub enum ColourError {
    #[error("parent chain for {0:?} exceeds the depth limit or contains a cycle")]
    ParentChain(ColourId),
    #[error("unknown colour id {0:?}")]
    Unknown(ColourId),
}

impl ColourTable {
    pub const MAX_PARENT_DEPTH: usize = 16;

    pub fn insert(&mut self, def: ColourDef) -> ColourId;
    pub fn get(&self, id: ColourId) -> Option<&ColourDef>;
    pub fn by_name(&self, name: &str) -> Option<ColourId>;
    pub fn len(&self) -> usize;

    /// Walks the parent chain applying tint, shade and link semantics.
    /// Depth-limited and cycle-safe; falls back to `cached_rgb` on failure.
    pub fn resolve(&self, id: ColourId) -> ColourValue;
    pub fn try_resolve(&self, id: ColourId) -> Result<ColourValue, ColourError>;
    pub fn resolve_rgba8(&self, id: ColourId) -> Rgba8;
    /// Reports every cycle and depth violation; used by `xar-dump`.
    pub fn validate(&self) -> Vec<ColourError>;
}

/// Either a literal value or a live reference into the document palette.
#[derive(Clone, PartialEq, Debug)]
pub enum Colour {
    Direct(ColourValue),
    Indexed { id: ColourId, tint: Option<f32> },
}

/// A transparency is a fill payload, not an alpha channel: a scalar level plus
/// a compositing mode (`research/02 §5.5`).
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct Transparency { pub level: u8, pub mode: TranspMode }

/// Discriminants match the `.xar` transparency `type` byte
/// (`research/01 §8.4`). Unknown values map to `Mix`.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
#[repr(u8)]
pub enum TranspMode {
    None = 0, #[default] Mix = 1, StainedGlass = 2, Bleach = 3,
    Contrast = 13, Saturation = 16, Darken = 19, Lighten = 22,
    Brightness = 25, Luminosity = 28,
}
impl TranspMode { pub fn from_byte(v: u8) -> TranspMode; }

/// How a gradient interpolates between two stops (`research/01 §8.3`).
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum FillEffect { #[default] Fade, Rainbow, AltRainbow }

/// Implemented by `Colour` and `Transparency`, so that `xarast-doc` can define
/// one generic `FillGeometry<S: Stop>` instead of ~60 parallel classes.
pub trait Stop: Clone + PartialEq + core::fmt::Debug {
    fn lerp(&self, other: &Self, t: f32, effect: FillEffect) -> Self;
}

pub fn interpolate(a: ColourValue, b: ColourValue, t: f32, effect: FillEffect) -> ColourValue;
```

## Acceptance criteria

1. `cargo nextest run -p xarast-geom -p xarast-color` passes, and
   `cargo clippy -p xarast-geom -p xarast-color --all-targets -- -D warnings`
   is clean.
2. `std::mem::size_of::<Mp>() == 4`, `size_of::<Point>() == 8`,
   `size_of::<Rect>() == 16`, `size_of::<Verb>() == 1`,
   `size_of::<PointFlags>() == 1`, asserted by tests.
3. A `proptest` suite of ≥ 1 000 000 generated cases over a boundary-weighted
   value set (`MIN`, `MIN+1`, `EXTENT_MIN`, `-1`, `0`, `1`, `EXTENT_MAX`,
   `MAX-1`, `MAX`, plus uniform randoms) exercises every `Mp` operator in both
   debug and release builds with **zero panics and zero wraps**. Wrapping is
   detected by recomputing in `i64` and comparing against the saturating
   expectation.
4. For all pairs `a, b` inside `Mp::EXTENT`, `a.checked_add(b).is_some()` and
   `a.checked_sub(b).is_some()` — the extent guarantee, tested exhaustively at
   the boundary and by 1 000 000 random pairs.
5. `Matrix` round-trip: for 10 000 random matrices,
   `m.then(m.invert().unwrap())` is within `1e-9` of `IDENTITY` in `a..d` and
   within 1 mp in `e, f`.
6. Flattening deviation: for every path in the pathological corpus and for
   tolerances `{1, 10, 100, 1000}` mp, the maximum distance from 1 000 samples
   per curve segment to the produced polyline is `≤ tolerance`.
7. `Path::validate()` accepts every path produced by `PathBuilder` and rejects
   each of the five invariant violations, one test per invariant.
8. SVG round-trip: for the pathological corpus,
   `Path::from_svg_path_data(&p.to_svg_path_data()) == p` exactly (millipoints
   are integers, so this is exact equality, not a tolerance).
9. `kurbo` round-trip: `Path::from_bez_path(&p.to_bez_path()).0 == p` exactly,
   with the clamping flag false.
10. Boolean property suite, ≥ 10 000 `proptest` cases each:
    `union(A, ∅) == A`, `A ∩ A == A.normalised()`, `difference(A, A)` is empty,
    `xor(A, B) == difference(union(A,B), intersection(A,B))`, and
    `|area(union(A,B)) + area(∩(A,B)) − area(A) − area(B)| ≤ 1e-4 · area(A ∪ B)`.
11. Boolean determinism: running the same operation twice in the same process,
    and once in a fresh process, produces byte-identical `Path` serialisations,
    for all 200 cases of the pathological corpus.
12. Boolean robustness: all 200 pathological cases (self-intersections,
    tangencies, nested subpaths, zero-area subpaths, coincident edges,
    100 000-point paths) complete without panic, without unbounded memory, and
    within the budgets below.
13. `stroke_to_path` coverage agreement: for 100 stroke cases rasterised at
    512×512 through `tiny-skia` (test-only), our stroke outline filled and
    `tiny-skia`'s own stroke of the same input differ in `≤ 0.2 %` of pixels,
    with no pixel differing by more than 8/255 in coverage.
14. `stroke_to_path` with `width == Mp::ZERO` returns `Err(StrokeError::Hairline)`.
15. `offset` output has zero self-intersections for all ten spike cases,
    verified by `self_union(out).segment_count() == out.segment_count()`.
16. `hit_fill` agrees with a rasterised reference (point-in-polygon by sampling
    the filled bitmap) on 100 000 random query points across the pathological
    corpus, for both `NonZero` and `EvenOdd`.
17. `arclen` agrees with a 100 000-sample polyline approximation to within
    `1e-6` relative, for 1 000 random cubics.
18. Colour conversions round-trip: `RGB → CMYK → RGB` and `RGB → HSV → RGB`
    return within `1/255` for all 16 777 216 8-bit RGB triples (exhaustive; the
    test runs in the nightly job, and a 65 536-sample subset runs per push).
19. `Fixed24::INHERIT.to_f32()` is `None`, and a `ColourDef` with all four
    components inheriting resolves to exactly its parent's value.
20. Tint, shade and link resolution match the three worked examples in
    `research/01 §9.3` (`Black` CMYK, `Red` HSVT, `90% Black` as a tint of
    `Black`), asserted as unit tests.
21. A `ColourTable` containing a two-node parent cycle, and one containing a
    chain of depth 17, both resolve without hanging and return the
    `cached_rgb` fallback; `validate()` reports both.
22. `cargo +nightly fuzz run fuzz_path_boolean -- -runs=1000000` finds no
    crash, no timeout over 5 s, and no allocation beyond 256 MB.
23. All Phase 1 benchmarks run and every budget in the table below is met on
    the reference machine, with the numbers written to
    `target/perf-report.jsonl` and copied into `docs/memory/perf.md`.

## Performance budgets

Measured with `criterion` on the reference machine (recorded in
`docs/memory/perf.md`) in the `release` profile. These are **initial budgets**:
the first measurement re-bases any line that turns out to be off by more than
2×, and the re-based value is what CI then gates on. A regression against the
recorded value fails the build.

| Operation | Input | Budget |
|---|---|---|
| `Mp` arithmetic | — | Must compile to the same instruction count as raw `i32` saturating ops; verified by a `#[bench]` that the operator and the manual `saturating_add` differ by < 5 % |
| `Path::bounds` | 10 000 segments | ≤ 30 µs |
| `Path::tight_bounds` | 10 000 segments | ≤ 400 µs |
| `flatten` at 0.25 device px | 10 000 segments | ≤ 2 ms |
| `flatten_traced` | 10 000 segments | ≤ 1.3× the cost of `flatten` |
| `stroke_to_path` | 10 000 segments, round join | ≤ 8 ms |
| `dash` | 10 000 segments, 4-element pattern | ≤ 6 ms |
| `boolean` union | 2 × 1 000 segments | ≤ 3 ms |
| `boolean` union | 2 × 10 000 segments | ≤ 40 ms |
| `boolean` union | 2 × 100 000 segments | ≤ 600 ms |
| `HitIndex::build` | 10 000 segments | ≤ 500 µs |
| `hit_fill` via `HitIndex` | 10 000 segments | ≤ 15 µs |
| `arclen` at 1e-6 | 1 000 cubics | ≤ 200 µs |
| `ColourTable::resolve` | depth-3 tint chain | ≤ 100 ns |
| `interpolate` | any effect | ≤ 20 ns |

`hit_fill` is the one with a real user-facing consequence: it runs on every
pointer move over a document, potentially against many objects, and 16 ms of
frame budget disappears quickly.

## Risks and mitigations

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| 1 | Curve quality degrades after repeated booleans, because refitting loses the original cubics | **High** | **High** | `SegmentTrace` (W1.4.3) is a first-class deliverable, not an optimisation; criterion 10's area-conservation property plus a "apply 20 booleans in sequence, compare to the analytic result" regression test |
| 2 | `i_overlay` breaks its API in 10.0 | Medium | Low | Everything goes through `boolean()`; no `i_overlay` type appears in the public API |
| 3 | Degenerate inputs — tangencies, coincident edges, zero-area subpaths — produce wrong results rather than errors | **High** | High | The 200-case pathological corpus is built *before* the pipeline, from the categories `research/05 §8.4` lists; `fuzz_path_boolean` runs nightly |
| 4 | Boolean performance collapses on 100 000-segment paths | Medium | Medium | `i_overlay`'s reusable-buffer API; parallelise by subpath with `rayon` if the 600 ms budget is missed. Measure before optimising |
| 5 | No arc segment type means elliptical arcs from SVG import are converted to cubics and lose exactness | Medium | Low | Accept for now: `.xar` has no arc primitive either (`research/01 §7.4` has three verbs), so nothing in the corpus needs it. Revisit if `i_curve` (which supports rational arcs natively) is adopted |
| 6 | The FIXED16 matrix quantisation gets applied to intermediates and rotation error accumulates | Medium | Medium | `quantise_fixed16` is the only entry point, its doc says so, and a test composes 1 000 random rotations and asserts the drift stays under 1e-9 in the unquantised path |
| 7 | `f64` results differ between x86_64 and aarch64, breaking cross-platform golden images | Low | High | No `fma`, no `-ffast-math` equivalent, no `mul_add` in the geometry path. Determinism is checked per-target as a gate and cross-target as a warning until Phase 12 |
| 8 | Someone adds `Mp * Mp` or a `to_f32` "for convenience" and reintroduces the precision bug the extent was designed to avoid | Medium | Medium | Both are absent by design, with a doc comment explaining why; a test asserts `Mp: !Mul<Mp>` via a `trybuild` compile-fail case |
| 9 | Naive CMYK→RGB is "corrected" by a well-meaning contributor and every imported document shifts colour | Medium | Medium | The doc comment states it is deliberate and cites `research/01 §9.5`; criterion 20's worked examples fail loudly if it changes |
| 10 | `Tolerance::BOOLEAN` is set too tight and booleans become slow, or too loose and small features vanish | Medium | Medium | The sweep in W1.4 sets it from data; the chosen value and the sweep table go in `docs/memory/perf.md` |

## Test plan

**Unit.** Every public function with a nontrivial branch: `Mp` operators and
conversions, `Rect` union/intersection with the empty sentinel, `Matrix`
composition order (`a.then(b)` is `b ∘ a` — get it wrong and everything is
mirrored), `Verb::arity`, `PathBuilder` invariant enforcement, `Fixed24`
sentinel handling, every colour conversion pair, `BuiltinColour::from_ref` for
`-1..-10` and for out-of-range values.

**Property (`proptest`).** The `Mp` overflow suite (criteria 3 and 4). Path
invariants preserved by `transformed`, `reversed`, `normalised`. Boolean
algebraic identities (criterion 10). `flatten` deviation as a property rather
than a fixed corpus. Colour round-trips. Idempotence of `normalised`.

**Golden.** Not images in this phase — `xarast-geom` produces geometry, not
pixels — but **`insta` text snapshots of SVG path data** for every operation in
the pathological corpus. A geometry change then shows up as a readable diff
rather than a pixel diff, which is far easier to review. The rasterised
comparisons against `tiny-skia` (criterion 13) do use the Phase 0 golden
harness.

**The pathological corpus.** 200 cases, authored in this phase as SVG path data
under `tests/data/paths/`, covering: self-intersecting outlines; figure-eights;
subpaths nested three deep; coincident and collinear edges; tangential
touching; zero-length segments; zero-area subpaths; cusps; very small features
next to very large ones (1 mp beside 10^9 mp); paths with 1, 2, 3 and 100 000
points; unclosed subpaths mixed with closed ones; both winding rules disagreeing
on the same outline. These are **our own** test data, authored from geometric
categories, not extracted from the original — the clean-room rule applies to
test data as much as to code.

**Fuzz.** `fuzz_path_boolean` takes arbitrary bytes, decodes them into two
paths via `arbitrary`, and runs every `BoolOp` × `FillRule`. Invariants: no
panic, no allocation over 256 MB, completion within 5 s, and
`Path::validate()` on the output. Also `fuzz_svg_path_parse` on
`Path::from_svg_path_data`.

**Cross-checks.** `tiny-skia` for stroke coverage; a brute-force rasterised
point-in-polygon for `hit_fill`; a dense polyline for `arclen`; `i_overlay`'s
own integer API compared against our `f64` façade on integer-exact inputs.

## Memory note

Create **`docs/memory/geometry.md`** (a new note; add the row to
`docs/memory/INDEX.md`) and update **`docs/memory/perf.md`**.

`geometry.md` must record:

- **Current state.** Which of `xarast-geom`'s modules are complete, and the
  version of `kurbo`, `i_overlay` and `tiny-skia` in use.
- **Decisions taken (and why).** The full `Mp` overflow contract, in the same
  terms as W1.1, because it will be questioned repeatedly and the reasoning must
  survive. The `±(2^30 − 1)` extent and the exactness guarantee it buys. No
  `Mul<Mp>`, no `to_f32`. The segment-oriented path with the `points.len() ==
  .xar point count` identity, and why it is what makes `TAG_PATH_FLAGS` trivial.
  Fill/stroke intent living in `xarast-doc`, not in `Path`. `i_overlay` chosen,
  `flo_curves` as the named fallback, `i_curve` as the 2027 candidate. The
  flatten/boolean/refit pipeline and the role of `SegmentTrace`. The chosen
  `Tolerance::BOOLEAN` value and the sweep that produced it. Hairline handled as
  an error rather than a fake width. Naive CMYK→RGB kept deliberately.
  `Fixed24::INHERIT` handling. The `MAX_PARENT_DEPTH` cycle guard.
- **Invariants that must not be broken.** The five `Path` invariants. Y is up and
  `Rect::lo` is bottom-left. Points translate, vectors do not. FIXED16
  quantisation happens exactly once, on write. Colour components are clamped and
  never NaN. `Path: Eq` means "the same shape", so nothing editor-side may be
  stored in it.
- **Dead ends (do not retry).** `parry2d` (Apache-2.0, physics-shaped, its shape
  model does not fit Béziers). `geo` (polylines only, GIS coordinates).
  `bezier-rs` (its own authors moved to `kurbo`). `geo-booleanop` (abandoned in
  2020). `f32` coordinates anywhere in the model. Panicking `Mp` operators.
- **Open TODOs.** The `cavalier_contours` offset spike result. Whether the
  uniform grid suffices for `HitIndex` at 100 000 segments. Arc segments, if
  `i_curve` is adopted. Cross-architecture determinism, promoted to a gate in
  Phase 12.

`perf.md` gets the reference machine's specification, the Phase 1 budget table
with measured values beside the targets, and the `Tolerance::BOOLEAN` sweep.
