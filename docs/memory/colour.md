# colour

Memory note for **colour models, the palette and palette/fill editing** —
`xarast-color` (models, conversions, `ColourTable` editing) and the phase-8
command modules of `xarast-doc` (`palette`, `fill_edit`). Phase spec:
[`../phases/phase-08-colour-fills-transparency.md`](../phases/phase-08-colour-fills-transparency.md)
§W8.1, §W8.2. Facts about the original: `research/02 §5.10.1` (added in this
phase). Phase-1 colour decisions (naive CMYK, the inherit sentinel, the
depth limit) stay in [`geometry.md`](geometry.md).

## Current state (2026-09-23, XARA-US-0037 / XARA-US-0038)

| Piece | Where | State |
|---|---|---|
| Conversions matching the original (CMYK black generation, grey weights, HSV grey threshold) | `xarast-color/src/model.rs` | done |
| `pack_component` / `to_rgba8_packed` — the original's 8-bit packing | `model.rs` | done |
| `tinted` (per model), `shaded` (signed, in HSV) | `model.rs` | done |
| `ColourKind::from_raw` (signed shade), used by the `.xar` reader | `table.rs`, `xarast-xar/src/colour.rs` | done |
| `ColourContext` (`srgb_of`, `convert`, `resolve`) | `context.rs` | done, holds nothing yet |
| `ColourTable` editing: `redefine`, `rename`, `reparent`, `remove(OnDelete)`, `refresh_from`, `refresh_all`, `repair_cycles`, `resolve_order`, `descendants_of`, `children`, `epoch`, `advance_epoch_past` | `table/edit.rs` | done |
| `Action::SetPalette`, `EditError::{Palette, FillEdit}` | `xarast-doc/src/history.rs` | done |
| Palette commands `CreateColour`, `RedefineColour`, `RenameColour`, `ReparentColour`, `DeleteColour` | `xarast-doc/src/palette.rs` | done |
| `ColourUses` (rebuild-only), `PaletteResolver`, `palette_refs`, `for_each_colour`, `map_colours`, `changed_between`, `Document::palette_epoch` | `palette.rs` | done |
| Palette in `canonical_digest` | `document.rs` → `palette::digest_palette` | done |
| Palette loops broken at build (`repair_cycles` + `Repaired` diagnostic) | `builder.rs` | done |
| Fill commands `SetFillGeometry`, `MoveFillControl`, `InsertStop`, `MoveStop`, `RemoveStop`, `SetStopValue`, `SetFillProfile`, `SetRampMapping`, `SetFillEffect`, `SetTiling`, `SetTranspMode` | `xarast-doc/src/fill_edit.rs` | done |
| Pure helpers `move_control`, `set_stop`, `stop_value`, `ramp_insert`, `ramp_move`, `rebuild_ramp`, `fill_arm`, `arm_position`, `fill_in_force`, `set_own_attr` | `fill_edit.rs` | done |
| `MutateFill`, `mutate_fill` (the W8.2 mapping table), `FillShape`, `MutationLoss` | `xarast-doc/src/fill_mutate.rs` | done (XARA-US-0039, for the type menu) |
| Group transparency (T8.2.7), "no colour" value (T8.1.5), `.xarast` palette round trip (T8.1.7) | — | **not done**, filed |
| App `EditCommand::Fill` for the fill commands | `xarast-app/src/ops.rs`, `fill_tool.rs` | done (XARA-US-0039) |
| Walker using `PaletteResolver` + epoch in cache keys; palette `EditCommand` variants | `xarast-app` | **not done**, filed |

Tests: `xarast-color/tests/palette.rs` (24, incl. `table::cycles` over 1 000
random graphs and the round-trip error table), `xarast-xar/tests/palette_corpus.rs`
(corpus oracle), `xarast-doc/tests/fill_palette.rs` (14, every command with
digest-exact undo/redo, a 60-event drag = one step, a 5 000-object
`ColourUses` dirty set, detach sweep, ramp-edit property test).

### Measured

**Conversion round trip** (`round_trip_error_table`, max |error| over a dense
grid, units of 1/255; the bound is 1 for RGB/HSV/grey, 2 through CMYK):

| from → via → from | error |
|---|---|
| RGB → HSV | 0.000091 |
| RGB → CMYK | 0 |
| HSV → RGB | 0 |
| HSV → CMYK | 0 |
| CMYK → RGB | 0 |
| CMYK → HSV | 0.000091 |
| grey → RGB / HSV / CMYK | 0.000030 |

