# Phase 13 — Live effects

> After this phase Xarast has the subsystems that make a drawing recognisably Xara rather than
> generically vector: blends, contours, moulds, bevels, shadows and feathering, all
> non-destructive and re-editable, plus fractal and noise fills and the twelve transparency
> blend families — built on one shared offscreen-and-blur pipeline instead of six private ones.

## Goal

Implement the live-object subsystems deliberately cut from the MVP (`research/04 §2.3`) and
ranked in `research/04 §3` as differentiators 6–9, 11 and 12, as **non-destructive controllers**
following the controller/generated/original pattern of `research/02 §6.1`, with the generated
subtree recomputable at any resolution and never authored by the user.

Concretely, at the end of this phase a user can: blend between two objects along a curve;
contour a shape inwards or outwards with a spacing profile; wrap a group in an envelope or a
perspective and see its fills deform with it; put a bevel, a wall/floor/glow shadow or a
feather on anything; fill a shape with clouds or plasma; and set any object's transparency to
Stained Glass, Bleach, Contrast, Saturation, Darken, Lighten, Brightness, Luminosity or Hue.
All of it survives a save to `.xarast`, a reload, and a re-edit.

## Scope

### In scope

| Subsystem | Roadmap name | Research anchor |
|---|---|---|
| Shared live-object infrastructure (controller/generated/original, regeneration queue, change propagation, permissions, `BecomeA`) | — | `research/02 §6.1–6.3`, §10.10 |
| Shared offscreen layer rendering + blur pipeline (disc and gaussian) | — | `research/03 §2.9`, §3.5, §3.6 |
| Shadow (wall, floor, glow, inner) | Shadow | `research/02 §6.9`, `research/03 §2.9` |
| Feather | Feather | `research/02 §6.12`, `research/06 §6.8.6` |
| Bevel | Bevel | `research/02 §6.10`, `research/03 §2.9` |
| Contour | Contour | `research/02 §6.8` |
| Blend (including blend on a path) | Blend | `research/02 §6.6` |
| Mould: envelope 4×4, envelope 2×2, perspective | Mould | `research/02 §6.7` |
| Fractal (clouds) and noise (plasma) fills | Fractal fills | `research/02 §5.8`, `research/03 §2.8` |
| The 12 transparency blend families — **completion and verification only**; Phase 4 M2 implements them and Phase 8 exposes them (see workstream I) | — | `research/03 §2.7`, §3.4 |
| `.xar` import and `.xarast` read/write for all of the above | — | `research/06 §6.8` |
| Tools and on-canvas interaction for each | — | `research/04 §1.10` |

### Explicitly out of scope (and which phase owns it)

| Out of scope | Owner / reason |
|---|---|
| ClipView | **Unowned as of writing** — no earlier phase document claims it, although `research/04 §1.10` marks it P1 and it belongs with Phase 7's structure operations. It is a live controller of exactly this shape (`research/02 §6.11`) and rides on workstream A's infrastructure. **Rule:** if Phase 7 or 8 has not delivered it when Phase 13 opens, Phase 13 adopts it as three extra tasks in workstream A (params + clip-region rendering + the Apply/Remove ClipView commands, roughly S+M+S), not as a new workstream — and this row is updated to say so |
| XPE / external bitmap plug-in bridge, legacy effects | **Never.** `research/04 §2.3` and §7: depends on a plug-in ecosystem that does not exist on Linux; zero current value |
| A generic user-visible effect stack with arbitrary chained bitmap effects | **Phase 15 at the earliest.** The `<xarast:effects>` chain of `research/06 §6.8.7` is *written and preserved* in this phase but only the effect kinds implemented here are generated |
| Brushes and stroke types (`NodeBrush`, `BrushParams`) | Post-1.0. It shares `LiveKind` but is its own subsystem (`research/04 §2.3`) |
| Windows and macOS shells | **Phase 14** |
| Print/CMYK behaviour of effects, effect rasterisation for PDF export at print resolution | **Phase 15** (the `RegenerateForPrinting` hook is *defined* here, its print consumers land there) |
| Mesh gradients, 3- and 4-colour fills, conical/square fills | Phase 8 owns the fill taxonomy; if any are missing they are Phase 15 tail work, not Phase 13 |
| Photo live adjustments (levels, curves) as a live effect chain | Phase 10 owns non-destructive photo adjustments; this phase only guarantees they compose with the offscreen pipeline |

## Prerequisites

- **Phase 12 closed.** v0.1 is out; the perf gates exist and will now be defended while this
  phase adds the most expensive features in the product.
- Phase 4/5: the render engine's layer push/pop, per-node cache keyed by
  `(content hash, quantised scale, quality, variant)` and Draft/Final quality levels
  (`research/03 §3.5`) exist. This phase extends them; it must not invent a second caching
  system.
- Phase 2: `NodeKind` is an enum, change notification walks ancestors, and the command bus
  produces inverses. `LiveKind` is a new `NodeKind` variant — a change that touches every
  exhaustive `match` in the workspace, which is the intended cost (`docs/10-architecture.md`
  §3.2).
- Phase 3: the `.xar` importer's record layer can reach the effect tags
  (105, 106, 107, 108, 109, 110, 4012, 4050–4057, 4060–4062, 4066, 4067, 4072–4074, 4086,
  4125–4127); the handlers are written here.
- Phase 6: `.xarast` unknown-data preservation works, so a file written by a future version
  with an unknown effect kind survives a round trip.
- Phase 8: gradient ramps with bias/gain profiles (`research/03 §2.6.3`) exist —
  `Profile::map` is reused verbatim by blend, contour, shadow and feather profiles.

## Workstreams

