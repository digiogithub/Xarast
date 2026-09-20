# geometry

Memory note for **`xarast-geom`** and **`xarast-color`**, the two leaf crates
everything else depends on.
Specification: [`../phases/phase-01-geometry-and-colour.md`](../phases/phase-01-geometry-and-colour.md).
Format facts: [`../research/01-xar-format.md`](../research/01-xar-format.md) §5, §7, §8, §9.
Arbitration: [`../10-architecture.md`](../10-architecture.md) §3.3, §3.4.

## Current state

Phase 1 implemented. Both crates are `#![forbid(unsafe_code)]`,
`#![deny(missing_docs)]`, build with no display, and depend on nothing
outside `kurbo`, `i_overlay`, `bitflags`, `thiserror` and `slotmap`.

| Module | Contents | State |
|---|---|---|
| `geom::mp` | `Mp`, the overflow contract, unit conversions, `Display`/`FromStr` | complete |
| `geom::fixed` | `Fixed16` codec | complete |
| `geom::point` | `Point`, `Vector` | complete |
| `geom::rect` | `Rect`, the empty sentinel | complete |
| `geom::matrix` | `Matrix`, composition, inversion, `quantise_fixed16` | complete |
| `geom::profile` | `BiasGain` and its LUT | complete |
| `geom::path` | `Verb`, `PointFlags`, `Path`, `PathBuilder`, SVG and `kurbo` conversion, canonical form | complete |
| `geom::flatten` | `Tolerance`, `Polyline`, `SegmentTrace`, `flatten`, `flatten_traced` | complete |
| `geom::stroke` | `Cap`, `Join`, `FillRule`, `DashPattern`, `StrokeStyle`, `stroke_to_path`, `dash`, `offset` | complete |
| `geom::boolean` | `BoolOp`, `boolean`, `self_union`, the refit pipeline | complete |
| `geom::measure` | `arclen`, `point_at_arclen`, `nearest_point`, `hit_fill`, `hit_stroke`, `HitIndex` | complete |
| `color::model` | `ColourModel`, `ColourValue`, `Rgba8`, every conversion | complete |
| `color::fixed24` | `Fixed24` and the inherit sentinel | complete |
| `color::builtin` | `BuiltinColour`, the negative-reference table | complete |
| `color::table` | `ColourId`, `ColourKind`, `ColourDef`, `ColourTable`, `Colour` | complete |
| `color::interp` | `FillEffect`, `Stop`, `Transparency`, `TranspMode`, `interpolate` | complete |

Dependency versions in use: `kurbo` 0.13.1, `i_overlay` 9.0.0 (with
`i_float` 5.0.0, `i_shape` 5.0.0), `bitflags` 2.13, `thiserror` 2.0,
`slotmap` 1.1, and `tiny-skia` 0.12 **as a test-only dependency**.

## Measured performance

Measured with `criterion` in the `release` profile on the **development
container**, which is *not* the reference machine the phase document's
budgets assume — a shared CPU with no frequency guarantees. Treat these as
ratios and as a regression baseline, not as absolutes, and re-measure on the
reference machine before gating CI on them.

| Operation | Input | Budget | Measured | |
|---|---|---|---|---|
| `Mp` operator vs raw `i32` saturating | 4 096 adds | within 5 % | 4.89 µs vs 5.05 µs (−3 %) | ok |
| `Path::bounds` | 10 000 segments | 30 µs | 15.1 µs | ok |
| `Path::tight_bounds` | 10 000 segments | 400 µs | 422 µs | **+5 %** |
| `flatten` at 0.25 device px | 10 000 segments | 2 ms | 163 µs | ok |
| `flatten_traced` | 10 000 segments | ≤1.3× `flatten` | 161 µs (0.99×) | ok |
| `stroke_to_path`, round join | 10 000 segments | 8 ms | 6.23 ms | ok |
| `dash`, 4-element pattern | 10 000 segments | 6 ms | 2.26 ms | ok |
| `boolean` union | 2 × 1 000 segments | 3 ms | 231 µs | ok |
| `boolean` union | 2 × 10 000 segments | 40 ms | 4.60 ms | ok |
| `boolean` union | 2 × 100 000 segments | 600 ms | 39.0 ms | ok |
| `HitIndex::build` | 10 000 segments | 500 µs | 634 µs | **+27 %** |
| `hit_fill` via `HitIndex` | 10 000 segments | 15 µs | 0.117 µs | ok, 128× |
| `hit_fill` exact, for contrast | 10 000 segments | — | 390 µs | — |
| `arclen` at 1e-6 | 1 000 cubics | 200 µs | 49.5 µs | ok |
| `ColourTable::resolve` | depth-3 tint chain | 100 ns | 141 ns | **+41 %** |
| `interpolate` fade | — | 20 ns | 14.3 ns | ok |
| `interpolate` rainbow | — | 20 ns | 38.0 ns | **+90 %** |

