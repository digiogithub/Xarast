# Phase 8 — Colour, fills and transparency

> After this phase a gradient is something you grab and drag on the canvas, and a
> palette colour is something you drop onto an object — or onto one stop of its
> ramp — instead of something you type into a dialog.

## Goal

Make Xara's two signature interactions real in Xarast:

1. **Fills are edited in place.** Selecting the fill tool over a filled object
   draws its control arrows and blobs directly over the artwork; dragging them
   changes the fill live, at interactive frame rates, with no modal dialog.
2. **Colour is a draggable thing.** A swatch from the on-screen colour bar or
   the colour gallery can be dropped on an object (fill), on its outline
   (stroke), or on an individual gradient stop, and the drop target is shown by
   the cursor before the button is released.

Everything else in this phase — the colour editor, named document colours with
tints/shades/links, multi-stop ramps, non-linear ramp profiles, the
transparency tool — exists to make those two interactions worth having.

Phase 4 already **renders** gradients and transparency. Phase 7 already gives us
a selection, a tool framework and a command bus. Phase 8 is the phase where the
user can **author** them.

## Scope

### In scope

**Colour model (`xarast-color`, extending what phase 1 delivered)**

- The five editable colour models: **RGB**, **HSV**, **greyscale**, **CMYK**,
  plus the web-safe RGB restriction used by the palette. (`research/02 §5.10`,
  `colmodel.h:199-215`.)
- **Named document colours**: phase 1's `ColourTable`, owned by
  `DocumentResources`, gains create / rename / delete / redefine, and the
  live-reference colour `Colour::Indexed` starts repainting every use when the
  entry it points at changes.
- **Derived colours**: phase 1's `ColourKind::{Tint, Shade, Linked}` with
  `parent: Option<ColourId>`, now guaranteed to form an acyclic graph that
  resolves in one pass in a cached parent-before-child order.
- **"No colour"** as a first-class palette entry and a first-class fill value.
- Conversion between models through an explicit conversion context, with sRGB
  as the canonical resolved value. Every named colour carries a resolved sRGB
  value at all times (this is what `research/06 §6.12.1` makes mandatory in the
  file format).

**Fill geometry editing (`xarast-doc`, `xarast-app`)**

- Interactive editing of these fill shapes, colour **and** transparency
  variants, both of which are the same `FillGeometry<S>` from
  `research/02 §10.7`:

  | Shape | `FILLSHAPE_*` | Editable control points in this phase |
  |---|---|---|
  | Flat | `FLAT` (0) | — (colour only) |
  | Linear | `LINEAR` (1) | start, end, **end2** (the skew/3-point axis) |
  | Radial circular | `CIRCULAR` (2) | centre, major (radius handle) |
  | Radial elliptical | `ELLIPTICAL` (3) | centre, major, minor |
  | Conical | `CONICAL` (4) | centre, zero-direction |
  | Diamond (square) | `DIAMOND` (5) | centre, corner1, corner2 |
  | Three colour | `3POINT` (6) | origin, axis1, axis2 (3 colour blobs) |
  | Four colour | `4POINT` (7) | origin, axis1, axis2, axis3 (4 colour blobs) |

- **Multi-stop ramps** (`Ramp<S>`): add a stop by dropping a colour on the
  gradient line, move a stop by dragging its blob, delete a stop, reorder
  automatically on drag-past, select a stop, and nudge a selected stop.
  Start and end stops are **not** in the ramp — they are `from`/`to` — exactly
  as `research/02 §5.6` describes; the editing code must not lose that
  distinction on round-trip.
- **Non-linear ramp profile** (bias/gain, `research/03 §2.6.3`): a profile
  gadget on the fill infobar plus keyboard entry, driving
  `BiasGain { bias, gain }` in `[-1, 1]`, identity at `(0, 0)`.
- **Ramp mapping** (`Linear` / `Sin`) and **fill effect**
  (`Fade` / `Rainbow` / `AltRainbow`) as infobar choices.
- **Mutate fill** (`OPTOKEN_MUTATEFILL`): change gradient type while keeping
  colours, ramp and a best-effort mapping of control points.
- **Fill nudge** by keyboard for the selected control point / stop.
- **Repeat / tiling** (`Simple`, `Repeat`, `RepeatInverted`) exposed on the
  infobar for the shapes that support it.

**Transparency (`xarast-app`, `xarast-render` glue)**

- The transparency tool, sharing the whole geometry machinery with the fill
  tool (in the original both live in `tools/filltool.cpp`; here they are two
  configurations of one state machine).
- Transparency value convention kept as Xara's: **0 = opaque, 255 = fully
  transparent** (`research/03 §2.7.1`). The conversion to the renderer's alpha
  happens in exactly one function.
- Blend modes exposed in the UI, all as both flat and graduated variants:

  | Exposed in phase 8 | `TranspType` | Notes |
  |---|---|---|
  | Mix | `TT_Mix` (1) | default; normal alpha |
  | Stained Glass | `TT_StainGlass` (2) | multiplicative |
  | Bleach | `TT_Bleach` (3) | screen |
  | Contrast | `TT_CONTRAST` | LUT family |
  | Saturation | `TT_SATURATION` | analytic (reads destination luminance) |
  | Darken | `TT_DARKEN` | LUT family |
  | Lighten | `TT_LIGHTEN` | LUT family |
  | Brightness | `TT_BRIGHTNESS` | LUT family |
  | Luminosity | `TT_LUMINOSITY` | analytic |
  | Hue | `TT_HUE` | analytic, RGB↔HSV |

  Ten user-selectable modes. Their rendering is phase 4 work; phase 8 only
  exposes them, wires the infobar, and adds the per-mode golden images.