RGB → grey → RGB is lossy by definition and not in the table. Through
CMYK only the colour channels are compared: CMYK has no transparency.

**Corpus oracle**: all **5 760** palette entries of the 59 files resolve to
their record's cached RGB **exactly** — normal CMYK 867, HSV 3 762, RGB 410,
grey 81; tints CMYK 231, HSV 300, grey 10; shades HSV 77, RGB 4; links 18.
Plain round-to-nearest would get **545** of them wrong.

**Benchmarks** (`cargo bench -p xarast-color --bench colour -- --quick`):
`redefine` on 256 entries with a 4-deep chain **0.94 µs** (budget 20 µs);
`ColourContext::convert` RGB→CMYK 6.5 ns, RGB→HSV 9.0 ns (budget 20 ns).

## Decisions taken (and why)

1. **Conversions follow the shipped (non-CMS) branch of the original.**
   LX defines `NO_XARACMS`. RGB → CMYK generates black past 50 %; the
   phase-1 note that "any forward transform other than `K = 0` loses
   colour" was wrong — this one is exactly invertible under the naive
   reverse (`(C − K) + K = C ≤ 1`).
2. **Grey is 0.305 / 0.586 / 0.109** (`GREY_MODEL_WEIGHTS`), the kernel's.
   Not the renderer's blend luminance, which is CDraw's and lives in
   `xarast-render` (`research/03 §2.10`). Two constants on purpose.
3. **Two quantisers.** `to_rgba8` rounds; `to_rgba8_packed` is the
   original's `+0x8000 … ×255 >> 24` and is what `cached_rgb`,
   `ColourContext::srgb_of` and `PaletteResolver` produce. `to_rgba8` was
   not changed, so no render golden moved; switching the walker is part of
   the filed wiring task.
4. **Tints apply in the child's model, shades in HSV, both after bringing
   the parent into the child's model.** The original skips the conversion
   for tints because a tint takes its parent's model when linked; converting
   is identical then and sensible if they drift. `reparent` to a tint or
   shade sets the model to the parent's, as the original does.
5. **A normal or spot colour ignores its `parent` field** in resolution, as
   the original's `GetSourceColour` does. `live_parent` (derived kind + the
   parent exists) is the one definition of "edge" everywhere: order,
   descendants, cycle checks.
6. **Shade coordinates are signed** and read with `ColourKind::from_raw`;
   `Fixed24::to_f32` clamps to `0..1` and would have turned every darkening
   shade into "no change".
7. **Palette edits swap the whole table** (`Action::SetPalette`). Undo of a
   delete brings the entry back under its **old `ColourId`** — impossible
   with per-entry re-insertion into a generational slot map. The clone is
   microseconds for a real palette.
8. **The epoch only moves forward**, undo included (`advance_epoch_past`):
   after undo + a different edit, a restored epoch number would otherwise
   name two different palettes.
9. **`reparent` is the only editing path to `kind`/`parent`.** It refuses
   `Cycle` (self or a descendant), `TooDeep` (any chain would reach
   `MAX_PARENT_DEPTH` ancestors), `MissingParent`, `UnexpectedParent`.
   Unlinking bakes the resolved value into the components (original
   behaviour); linking from another kind starts with all four components
   overriding. `set_parent` stays for loaders, followed by `repair_cycles`.
10. **`repair_cycles` demotes the youngest (highest slot) entry on a loop**
    to a normal colour holding what it resolves to now (its cached RGB), or
    the offending entry itself for a chain that is merely too deep. Runs in
    `DocumentBuilder::finish` with a `Repaired` warning. A file is never
    rejected for this.
11. **`refresh_from` reports `id` plus descendants whose packed value
    moved** — the repaint set — and rewrites their `cached_rgb`. Loaders do
    not refresh: the file's cached value is the fallback for unresolvable
    entries (and equals our resolution anyway, per the oracle).
12. **Delete policy**: `Reject` fails on any use (attribute, layer guide
    colour, guideline, derived colour); `Detach` (the original's forced
    delete) turns attribute uses into `Colour::Direct` of what they
    resolved to, local tint included, clears guide colours to the default,
    and makes derived entries normal. The sweep covers **every node in the
    arena**, reachable or retained by the history, so undoing an old
    deletion can never resurrect a dangling reference.