### A. Shared live-object infrastructure

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| A1 | `NodeKind::Live(LiveNode)` with `LiveKind`, `RegenState`, controller name; role marking for `Source` vs `Generated` children | `xarast-doc` | L | — |
| A2 | Structural invariants: a controller has exactly one `Generated` subtree and one `Source` subtree; `Generated` nodes are `needs_parent` and excluded from marquee selection, from clipboard copy and from independent deletion | `xarast-doc` | M | A1 |
| A3 | `regenerate(tree, controller, attrs, rasters, dpi)` as a pure-ish function (source + params + dpi → new generated subtree), dispatching on `LiveKind` | `xarast-doc` | M | A1 |
| A4 | `RegenQueue`: mark/dedup/flush-before-paint, with the invalidate-bbox → regenerate → invalidate-bbox order | `xarast-doc` | M | A3 |
| A5 | `notify_ancestors(tree, from, kind, phase)`: change flags and phases, marking ancestor controllers `Deferred` | `xarast-doc` | M | A1 |
| A6 | `permits(tree, id, kind)`: ancestor veto, replacing the original's two-bit permission state | `xarast-doc` | M | A5 |
| A7 | Hit-test promotion (clicking a generated child selects the controller) and transform-with-children | `xarast-doc`, `xarast-app` | M | A2 |
| A8 | Attribute-application promotion: dropping a colour on a generated child applies it to the controller | `xarast-doc`, `xarast-app` | M | A2 |
| A9 | `BecomeA`: convert any live controller to plain shapes/groups (the "Convert to editable shapes" command), undoable | `xarast-doc`, `xarast-app` | L | A3 |
| A10 | `set_dpi` / `get_dpi` per controller and a `regenerate_for_output(dpi)` entry point used by export and (later) print | `xarast-doc` | M | A3 |
| A11 | Undo integration: a regeneration is **not** an undo step; the parameter change that caused it is | `xarast-doc` | M | A3, A4 |
| A12 | Property tests: regenerate is idempotent for unchanged params; controller round-trips through save/load; deleting a controller deletes its whole subtree | `xarast-doc` | M | A1–A11 |

Three things here are easy to get wrong and expensive to fix later.

**Regeneration is not an edit.** `research/02 §6.2` describes the original's rule precisely: if
no operation is in progress, the change is marked finished, it came from a child, and the mask
asks for regeneration, then the controller regenerates and reports "done, do not record undo".
In our command bus that becomes: the `Command` that changed a parameter records its own
inverse; the regeneration it triggers records nothing. If regeneration ever appends to the undo
log, a user will press Ctrl+Z and watch a shadow flicker instead of seeing their edit undone.
A11's test asserts exactly that: one parameter change ⇒ exactly one undo step.

**The double invalidation is load-bearing.** A4 invalidates the bounding box *before* and
*after* regenerating, because the new generated geometry may be smaller than the old
(`research/02 §6.3`). Only doing it afterwards leaves the shrunken difference painted on
screen. This is one of those bugs that looks like a compositor problem for two days.

**Deferred beats immediate.** Dragging a shadow's penumbra emits a change per mouse event.
Regenerating synchronously on each one is how the tool becomes unusable at 200 Hz tablet
sampling. The queue collapses them, and the flush happens once, immediately before paint. The
`Deferred` state exists for exactly this and is the default for interactive changes;
`Dirty` (regenerate now) is reserved for programmatic changes such as export and `BecomeA`.

### B. The shared offscreen + blur pipeline (built once)

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| B1 | `LayerTarget`: allocate an offscreen surface (GPU texture or CPU premultiplied buffer) in **device space** with an explicit pixel width, padded for filter growth | `xarast-render` | L | — |
| B2 | `render_subtree_to_layer(node, pixel_width, quality) -> LayerTarget` in both backends, tile-aware and cancellable | `xarast-render` | L | B1 |
| B3 | Alpha-only extraction (`SourceAlpha` equivalent) as a first-class layer kind, since shadow, feather and bevel all start there | `xarast-render` | M | B2 |
| B4 | `Blur` module: two kernels, `Kernel::Disc { radius_px }` (Xara-compatible) and `Kernel::Gaussian { sigma_px }` (SVG-compatible), one shared API, one shared normalisation table builder | `xarast-render` | L | B3 |
| B5 | CPU blur: separable gaussian (three-pass or IIR) plus the exact disc convolution as the reference implementation; `u16` intermediate precision, `rayon` over rows | `xarast-render` | L | B4 |
| B6 | GPU blur: separable two-pass compute/fragment shader with the same σ mapping, and a disc approximation validated against B5 | `xarast-render` | L | B4, B5 |
| B7 | Radius clamping and the resolution policy: radius is in document units, converted per view; clamp to the original's 100 px ceiling at generation resolution and document what happens above it | `xarast-render` | M | B4 |
| B8 | Profile application to the blurred alpha (`Profile::map` as a 256-entry transfer table over the alpha channel) | `xarast-render` | M | B4 |
| B9 | Layer cache: offscreen results keyed by `(node content hash, quantised pixel width, quality, variant)`, LRU-by-cost, sharing the Phase 4 budget | `xarast-render` | M | B2 |
| B10 | Destination-read compositing: ping-pong tiles for the exotic blend families, with the display list marking `needs_dst_read`. **Phase 4 (render milestone M2) owns this**; the task here is to confirm it holds for offscreen layers and to finish it if it does not | `xarast-render` | L | — |
| B11 | The 12 blend families as 256×256 R8 LUT textures (GPU) and the identical tables in the CPU compositor, generated once from a single source of truth. **Also Phase 4 M2**; here it must additionally serve the `Bevel` family, which had no producer until workstream D | `xarast-render` | L | B10 |
| B12 | Bit-exact CPU↔GPU parity test for every family at Final quality | `xarast-render` | M | B11 |

This workstream is the reason the phase is ordered the way it is: **shadow, feather, bevel and
the generic effect chain are four different UIs on top of the same two primitives** — render a
subtree to an offscreen alpha layer, and blur it. Building them separately guarantees four
subtly different blurs, four caches, and four sets of edge artefacts.