Notes.

- The boolean figures are **after** fixing a quadratic scan in
  `RestoreTable::build`, which looked up each run's source segment by a
  linear search of the flat segment list. Before the fix the 10 000-segment
  union took **70.5 ms** and the 100 000-segment one did not finish inside
  the benchmark's budget; grouping the segments by subpath once brought them
  to 4.6 ms and 39 ms. If the boolean ever looks slow again, look here first.
- `Mp` arithmetic compiles to the same work as the raw `i32` loop, so the
  newtype and the whole overflow contract are free. That was the point.
- `hit_fill` is the budget line with a real user-facing consequence, since
  it runs on every pointer move over a document. The index is worth **128×**
  over the exact winding test — 117 ns against 390 µs — which is the whole
  reason it exists. The uniform grid is therefore comfortably enough at
  10 000 segments; 100 000 is still untested.
- `HitIndex::build` is 27 % over. It flattens at `Tolerance::EXPORT`, 1 mp,
  which is far finer than a build-time index needs; the obvious fix if the
  budget matters is to build at the render tolerance instead and accept a
  coarser near-boundary answer.
- `tight_bounds` is 5 % over on a shared container and is within the noise
  of the budget; re-measure before treating it as a miss.
- `ColourTable::resolve` and `interpolate` for the rainbow effects are over
  budget by 1.4× and 1.9×. Both are under the 2× the phase document sets as
  the re-basing threshold, so by its own rule the budgets stand and the code
  needs the work rather than the numbers needing adjustment. The cost in
  both cases is the model round trip: a tint resolves through RGB and
  converts back to the definition's model, and a rainbow interpolation goes
  RGB → HSV → HSV → RGB. Caching a resolved palette (which the document
  model will want anyway) removes most of the first.

## Decisions taken (and why)

### The `Mp` overflow contract

Stated in full in `crates/xarast-geom/src/mp.rs`; repeated here because it
will be questioned and the reasoning has to survive.

Three properties are in tension. We must never panic, because a parser that
panics on hostile input is a denial of service. We must never wrap, because a
wrapped coordinate produces mirrored geometry that looks *plausible* and so
escapes review — the worst kind of failure. And we must not force every
expression in the flattening and hit-test inner loops to return `Option`.

1. `Add`, `Sub`, `Neg`, `AddAssign`, `SubAssign` **saturate**, identically in
   debug and release. Saturation is total, deterministic and
   order-preserving.
2. `Mp::MIN` is `i32::MIN + 1`, not `i32::MIN`, so that negation is total.
   `Mp(i32::MIN)` is constructible through the public field but is outside
   the canonical range; every operation normalises it, and a test asserts so.
3. **No `Mul<Mp> for Mp`.** Millipoints times millipoints is an area.
   Scaling goes through `scale` (f64, half away from zero, saturating) or
   `mul_ratio` (exact in `i64` until the final narrowing). `Div<Mp> for Mp`
   yields a dimensionless `f64`.
4. The `checked_*` family returns `Option` and is what every I/O boundary
   uses, so an unrepresentable `.xar` record becomes a diagnostic rather than
   a silently clamped point.
5. **The document extent is `±(2^30 − 1)` mp** (≈ ±14.9 km). Inside it,
   `a + b` and `a − b` are exactly representable and provably never saturate.
   That is what lets the rest of the codebase stop thinking about overflow.