- **Group transparency**: apply a transparency attribute to a group as a unit
  (the render side is phase 4's layer push/pop; phase 8 adds the command and
  the tool affordance).
- **Transparency profile** (bias/gain on the alpha ramp), same gadget as the
  colour ramp profile.

**UI surfaces (`xarast-ui`)**

- **Colour editor**: 2D field + slider, numeric entry per model, model
  selector (RGB / HSV / greyscale / CMYK), colour-type selector
  (normal / tint / shade / linked), name field, live preview, and "apply to
  selection" vs "redefine palette entry" as two distinct actions.
- **On-screen colour bar**: horizontal strip below the canvas. Left click =
  fill, right click or `Shift`+click = stroke, drag = drop onto a target,
  context menu, scrolling/paging, and the "no colour" entry at the left end.
- **Colour gallery** (`F9`): the document's named colours as a list with
  thumbnails, showing the derivation tree (a tint listed under its parent),
  with create / edit / rename / delete / drag-to-object.
- **Eyedropper** (`Ctrl+E`): pick a colour from any pixel of the document
  (from the rendered CPU surface, not the screen — see "out of scope").
- **On-canvas handle overlay**: the arrows, blobs and dashed guide lines drawn
  over the artwork in a dedicated overlay pass, in device space, never
  scaled by the document zoom.

### Explicitly out of scope (and which phase owns it)

| Not in this phase | Owner |
|---|---|
| **Bitmap fills** (`FILLSHAPE_BITMAP`) and their handles, tiling and contone | **Phase 10** — they need the bitmap resource model first |
| **Fractal and noise fills** (`FILLSHAPE_CLOUDS`, `FILLSHAPE_PLASMA`) | **Phase 13** (live effects and fractal fills, per roadmap) |
| **Perspective fills** (`Perspective { p2, p3 }` on any shape) | **Phase 13** — perspective only becomes reachable once moulds exist |
| **Feather** (edge fade), even though the original groups it with transparency | **Phase 13** |
| **Bevel transparency** (`TT_BEVEL`) — never user-selectable, it is the bevel effect's internal channel | **Phase 13** |
| `T_SPECIAL_1/2/3` | **Never.** `research/02 §5.5`: not legal in document data structures, GDraw-internal only |
| **Spot inks, colour separation, screen angles, overprint** | **Phase 15** (print/prepress fidelity) |
| **ICC profiles and document-level colour management** | **Phase 10** reserves the slot on bitmaps; full CMS is **phase 15**. Phase 8 must not hard-code "everything is sRGB" anywhere it stores a colour |
| **Palette sort by hue/luminance/use, web-safe palette command, palette import/export (`.gpl`/`.ase`)** | **Phase 12** (polish) or later; P2/P3 in `research/04 §1.5` |
| **Screen-wide eyedropper** (picking outside the application window) | Deferred indefinitely: it needs a compositor screenshot portal on Wayland. Phase 8 picks from the document's own CPU render surface |
| **Named attribute styles** (`AttrStyle`) | Post-v1.0 (P3) |
| **Dash patterns, arrowheads, brushes, variable-width strokes** | Phase 13 / later. Phase 8 touches stroke **colour** and **transparency** only |

## Prerequisites

| Needs | From | Specifically |
|---|---|---|
| `FillGeometry<S>`, `Ramp<S>`, `BiasGain`, `Colour`, `Transparency` types | Phase 1 / 2 | `research/02 §10.7`, `§10.8` |
| Attribute stack with lexical scoping, `AttrValue::Fill` / `::TranspFill` | Phase 2 | architecture §3.6 |
| Command bus with inverse-producing commands | Phase 2 | architecture §4 |
| Gradient rendering: 5 shapes × repeat × ramp, LUT built with the bias/gain profile | Phase 4 | `research/03 §2.6`, `§3.6` |
| The 12 blend families implemented on both backends | Phase 4 (render milestone M2) | `research/03 §2.7`, `§3.4` |
| `.xar` import of fill and transparency tags | Phase 3 | tags 150-172, 190-206, 4010, 4075-4088, 4121-4123 |
| Tool framework, selection, drag handling, infobar host | Phase 7 | |
| Canvas widget with an overlay pass and device-space hit-testing | Phase 5 | |
| Palette and derived-colour serialisation | Phase 6 | `research/06 §6.12` |

**Hard gate:** if phase 4's blend-family work has not landed, phase 8 ships with
Mix / Stained Glass / Bleach only and the other seven modes disabled in the
infobar with a tooltip. That is a degradation, not a redefinition of scope: the
phase does not close until all ten are selectable.

## Workstreams

### W8.1 — Colour model and palette

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T8.1.1 | `ColourValue` conversions: RGB↔HSV↔grey↔CMYK↔sRGB, with round-trip property tests | `xarast-color` | M | — |
| T8.1.2 | Editing on phase 1's `ColourTable`: `redefine`, `rename`, `reparent`, `remove`, `refresh_from` | `xarast-color` | M | T8.1.1 |
| T8.1.3 | Cycle detection on `reparent`, cached `resolve_order`, `PaletteEpoch`, dirty propagation | `xarast-color` | M | T8.1.2 |
| T8.1.4 | `Colour::Indexed` resolution through the palette at render/scene-build time | `xarast-doc` | S | T8.1.3 |
| T8.1.5 | "No colour" as a distinct value, separate from transparent black | `xarast-color` | S | T8.1.1 |
| T8.1.6 | Commands: `CreateColour`, `RedefineColour`, `RenameColour`, `DeleteColour` (with "replace uses with" policy), `ReparentColour` | `xarast-doc` | M | T8.1.3 |
| T8.1.7 | Palette round-trip through `.xarast` (write + read + compare) | `xarast-format` | S | T8.1.3, phase 6 |