The blur needs **two kernels, not one**, and the reason is a format requirement rather than an
aesthetic one. `research/03 §2.9` documents the original's blur as a **disc convolution**
(`Blur8BppBitmap`, radius ≤ 100 px, normalisation table compressed to ≤ 0x800 entries); that
is what a `.xar` file's appearance was authored against, so `.xar` import fidelity wants a
disc. But `research/06 §6.8.1/§6.8.6` bakes shadows and feathers into SVG `feGaussianBlur`,
and an external viewer will therefore render a gaussian. The two must be reconciled, not
chosen between: Xarast renders with the disc kernel, and the `.xarast` writer emits the
gaussian whose visual width matches — the worked example in `research/06 §6.8.1`
(`xarast:blur="6.2"` → `stdDeviation="3.1"`) fixes the convention at **σ = radius / 2**.
B4's API therefore takes a kernel, and the writer converts. Any deviation from σ = radius/2
must be justified by measurement and recorded.

Two details that will otherwise cost a week each. First, **filters grow the bounding box**: a
blurred layer is larger than its source, and the offscreen allocation must be padded (the SVG
baking in `research/06` uses `-30% / 180%`; our own padding is computed as
`ceil(radius) + join slack`, not guessed). Second, **`color-interpolation-filters="sRGB"` is
mandatory** on every emitted filter (`research/06 §6.8.1`) because SVG's default is linearRGB
and would make every exported shadow visibly wrong — while internally we blur in linear or
`u16` space to avoid banding (`research/03 §3.7`) and convert back. Those two statements are
not in conflict: the *interchange* declaration says sRGB because the numbers we bake are sRGB;
the *intermediate* precision is ours.

### C. Shadow and feather

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| C1 | `ShadowParams { kind: Wall\|Floor\|Glow\|Inner, offset, blur, darkness, profile, scale, tilt, colour }` and its controller | `xarast-doc` | M | A1 |
| C2 | Wall shadow generation: alpha layer → disc blur → profile → flood colour → composite behind the source | `xarast-render` | M | B4 |
| C3 | Floor shadow: the perspective/skew transform of the silhouette before blurring, parameterised by scale and tilt | `xarast-render`, `xarast-geom` | L | C2 |
| C4 | Glow: no offset, optional dilation before blur | `xarast-render` | M | C2 |
| C5 | Inner shadow: composite with an `out` operation against the source alpha | `xarast-render` | M | C2 |
| C6 | Feather as an **attribute**, not a controller: `AttrFeather { width, profile }` applied to shapes, groups and text, implemented as alpha blur on the object's own layer | `xarast-doc`, `xarast-render` | L | B4 |
| C7 | Shadow tool: drag the shadow directly on canvas (offset), penumbra handle for blur, infobar for angle/height/scale/darkness/profile/glow width | `xarast-ui`, `xarast-app` | L | C1–C5 |
| C8 | Feather infobar: size and profile, live | `xarast-ui` | M | C6 |
| C9 | `.xar` tags 4050/4051 (shadow) and 4086/4127 (feather) → params | `xarast-xar` | M | C1, C6 |
| C10 | `.xarast` read/write per `research/06 §6.8.1` and §6.8.6, including the baked filter | `xarast-format` | M | C1, C6 |

Feather is an **attribute** and shadow is a **controller**, and that asymmetry is deliberate:
`research/04 §3` (differentiator 11) describes feather as "an attribute, not a destructive
effect", applicable to shapes, groups and text, and the original implements it both as
`AttrFeather` and as a `NodeFeatherEffect` for the effect-stack path. Modelling it as an
attribute means it inherits through the attribute stack and composes with groups for free.
Shadow cannot be an attribute because it generates geometry that sits *behind* its source in
paint order and can be selected and dragged.

The interaction model that must be preserved (`research/04 §3`, differentiator 6) is the live
drag: the penumbra recalculates *while* dragging. That works only if the drag path uses Draft
quality — reduced blur radius, no regeneration of the source layer, the cached alpha reused —
and the Final pass runs on the 120 ms idle timer (`research/03 §3.5`). Wire C7 to those two
levels explicitly; do not let a shadow drag trigger a Final regeneration per frame.

### D. Bevel

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| D1 | `BevelParams { bevel_type, indent, outer, light_angle, light_tilt, contrast, join }` | `xarast-doc` | M | A1 |
| D2 | Face generation: from the source path, build the bevel faces (triangles/trapezia) with 2D normals, for each bevel profile type (round, flat, chisel, ridge, mesa) | `xarast-geom` | L | — |
| D3 | Lighting map: rasterise the faces into an 8-bit height/illumination channel of an offscreen layer | `xarast-render` | L | D2, B2 |
| D4 | Apply the lighting map through the `Bevel` transparency family LUT (family 34–36 in `research/03 §2.7.1`) | `xarast-render` | M | D3, B11 |
| D5 | Bevel fill and "inking node": a bevel may carry its own fill; generate it as a sibling path | `xarast-doc`, `xarast-render` | M | D3 |
| D6 | Bevel tool + infobar: indent, light angle (draggable light on canvas), tilt, contrast, type, join, inner/outer | `xarast-ui`, `xarast-app` | L | D1–D5 |
| D7 | `.xar` tags 4052–4057 → params | `xarast-xar` | M | D1 |
| D8 | `.xarast` per `research/06 §6.8.2`, baking the `feGaussianBlur → feSpecularLighting → feComposite` chain | `xarast-format` | M | D1 |