6. `Mp::sum` accumulates in `i64` and saturates once at the end, so a long
   polyline's running total cannot saturate mid-way and recover into a wrong
   answer.
7. **No `From<f32>`, no `to_f32`.** `f32` loses millipoint precision above
   16 777 216 mp. `to_f64` is exact; `from_f64_round` rounds half away from
   zero and saturates; a NaN becomes `ZERO`, because a NaN coordinate has no
   defensible clamped value and `MIN`/`MAX` would put geometry 14 km away.

The extent value turned out to matter twice over: it is **exactly**
`i_overlay`'s 32-bit coordinate limit, so millipoints go into the boolean
engine untouched. See below.

### Boolean engine: `i_overlay` 9, through its **integer** API

Chosen: `i_overlay` 9.0 (`MIT OR Apache-2.0`).

The phase document anticipated the float API with a fixed `grid_size`. The
integer API is strictly better and is what is implemented: `i_overlay`'s
`i32` engine accepts `-2^30..=2^30 - 1`, which is `Mp::EXTENT`. Millipoints
go in as they are, so there is **no scaling step, no grid to choose, and no
dependence on the input's magnitude** — results are bit-reproducible by
construction rather than by configuration. Golden-image tests and "undo then
redo gives the same document" both rest on that, and a test asserts it over
the corpus.

`boolean()` and `self_union()` are the only entry points; no `i_overlay` type
appears in the public API, so an API break in 10.0 is contained.

- **Named fallback:** `flo_curves` 0.8 (Apache-2.0). Not wired up — adding a
  cargo feature for an engine nobody has needed would be dead code. The
  facade is what makes swapping it a contained change.
- **2027 candidate:** `i_curve`, booleans natively on cubics and rational
  arcs, built on `i_overlay`. Re-evaluate at ≥1.0 with six months of release
  history.

### The flatten → boolean → refit pipeline

1. `flatten_traced` both operands at `Tolerance::BOOLEAN`, keeping the trace.
2. Run `i_overlay` on the integer millipoints, with `min_output_area = 1`
   (a contour below a square millipoint is numerical noise, and keeping it
   would make `difference(A, A)` return a sliver instead of nothing).
3. Refit: an output run that came wholly from one untouched input segment,
   endpoints included, has its original cubic restored **verbatim**.