**The tricky part is deletion and the derivation DAG.** Deleting a colour that
other colours derive from, or that objects use, cannot silently leave dangling
references. The rule this phase fixes: a delete command takes an explicit
`OnDelete` policy — `Detach` (children become direct colours holding their
currently resolved value; uses become direct colours) or `Reject` (the command
fails and the UI asks). There is no third option and no reference counting:
`collect_unused` (`research/02 §10.12`) is a sweep, not a refcount, and it runs
on save, not on edit.

Cycle detection runs on `ReparentColour` and on load. A cycle found at load time
is broken by demoting the youngest edge to a direct colour and logging a
warning — an imported file must never be rejected for this.

**Redefinition must be cheap.** Changing one palette entry can touch thousands
of objects. Do not walk the tree. `Colour::Indexed` resolves through the palette
at scene-build time, so a palette change invalidates the render cache by bumping
a `palette_epoch` that is part of every `CacheKey`, and the dirty region is the
union of the bounds of nodes whose resolved colour changed. Computing that union
needs the reverse index `ColourUses` (`ColourId -> [NodeId]`) maintained by the
attribute-set commands; that index is the one piece of denormalised state phase 8
introduces, and it is rebuilt from scratch on load.

### W8.2 — Fill and transparency edit commands

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T8.2.1 | `SetFillGeometry` command with inverse, for both payload types | `xarast-doc` | M | — |
| T8.2.2 | `MoveFillControlPoint { node, handle, to }` — coalescing during a drag | `xarast-doc` | M | T8.2.1 |
| T8.2.3 | Ramp edits: `InsertStop`, `MoveStop`, `RemoveStop`, `SetStopValue` | `xarast-doc` | M | T8.2.1 |
| T8.2.4 | `SetFillProfile` (bias/gain), `SetRampMapping`, `SetFillEffect`, `SetTiling` | `xarast-doc` | S | T8.2.1 |
| T8.2.5 | `MutateFill`: type change preserving colours, ramp and control points | `xarast-doc` | L | T8.2.1 |
| T8.2.6 | `SetTransparencyMode` (the 10 exposed modes) | `xarast-doc` | S | T8.2.1 |
| T8.2.7 | `ApplyGroupTransparency` / `RemoveGroupTransparency` | `xarast-doc` | M | T8.2.6 |
| T8.2.8 | Undo coalescing policy for continuous drags | `xarast-doc` | M | T8.2.2 |

**Drag coalescing is the subtle one.** A gradient drag emits a
`MoveFillControlPoint` per mouse move — sixty a second. Each is a legal command
with an inverse, but the user expects one undo step for the whole drag. The rule:
commands carry a `CoalesceKey`; the history merges a new command into the
previous one when the keys are equal *and* no other command intervened *and* the
previous command is still the newest entry. The key for a control-point drag is
`(node, handle)`; for a stop drag `(node, stop_index)`. `CProfileBiasGain` had
the same need and solved it with a `generatesInfiniteUndo` flag
(`biasgain.h:241`) — we solve it in the history, not in the value, so every
value type gets it for free.

`MutateFill` is `L` because the point mapping between shapes is not obvious and
must be specified rather than improvised. The mapping table this phase fixes:

- any → **Flat**: take `from`.
- Flat → any gradient: `from` = the flat colour, `to` = the same colour at 0
  saturation (matching what a user expects when they pick a gradient on a flat
  object), control points = the object's bounding box diagonal (linear), the
  inscribed circle (radial/conical/diamond).
- Linear ↔ Diamond ↔ Radial: `centre := start`, `major := end`,
  `minor := start + perp(end - start)`; preserve `from`, `to`, `ramp`, profile.
- to/from **Three/Four colour**: ramps are dropped (those shapes cannot carry
  one, `fillval.h:697`); the third/fourth colour is seeded from the ramp's
  middle stop if there is one, otherwise from `to`. The drop of the ramp is a
  visible loss, so the command records it and the UI reports it once.

### W8.3 — On-canvas handle overlay

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T8.3.1 | `FillHandles` derivation: `FillGeometry<S>` + object matrix → handle set in document space | `xarast-app` | M | T8.2.1 |
| T8.3.2 | Device-space hit-testing with a fixed pick radius, z-ordered (stops above arrows above lines) | `xarast-app` | M | T8.3.1 |
| T8.3.3 | Overlay drawing: arrows, square/round blobs, dashed guides, selected-stop highlight | `xarast-ui` | M | T8.3.1 |
| T8.3.4 | Overlay for multiple selected objects sharing a fill (edit all, show one handle set) | `xarast-app` | M | T8.3.2 |
| T8.3.5 | Handle nudge by keyboard, with `Ctrl`/`Shift` step modifiers | `xarast-app` | S | T8.3.2 |
| T8.3.6 | Constraint modifiers during drag: axis lock, aspect lock, snap to 15° | `xarast-app` | M | T8.3.2 |