Bevel is the effect with the widest gap between "roughly right" and "right". `research/03 §2.9`
is specific about how the original did it: the kernel generated faces with 2D normals, CDraw
rasterised them into the high channel of a 32 bpp bitmap with `FillTriangle`/`FillTrapezium`,
and then the `T_BEVEL` transparency family applied that index through two LUTs parameterised by
contrast, lightness and darkness. We replicate the **structure** — face generation is ours,
rasterising the illumination index is a render-engine job, and the final application is one of
the twelve blend families from B11 — rather than inventing a normal-mapped shading pass. Doing
it this way also means the bevel's appearance is automatically consistent with the other blend
families and shares their CPU/GPU parity test.

The five profile types in `research/06 §6.8.2` (round, flat, chisel, ridge, mesa) are the
public vocabulary; the mapping from profile type to the face cross-section is **to be
determined in this phase**, by rendering each type in the original in a VM at a known indent
and light angle and fitting the cross-section (the same empirical method `research/03 §2.10`
prescribes for the grey-conversion weights). Record the resulting curves in
`docs/memory/render.md`; do not ship guessed ones.

### E. Contour

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| E1 | `ContourParams { steps, width (signed), outer, include_line_widths, join, profile, flatness }` | `xarast-doc` | M | A1 |
| E2 | Robust path offsetting for a single step: joins (mitre/round/bevel), self-intersection removal, degenerate-input handling | `xarast-geom` | L | — |
| E3 | Step generation with the bias/gain profile controlling spacing, and the summed path for the controller's bounds | `xarast-geom` | M | E2 |
| E4 | Colour interpolation across steps (fade / rainbow / alt-rainbow, as in blend) | `xarast-color`, `xarast-doc` | M | — |
| E5 | Inner contours (negative width) including the "collapse" case where the offset vanishes | `xarast-geom` | M | E2 |
| E6 | Contour tool + infobar: steps, width, inner/outer, join, object profile, attribute profile | `xarast-ui` | M | E1–E5 |
| E7 | `.xar` tags 4066/4067 | `xarast-xar` | M | E1 |
| E8 | `.xarast` per `research/06 §6.8.3` — baked as **real offset paths**, never as increasing `stroke-width` | `xarast-format` | M | E1 |

The whole difficulty is E2. Path offsetting is the numerically nastiest operation in the
product after boolean ops, and it fails in exactly the cases users hit: sharp corners past the
mitre limit, self-intersecting offsets on concave shapes, and inner offsets that collapse to
nothing partway through the step sequence. Two mitigations, both mandatory. First, **offset
then clean**: generate the raw offset, then run it through the boolean union built in Phase 1
to remove self-intersections — this is the same approach the original took via
`ClipPathToPath`. Second, a **property-test suite** (`proptest`): offsetting by 0 is identity;
offsetting outwards never decreases area; N single-width offsets and one N-width offset stay
within a bounded Hausdorff distance; no offset produces NaN or an unclosed path from a closed
input. Budget E2 as a task in its own right and expect it to take longer than the rest of the
contour work put together.

### F. Blend

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| F1 | `BlendParams { steps, step_distance, one_to_one, antialias, tangential, along_path, colour_effect, profile, reverse }` and the multi-object controller (N objects ⇒ N−1 blenders) | `xarast-doc` | L | A1 |
| F2 | Subpath correspondence: automatic matching between the two ends, plus explicit one-to-one mapping, plus redundant-path elimination | `xarast-geom` | L | — |
| F3 | Node correspondence within a subpath: matching point counts by insertion, with a stable rule for start-point alignment and direction | `xarast-geom` | L | F2 |
| F4 | Geometry interpolation per step with the position bias/gain profile | `xarast-geom` | M | F3 |
| F5 | Attribute interpolation: fill, stroke, transparency, line width, with an independent attribute profile; colour interpolation fade / rainbow / alt-rainbow | `xarast-doc`, `xarast-color` | L | F4 |
| F6 | Blend along a path: distribute steps along a curve by arc length, with optional tangential rotation and start/end angles | `xarast-geom` | L | F4 |
| F7 | **Steps are not tree nodes.** Generate them at render time from the blender state; materialise real nodes only on `BecomeA` | `xarast-doc`, `xarast-render` | L | F4, A9 |
| F8 | Blend tool: drag from object to object, step count/distance in the infobar, attach/detach path, edit end objects live, remove blend | `xarast-ui`, `xarast-app` | L | F1–F7 |
| F9 | `.xar` tags 105, 106, 4060, 4061, 4062, 4072, 4073, 4074 | `xarast-xar` | L | F1 |
| F10 | `.xarast` per `research/06 §6.8.4`, including `<xarast:blend-map>` for one-to-one correspondence and the >64-step rasterisation rule | `xarast-format` | M | F1 |

F7 is the single most important architectural decision in this workstream and it comes straight
from `research/02 §6.6`: the intermediate steps **are not materialised as nodes**. A 200-step
blend of a 500-node path would otherwise put 100,000 nodes in the arena, wreck the undo log,
and destroy the Phase 12 memory budget. The blender holds the two end states and produces the
steps during display-list construction; the cache holds the resulting geometry keyed by content
hash and scale. `BecomeA` is the only path that creates real nodes, and it is an explicit user
command.

F2/F3 are where blends look wrong. Two shapes with different subpath counts or different node
counts must be reconciled before interpolation, and the choice of correspondence is visible:
get it wrong and the blend twists inside out halfway through. The rule set to implement, in
order: match subpath counts by dropping the smallest redundant subpaths (as the original's
`BlendRef` does); within a subpath, insert nodes into the sparser path at parameter positions
that preserve shape; align start points by minimising total travel; align direction by
comparing signed area. Where the original's exact heuristic is unknown, **it is determined in
this phase by comparing against the original in a VM** over a fixed set of 12 blend pairs, and
the chosen rule is recorded in `docs/memory/render.md`.