4. Clean: drop repeated vertices (coordinates are integers, so "closer than
   1 mp" and "identical" are the same test), then canonicalise ordering.

`SegmentTrace` is the whole point of step 3 and is a first-class deliverable,
not an optimisation: without it every boolean turns every curve in the
document into a polyline, cumulatively and irreversibly. A test asserts that
a circle unioned with a distant square still has its four cubics, and another
applies twenty successive unions and checks the area holds to 1e-4.

### `Tolerance::BOOLEAN` = 10 mp

0.01 pt, 3.5 µm. Tighter buys no accuracy that survives quantisation back to
integer millipoints, since the engine's own output is on the millipoint
lattice. Looser starts to erode small features. The conservation identities
hold over the corpus at this value with the slack described under
"Invariants" below.

### The path representation

Segment-oriented: parallel `verbs`, `points` and optional `flags` vectors,
one verb per segment. SVG, `kurbo` and every rasteriser want this; `.xar` is
point-oriented. They reconcile through an identity worth keeping:

> **`points.len()` equals the `.xar` point count exactly.** `MoveTo` and
> `LineTo` consume one point each, `CubicTo` three, `Close` none — and in
> `.xar` closure is a bit on an existing point, not a point of its own.

That is what makes `TAG_PATH_FLAGS`, one byte per point, map onto `flags`
with no index arithmetic at all.

Fill and stroke intent live in `xarast-doc`, not here, so that `PartialEq`
on a `Path` means "the same shape" — which is what makes cheap subtree
comparison and resource deduplication possible. `PointFlags` has no
`SELECTED` bit for the same reason.

`flags` is empty when every point is default, keeping the common path (SVG
import, boolean output) one allocation lighter.

### Flattening: **not** `kurbo::flatten`

This is the other decision that changed during implementation, and it is a
correctness one.

`kurbo::flatten` converts cubics to quadratics and estimates the conversion
error. The estimate is excellent for ordinary curves and unreliable near a
cusp: measured over 20 000 random cubics
(`crates/xarast-geom/examples/devsweep.rs`, still in the tree so the
measurement can be repeated) it exceeded the requested tolerance by up to a
factor of **nine** — 462 mp at a requested 50 mp.

Since "the polyline is within `tol` of the curve" is a contract this crate
advertises and the boolean pipeline relies on, flattening uses the rigorous
control-polygon bound on `|B(t) − L(t)|` instead:

```
u = 3p₁ − 2p₀ − p₃ ,  v = 3p₂ − p₀ − 2p₃
error ≤ sqrt( (max(uₓ², vₓ²) + max(u_y², v_y²)) / 16 )
```

and recursive de Casteljau subdivision, capped at 20 levels. Measured worst
case after the change: **1.53 mp at a requested 1 mp**, where the 0.53 is
exactly the quantisation of the output vertices to integer millipoints.

The obvious alternative bound — three quarters of the control points'
perpendicular distance from the chord — **is wrong and was tried**: it
measures deviation from the infinite *line*, so a cubic whose control points
overshoot along the chord direction registers as flat while the curve bulges
far past its own endpoint. It measured 47× over tolerance. Do not go back
to it.

### `Matrix`: `f64` linear part, `Mp` translation

`a..d` stay `f64` through composition and inversion; `quantise_fixed16()` is
the only thing that rounds them and is for the file boundary alone. A test
composes a thousand rotations and asserts the drift stays under 1e-9.

Consequence worth knowing: the **translation** is quantised to a whole
millipoint at every composition, and a subsequent scale multiplies that
rounding by its own factor. Compose the whole chain into one `Matrix` and
apply it once; applying step by step is both slower and less accurate. The
property tests bound the error by `max_scale` rather than by a constant,
which is what made this visible.

### Stroke, dash, offset

- A width of `Mp::ZERO` is a **hairline**, which has no document-space
  outline at all. `stroke_to_path` returns `Err(StrokeError::Hairline)`
  rather than inventing a width, so the renderer cannot ignore the case.
- `kurbo::Stroke` takes a start and an end cap, matching `.xar`'s separate
  `TAG_STARTCAP`/`TAG_ENDCAP`. When the two differ the path is stroked twice
  and unioned — rare enough in real documents to beat hand-writing a stroker.
- `offset()` offsets each segment, rebuilds the joins, and runs the result
  through `self_union` to delete the loops that offsetting a concave region
  always produces. That dependency on the boolean engine is intrinsic.
- Validated against `tiny-skia` (test-only) by rasterising both strokes at
  512×512 over 100 cases: worst disagreement **0.31 %** of pixels, gated at
  0.5 %. Two rasterisers antialiasing the same outline account for most of it.

### Colour

- **CMYK → RGB is naive**, `R = 1 − min(1, C + K)`, as the original does it.
  A fidelity decision: matching Xara's appearance for two decades of
  documents beats being colourimetrically right, and the format carries no
  ICC profiles to be right *with*.
- **RGB → CMYK is therefore its exact inverse** (`C = 1 − R`, `K = 0`), not
  maximum-black extraction. Forced: with the reverse transform fixed for
  fidelity, any other forward transform makes `RGB → CMYK → RGB` lose
  colour, and users round-trip through the colour dialog constantly.
  Under-colour removal and black generation belong with the ICC path.
- `Fixed24::INHERIT` (`0xF800_0000`, reading as −8.0) means "inherit from the
  parent". `to_f32` returns `Option<f32>` and `ColourDef::components` is
  `[Option<f32>; 4]`, so forgetting the check is a compile error rather than
  a rendering bug.
- `ColourTable::resolve` is depth-limited to `MAX_PARENT_DEPTH = 16` and
  cycle-safe, falling back to the file's `cached_rgb` — which is exactly what
  Xara writes that field for. A corrupt palette entry must not stop a
  document opening. `validate()` reports cycles and depth violations for
  `xar-dump`.
- Components are `f32` in `0.0..=1.0`, clamped on construction, so no
  `ColourValue` can hold a NaN. `Fixed24::to_f32` is the only entry point
  that can see out-of-range input and is where clamping happens.
- Interpolation is written `a(1 − t) + bt`, not `a + (b − a)t`: the second is
  not exact at `t == 1`, which showed up as a gradient's last stop landing
  one 8-bit step off the colour the user picked.

### `lyon_algorithms` evaluated and not taken

`10-architecture.md` lists it beside `kurbo` for this crate. It is not used:
it is `f32`, and everything this crate would want from it — walking,
raycasting, AABBs — `kurbo` already provides in `f64`. Taking it would add a
second curve vocabulary and a precision boundary for nothing. Revisit only if
tessellation lands here, which the architecture assigns to `xarast-render`.

## Invariants that must not be broken

1. **The five `Path` invariants**, enforced by `PathBuilder` by construction
   and checked by `Path::validate()` for paths that did not come through it
   (in practice, the `.xar` importer):
   1. `points.len()` equals the total verb arity.
   2. `flags` is empty or exactly as long as `points`.
   3. The first verb is `MoveTo`; every `Close` is followed by a `MoveTo` or
      by the end of the path.
   4. No two consecutive `MoveTo` verbs.
   5. Every coordinate is inside `Mp::EXTENT`.
2. **Y is up.** `Rect::lo` is the bottom-left. The flip to a Y-down frame
   belongs to the consumer — SVG, PDF, the screen — never to this crate.
   Putting it here would mean the document model silently carried two
   conventions and every bug in it would look like a rendering bug.
3. **Points translate, vectors do not.** `.xar` writes the major and minor
   axes of regular shapes *without* the coordinate-origin translation while
   everything else gets it, so `transform_point` and `transform_vector` are
   separate methods on purpose.
4. **FIXED16 quantisation happens exactly once, on write.** Never on an
   intermediate.
5. **Colour components are clamped and never NaN.**
6. **`Path: Eq` means "the same shape".** Nothing editor-side may be stored
   in a `Path`.
7. **`Rect::EMPTY` is the union identity**, so bounding-box loops need no
   `Option<Rect>` and no special case.
8. **The boolean engine's orientation is authoritative.** `i_overlay` emits
   outer contours counter-clockwise and holes clockwise, exactly.
   `Path::normalised()` recomputes orientation from a nesting heuristic and
   is *worse*; on a shape whose subpaths touch it silently flipped a hole
   into an outer contour (a 2.4 M mp² area error, found by the
   inclusion–exclusion test). Boolean output therefore goes through
   `Path::canonically_ordered()`, which canonicalises only the start vertex
   and the subpath order — the two things the engine does not define.
9. **Area identities carry two slack terms, and both are physical.** The
   relative `1e-4` from the phase document, plus `4·sqrt(area)` for the
   millipoint lattice (a one-millipoint band around a shape of area `A` has
   area of order `4·sqrt(A)`), plus — for identities that compare results
   reached by different routes through the pipeline — `2·tol·perimeter`,
   because flattening at tolerance `t` can move a boundary by `t`. Demanding
   better than that of a curved input demands accuracy a polygonal engine
   cannot have.

## Dead ends (do not retry)

- **`kurbo::flatten` for anything whose tolerance is a contract.** Up to 9×
  over on cusp-heavy cubics. See above.
- **The `(3/4)·max(d1, d2)` perpendicular flatness bound.** Ignores
  tangential overshoot; measured 47× over tolerance.
- **`Path::normalised()` on boolean output.** Overrides an orientation the
  engine already had exactly right. Use `canonically_ordered()`.
- **A subpath's first vertex as the nesting probe in `normalised()`.** Start
  vertices are routinely shared between touching subpaths, where containment
  is a coin flip, and rotating the subpath moves the probe, so the result is
  not idempotent. The probe is now the lexicographically smallest vertex
  nudged towards the vertex mean — both invariant under rotation and
  reversal. The *centre* does not work either: for a square with a hole,
  each centre is inside the other's outline and both come out as holes.
- **Reversing open subpaths in `normalised()`.** Direction is meaningful for
  them (stroke direction, arrowhead end) and reversing moves the start point,
  breaking idempotence.
- **A flattened polyline as an arc-length reference.** Over 100 000 vertices
  the millipoint quantisation jitter *adds* length, so the "dense
  approximation" comes out longer than the exact value. Sample the curve in
  `f64` instead.
- **`parry2d`** — pure Apache-2.0, physics-shaped, its shape model does not
  fit Béziers. **`geo`** — polylines only, GIS coordinates. **`bezier-rs`** —
  its own authors moved to `kurbo`. **`geo-booleanop`** — abandoned in 2020.
- **`f32` coordinates anywhere in the model.** **Panicking `Mp` operators.**
  **Wrapping `Mp` operators** — plausible-looking wrong geometry is the worst
  outcome of all.

## Evidence: why we do not use `kurbo::flatten`

Replacing a mature library's flattener needs paying for in measurement.
`examples/kurbo_vs_ours.rs` runs both over the same 20,000 random cubics
spanning ±200,000 mp, with the same deviation metric and sample count:

| Requested tolerance | Ours, worst | Ratio | `kurbo`, worst | Ratio |
|---:|---:|---:|---:|---:|
| 1 | 1.53 | 1.53× | 2.04 | 2.04× |
| 10 | 10.40 | 1.04× | 18.49 | 1.85× |
| 50 | 49.90 | 1.00× | 462.66 | **9.25×** |
| 100 | 99.64 | 1.00× | 311.19 | 3.11× |
| 1000 | 986.21 | 0.99× | 3898.89 | 3.90× |

`kurbo` violates its own tolerance contract by up to 9×. Boolean operations
depend on that bound — a flattening error larger than requested changes
topology, not just smoothness — so the contract has to hold. Our 1.53× at
tolerance 1 is millipoint quantisation, not algorithm error: you cannot place
a vertex to better than 1 mp.

The cost is roughly **2× the vertices** for the same requested tolerance. That
is the honest trade, and it is an open optimisation: nobody has yet measured
ours at tolerance *t* against `kurbo` at tolerance *t/10*, which is the fair
comparison at equal achieved accuracy. If `kurbo` wins that, the custom
flattener should go.

## Open TODOs

- ~~`Mp::PER_MM` is inconsistent with `Mp::PER_INCH`.~~ **Decided: derive it.**
  `PER_MM` is now `PER_INCH as f64 / 25.4`, so millimetre and inch conversions
  are exactly reciprocal. The phase document's `2834.652715` came from the
  original (`Kernel/units.h:120`), is 2.5 ppm high, and matches no definition
  of the inch — and the original contradicted itself anyway, clamping line
  widths with the correct `2834646` mp/metre in `Kernel/linwthop.cpp:123`.
  Millipoints are the storage unit in both file formats and millimetres are
  only an entry and display unit, so nothing round-trips through this constant
  and there was no compatibility to keep, only 2.5 µm per metre of error to
  drop. A test pins the reciprocity.
- **`fuzz_path_boolean` and `fuzz_svg_path_parse` are not written.** The
  workspace has no `fuzz/` directory yet and creating one was outside this
  task's scope. Both targets are straightforward once it exists:
  `arbitrary` → two paths → every `BoolOp` × `FillRule` → assert no panic and
  `Path::validate()` on the output.
- **Mixed-run refitting is not implemented.** Step 3 restores an untouched
  cubic verbatim; a run the boolean *cut* stays a polyline rather than going
  through `kurbo::fit_to_bezpath_opt`. Fitting a run that contains a corner
  would round the corner off, so it needs corner detection first. The
  verbatim restore handles the case that actually matters — a curve the
  operation did not touch.
- **The `cavalier_contours` offset spike** has not been run. The current
  `offset()` is kurbo-per-segment plus self-union and meets its tests.
- **Whether the uniform grid suffices for `HitIndex` at 100 000 segments.**
  Fallback is a static BVH built here, not a new dependency.
- **Arc segments**, if `i_curve` is adopted.
- **Cross-architecture `f64` determinism** is not gated. No `mul_add` and no
  fast-math anywhere in the geometry path, which is the precondition;
  promote to a gate in phase 12.
- **`insta` snapshots of SVG path data** for the corpus are not wired up; the
  corpus is exercised by assertions instead.