**Handles live in device space for hit-testing and in document space for
semantics.** The pick radius is a constant in pixels (proposal: 5 px, with an
8 px radius for touch/pen input, confirmed in this phase by the phase 4 UI
spike's input-device reporting). Never express it in millipoints: at 3200 % zoom
a millipoint radius makes handles unpickable, and at 5 % zoom it makes them
overlap. Translate the document-space handle position to device space, compare
there.

Handle geometry per shape (this is the contract the overlay renderer codes
against, and it is what makes Xara look like Xara):

- **Linear**: a filled arrow from `start` to `end`; a square blob at `start`, an
  arrowhead at `end`; when `end2` differs from the perpendicular default, a
  second dashed arm to `end2`.
- **Radial elliptical**: a blob at `centre`, arrows to `major` and `minor`;
  the ellipse itself outlined dashed. Circular shows only the `major` arm and
  the `aspect_locked` flag is what distinguishes the two.
- **Conical**: a blob at `centre`, one arm to `zero_dir`, and a dashed circle
  showing the sweep.
- **Diamond**: blob at `centre`, arms to `corner1` and `corner2`, dashed
  rhombus.
- **Three/four colour**: one blob per colour at `origin`, `axis1`, `axis2`
  (`axis3`), with dashed edges between them.
- **Ramp stops**: small diamonds sitting *on* the gradient arm at the parametric
  position of each stop, `from` and `to` drawn as the arm endpoints. Clicking an
  empty part of the arm with a colour in hand inserts a stop there.

### W8.4 — The fill tool and the transparency tool

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T8.4.1 | One generic `FillLikeTool<S>` state machine, instantiated for colour and transparency | `xarast-app` | L | T8.3.2, T8.2.2 |
| T8.4.2 | Drag-out-a-new-gradient on an object with a flat fill | `xarast-app` | M | T8.4.1 |
| T8.4.3 | Fill infobar model: type, effect, mapping, tiling, profile, mode, stop position/value | `xarast-app` | M | T8.4.1 |
| T8.4.4 | Transparency infobar: adds the blend-mode selector, reuses everything else | `xarast-app` | S | T8.4.3 |
| T8.4.5 | Infobar widgets and binding | `xarast-ui` | M | T8.4.3 |
| T8.4.6 | Status-line feedback and cursor changes per hover target | `xarast-ui` | S | T8.3.2 |

The state machine, stated once because both tools are the same machine:

```
Idle ──hover handle──▶ HoverHandle ──press──▶ DragHandle ──release──▶ Idle
  │                                                │
  │                                            (Esc) cancel → restore pre-drag value
  ├──press on object with flat fill──▶ DragNewGradient ──release──▶ Idle
  ├──press on empty gradient arm with colour in hand──▶ InsertStop → DragHandle
  ├──click handle──▶ Idle (handle becomes the selected stop; infobar rebinds)
  └──drop from palette──▶ (see W8.7)
```

Escape during a drag must restore the exact pre-drag value, not "undo one step":
by then the coalesced command may already have merged several moves. The tool
therefore snapshots the fill value on press and issues a `SetFillGeometry` back
to it on cancel, then drops the whole coalesced run from the history.

### W8.5 — Ramp evaluation and live render feedback

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T8.5.1 | `build_ramp(stops, profile, space, len)` shared by both backends and by the overlay preview | `xarast-render` | M | phase 4 |
| T8.5.2 | Ramp cache keyed by `(stops hash, profile, space, len)` with LRU eviction | `xarast-render` | S | T8.5.1 |
| T8.5.3 | Draft/Final ramp length: 256 during a drag, 2048 at rest (`research/03 §2.6.2`) | `xarast-render` | S | T8.5.1 |
| T8.5.4 | Dirty-region computation for a fill edit: union of old and new fill extents ∩ object bounds | `xarast-render` | M | T8.2.2 |
| T8.5.5 | Golden images: every shape × every exposed blend mode × {flat, graduated} | `xarast-render` | M | T8.5.1 |

`build_ramp` must be **the only** implementation of the bias/gain curve in the
tree. The formula (`research/03 §2.6.3`) is short enough that someone will be
tempted to inline it in the overlay preview; that is how the preview and the
render drift apart. Export it from `xarast-render` and have the UI call it.

The identity short-circuit at `bias == 0 && gain == 0` is not an optimisation,
it is a correctness requirement: the Schlick formulation divides by
`(1 - 2b)·(1 - x) + b`, which for `b = 0.5` is exactly `0.5` and fine, but the
floating-point path still introduces a few ULP of drift across 2048 entries, and
"linear gradient is not exactly linear" is the kind of bug that costs a week.

### W8.6 — Colour editor

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T8.6.1 | `ColourEditorModel`: current colour, model, derivation, live/committed split | `xarast-app` | M | T8.1.2 |
| T8.6.2 | 2D field + slider widget, per model (HSV wheel-square, RGB planes, CMYK) | `xarast-ui` | L | T8.6.1 |
| T8.6.3 | Numeric entry with per-model ranges and units, clamped, keyboard-accessible | `xarast-ui` | M | T8.6.1 |
| T8.6.4 | Derivation editor: choose parent, tint/shade amount, HSV delta, with live preview | `xarast-ui` | M | T8.1.2 |
| T8.6.5 | Apply-to-selection vs redefine-palette-entry as two explicit actions | `xarast-app` | S | T8.1.6 |

The "live vs committed" split matters for undo: dragging in the 2D field must
repaint continuously but create one undo step. Same coalescing key mechanism as
W8.2, keyed on `(palette_id)` or `(selection_generation, attr_slot)`.

### W8.7 — Colour bar, colour gallery and drag-and-drop

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T8.7.1 | Colour bar widget: layout, paging, "no colour" entry, context menu | `xarast-ui` | M | T8.1.3 |
| T8.7.2 | Click semantics: left = fill, right / `Shift`+left = stroke | `xarast-app` | S | T8.7.1 |
| T8.7.3 | Internal drag-and-drop framework: drag source, drop target resolution, cursor feedback | `xarast-app` | L | phase 7 |
| T8.7.4 | Drop targets: object fill, object stroke, gradient stop, gradient arm (insert stop), palette slot (reorder) | `xarast-app` | M | T8.7.3, T8.3.2 |
| T8.7.5 | Colour gallery panel with the derivation tree | `xarast-ui` | M | T8.1.3 |
| T8.7.6 | Eyedropper command (`Ctrl+E`) reading from the CPU render surface | `xarast-app` | M | phase 5 |

**Drop-target resolution is the interaction detail that makes or breaks this
phase.** While a colour drag is in flight, every mouse move must resolve a drop
target and change the cursor. Resolution order, highest priority first:

1. A gradient **stop blob** under the pointer (only when the fill or
   transparency tool is active and that object's handles are shown).
2. A point **on the gradient arm** of a shown handle set → insert a new stop.
3. The **outline** of an object (within the stroke's device-space half-width,
   minimum 3 px) when `Shift` is held → stroke colour.
4. The **interior** of an object → fill colour.
5. A palette slot in the colour bar or gallery → reorder / redefine.
6. Nothing → forbidden cursor.

Dropping on a stop changes **only that stop**. Dropping on an object that has a
gradient fill, away from any handle, replaces the whole fill with a flat colour
— that is Xara's behaviour and it surprises people, so the status line says what
will happen while the drag is in flight.

Hit-testing an object interior during a drag must not walk the whole tree per
mouse move on a 100k-object document. Reuse phase 7's spatial index; if the
index is not there yet, this phase adds a coarse one (a uniform grid over the
visible viewport, rebuilt on scroll) and records the decision in
`docs/memory/ui.md`.

### W8.8 — Import, round-trip and corpus

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T8.8.1 | Verify `.xar` fill/transparency tags land in `FillGeometry` losslessly; fix gaps | `xarast-xar` | M | phase 3 |
| T8.8.2 | `.xarast` write/read of profiles, ramps, effects, tiling, blend modes | `xarast-format` | M | phase 6 |
| T8.8.3 | Ramp-profile baking to SVG stops with error ≤ 2/255 and `xarast:profile` re-read | `xarast-format` | M | T8.5.1 |
| T8.8.4 | `xarast-cli inspect --fills` reporting every fill in a document | `xarast-cli` | S | T8.8.1 |

`research/06 §6.4` requires the reader to **discard** baked intermediate stops
when `xarast:profile` is present and recompute them. Do not skip this: without
it, every save/load cycle re-bakes a baked curve and the error accumulates.

## Public API introduced

```rust
// ─────────────────────────────── xarast-color ───────────────────────────────

/// Phase 1 already owns the colour value types and the palette table:
/// `ColourValue`, `ColourModel`, `ColourId`, `ColourKind`, `ColourDef`,
/// `ColourTable` and `Colour::{Direct, Indexed}` all come from
/// `xarast-color` as delivered by phase 1, which built them to read `.xar`.
/// Phase 8 does NOT introduce a parallel palette type. It adds **editing** to
/// the table phase 1 already resolves, plus the invariants that editing needs.

/// Phase 1's `ColourKind` already carries `Normal | Spot | Tint { factor } |
/// Linked | Shade { x, y }` with a separate `parent: Option<ColourId>`.
/// Phase 8 adds no variant. What it adds is the guarantee that `parent`
/// forms a DAG and that every entry's `cached_rgb` is current.

/// Editing operations on phase 1's table. Cycle-safe by construction:
/// `reparent` is the only way to change `parent`, and it refuses a cycle.
impl ColourTable {
    /// Change an entry's own components. Returns every id whose resolved
    /// value changed, itself included — that is what the repaint needs.
    pub fn redefine(&mut self, id: ColourId, components: [Option<f32>; 4],
                    model: ColourModel)
        -> Result<SmallVec<[ColourId; 8]>, ColourEditError>;

    /// Rename. Fails on a duplicate name, because names are how `.xarast`
    /// and the UI refer to entries.
    pub fn rename(&mut self, id: ColourId, name: Arc<str>)
        -> Result<(), ColourEditError>;

    /// The only way to change `kind`/`parent`. `Err(Cycle)` if it would close
    /// a loop; `Err(TooDeep)` past `MAX_PARENT_DEPTH`.
    pub fn reparent(&mut self, id: ColourId, kind: ColourKind,
                    parent: Option<ColourId>) -> Result<(), ColourEditError>;

    pub fn remove(&mut self, id: ColourId, policy: OnDelete)
        -> Result<(), ColourEditError>;

    /// Parents before children: a valid order for a full recompute.
    /// Cached; invalidated by `reparent` and by `insert`.
    pub fn resolve_order(&self) -> &[ColourId];

    /// Bumped by every mutation. A component of the render `CacheKey`.
    pub fn epoch(&self) -> PaletteEpoch;

    /// Recompute `cached_rgb` for `id` and everything derived from it.
    /// Called by `redefine`/`reparent`; exposed for loaders.
    pub fn refresh_from(&mut self, id: ColourId) -> SmallVec<[ColourId; 8]>;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OnDelete {
    /// Derived entries and object uses keep their currently resolved value:
    /// children become `ColourKind::Normal` with `parent: None`, and
    /// `Colour::Indexed` uses become `Colour::Direct`.
    Detach,
    /// Fail if anything still refers to it.
    Reject,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
pub enum ColourEditError {
    #[error("would create a cycle in the derivation graph")]
    Cycle,
    #[error("parent chain would exceed ColourTable::MAX_PARENT_DEPTH")]
    TooDeep,
    #[error("no such colour")]
    NotFound,
    #[error("still referenced; use OnDelete::Detach")]
    StillReferenced,
    #[error("a colour with that name already exists")]
    NameInUse,
}

/// Monotonic. Part of every render `CacheKey`, so one palette edit
/// invalidates exactly the caches that depend on the palette.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct PaletteEpoch(pub u64);

/// Reverse index maintained by the attribute-set commands, so a palette edit
/// can compute its dirty region without walking the tree. Denormalised state:
/// rebuilt from scratch on load, never serialised.
pub struct ColourUses { /* HashMap<ColourId, SmallVec<[NodeId; 4]>> */ }

impl ColourUses {
    pub fn users(&self, id: ColourId) -> &[NodeId];
    pub fn rebuild(&mut self, doc: &Document);
}

/// Conversion between colour models. Document-scoped because it will later
/// carry ICC profiles (phase 15) — do not replace it with free functions.
pub struct ColourContext { /* … */ }

impl ColourContext {
    pub fn srgb_of(&self, v: ColourValue) -> Rgba8;
    pub fn convert(&self, v: ColourValue, to: ColourModel) -> ColourValue;
    pub fn resolve(&self, c: &Colour, table: &ColourTable) -> Rgba8;
}

/// The sentinel for "no colour" — phase 1's `BuiltinColour::None` promoted to
/// a first-class fill value. Distinct from a fully transparent colour: an
/// object with a `NONE` fill is not hit-testable in its interior.
impl Colour { pub const NONE: Colour; pub fn is_none(&self) -> bool; }

// ──────────────────────────────── xarast-doc ────────────────────────────────

/// Which slot of an object a fill-like attribute occupies.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PaintSlot { Fill, Stroke }

/// Identifies one draggable thing in a fill's handle set.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FillHandle {
    Start, End, End2, End3,
    Centre, Major, Minor,
    Corner1, Corner2,
    /// Index into `Ramp::stops`, i.e. an intermediate stop only.
    Stop(u16),
}

/// Commands. Each produces its inverse before applying (architecture §4).
pub struct SetFillGeometry   { pub node: NodeId, pub slot: PaintSlot, pub value: Paint }
pub struct MoveFillControl   { pub node: NodeId, pub slot: PaintSlot,
                               pub handle: FillHandle, pub to: Point }
pub struct InsertRampStop    { pub node: NodeId, pub slot: PaintSlot,
                               pub pos: f32, pub value: StopValue }
pub struct MoveRampStop      { pub node: NodeId, pub slot: PaintSlot,
                               pub index: u16, pub pos: f32 }
pub struct RemoveRampStop    { pub node: NodeId, pub slot: PaintSlot, pub index: u16 }
pub struct SetStopValue      { pub node: NodeId, pub slot: PaintSlot,
                               pub target: StopTarget, pub value: StopValue }
pub struct SetFillProfile    { pub node: NodeId, pub slot: PaintSlot, pub profile: BiasGain }
pub struct SetFillEffect     { pub node: NodeId, pub slot: PaintSlot, pub effect: FillEffect }
pub struct SetRampMapping    { pub node: NodeId, pub slot: PaintSlot, pub mapping: RampMapping }
pub struct SetTiling         { pub node: NodeId, pub slot: PaintSlot, pub tiling: Tiling }
pub struct SetTranspMode     { pub node: NodeId, pub mode: TranspMode }
pub struct MutateFill        { pub node: NodeId, pub slot: PaintSlot, pub to: FillShape }

/// `from`, `to`, or an intermediate stop — the thing a dropped colour lands on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StopTarget { From, To, Mid(u16), Corner(u8) }

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum StopValue { Colour(Colour), Transparency(u8) }   // 0 = opaque, 255 = clear

/// Type-change mapping, specified in W8.2. Returns what was lost.
pub fn mutate_fill<S: Stop>(g: &FillGeometry<S>, to: FillShape, obj_bounds: Rect)
    -> (FillGeometry<S>, MutationLoss);

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct MutationLoss { pub ramp_dropped: bool, pub points_approximated: bool }

/// Merge key for continuous drags. Two commands with equal, `Some` keys and
/// nothing between them become one undo step.
pub trait Coalesce { fn coalesce_key(&self) -> Option<CoalesceKey>; }

// ──────────────────────────────── xarast-app ────────────────────────────────

/// A single on-canvas control, already in document coordinates.
pub struct Handle {
    pub id: FillHandle,
    pub pos: Point,
    pub kind: HandleKind,
    pub selected: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandleKind { ArrowTail, ArrowHead, Blob, StopDiamond, CornerBlob }

/// Everything the overlay draws for one object's fill.
pub struct FillHandles {
    pub handles: Vec<Handle>,
    pub guides: Vec<Guide>,       // dashed arms, circles, rhombi
    pub arm: Option<(Point, Point)>,   // the parametric line stops sit on
}

/// Derive the handle set. Pure: no document mutation, no UI dependency.
pub fn fill_handles<S: Stop>(g: &FillGeometry<S>, object_to_doc: &Matrix) -> FillHandles;

/// Device-space hit test. `pick_radius_px` is a constant in pixels, never millipoints.
pub fn hit_handle(h: &FillHandles, doc_to_device: &Matrix,
                  p: DevicePoint, pick_radius_px: f32) -> Option<FillHandle>;

/// The fill tool and the transparency tool are one machine over two payloads.
pub struct FillLikeTool<S: Stop> { /* … */ }
pub type GradFillTool  = FillLikeTool<Colour>;
pub type TransparencyTool = FillLikeTool<Transparency>;

/// What a colour drag would do if released here. Resolved every mouse move.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColourDropTarget {
    ObjectFill(NodeId),
    ObjectStroke(NodeId),
    Stop { node: NodeId, slot: PaintSlot, target: StopTarget },
    InsertStop { node: NodeId, slot: PaintSlot, pos_on_arm: OrderedF32 },
    PaletteSlot(ColourId),
    None,
}

pub fn resolve_colour_drop(app: &AppState, p: DevicePoint, mods: Modifiers)
    -> ColourDropTarget;

// ─────────────────────────────── xarast-render ──────────────────────────────

/// THE ramp evaluator. One implementation, used by both backends and by the UI
/// preview. Do not reimplement the bias/gain curve anywhere else.
pub fn build_ramp(from: Rgba8, to: Rgba8, stops: &[RampStop<Colour>],
                  profile: BiasGain, space: RampSpace, len: usize) -> Arc<[Rgba8]>;

pub fn build_transparency_ramp(from: u8, to: u8, stops: &[RampStop<Transparency>],
                               profile: BiasGain, len: usize) -> Arc<[u8]>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RampSpace { Rgb, HsvShort, HsvLong }   // == FillEffect Fade/Rainbow/AltRainbow

impl BiasGain {
    /// Schlick bias/gain, identical to CProfileBiasGain (research/03 §2.6.3).
    /// Short-circuits to the identity at (0, 0).
    pub fn map(self, x: f64) -> f64;
}
```

## Acceptance criteria

Each is a command that can be run or a number that can be measured.

1. `cargo test -p xarast-color` passes, including a property test asserting
   `convert(convert(v, m), v.model())` round-trips within 1/255 per channel for
   RGB↔HSV↔grey and within 2/255 for anything through CMYK (the naive CMYK
   conversion of `research/03 §2.10` is not invertible to better than that).
2. `cargo test -p xarast-color table::cycles` proves `reparent` rejects every
   cycle: a randomised test building 1,000 random derivation graphs finds no
   accepted cycle and no rejected acyclic edge.
3. `xarast-cli inspect --fills Designs/'Fill Types simple.xar'` lists every fill
   in the file with its shape, stop count, profile and effect, and the shape
   histogram matches the one produced by `xar-dump --tags` for tags
   150-172 / 190-206 / 4075-4088.
4. For each of the 8 editable shapes, `fill_handles` returns exactly the handle
   count in the table in "In scope", asserted by a unit test.
5. `hit_handle` picks the intended handle at zoom levels 5 %, 100 % and 3200 %
   for every shape — one test per (shape, zoom), no failures. This is the test
   that catches a millipoint pick radius.
6. Golden images: one PNG per (shape × {flat, graduated} × 10 blend modes) =
   **160 images minimum**, produced through the **CPU backend**, all within the
   phase-3 perceptual gate (architecture open question 5). Inside flat colour
   regions the match is exact.
7. `build_ramp` with `BiasGain::IDENTITY` produces a byte-exact linear ramp for
   `len` in {256, 2048}: asserted, not eyeballed.
8. `build_ramp` with 20 randomly chosen `(bias, gain)` pairs matches a
   64-bit reference implementation of the Schlick formulas to ≤ 1/255 per
   channel at every index.
9. Save → load → save of a document containing every shape, a 7-stop ramp, a
   non-identity profile and each fill effect produces **byte-identical**
   `document.svg` on the second save, and the loaded `FillGeometry` values
   compare equal to the originals. This is the test that proves the reader
   discards baked stops (`research/06 §6.4`).
10. Dropping a palette colour on an intermediate stop changes that stop and
    nothing else: a test diffs the whole `FillGeometry` before and after and
    asserts a single field changed.
11. Redefining a palette entry used by 5,000 objects repaints all of them: the
    dirty rect returned by the render pass equals the union of their bounds, to
    the pixel.
12. Deleting a palette entry with `OnDelete::Detach` leaves zero dangling
    `Colour::Indexed` values: asserted by a full-tree sweep.
13. A drag of a gradient handle across 60 mouse-move events produces exactly
    **one** undo step, and `Esc` mid-drag restores the pre-drag geometry exactly
    (compared field by field) and leaves the history unchanged.
14. All ten blend modes are selectable in the transparency infobar and each
    produces a different rendering of the same test document (pairwise image
    distance > 0 for all 45 pairs).
15. `cargo clippy --workspace -- -D warnings` and `cargo deny check licenses`
    pass.

## Performance budgets

| Budget | Target | How measured |
|---|---|---|
| Frame time while dragging a gradient handle, `Designs/Spitfire.xar` at 100 % | ≤ 16 ms | `criterion` harness replaying a recorded drag |
| Frame time while dragging a handle, synthetic 100k-object document | ≤ 16 ms | must hold the roadmap's global pan/zoom budget |
| `build_ramp`, 8 stops, len 256 | ≤ 50 µs | `criterion` |
| `build_ramp`, 8 stops, len 2048 | ≤ 400 µs | `criterion` |
| Ramp cache hit | ≤ 200 ns | `criterion` |
| `ColourTable::redefine` on a 256-entry palette with a 4-deep derivation chain | ≤ 20 µs | `criterion` |
| Repaint after redefining a colour used by 5,000 objects | ≤ 100 ms to first paint | CLI timing harness |
| `resolve_colour_drop` per mouse move, 100k-object document | ≤ 1 ms | `criterion`, viewport-grid index |
| `ColourContext::convert` | ≤ 20 ns | `criterion` |
| Undo of a coalesced gradient drag | ≤ 1 ms | roadmap's global undo budget |

## Risks and mitigations

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Phase 4's exotic blend modes are not exact, so phase 8's golden images bake in the error | Medium | High | Phase 8 does **not** own blend correctness. Its golden images are regression locks, regenerated when phase 4 corrects a family. The LUT-extraction harness of `research/03 §3.8.3` is the authority, not these images |
| Grey-conversion weights are unknown (`research/03 §2.10`: Xara LX never calls `SetGreyConversionValues`, so CDraw's internal defaults apply and must be recovered empirically) | High | Medium | Phase 4 recovers them. Phase 8 keeps them in **one** constant, `render::GREY_WEIGHTS`, so correcting it is a one-line change and a golden-image regeneration. BT.601 (0.299/0.587/0.114) is the starting hypothesis |
| `egui`'s immediate mode makes a dense colour editor and a 256-swatch colour bar feel sluggish (architecture open question 2) | Medium | Medium | Colour bar is a single custom painted widget, not 256 widgets. The colour editor's 2D field is one texture regenerated only when the fixed axis changes. If it still misses, the fallback is a retained-mode custom widget layer, decided in phase 4's UI spike and recorded in `docs/memory/ui.md` |
| Drag-and-drop on Wayland: internal DnD must not go through the compositor | Low | Medium | All drags in this phase are **intra-window** and handled entirely in `xarast-app`. No `wl_data_device` involvement. Cross-application colour drag is out of scope |
| The derivation DAG turns into a performance problem on documents with hundreds of linked colours | Low | Low | `resolve_order` is computed once and cached; redefinition walks only the affected subtree. Measured by the budget above |
| `MutateFill`'s point mapping surprises users, producing gradients that jump | Medium | Low | The mapping is specified in W8.2 rather than improvised, and the phase adds a visual test sheet (one document, mutate every shape to every other shape, 56 cells, eyeballed once and then locked as a golden image) |
| Coalescing merges two drags the user meant to be separate | Low | Medium | The key includes the handle identity and merging stops at any intervening command; additionally, a drag that starts more than 250 ms after the previous one ended starts a new step |
| Users expect a colour dropped on a gradient-filled object to tint the gradient, not flatten it | Medium | Low | Match the original's behaviour (flatten), but announce it in the status line during the drag. Revisit only with user feedback |

## Test plan

**Unit.** Colour conversions (round-trip properties, known-value tables for the
sRGB primaries and for 8 CMYK reference patches). Palette resolution order,
cycle rejection, delete policies. `build_ramp` against a `f64` reference.
`mutate_fill` for all 56 shape pairs. `fill_handles` counts and positions.
`hit_handle` across zoom levels.

**Property.** Random `FillGeometry` values survive
`serialise → deserialise → compare`. Random sequences of ramp edits keep
`stops` sorted by position and keep `from`/`to` out of the ramp. Random
command sequences followed by full undo return a document byte-identical to the
start (this is the phase-2 invariant, re-run with phase 8's commands added).

**Golden images.** The 160-image matrix from acceptance criterion 6, through the
CPU backend. Plus per-corpus renders of `Designs/Fill Types simple.xar`,
`Designs/SimpleSphere.xar`, `Designs/WATCH.xar`, `Designs/Groucho2.xar` and
`testfiles/20000GradFilledShapes50PCtransparent.xar` — the last one is the
stress case for graduated transparency at volume.

**GPU/CPU parity.** Every golden image is also produced on the GPU backend and
compared; the `Final` path must match bit for bit per architecture §3.3.

**Round-trip.** For each of the 59 corpus files: import `.xar` → save `.xarast`
→ reload → render → compare against the render of the direct import. Zero
differing pixels. A file whose fills do not survive is a phase-8 bug even if the
importer is phase 3's code.

**Interaction.** Scripted input sequences through the command bus (not through
the toolkit) driving: drag each handle type; insert / move / delete stops; drop
a colour on each drop-target kind; cancel a drag with `Esc`; mutate a fill;
redefine a palette colour mid-drag. Each asserts the resulting document state
and the resulting undo-stack depth.

**Fuzz.** `cargo-fuzz` target on `ColourTable` command sequences (create / reparent /
redefine / delete with random ids), asserting no panic and no cycle ever becomes
reachable.

**Accessibility.** The colour editor must be fully operable from the keyboard
and every swatch must expose a name and its resolved sRGB value through
`accesskit`. Verified by a test that walks the accessibility tree and asserts
every swatch node has a non-empty label.

## Memory note

Update **`docs/memory/render.md`** with:

- The ramp construction contract: `build_ramp` is the single implementation;
  256 entries in `Draft`, 2048 in `Final`; the identity short-circuit is
  mandatory.
- The value of `GREY_WEIGHTS` actually in use and how it was determined.
- Which blend families are exposed to users and which exist only internally.
- The dirty-region rule for a fill edit (union of old and new fill extents,
  intersected with object bounds) and the `palette_epoch` component of
  `CacheKey`.

Update **`docs/memory/ui.md`** with:

- The fill/transparency tool state machine as implemented, including the
  `Esc`-cancel and coalescing rules.
- The pick radius finally chosen and why.
- The drop-target resolution order.
- The spatial index used for interior hit-testing during drags.
- Whether `egui` held up at colour-bar and colour-editor density (this is
  evidence for architecture open question 2).

Create **`docs/memory/colour.md`** from the template in
`docs/memory/INDEX.md`, and add its row to that index. It must record:

- The derivation model (phase 1's `ColourKind`) and the decision that
  resolution is a cached parent-before-child order, not a refcount.
- The `OnDelete` policy set and why there is no third option.
- The invariant "`ColourDef::cached_rgb` is never stale after an edit" and
  where it is enforced.
- The reserved slot for ICC (phase 15): nothing may assume sRGB at storage time.
- Dead ends encountered while matching Xara's tint/shade arithmetic.