### G. Mould: envelope and perspective

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| G1 | `MouldParams::{ Envelope { shape, source }, Envelope2x2 { shape, source }, Perspective { corners, source } }` | `xarast-doc` | M | A1 |
| G2 | `MouldGeometry` trait: validate, define, describe, `make_valid_from`, `mould_point`, `mould_path`, `mould_bitmap_to_tile`, transform, copy | `xarast-geom` | L | G1 |
| G3 | Envelope maths: a 4-sided Bézier patch with 4×4 control points (and the simpler 2×2 case); adaptive subdivision of input Béziers against a flatness threshold | `xarast-geom` | L | G2 |
| G4 | Perspective maths: the projective transform from four corners, with the degenerate-to-affine detection the `.xarast` writer needs | `xarast-geom` | M | G2 |
| G5 | `MouldTransform` adapter so any node can be transformed through a mould; it is **not invertible**, and every consumer must cope | `xarast-geom`, `xarast-doc` | M | G2 |
| G6 | Fills deform with the object: the mould applies to the fill's control parallelogram/quadrilateral, not just to the outline | `xarast-doc`, `xarast-render` | L | G2 |
| G7 | Bitmaps under a mould: deform to a tile (perspective mapping already exists in the paint model as `GradMapping::Perspective`) | `xarast-render` | M | G4 |
| G8 | Text under a mould: keep the live text in the source subtree, bake curves into the generated subtree | `xarast-text`, `xarast-doc` | M | G3 |
| G9 | Mould tool: edit the mould shape with node handles, envelope and perspective presets, draggable vanishing point, copy/paste envelope/perspective, detach/remove, grid toggle | `xarast-ui`, `xarast-app` | L | G1–G8 |
| G10 | `.xar` tags 107, 108, 109, 110, 4012 | `xarast-xar` | M | G1 |
| G11 | `.xarast` per `research/06 §6.8.5`, storing the undeformed source in `<xarast:mould-source>` | `xarast-format` | M | G1 |

G6 is what separates a real mould from a cheap one, and `research/02 §5.9` names the exact
hook: every fill value implements `Mould(...)` (deform *n* source coordinates to destinations)
and `MouldIntoStroke(...)`. Because `docs/10-architecture.md` §3.6 collapsed the original's ~40
parallel fill classes into one generic `FillGeometry`, we implement this **once** on
`FillGeometry` — deform its control points — instead of per fill type. That is the payoff of
that architectural decision and it should be stated in the code comment.

G5's non-invertibility ripples further than expected. Hit-testing inside a mould, dragging a
node of a moulded path, and snapping all want the inverse map. The rule: **interaction happens
in source space**. The tool maps the pointer into source space by numerical inversion
(Newton iteration on the patch, with a bounded iteration count and a fallback to the nearest
sample from a coarse grid), edits the source, and re-moulds. Do not attempt to make the
generated geometry directly editable.

### H. Fractal and noise fills

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| H1 | `FractalParams { seed, graininess, gravity, squash, dpi, tileable, dim }` (clouds) and `NoiseParams { seed, dpi, tileable, dim, grain }` (plasma) in the paint model | `xarast-render`, `xarast-doc` | M | — |
| H2 | Midpoint-displacement (diamond-square) generator implementing the documented per-level rule: `noise = (graininess · (potential >> 17)) >> level`, `attraction = gravity >> (2 · level)`, `displacement = noise − attraction` | `xarast-image` | L | H1 |
| H3 | Tileable variant (wrap the lattice) and the `squash` anisotropy | `xarast-image` | M | H2 |
| H4 | Perlin-style noise generator for the plasma variant | `xarast-image` | M | H1 |
| H5 | Materialise to an `Image` paint, cached by the full parameter tuple (the `IsSameAsCachedFractal` equivalent), with a `Randomise` command | `xarast-render` | M | H2, H4 |
| H6 | Deterministic PRNG with a documented algorithm, so the same seed gives the same bitmap on every platform and every release | `xarast-image` | M | H2 |
| H7 | Fill-tool integration: fractal/noise as fill shapes with on-canvas control handles, and the contone/colour mapping | `xarast-ui` | M | H5 |
| H8 | `.xar` import of the fill attributes; `.xarast` write (parameters plus a baked raster resource, since SVG cannot express the generator) | `xarast-xar`, `xarast-format` | M | H1 |

H6 is not optional. If the PRNG changes between releases, every document with a fractal fill
changes appearance on reload — a silent data-fidelity bug. Pick one algorithm, name it in the
`.xarast` extension attributes, freeze it, and cover it with a golden test that hashes the
generated bitmap for three fixed seeds.

The exact seeding and lattice-traversal order of the original are **not fully documented** in
`research/03 §2.8` — only the displacement rule is. Bit-identical reproduction of a `.xar`
document's fractal is therefore **not** a goal; the acceptance bar is that clouds and plasma
are visually of the same family and respond to the same parameters in the same direction.
State that limitation in the `.xar` import notes and in the user manual.

### I. Advanced transparency blend modes

**Ownership, stated plainly.** `docs/phases/phase-08-colour-fills-transparency.md` records
that the 12 families are implemented by Phase 4 (render milestone M2) and exposed in the
transparency infobar by Phase 8. Phase 13 therefore does **not** implement them from scratch;
it closes what those phases necessarily leave open: the `Bevel` family (which had no producer
until workstream D exists), any family not exposed because it had no consumer, the empirical
grey-conversion weights that every luminance-based family depends on, and bit-exact CPU↔GPU
parity for all families **through the offscreen-layer path** that live effects introduce — a
code path Phase 4 never exercised. If Phases 4 and 8 delivered everything else, workstream I
collapses to I3, I5 and verification, and that is a good outcome, not a gap.

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| I1 | `Transparency { family: BlendFamily, source: Flat\|Gradient\|Image }` in the document model, covering all 12 families | `xarast-doc` | M | — |
| I2 | LUT generation from one shared source of truth, consumed by both backends (B11) | `xarast-render` | M | B11 |
| I3 | Recover the grey-conversion weights empirically (render a Darken over a known gradient in the original and fit), then freeze them | `xarast-render` | M | — |
| I4 | Graduated and bitmap-sourced transparency for every family, with bias/gain profiles and multi-stop ramps | `xarast-render` | M | I1 |
| I5 | UI: transparency type selector, on-canvas transparency handles already exist from Phase 8 and must now carry the family | `xarast-ui` | M | I1 |
| I6 | `.xar` mapping from `TranspType` to family; `.xarast` per `research/06 §6.5` | `xarast-xar`, `xarast-format` | M | I1 |