13. **Fill commands address `(node, PaintSlot, FillChannel)`** and edit the
    fill **in force**, writing it back as the node's own attribute (replace
    the attribute child of that slot, else add one first). Editing an
    inherited fill localises it; undo removes the added attribute.
    Effect and tiling are separate attributes, exactly as in the format.
14. **Coalescing is keyed by the drag**: `MoveFillControl`/`MoveStop` take
    `drag: Option<u64>` (the tool passes its bus gesture) and hash
    `(drag, node, slot, channel, handle)` into the `CoalesceKey`. Same
    handle, same drag → one step; the history only merges into its newest
    step, so anything in between splits it.
15. **Handle names per shape** are in `FillHandle`'s docs. A linear fill's
    third (skew) point is not in `FillGeometry::Linear`, so `End2` is
    refused there. Moving a centre moves the whole fill; an aspect-locked
    radial keeps its minor axis at 90° and equal length.
16. **`Ramp`'s stop list is private**, so the edit module rebuilds ramps
    through `Ramp::insert` (`rebuild_ramp`) instead of widening `fill.rs`.
    A moved stop re-sorts, after any stop at the same position;
    `ramp_move` returns the new index so a tool keeps hold of the stop.
17. **A transparency stop value is a level**; the stop keeps its mode.
    `SetTranspMode` rewrites every stop's mode.
18. **`mutate_fill` follows the W8.2 table**: flat → gradient takes the
    bounding box (diagonal for linear, inscribed circle otherwise) and a
    caller-given far value (`desaturated` for colour, `clear_end` = 255 for
    transparency); gradient ↔ gradient maps `start`/`centre` and
    `end`/`major`/`corner1`, the third point `start + perp(end − start)`
    unless the source had one; to three/four colour drops the ramp and
    seeds the extra colours from its middle stop or `to`. Bitmap, fractal
    and noise sources are refused. The fill commands derive `PartialEq`.

## Invariants that must not be broken

1. Every `ColourValue` component is in `0..=1` and finite (phase 1); shade
   coordinates in `[-1, 1]`.
2. After any `ColourTable` edit, the derived graph is acyclic and no entry
   has `MAX_PARENT_DEPTH` or more ancestors (`table::cycles`).
3. After any edit, `cached_rgb == resolve(id).to_rgba8_packed()` for every
   entry the edit touched (`cached_values_stay_current`).
4. No table edit leaves a `parent` pointing at a removed slot.
5. The epoch is strictly increasing across every mutation, undo included.
6. `canonical_digest` covers the palette (slot order, ids, all fields but
   the epoch and the order cache): palette undo is proved by digest.
7. A refused command (palette or fill) leaves the digest and history
   unchanged.

## Dead ends (do not retry)

- **`RGB → CMYK` as `C = 1 − R, K = 0`** "because anything else breaks the
  round trip": false; the original's black generation is invertible.
- **Rec. 601 for the grey model**: the kernel uses 0.305/0.586/0.109.
- **`round(v × 255)` for cached values**: 545 corpus entries off by one.
- **Shade as `s × x, v × y`** (phase 1's guess): the original's shade is a
  signed move towards 0 or 1; the guess made `(0, 0)` black.
- **Re-inserting a deleted palette entry on undo**: a slot map cannot put
  it back under the same key.

## Open TODOs

- [ ] Walker: resolve through `PaletteResolver`, fold `palette_epoch` into
      render cache keys, repaint `ColourUses::users_of(changed)`
      (XARA-T-0203).
- [x] App: `EditCommand::Fill` and the fill/transparency tools
      (XARA-US-0039). Palette commands still dispatch directly
      (rest of XARA-T-0212).
- [ ] `ColourUses` incremental maintenance in the attribute-set paths
      (today: rebuild on load and after a batch).
- [ ] T8.1.5 "no colour" as a first-class `Colour` value (XARA-T-0204;
      today it is `Option<Colour>` in the importer); T8.1.7 palette
      `.xarast` round trip with signed shades (XARA-T-0205); T8.2.5
      `MutateFill` (XARA-T-0210, done); T8.2.7 group transparency (XARA-T-0211).
- [ ] `ColourModel::Ciet` has no converter in the original at all; ours is
      a real XYZ transform. No corpus file uses it.
- [ ] The original drops transparency in every model conversion; we carry
      it. Revisit only if a file shows the difference.