`research/03 §2.7.2` gives verified formulas for Darken, Lighten, Brightness, Contrast,
Saturation, Luminosity, Hue, Mix, Stained Glass and Bleach, and the structural conclusion that
every family is "(scalar derived from source) → tone LUT applied to destination", which maps to
one 256×256 R8 texture per family with no branches. Two cautions carried from the research:
Contrast is a **static table**, not a closed formula, so it is extracted rather than derived;
and the grey weights are **unknown** because the original never calls
`SetGreyConversionValues`, with BT.601 (0.299/0.587/0.114) as the starting hypothesis (I3).
Do not treat BT.601 as fact until I3 confirms it; the weights feed every luminance-based family.

Compositing is in **8-bit non-linear sRGB** (`research/03 §2.10`) — replicating these blends in
linear space gives different, wrong results. That is the one place where the usual
"always composite in linear light" instinct must be suppressed, and it needs a comment at the
compositor's entry point saying so, or someone will "fix" it.

### J. Tools, interaction and integration

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| J1 | Five new tools registered in the command registry with their infobars and cursors: Blend, Mould, Contour, Shadow, Bevel | `xarast-app`, `xarast-ui` | L | C7, D6, E6, F8, G9 |
| J2 | Every parameter reachable **numerically** from the infobar as well as by dragging (the Phase 12 accessibility rule) | `xarast-ui` | M | J1 |
| J3 | Draft/Final discipline: every live drag runs at Draft; Final on the idle timer; a visible but unobtrusive "refining" affordance | `xarast-render`, `xarast-ui` | M | B9 |
| J4 | Nesting: a shadow on a blend on a moulded group must work; define and test the composition order | `xarast-doc` | L | A1–A12 |
| J5 | Strings, keyboard reference, manual chapters and sample documents for all new features | `xarast-ui`, docs | M | J1 |
| J6 | `xarast-cli render` covers documents containing every effect (this is how golden tests reach them) | `xarast-cli` | S | J1 |

J4 is the integration risk of the phase. Each effect works in isolation long before they
compose. The composition contract to fix and test: a controller's `Source` subtree may itself
contain controllers; regeneration is **bottom-up** (flush the queue in depth order, deepest
first) so an outer shadow sees the regenerated inner blend; the DPI set on an outer controller
propagates down; and a change deep inside marks every ancestor controller `Deferred` in one
pass (A5 already does this). The test matrix is small and must be exhaustive: for each ordered
pair of the six effect kinds, one document that nests them, rendered and golden-compared.

## Public API introduced

```rust
// xarast-doc
pub struct LiveNode { pub kind: LiveKind, pub regen: RegenState, pub name: Option<Arc<str>> }
pub enum LiveKind { Blend(Box<BlendParams>), Contour(Box<ContourParams>), Shadow(Box<ShadowParams>),
                    Bevel(Box<BevelParams>), Mould(Box<MouldParams>), Effect(Box<EffectParams>) }
pub enum RegenState { Clean, Dirty, Deferred }
pub enum ChildRole { Source, Generated }

pub fn regenerate(tree: &mut Tree, controller: NodeId, attrs: &mut AttrResolver,
                  rasters: &mut RasterCache, dpi: f64) -> Result<(), RegenError>;
pub struct RegenQueue;
impl RegenQueue { pub fn mark(&mut self, id: NodeId); pub fn flush(&mut self, tree: &mut Tree, ctx: &mut RegenCtx); }
pub fn notify_ancestors(tree: &mut Tree, from: NodeId, kind: ChangeKind, phase: ChangePhase, q: &mut RegenQueue);
pub fn permits(tree: &Tree, id: NodeId, kind: ChangeKind) -> Permission;
pub fn become_shapes(tree: &mut Tree, controller: NodeId, cmd: &mut CommandBuilder) -> Result<NodeId, BecomeError>;

pub struct AttrFeather { pub width: Millipoints, pub profile: BiasGain }

// xarast-geom
pub trait MouldGeometry {
    fn describe(&self) -> MouldKind;
    fn validate(&self, shape: &PathData) -> Result<(), MouldError>;
    fn mould_point(&self, p: Point) -> Point;
    fn mould_path(&self, src: &PathData, flatness: f64) -> PathData;
    fn invert_point(&self, p: Point) -> Option<Point>;   // numerical; None past the iteration budget
}
pub fn offset_path(src: &PathData, delta: f64, join: JoinType, mitre_limit: f64, flatness: f64) -> PathData;
pub fn contour_steps(src: &PathData, width: f64, steps: u32, profile: BiasGain, join: JoinType) -> Vec<PathData>;
pub fn blend_paths(a: &PathData, b: &PathData, t: f64, corr: &Correspondence) -> PathData;
pub struct Correspondence;
pub fn correspond(a: &PathData, b: &PathData, mode: CorrespondenceMode) -> Correspondence;

// xarast-render
pub struct LayerTarget { pub bounds: DeviceRect, pub pixel_width: f64 /* … */ }
pub fn render_subtree_to_layer(scene: &Scene, node: NodeId, pixel_width: f64, q: Quality, kind: LayerKind) -> LayerTarget;
pub enum LayerKind { Colour, AlphaOnly }
pub enum Kernel { Disc { radius_px: f32 }, Gaussian { sigma_px: f32 } }
pub fn blur(layer: &mut LayerTarget, kernel: Kernel, profile: Option<&BiasGain>);
pub fn sigma_for_disc_radius(radius_px: f32) -> f32;   // the σ = r/2 interchange convention
pub enum BlendFamily { Mix, StainedGlass, Bleach, Contrast, Saturation, Darken, Lighten,
                       Brightness, Luminosity, Hue, Bevel, None }
pub fn blend_lut(family: BlendFamily) -> &'static [u8; 256 * 256];

// xarast-image
pub fn generate_fractal(params: &FractalParams) -> ImageBuffer;   // clouds
pub fn generate_noise(params: &NoiseParams) -> ImageBuffer;       // plasma
```

## Acceptance criteria

1. `cargo nextest run --workspace` passes and `cargo clippy --workspace -- -D warnings` is
   clean with the six new `LiveKind` variants handled in every exhaustive `match`.
2. Golden-image suite: **≥ 60 new cases** covering every effect, every parameter at three
   values (min/typical/max), and each of the 12 transparency families, rendered through
   `vello_cpu` with **exact** pixel match against the committed goldens
   (`cargo nextest run -p xarast-render golden::effects`).
3. GPU↔CPU parity at Final quality for all 12 blend families is **bit-exact**
   (`cargo nextest run -p xarast-render parity::blend_families` on the lavapipe runner).
4. Nesting matrix: for each of the 30 ordered pairs of effect kinds, a document nesting them
   renders without panic and matches its golden
   (`cargo nextest run -p xarast-render golden::nesting`; 30/30 pass).
5. Round trip: every effect document survives `.xarast` write → read → write with byte-identical
   second output (`cargo nextest run -p xarast-format roundtrip::effects`).
6. External-viewer check: each baked `.xarast` renders in a headless Chromium and in Inkscape
   with **mean ΔE₀₀ < 4.0** against Xarast's own render of the same file — the measurable form
   of "graceful degradation" (`cargo xtask check-external-render`).
7. `.xar` import: every corpus file containing an effect tag imports with **zero** unhandled
   tags reported by `xar-dump --unhandled`, and its render is reviewed against the original in
   the reference VM with mean ΔE₀₀ < 2.0 and 99th percentile < 6.0 over the effect's bounding
   box.
8. One parameter change produces exactly **one** undo step, and undo restores the previous
   generated geometry (`cargo nextest run -p xarast-doc undo::live_effects`).
9. `become_shapes` on each effect kind produces a plain shape/group tree whose render matches
   the live render within mean ΔE₀₀ < 1.0, and is itself undoable
   (`cargo nextest run -p xarast-doc become_a::effects`).
10. Offsetting property tests pass with 10,000 generated cases: identity at delta 0, no NaN, no
    unclosed output from closed input, monotone area for outward offsets
    (`cargo nextest run -p xarast-geom proptest::offset`).
11. Fractal determinism: three fixed seeds produce bitmaps whose SHA-256 matches the committed
    hashes on both x86_64 and aarch64 (`cargo nextest run -p xarast-image fractal::determinism`).
12. Grey-conversion weights (I3) are determined, committed as constants with the measurement
    procedure recorded, and every luminance-based family's golden regenerated against them.
13. Performance budgets in the table below are met under
    `cargo xtask bench --check-budgets --suite effects`.
14. No Phase 12 budget regresses: `cargo xtask bench --check-budgets` (full suite) still exits 0.
15. Accessibility: every parameter of every new tool is settable from the infobar by keyboard
    alone (`cargo nextest run -p xarast-ui keyboard_only::effects`).
16. i18n: `cargo xtask i18n check` reports 0 missing/unused keys and 0 unauthorised literals
    after the new tools land.
17. Manual chapters for all six effects exist and `mdbook build docs/manual` succeeds; the
    generated keyboard reference lists the five new tools.

## Performance budgets

Measured on the Phase 12 reference machine, with scenario documents committed under
`benches/docs/effects/`.

| Budget | Target | Notes |
|---|---|---|
| Shadow drag, single object, 1000×800 view, Draft | ≤ 16 ms p95 per frame | Uses the cached alpha layer; no source regeneration during drag |
| Feather drag, group of 200 objects, Draft | ≤ 16 ms p95 | |
| Blend with 50 steps of 200-node paths, pan at Draft | ≤ 16 ms p95 | Steps are generated, not stored (F7) |
| Blend regeneration after a step-count change, Final | ≤ 100 ms | Perceived as instant |
| Contour, 24 steps on a 500-node path, Final | ≤ 250 ms | Dominated by offsetting |
| Mould envelope drag on a 5000-node group, Draft | ≤ 16 ms p95 | Adaptive subdivision reduced at Draft |
| Bevel regeneration, 400×400 px at 96 dpi, Final | ≤ 150 ms | |
| Disc blur, radius 20 px, 1024×1024 alpha, CPU | ≤ 12 ms | `rayon`, `u16` intermediate |
| Disc blur, same, GPU | ≤ 2 ms | |
| Fractal fill generation, 512×512 | ≤ 60 ms | Cached thereafter; regeneration only on parameter change |
| Blend-family compositing overhead vs Mix, full-window | ≤ 1.3× | The LUT path must not be a cliff |
| Memory: 100-step blend of 1000-node paths | ≤ 40 MB of live-effect cache | Enforced by the Phase 12 cache ceiling |
| Idle CPU with 20 live effects on screen, no interaction | < 1 % of one core | No background regeneration loop |

Values marked as ranges rather than derivations are first estimates to be **calibrated in
week 2** against the first working implementation, using the Phase 12 rule: measure, add 20 %,
commit the number.

## Risks and mitigations

| Risk | Probability | Impact | Mitigation |
|---|---|---|---|
| Path offsetting (E2) is not robust enough and contours fail on real art | **High** | High | Offset-then-boolean-clean; 10,000-case property suite; a corpus of 50 hand-picked nasty shapes; if still failing at the phase's midpoint, evaluate `cavalier_contours` (already noted in `research/05 §8`) as the offsetting backend behind our API |
| Blend correspondence heuristics differ visibly from the original | High | Medium | Determine empirically against a 12-pair reference set in the VM (F2/F3); record the rule; accept "same family, not identical" and say so in the manual |
| Bevel profile cross-sections are guessed and look wrong | Medium | High | D's empirical determination step is a gate, not a nicety; no bevel ships with a guessed profile |
| The offscreen pipeline is built twice because shadow lands before B is ready | Medium | High | Sequence: B1–B9 complete and merged **before** C2 starts. Enforce by making C2's PR depend on B9's |
| Live-effect caches blow the Phase 12 memory budget | Medium | High | Share one budget and one eviction policy (B9); the Phase 12 gate runs on every merge and will catch it at the commit that causes it |
| Effect nesting explodes regeneration cost (O(n²) on deep chains) | Medium | High | Bottom-up single-pass flush (J4); a dedicated benchmark with five nested controllers; cache at every level so an unchanged inner level is not recomputed |
| Mould inversion (G5) diverges and hit-testing feels broken | Medium | Medium | Bounded Newton iteration with a coarse-grid fallback; a property test that `invert(mould(p)) ≈ p` within 0.5 device pixels over the patch |
| GPU destination-read ping-pong is slow or wrong on some drivers | Medium | High | It is already the Phase 4 design (`research/03 §3.4`); test on lavapipe, Intel, AMD and NVIDIA; the CPU compositor is always available as the escape hatch via `XARAST_RENDERER=cpu` |
| 8-bit sRGB compositing gets "fixed" to linear by a well-meaning change | Medium | High | A comment at the compositor entry point, a test that asserts a known Darken result, and a note in `docs/memory/render.md` |
| Scope creep into a generic effect stack | Medium | Medium | `<xarast:effects>` is written and preserved, but only the effect kinds in this phase are generated; anything else is Phase 15 |

## Test plan

**Unit and property**
- `xarast-geom`: offsetting (criterion 10), blend correspondence invariants, mould round-trip
  `invert(mould(p)) ≈ p`, adaptive subdivision never exceeds the flatness threshold.
- `xarast-doc`: regeneration idempotence, exactly-one-undo-step, structural invariants
  (one `Source`, one `Generated`), permission vetoes, deferred-queue deduplication.
- `xarast-image`: fractal/noise determinism across architectures.
- `xarast-render`: LUT generation matches the reference tables for all 12 families; blur
  normalisation sums to unity; padded layer bounds always contain the blurred result.

**Golden images** (`vello_cpu`, exact match)
- Per effect: a parameter sweep at min/typical/max for every parameter.
- Per transparency family: source over a known gradient destination, flat and graduated.
- The 30-pair nesting matrix.
- Effects on every object kind: path, group, text, bitmap, QuickShape.
- Edge cases: zero steps, zero width, degenerate mould (all four corners coincident),
  self-intersecting source, empty source, blur radius at and beyond the 100 px clamp.

**Parity and comparison**
- GPU↔CPU bit-exact at Final for the blend families; ΔRMS < 0.5 % elsewhere (the Phase 12
  nightly job, extended with the new corpus).
- Original-vs-Xarast comparison in the reference VM for the `.xar` corpus files that use
  effects, reported as mean and p99 ΔE₀₀ over the effect bounding box (criterion 7).
- External viewer check in Chromium and Inkscape (criterion 6).

**Round trip and import**
- `.xarast` write→read→write byte equality for every effect document.
- Unknown-effect preservation: a hand-authored `.xarast` with an unrecognised
  `xarast:kind` survives a load/save cycle with its baked output intact and
  `xarast:base-authoritative="true"` respected.
- `.xar` import of all effect tags with `xar-dump --unhandled` reporting zero.
- Fuzzing: extend `fuzz_xar_parse` and `fuzz_xarast_parse` corpora with effect documents;
  add `fuzz_path_offset` and `fuzz_mould` targets.

**Performance**
- The effects benchmark suite (criterion 13) plus the full Phase 12 suite (criterion 14) on
  every merge.
- A dedicated deep-nesting benchmark (five nested controllers) to catch the O(n²) case.

**Manual, recorded**
- Live-drag feel for shadow penumbra, mould handles and blend attachment, on both an
  integrated GPU and a discrete one — the thing no automated test can assert, and the reason
  differentiator 6 exists.

## Memory note

On close, update:

- **`docs/memory/render.md`** — the offscreen layer API and its cache key; the two blur
  kernels, the σ = radius/2 interchange convention and any measured deviation; the 100 px clamp
  policy; the recovered grey-conversion weights and how they were measured; the source of truth
  for the 12 LUTs and the reason compositing stays in 8-bit sRGB; the bevel profile
  cross-sections as determined; GPU ping-pong findings per driver.
- **`docs/memory/document-model.md`** — `LiveNode`, `LiveKind`, `RegenState`, the
  controller/generated/original invariants, the "regeneration is not an undo step" rule, the
  bottom-up nesting flush order, the permission model, and the `BecomeA` contract.
- **`docs/memory/xar-import.md`** — the effect tags now handled, the ones deliberately not
  handled, and the fidelity limits accepted (notably fractal fills, which are same-family but
  not bit-identical).
- **`docs/memory/xarast-format.md`** — the baked representation actually emitted for each
  effect, the >64-step blend rasterisation rule as applied, how `<xarast:mould-source>` is
  written, and the external-viewer ΔE₀₀ results.
- **`docs/memory/perf.md`** — the calibrated effect budgets with measured numbers, and the cost
  model for nested controllers.
- **`docs/memory/ui.md`** — the five new tools, their infobars and cursors, the Draft/Final
  interaction rules, and the numeric-equivalence table for the new on-canvas handles.
