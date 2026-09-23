# xarast-format

Memory note for the native **`.xarast`** format (`crates/xarast-format`).
Normative spec: `docs/research/06-xarast-format.md`. Plan:
`docs/phases/phase-06-xarast-format.md`. Epic XARA-EP-0007.

## Current state

Phase 6, round 1 (2026-09-23): the byte layer. Round 2 (2026-09-23): the
SVG profile writer (W3), the document-level save and `xarast-cli convert`.
Round 3 (2026-09-23): passes 4–5, `zlib-rs` everywhere, the path-data
separator fix; the reader (W4): `svg::read_svg`, `normal_form`, `open`,
`save_opened`, F4.7 marking in `xarast-doc`, `.xarast` in the app's open
path. Round 4 (2026-09-23): the writer gaps the corpus round trip found —
opaque subtrees (XARA-T-0108), fill mapping (T-0109), first re-save
differences (T-0110), ambiguous twins (T-0111) — closed; the round trip
is exact for all 59 files (below) and its tests have no exception list.
Round 5 (2026-09-23, after bitmaps started decoding — T-0129): the
JPEG8BPP palette (XARA-T-0154, also the cause of T-0157's scope3 gap),
bitmap transparencies as luminance masks and contone fills as duotone
filters for browsers; the render round trip is exact again for 59/59 and
`KNOWN_RENDER_GAPS` is deleted. Round 6 (2026-09-23, after text started
rendering from the real layout — XARA-T-0172): stories are written as
exact runs (`research/06 §6.7.1`), placed for browsers by the
application's layout; the render round trip is exact for 59/59 again and
`KNOWN_TEXT_GAPS` is deleted; the model's default text size is 16 pt.

| Workstream | State | Where |
|---|---|---|
| W1 container | F1.1–F1.8 done | `name.rs`, `sniff.rs`, `eocd.rs`, `reader.rs`, `writer.rs`, `limits.rs` |
| W2 manifest | F2.1–F2.5, F2.8 (diagnostics only) done; **F2.6/F2.7 `meta.xml` model open** (a minimal `meta.xml` writer exists: `save::meta_xml`) | `manifest.rs`, `digest.rs`, `reader.rs::consistency` |
| W3 SVG write | F3.1–F3.8, F3.11 done; F3.9 all eight passes done (4–5: XARA-T-0101, round 3); F3.10 baking open (XARA-T-0102) | `svg/` (`num`, `pathdata`, `frame`, `xml`, `defs`, `paint`, `style`, `emit`), `save.rs` |
| W4 SVG read + preservation | F4.1–F4.6, F4.8–F4.10 done; F4.7 marking done, deletion accounting open (XARA-T-0113); XARA-T-0105 (localise + normal form) and T-0107 (passes 4–5) done | `svg/read/` (`dom`, `parse`, `style`, `build/{ink,paint,root}`, `normal`), `open.rs` |
| W5 resources | F5.1–F5.4, F5.6 done; F5.8 contract + validation done (no provider implementation) | `resource.rs`, `policy.rs`, `thumbnail.rs` |
| W6 durability | F6.1 (`write_atomic`, `.bak` = F6.2) and F6.3 (`DocumentLock`) done; F6.4–F6.9 open | `durability/` |

Public entry points: `XarastReader::{open, open_with}`, `PackageWriter`,
`ResourceIndex`, `Manifest`, `sniff`/`sniff_bytes`, `write_atomic[_with]`,
`DocumentLock`, **`svg::write_svg`**, **`save`/`save_to`** (the spec's
`save_atomic` for a first save: SVG + resources + `meta.xml` + container,
atomically) and `meta_xml`; **`svg::read_svg`**, **`svg::normal_form`**,
**`open`/`open_with`/`open_reader`** (→ `OpenedDocument`, which keeps the
package reader) and **`save_opened`/`save_opened_to`** (re-save with raw
copies). `xarast-cli convert <in.xar|DIR>… (-o F | --out-dir D)` drives
the writer; `cargo xtask svg-render <svg> <png> [width]` is the resvg
check; `cargo run --release -p xarast-format --example roundtrip --
FILE.xar|DIR…` prints where save → open → save first differs.

### How the SVG layer plugs in (W3/W4)

- **Save:** serialise the model to `document.svg` bytes and `meta.xml` bytes →
  `PackageWriter::set_document` / `set_meta`; `ResourceIndex::begin_recount`,
  `count_path` for every `href` written, `mark_referenced_in` over the foreign
  baggage text, `gc`, then `add_resources`; `carry_from(&source_reader)` for
  preserved entries; `finish_with_source(file, Some(&mut source_reader))`
  inside `write_atomic`.
- **Open:** `XarastReader::open` (never parses the SVG) → show
  `diagnostics()` / `suggests_read_only()` → `document_bytes()` to the SVG
  reader → `ResourceIndex::from_package(&reader)` → keep the reader alive for
  raw copies on the next save.
- `ThumbnailProvider` takes `&xarast_doc::Document`; the implementation lives
  over the renderer (xarast-app), never in this crate.

## The SVG profile writer (W3) — how it works

`write_svg(doc, &mut ResourceIndex, &SvgOptions) -> SvgDocument { svg,
stats, foreign_count, foreign_digest }`.

- **One model node, one element.** Every reachable non-attribute node is
  exactly one element with `id="x" + tag in Crockford base 32` (`xarast:id`
  on `xarast:` elements), in document order, so z-order is document order
  and W4 can map elements to nodes one to one. Exceptions: chapters (rows in
  `<xarast:document>`), text lines (`<tspan id>`), characters (text). The
  clip shape of a ClipView is its element, placed inside the `<clipPath>`.
- **Attribute nodes produce no element; every ink element's paint is
  resolved** with `AttrStack` exactly as the renderer's walker does (a
  parent's ink after its children's attributes), then passes 4–5 move
  shared values onto `<g>`s and into CSS classes — see "Passes 4–5" below
  for exactly what the reader must resolve. Consequence for W4: see
  XARA-T-0105 (the reader must localise, and the model round trip compares
  a normalised form).
- **Coordinates** (`svg/frame.rs`): per spread, `(x − ox, oy − y)` in
  integer millipoints, `(ox, oy)` = top-left of the spread's pages, written
  on the spread as `xarast:origin` so the inverse is exact. Formatting is
  from the integer (`num.rs`), never through a float.
- **Spreads**: the first is a `<g>` framed by the root viewBox (its pages);
  the others are nested `<svg x=0 y=…>` stacked below, outside the root
  viewBox, each with its own frame.
- **Shapes**: `<rect>` only when axis-aligned in the canonical orientation
  (major right, minor up in the document); `<ellipse>`/`<circle>` only with
  even diameters (the centre must be an integer millipoint); otherwise
  `<path>` + `xarast:shape` + `xarast:parallelogram` (the exact frame).
  Quick shapes: generated outline + `<xarast:quickshape>`; edge templates
  are written in the shape's own space, **not flipped**.
- **Bitmaps**: `.xar` corners put the image's **top-left at the origin**,
  the major axis along the top row, the minor axis down the left column
  (`research/01 §4.5`: as a fill p3 is start, p2 end, p0 second end). A
  bitmap *fill*'s origin is the image's bottom-left and `axis_y` its
  top-left. Both verified visually (Spitfire, SimpleText, leafgirl) in
  resvg, Inkscape and Chrome. The original encoded bytes go into the
  `ResourceIndex` once per bitmap; every `href` written counts one
  reference.
- **Bitmap palettes** (`research/06 §6.9.1`, XARA-T-0154): a `.xar`
  tag-71 bitmap keeps its snap palette in `BitmapData::palette`; the
  writer stores it as a `resources/blobs/b3-….bin` blob (`r g b a` per
  entry, 1–256) and writes `xarast:palette` wherever it writes the
  image's `href` — the node `<image>`, the fill pattern's `<image>`, the
  transparency twin, the mask pattern's `<image>` (`BitmapRef::attrs`).
  `write_svg`'s href closure inserts the blob and counts it on every
  cache hit like the image. The reader keys bitmaps by *(href, palette)*
  (`bitmap_for`); a bad palette is a `DanglingReference` warning. The
  normal form prints the palette's BLAKE3 with the image's.
- **Bitmap transparency / contone for browsers** (`§6.9.2`): a bitmap
  transparency also gets a `<mask>` (rect of the image pattern through
  `<filter xarast:filter="transparency-mask">`, `1 − BT.601 luma`, alpha
  forced to 1 — what the CPU renderer's `LevelSampler` reads); a contone
  pattern's `<image>` goes through `<filter xarast:filter="contone">`
  (luma matrix + `feComponentTransfer` tables from the key colours as
  written: 2 entries for a fade, 17 for rainbows). The reader knows a
  `<filter>` only by that marker (`root.rs::is_known_def`); any other is
  foreign and kept. A `<mask>` whose rect is not a gradient falls back to
  the twin (`flat_or_twin` with alpha 1), and the mask box padding
  (`mask_pad`, the only place an unstroked element's line width lives)
  is read whether the mask is a gradient or not.
- **Gradients** are exact for linear/circular/elliptical fills. With
  `gradientUnits="userSpaceOnUse"` a radial gradient's `cx`/`cy` default to
  **50 % of the viewport**, so an elliptical one (unit circle mapped by
  `gradientTransform`) must write `cx="0" cy="0"` — found by rendering
  `Fill Types simple.xar` (all elliptical fills came out black). Only the
  "extra" repeat tiles (`spreadMethod="repeat"`), matching the renderer.
- **Ramp baking**: resolved colours in a `Ramp<ColourValue>`, sampled with
  `Ramp::sample` (so profile, sin easing and HSV rainbow are the model's own
  maths), 8 uniform segments bisected up to 5 times until the midpoint error
  is ≤ 2/255; twin: `xarast:profile`, `xarast:ramp-mapping`,
  `xarast:fill-effect`, `xarast:stops="pos:#rrggbb[aa] …"`. A plain RGB ramp
  gets its key stops only (rule 5).
- **Transparency**: flat → multiplied into `fill-opacity`/`stroke-opacity`;
  graduated → `<mask>` over the element's box (+ stroke) with a greyscale
  gradient, `color-interpolation="sRGB"` on both; a fill transparency is
  applied only when there is a fill. Mode → `style="mix-blend-mode:…"` +
  `xarast:blend` (contrast/brightness have no CSS keyword: name only).
- **Defs** are keyed by their full text; id = one-letter kind + BLAKE3
  prefix (4 hex, extended on collision): identical gradients/masks/patterns
  collapse and ids are stable across saves. Palette ids are `c-N`.
- **Foreign baggage** (`Tree::foreign`): attributes after the known ones,
  sorted by (URI, local); namespaces declared once on the root, reusing the
  reader's prefix when free; a clash with a known attribute or a non-NCName
  is dropped and counted. Fragments are written raw at their position, only
  if well-formed (`xml::fragment_is_well_formed`). Marks become
  `xarast:foreign-dirty` / `foreign-stale` / `base-authoritative`.
  `xarast:foreign-digest="blake3:<hex>"` = BLAKE3 over, in emission order,
  for each owner: its id, then each written attribute's URI, local name and
  value, then each fragment's raw text — every field as u64-LE length +
  UTF-8 bytes. `foreign-count` = attributes + fragments written; both are
  omitted when zero. **W4 must compute the digest the same way.**
- **Opaque nodes** (an unknown `.xar` record): `<xarast:opaque
  xarast:tag xarast:encoding="base64">PAYLOAD` then the record's own
  subtree as ordinary profile elements, then `</xarast:opaque>`
  (`research/06 §8.2.1`). Not SVG, so resvg, Chrome and Inkscape draw
  none of it — the same as the renderer, which skips opaque subtrees. It
  is a `GroupKind::Block` for passes 4–5: nothing hoists into or through
  it. Childless opaque nodes keep the old one-line form.
- **Exact twins** (`research/06 §6.14`, round 4): whatever a reader
  would have to guess is written, and whatever is derived (baked stops,
  flat approximations) is computed from the values **as written**
  (`paint::key_colour`: a palette key resolves exactly, a literal one is
  quantised to its 8-bit spelling first). Palette components, tint
  amounts, shade factors and key positions use `num::f32s` (shortest
  round-trip `f32`); key colours carry `xarast:stop-refs` /
  `colour-refs` / `contone-refs`; plain ramps whose key positions four
  decimals do not pin also get `xarast:stops` / `xarast:levels`; a
  `cx cy r` circle gets `xarast:major` when the reader would not derive
  it; `xarast:fill-repeat` on bitmap patterns, `xarast:repeat` and
  `xarast:fill-effect` on every twin; transparency twins carry levels,
  profile, procedural parameters, the bitmap `href` and the mapping; the
  stroke's own transparency is `<xarast:stroke-transparency>`,
  `xarast:stroke-mask` and `xarast:stroke-blend` (then `xarast:blend` is
  the fill's alone). `meta.xml` counts the bitmaps written
  (`Stats::bitmaps`), not the model's.
- **Live effects**: controller `<g xarast:kind="blend|…">` with the
  parametric element first; generated children in `<g
  xarast:generated=… xarast:generated-by=… xarast:base-authoritative="true">`
  — nothing regenerates them before Phase 13, so a reader must keep them.
- **Text** (round 6, XARA-T-0172; normative in `research/06 §6.7.1`,
  shared code in `svg/text.rs`): `<text xarast:exact="true" transform>`
  (story matrix conjugated by the flip; `xarast:matrix` with the exact
  `a b c d` when six decimals do not pin them), one `<tspan>` per
  `TextLine` (its node ruler as `xarast:ruler`), and inside it one
  `<tspan>` per **run**: consecutive items whose *written* attributes are
  identical. Grouping: a snapshot of the `AttrStack` at each item whose
  state changed (dirty flag); an equal snapshot continues the run, a
  different one builds the candidate run's start tag and merges when it
  writes the same (so restore attributes and slots the profile does not
  carry never split runs, which keeps the first re-save a fixed point).
  Each run: text attributes + twins (`run_text_attrs`), then paint through
  the ink `paint()` (strokes, transparencies, twins as child elements),
  registered as a styler slot inside a `GroupKind::Block` frame opened on
  the `<text>`. Content: characters, `<xarast:kern xarast:em>`,
  `<xarast:eol [xarast:soft]>`, `<xarast:char xarast:code>` for non-XML
  characters; nothing between elements (`xml:space="preserve"`). An
  item-less line writes one empty run from the state **outside** the line
  scope (what `StoryText` resolves for it). Placement: with
  `SvgOptions::text` (a `Placer` over a `TextPlacer`; `xarast-app`'s
  `svg_text::placer()`, used by `xarast-cli convert`) every run gets `x`
  / `y` lists from the real layout and substituted families join the
  `font-family` chain (+ `xarast:font-substitute`, informative); without
  it the old line-per-baseline fallback (`x=0`, `y` += line height,
  `text-anchor`). Stories on a path are still laid out as lines.

### Passes 4–5: hoisted paint and CSS classes (XARA-T-0101) — the reader contract

`svg/style.rs`. The walk writes every start tag *except* its paint and
leaves a **slot** (13 interned values) at the end of the tag; every `<g>`
gets one too. Hoisting runs when a `<g>` closes; classes are chosen after
the walk; the body is then copied out with every slot expanded. Nothing is
parsed back, and `SvgOptions { hoist, classes }` (both on by default)
switch each pass off alone. Output written **before** round 3 (no hoisting,
no classes) is a special case of this contract, so a reader implementing it
reads both.

**The properties** (and nothing else) that live in slots, in written order:
`fill`, `fill-opacity`, `fill-rule`, `xarast:fill-ref`, `stroke`,
`stroke-opacity`, `stroke-width`, `stroke-linecap`, `stroke-linejoin`,
`stroke-miterlimit`, `stroke-dasharray`, `stroke-dashoffset`,
`xarast:stroke-ref`. `vector-effect`, `opacity`, `mask`, `style`
(`mix-blend-mode`), `xarast:blend` and every other attribute stay on the
element; none of them is ever hoisted or classed. A slot's attributes come
**last** in the start tag (after foreign attributes).

**What the reader must resolve, per element, to get the resolved paint the
writer started from:**

1. **Inheritance (pass 4).** For each property above, the value is the
   element's own attribute if present; else the class rule's value (point
   2); else the **nearest ancestor `<g>`**'s attribute (or its class's
   value); else the SVG initial value (`fill` black, `stroke` none,
   `stroke-width` 1, …; for the two twins: *no palette reference*). This is
   plain SVG inheritance for the SVG properties; the profile declares
   `xarast:fill-ref` and `xarast:stroke-ref` **inherited the same way**.
   The writer guarantees it only hoists a value when every painting child
   (ink, or a `<g>` over ink) had that exact value, and that text, images,
   foreign fragments and nested `<svg>` spreads block any hoist above
   them, so no element ever inherits a value it did not have. Values are
   hoisted onto *any* `<g>`: groups, layers, the first spread, ClipView and
   live-effect `<g>`s. A `<g>` whose foreign baggage already carries a
   paint property, a `class` or a non-`display` `style` never receives one.
2. **Classes (pass 5).** One `<style type="text/css">` is the **first
   child of `<defs>`**, holding only rules of the form
   `.NAME{prop:value;prop:value}` (one per line, class selectors only,
   properties from the list above minus the twins, values exactly as the
   attribute would be written). An element with `class="NAME"` takes those
   declarations as if they were its own attributes (it never also carries
   them as attributes). The class's **twins** are not CSS (resvg drops the
   rest of a rule at a custom property — found by rendering): they are
   `<xarast:paint-class xarast:class="NAME" xarast:fill-ref="#c-N"
   xarast:stroke-ref="#c-M"/>` elements right after the `<style>`, one per
   class that has twins; apply their `xarast:*-ref` attributes to every
   element of that class. `NAME` is `c1`, `c2`, … unless the document's
   foreign baggage already uses `c<digits>` class names, in which case the
   prefix is `xc`, `xrc` or `xarast-c` (first free one) — so a reader must
   take the names from the `<style>`, not assume `cN`. Classes go on ink
   elements and on `<g>`s alike; an element with a foreign `class`
   attribute never gets one.
3. **Precedence** is the CSS one, and the writer never makes it matter: an
   element has either the attribute or the class for a given property,
   never both; the properties it has neither for come from its ancestors.
4. **Localising** (XARA-T-0105): after resolving, the paint of an ink
   element is its complete resolved set; a `<g>`'s own hoisted attributes
   are *not* model attributes of the group — drop them once resolved (the
   model has them on the leaves, where `.xar` import put them).

Inside `<clipPath>` (the ClipView's clip shape, written into `<defs>`)
there are no slots: paint is written inline as before.

Size effect: ProbeX16 `document.svg` 56.7 → 50.8 MB (−10.4 %), corpus
103.1 → 96.5 MB (−6.4 %). Rendering: all 59 corpus files are
**pixel-identical in resvg** with and without the passes, identical in
Inkscape on 8 files checked; Chrome is identical on 7 and differs on
`20000GradFilledShapes50PCtransparent` by ≤ 11/255 on 0.09 % of pixels
(SSIM 0.99999: Chrome composites an inherited `fill-opacity` over a
gradient a hair differently than an explicit one).

### Conformance with bitmaps decoded (2026-09-23, round 5)

Same method (`xarast-cli render --frame page` vs `cargo xtask
svg-render` at the same width, 8 × 8 grey SSIM; script-driven, not in
CI): **mean 0.971, 34/59 ≥ 0.99** (was 0.942 / 28 before the reference
drew bitmaps). The bitmap files: Groucho2 0.995, WATCH 0.980, WATCH2
0.971, JagSS100 simple 0.941 → **0.969** and scope3 simple 0.900 →
**0.926** with the transparency masks (their shadows were black boxes in
resvg), leafgirl 0.875 (its JPEG8BPP figure: resvg draws the unsnapped
JPEG), Spitfire 0.850 (fine photo texture resampled differently),
`Fill Types simple` 0.844 (procedural rows still flat — XARA-T-0102).
Checked by eye: tag-68 PNGs with normalised alpha composite correctly
(scope3's logo), photos and bitmap fills sit where the CPU render puts
bitmap nodes, contone/duotone tiles show their colours. **Finding
(XARA-T-0171):** the CPU backend samples bitmap *fills and
transparencies* upside down relative to the documented mapping (Fill
Types' "Xara" tiles are mirrored vertically in Xarast, upright in resvg;
scope3/JagSS100 shadows land in the wrong place). The `.xarast` round
trip cannot see it (both sides share the renderer).

### Conformance (2026-09-23, by hand; XARA-T-0106 automates it)

All 59 corpus files convert, and every `document.svg`, `meta.xml` and
manifest passes `xmllint --noout`. Rendered at 100 % and compared
(8 × 8-window grey SSIM) with Xarast's own CPU render of the `.xar`:
resvg **mean 0.942**; 28 files ≥ 0.99, geometry/gradient files 0.98–1.0.
The low ones are low because **the reference is less complete than the
SVG**: Xarast does not draw text (Phase 9) or bitmaps (Phase 10) yet,
resvg does — AngledText 0.38, Spitfire 0.57, TextJust 0.79, leafgirl
0.87, GardenPlan 0.81 are all text or photos present only in the SVG.
Genuine approximations: `Fill Types simple` 0.85 (conical, 3/4-colour and
fractal rows drawn flat — XARA-T-0102). Inkscape 1.x and headless Chrome
against resvg on six files (WATCH2, Fill Types simple, leafgirl, Spitfire,
SimpleText, amurdove): Inkscape 0.945–0.997, Chrome (navigating to the
`.svg`) 0.945–0.996 — the three renderers agree; the 0.945 is leafgirl's
fine bitmap-filled figure, resampled differently by each.

## The SVG profile reader (W4) — how it works

`read_svg(bytes, &ReadOptions, &mut fetch) -> SvgRead { document,
diagnostics, stats, preservation }`; `open` wraps it with the container,
`meta.xml` (authoritative over the SVG's copies of title/dates) and a
`fetch` that reads `resources/…` from the package, digest-checked.

- **XML (`dom.rs`)**: quick-xml, `check_end_names`, no trimming,
  names validated as QNames, prefixes resolved by hand (as in the
  manifest), UTF-8 only, BOM stripped before the reader sees it, DOCTYPE
  and entities refused, depth/elements/bytes capped. The arena keeps each
  element's **byte span** and own `xmlns` declarations: an unknown element
  is stored as the exact text read, with the inherited declarations *it
  uses* inserted — never the default SVG namespace (every Xarast document
  has it; adding it changed the first read's text and the digest).
- **Numbers (`parse.rs`)** are decimals → exact millipoints (i128 mantissa
  and scale, half away from zero), never through a float: the writer's
  three decimals round-trip exactly. Path data is integer arithmetic for
  `M L H V C S Z`; `Q T A` become cubics (rounded). A malformed path keeps
  what precedes the error, as SVG renders it, with a warning.
- **Cascade (`style.rs`)**: inherited value < presentation attribute <
  class rule < `style`; `inherit` honoured; unknown `style` declarations
  are kept as a foreign `style` attribute. `xarast:fill-ref` /
  `stroke-ref` are pseudo-properties, inherited, also from
  `<xarast:paint-class>` twins (the pass 4–5 contract, XARA-T-0107). A
  palette twin only takes effect when the palette colour resolves to the
  element's actual colour: an editor that recolours without removing the
  stale ref gets its new colour. A `class` is consumed only when every
  class in it is one of the document's rules; otherwise it is foreign.
- **Nodes (`build*.rs`)**: one element → one node, created in document
  order; its id's tag claimed through `DocumentBuilder::tag` (a
  non-canonical, foreign or duplicate id → new tag + Info, F4.9). Known
  attributes per element kind; everything else, and every unknown child,
  comment and PI with its position among the known children, is baggage.
  Chapters come from `<xarast:document>`; a spread outside any chapter
  sits at the root. A document with no spread (a plain SVG) reads into
  one chapter/spread/page/layer sized by the viewBox. Transforms on
  groups compose into a CTM baked into geometry; an `<image>`'s or
  `<text>`'s own transform is its placement.
- **Twins win (F4.2)**: `xarast:parallelogram` over `d`; quick-shape
  parameters (the base `d` is kept as the outline cache when it differs
  from `QuickShape::outline()` — it does for 32k corpus shapes, whose
  import generated edge templates in shape space); conical, 3/4-colour,
  fractal/noise twins over their flat approximation; `xarast:stops` /
  `xarast:levels` keys over baked stops. Generated live-effect subtrees
  under their controller are kept as `LiveRole::Generated` (nothing
  regenerates before Phase 13); an orphan one becomes a plain group plus a
  warning (F4.3).
- **Paint (`build/paint.rs`)**, the inverse of the writer's `paint`,
  localised (XARA-T-0105): one attribute child per slot whose value
  differs from `default_for`. Rules for what SVG merges: `fill-opacity` =
  colour alpha × transparency alpha — a literal colour is taken as
  opaque (all of it becomes the transparency), a palette colour or a twin
  says its own alpha. `xarast:stroke-blend` is the stroke's mode and
  `xarast:blend` then the fill's; `<xarast:stroke-transparency>` and
  `xarast:stroke-mask` are the stroke's. Files written before round 4:
  `xarast:blend` goes to the fill transparency when there is a fill; two
  `<xarast:transparency>` twins are fill then stroke, one is the fill's
  when there is a fill. A key colour takes the palette colour its
  `*-refs` token names when that resolves to the written 8-bit value. A mask's
  box, padded by the writer with `width/2 + 1`, gives back the line width
  of an unstroked element. A circular radial gradient's major axis is
  the minor axis turned a quarter clockwise (the renderer takes both axes
  as given; `cx cy r` alone would shear it).
- **Palette**: rebuilt in order so `c-N` ids are stable. Components are
  read with `parse::f32_exact` (correctly rounded decimal → `f32`), so
  round-4 files come back bit for bit. Older files have six decimals,
  which do not always pin an `f32` component: where a colour resolves
  one level off the `xarast:srgb` the file records, the nearest `f32`
  that prints the same six decimals and resolves right is chosen
  (components, then a tint factor).
- **Preservation digest (F4.6)**: recomputed by running `write_svg` on
  the loaded document when it has baggage or the file declares any — the
  writer's own emission order, no second implementation. Count lower than
  declared → the §8.4 warning; same or higher but different → Info
  (another editor rewrote or added foreign data; Inkscape always does).
- **Security**: `<script>`, `<foreignObject>`, SMIL, `on*` and
  `javascript:` values are stripped with a warning, and so is any foreign
  fragment that contains them.

### The text reader (round 6)

`read/build/ink.rs::exact_line`: per line a `TextLine` (ruler from
`xarast:ruler`), then per run the **full state** (`full_state`: every
slot from the run's twins, text properties and `ink_paint`, defaults for
the rest) diffed against the running state, which starts at the
story-level state; one attribute node per differing slot, in slot order,
then the run's items. A line whose only run has no content pushes its
differences **before the line, at story level**, and updates the
story-level state. Without `xarast:exact` the legacy path reads (other
programs' `<text>`, pre-round-6 files), its size fallback now
`default_for` (16 pt).

### The normal form (XARA-T-0105)

`normal_form(doc)` is text, one fact per line: metadata; the palette
(model, kind, name, parent by position, 6-decimal components, resolved
sRGB, entry index); every non-attribute node in document order with its
id, kind and fields at the precision the profile carries (paths as the
writer's path data in the spread frame, text matrices to 6 decimals,
characters folded into styled runs); flags `LOCKED`/`MAGNETIC`; per ink
node the **resolved** attribute stack projected through the writer's own
paint code (`svg::paint::{colour_paint, transparency}`) with definitions
inlined by content and **derived data dropped** (baked stops of a keyed
ramp, the flat approximation of a twin), each drawn side's blend mode
and the stroke mask; baggage with clamped positions; Opaque nodes with
their children. **Its blind spot**: it projects through the writer, so
a value the writer never writes is invisible to it (the transparency
twins lost their levels and parameters with an equal normal form). Round
4 found those by diffing the *resolved attribute stacks* of every ink
node, import against reload (a scratch example, not committed). Two documents are equivalent iff their
normal forms are equal. It is independent of where attributes sit, of
hoisting and classes, and of def ids.

### Round trip numbers (2026-09-23, corpus of 59)

Round 6 (XARA-T-0172): model **59/59**, bytes **59/59**, render **59/59
pixel-identical** with the pinned test fonts; the 20 files of
`KNOWN_TEXT_GAPS` (all 14 `TextDesigns`, GardenPlan, Spitfire, TextCurve,
ProbeX16, ScaleTest, ScaleTest2) pass and the list and its "update the
list" assertion are deleted. Before the matrix twin, Rotated, AngledText,
TextCurve and hebrew still differed by 1–11 levels on ≤ 69 pixels: six
decimals of a rotation's `cos`/`sin` moved glyph outlines by a fraction
of a pixel. `tests/svg_text.rs` covers what the corpus does not: an
empty line, an item's own attributes, `\r`, U+0001, U+FFFE, soft breaks,
families SVG cannot spell, a mock placer. Fuzz (`fuzz_xarast_svg_read`,
seeds with a text story added to `fuzz_seeds`): 5 min, 379 577 runs,
15 995 edges, no finding. ProbeX16 save with the placer: 0.93–0.97 s
under load avg 18–50, best of 4 interleaved 0.95 s vs 0.92 s for the
previous build in the same conditions (+≈ 30 ms: system font enumeration
and 48 layouts); `save_to` without a placer adds nothing.

Text conformance, resvg vs the CPU reference (`render --frame page`,
8 × 8 grey SSIM, system fonts both sides), 20 text files, before → after:
mean **0.934 → 0.988**; every `TextDesigns` file 0.999–1.000 (were
0.911–0.997: hebrew 0.911, Rotated 0.930, embeddedFonts 0.933, TextJust
0.936), GardenPlan 0.795 → 0.982, TextCurve 0.651 → 0.969 (on-path text
is straight in both); Spitfire, Watch4, ScaleTest(2) unchanged (their
differences are not text). Inkscape 1.2.2 on Rotated / TextJust /
SuperSub: 0.944 / 0.942 / 0.975 → **0.998 / 0.999 / 1.000**. Checked by
eye: rotated columns, Hebrew (RTL, marks) and GardenPlan's labels sit on
the reference.

Round 5, with bitmaps decoded (the walker renders tag-68/-71 images,
T-0129): model **59/59**, bytes **59/59**, render **59/59
pixel-identical** — Groucho2, leafgirl and scope3 simple differed only by
the missing JPEG8BPP palette (scope3's two wood-tile JPEGs are tag 71;
T-0157 had no other cause). `KNOWN_RENDER_GAPS` and its "fail when a
listed file passes" mechanism are deleted. The duotone fill of `Fill
Types simple` (XARA-T-0155) already came back exact after the
exact-twins work (`xarast:contone-refs`). ProbeX16 save 0.79–0.82 s.

Round 4, after XARA-T-0108…0111; both tests have **no exception list**:

- **Model** (`tests/svg_roundtrip.rs`): normal form equal for **59/59**,
  passes 4–5 on and off alike, no warning on read. The resolved attribute
  stack of every ink node equals the import's for 59/59 but for two
  harmless spellings: `-0.0` vs `0.0` in a profile gain (Watch4) and the
  scale of an arrow spec that names no arrow (GardenPlan, Kerning:
  arrows are XARA-T-0103).
- **Bytes**: **59/59** byte-identical on the first re-save through
  `save_opened` (every entry, the container included).
- **Render** (`crates/xarast-app/tests/xarast_roundtrip.rs`, CPU,
  480×360, both framed on the original's drawing): **59/59
  pixel-identical** — the tolerance (2 levels on 0.1 % of pixels) is
  gone.
- **Browsers**: resvg renders every corpus `document.svg` pixel-identical
  before and after round 4 (59/59 at 800 px); `xmllint` passes on all
  118 `document.svg` + `meta.xml`. Size: corpus SVG 96.5 → 100.0 MB
  (+3.6 %), packages 14.95 → 15.29 MB.
- Before round 4: model 50/59 (the opaque subtrees), bytes 54/59, render
  50/59 identical + 6 within 2 levels.
- **Open time** (`open_reader`, release): ProbeX16 (518k nodes, 50 MB SVG)
  1.7–2.2 s; the 1,800-object files 3–15 ms (budget: 120 ms).

### Inkscape (1.2.2), by hand on CatWoman graphs and a foreign-data fixture

`inkscape --actions=…;export-type:svg;export-plain-svg:false` moved a
path, recoloured a quick shape, renamed a layer. Inkscape rewrote every
element (indentation, `d` absolute with commas, `#ff00ff`, a
`style="fill:url(#…)"` duplicate), added `sodipodi:docname` on the root
and `xmlns:svg`, dropped no `xarast:` attribute and kept `acme:state`,
`xarast:foreign-dirty` and a comment in place. Reread: 289/289 paths,
the move and the recolour taken (the stale `xarast:fill-ref` correctly
ignored), the layer renamed, `sodipodi:docname` kept as root baggage, no
warning; the digest reported "rewritten, nothing lost" (Inkscape's
spelling changes the fragment text). What does not survive yet: its
`namedview` view state and `<metadata>` additions (XARA-T-0112).

## Decisions taken (and why)

- **`zip` 8.6** (current stable major; 9.0 is pre-release), `MIT`, **every
  default feature off**, only `deflate-flate2`. No AES/bzip2/lzma/xz/ppmd/
  deflate64 in the reader's attack surface; `zopfli` is too slow for an
  interactive save. `quick-xml` **0.41** (already in the graph, `MIT`),
  `blake3` 1.8 (`CC0-1.0 OR Apache-2.0`). No `fs4`, no `tempfile` at
  runtime, no date crate: `std::fs::File::try_lock` (stable 1.89) gives
  `flock`, the temporary name is built by hand, and the two date conversions
  are the days-to-civil algorithm in `time.rs`.
- **DEFLATE backend: `zlib-rs`, the workspace's only one** (XARA-T-0090,
  round 3). flate2 selects one backend per *build*, and `zlib-rs` wins over
  `miniz_oxide` whenever any crate enables it, so the choice is made once,
  on the workspace `flate2` line (`default-features = false, features =
  ["zlib-rs", "runtime_detection"]`), never through `zip`'s
  `deflate-flate2-zlib-rs` feature. Round 2 had enabled it through `zip`
  alone, which switched the `.xar` reader only in workspace builds and
  broke a committed fuzz seed; now everything moved together and the seeds
  were regenerated. Two tests pin it: `deflate_backend_is_pinned` (raw
  DEFLATE at levels 6 and 1: a backend swap) and
  `deterministic_bytes_are_pinned` (a whole package: a layout change).
  zlib-rs output does not depend on the CPU: its SIMD paths
  (`compare256`, `slide_hash`) compute what the scalar ones do and the
  hash function depends only on the level — confirmed when turning on
  `runtime_detection` left both pins unchanged. Numbers in `perf.md`
  (20 MB save 273 → 89 ms; ProbeX16's 56 MB SVG deflates in ~335 ms vs
  ~965 ms; packages ~2 % larger).
- **Zstandard is not compiled in.** The `compact` profile is v1.0 scope;
  a zstd entry opens (listed, diagnosed `UnsupportedMethod`, raw-copyable on
  save) but cannot be decoded. `WriteOptions::profile = Compact` is refused.
- **The writer buffers, then writes in one pass.** The manifest is entry #2
  and describes every other entry, so sizes and digests must be known before
  anything after `mimetype` is written. Payloads are `Arc<[u8]>` (no copy of
  what the document already holds) or raw copies from the source package.
- **Every data entry gets a digest**, not only the mandatory
  document/meta/resources: it is what lets a reader flag corruption. Only
  `mimetype` and the manifest itself carry none.
- **EFS (UTF-8) flag:** set for every non-ASCII name (the `zip` crate does
  it); pure-ASCII names are written without it, exactly as the spec's own
  verified example (§12.2, flags `00 00`). The reader rejects a non-ASCII
  name without the flag, detected as "raw bytes ≠ the name `zip` decoded"
  (without EFS it decodes CP437).
- **`..` is a path segment, not a substring.** `a..b` is a legal name,
  `a/../b` is not. Also rejected: empty and `.` segments (a name with two
  spellings defeats duplicate detection) and a drive-letter prefix (`C:`).
- **The zip-bomb ratio has a floor** (`Limits::ratio_floor`, 1 MiB): entries
  up to 1 MiB uncompressed are exempt, because 64 KiB of whitespace
  legitimately deflates 650:1 and a bomb under 1 MiB is harmless.
- **The manifest cap is 16 MiB** (`Limits::max_manifest_size`). A
  66,000-entry manifest is ~17 MB, so packages that size need the caller to
  raise it (the ZIP64 test does).
- **End-record pre-check (`eocd.rs`).** `zip` sizes its first allocation from
  the declared entry count (bounded only by the file size) and silently
  collapses duplicate names into one map slot. So the reader parses the end
  record itself first: entry count against `Limits::max_entries`,
  `count × 46 ≤ central directory size`, single disk, no trailing garbage, no
  end-record signature inside the archive comment (a backwards-searching
  reader would try that forged record first). Afterwards
  `zip.len() != declared count` ⇒ duplicate names ⇒ refused.
- **Diagnostics vs errors.** Hostile structure (bad names, duplicates, bombs,
  encryption, no manifest, wrong major) is an error. Manifest/ZIP divergence,
  newer `min-reader`, unknown required capability, digest-less mandatory rows,
  out-of-order entries are `Diagnostic`s; `suggests_read_only()` implements
  §3.4 rule 9. Digests are verified lazily, per read (`entry`,
  `entry_stream`), and in full by `verify_all()`.
- **Manifest namespaces are resolved by hand**, not with `NsReader`: foreign
  fragments must be re-emitted namespace-complete and `NsReader` does not
  expose the in-scope bindings. Foreign fragments are the **verbatim input
  bytes** with the inherited declarations *they use* inserted in the start
  tag. Inserting every in-scope binding made the second save differ from the
  first; inserting only used prefixes reaches the fixed point on the first
  re-save (fuzz-asserted). Unknown `mf:` attributes (a newer minor version)
  are kept as foreign attributes in the `mf` namespace; unknown roles as
  `Role::Other(String)`; unknown `mf:method` values are dropped (informative,
  recomputed on write).
- **Resource refcounts start from `mf:refcount` (or 1) on open**, so a save
  before the SVG layer recounts can never collect a resource. The SVG layer
  must recount (`begin_recount` + `count_path`) before `gc`.
- **`carry_from` carries** `history/`, `extensions/`, other `META-INF/*`,
  unknown top-level entries, and `resources/` entries that do not follow the
  hash naming — as raw copies with their rows, warts included (a digest-less
  row stays digest-less).
- **Atomic save** follows symlinks (replaces the target, keeps the link),
  copies the target's permissions to the temporary, and treats a directory
  that refuses `fsync` as non-fatal (after the rename it is too late to
  roll back anyway).
- **Lock staleness** needs all of: same host, same boot id, pid not alive
  (`/proc/<pid>`, Linux only — elsewhere every pid is "alive", the safe
  direction), and the `flock` free. The loser of a `force` never deletes the
  winner's lock file: release compares inode/device, not text (two locks of
  one process in the same second have identical text).

## Invariants that must not be broken

- `mimetype` first, STORED, no extra field, content at offset 38
  (`tests/container.rs::magic_bytes_at_fixed_offsets`).
- One manifest row per ZIP entry (directory entries excepted), plus exactly
  one `/` row whose media type is the `mimetype` content.
- Names are validated and **rejected**, never sanitised.
- The reader never allocates from a declared size beyond 1 MiB up front;
  reads are capped at `declared + 1` and a mismatch is `ZipBomb`.
- `open` never reads `document.svg`
  (`tests/container.rs::open_does_not_parse_the_document`).
- Deterministic save: DOS 1980-01-01, `0o644`, system `Unix`, canonical
  order and manifest, one DEFLATE backend, `zlib-rs` (the two golden
  digest tests). The SVG writer is deterministic too: slots, classes and
  hoisting depend only on the document, never on hash order. An
  unchanged re-save through `from_package` + `carry_from` + raw copies is
  **byte-identical** (`resave_from_the_package_is_a_fixed_point`).
- Preserved entries are raw-copied (same compressed bytes, same method,
  same CRC) — never recompressed.
- No `unsafe` (`#![forbid(unsafe_code)]`); parser modules deny indexing,
  unwrap/expect, panic and unchecked arithmetic.
- **Read then write is a fixed point** on what the writer produced: the
  first re-save of a reloaded document equals the original save (59/59
  corpus files, `tests/svg_roundtrip.rs`; fuzzed synthetic input may
  settle one round later, `fuzz_xarast_svg_read`). A reader or writer
  change that breaks it is a bug, however small the difference.
- **Derived data comes from written values.** Baked stops and flat
  approximations are computed from the keys as a reader will rebuild
  them (`paint::key_colour`), never from the model's `f32` colours:
  otherwise the reload re-derives one level off.
- **The reload renders pixel-identical** (59/59, CPU): a twin that drops
  anything the renderer reads is a bug even when the normal form stays
  equal.
- **Opening writes nothing** (F4.10, `tests/svg_read.rs::opening_writes_nothing`):
  no lock, no temporary, the file's bytes and mtime untouched.
- A foreign fragment is stored as the text read (+ the declarations it
  uses) and goes back **inside the element it was read in**, before the
  known child it preceded. The writer re-emits it only if well-formed.
- Ids: every element with a Xarast id claims its tag once; the root is
  `x0` and is never claimed; a node the builder numbered itself gives its
  tag up (`DocumentBuilder::tag`).

### Fuzzing

Three targets, `fuzz_xarast_open` (reader + every read path + a full re-save
of whatever opens, which must reopen), `fuzz_xarast_manifest` (parser +
the write–parse fixed point) and `fuzz_xarast_svg_read` (W4, below).
Seeds are generated by
`cargo run -p xarast-format --example fuzz_seeds -- <dir>`, never committed
(`crates/xarast-xar/tests/fuzz_seeds.rs` rejects any file under
`fuzz/corpus/` it does not generate itself); CI generates them before the
run. Round 1, 2026-09-23, `Limits::FUZZ`, 1 MiB max input:

| Target | Runs | Executions | Coverage | Findings |
|---|---|---|---|---|
| `fuzz_xarast_open` | 3 × 10 min (last on the final build) | 5.17 M + 2.46 M + 2.16 M | 5 292 edges | none |
| `fuzz_xarast_manifest` | 4 × 10 min (the first three stopped at a finding) | final clean run 6.69 M | 2 970 edges | 4, all fixed (below) |
| `fuzz_xarast_svg_read` (W4) | 90 s + 10 min, `ReadOptions::fuzz()`, 3 seeds (writer output with and without passes 4–5, a hand-written third-party SVG) | 0.47 M + 0.84 M (~1 400/s: each input is read, saved, opened, saved) | 15 713 edges, 4 418 inputs, 666 MB peak | none |

`fuzz_xarast_svg_read` asserts: no panic on any text; what reads saves as
a package that opens, and the second save's `document.svg` equals the
first (or settles one round later), with an equal normal form on a third
read.

### Fuzz findings (round 1)

All four were in the manifest parser and all were "a save changes what
the file means", not crashes; each input is now a unit test in
`manifest.rs`.

1. `quick-xml` does not validate names: `<mf:file-entry0mf:full-path="x"`
   is read as one element name containing `="`. Every element and
   attribute name is now checked as a QName (and `:x` is refused).
2. `quick-xml` skips a leading BOM **without counting it** in
   `buffer_position`, so fragment offsets were 3 bytes short. The BOM is
   stripped before the reader sees the input; a second BOM is refused.
3. Fragments nested in the `/` row were re-emitted at manifest level, where
   a nested `mf:file-entry` became a real row; the same for capabilities in
   `mf:requires`. **Rule: a foreign fragment always goes back inside the
   element it was read in.**

## Dead ends (do not retry)

- Text runs split on snapshot inequality alone: a restore attribute or a
  slot the profile does not write splits a run in the original and not in
  the reload, so the first re-save differs. Split on what is *written*.
- An item-less line's run taken inside the line's scope: `StoryText`
  resolves such a line from the state after its scope closes, and a
  reader that pushes the run inside the line loses it. The run is the
  outer state and the reader puts it at story level.
- `text-anchor` with per-character `x` lists: every character is its own
  text chunk, so a centred anchor centres each glyph on its x.
- Six decimals for a rotated story's matrix (1-level pixel differences).

- Headless Chrome rendering the SVG through an `<img>`: SVG-as-image runs
  in secure static mode and loads **no** external resources, so every
  bitmap vanishes. Navigate to the `.svg` itself.
- Comparing against `xarast-cli render --width N`: that fits the frame
  *with a margin*. Render the reference at 100 % (`--frame page`) and
  resvg at the reference's width.
- Exponent-form numbers (`1e3`): only ever shorten round thousands of
  points, and `1e3mm` in `width` is a parser trap. Not written.

- JSON manifest (§3.5). `data:` URIs above 4 KiB. Recompressing already
  compressed resources. `roxmltree` as the main parser. Zstd method id 20.
- `NsReader` for the manifest (cannot enumerate in-scope bindings).
- Completing foreign fragments with *every* in-scope binding (not a fixed
  point on re-save).
- Selecting a DEFLATE backend through `zip`'s `deflate-flate2-zlib-rs`
  feature (changes every crate's backend; see Decisions).
- `default-features = false` on flate2 **without** `runtime_detection`:
  zlib-rs silently runs scalar code wherever no other crate brings
  flate2's defaults back (`png` does in the app, nothing does in
  `xarast-xar`'s own bench or the fuzz targets): `.xar` inflate ~50 %
  slower.
- CSS custom properties (`--xarast-fill-ref:…`) in the paint classes:
  resvg's CSS parser stops at the first one and drops the rest of the
  rule (3 corpus files rendered differently). Twins go into
  `<xarast:paint-class>` instead.
- The old path-data separator check scanned back through the output over
  `-`: after `1.5-5`, a `.5` lost its space and read as `-5.5`. It must be
  the previous number's own dot (`pathdata.rs` tracks it; 112 corrupted
  numbers in ProbeX16, 11 corpus files render closer to the reference
  since).
- Comparing lock-file text to decide ownership on release.
- Six decimals for palette components (and for key positions): they do
  not pin an `f32`, so a reload moved derived stops by a level. Shortest
  round-trip `f32` (`num::f32s`) instead.
- Trusting an equal normal form as "nothing lost": it cannot see what
  the writer does not write (see "The normal form"). Diff resolved
  attribute stacks, and render.
- Moving an opaque node's subtree out of `<xarast:opaque>` so browsers
  draw it: the renderer skips opaque subtrees, so the browser would show
  what Xarast does not.
- Reading the SVG through a float path parser (`Path::from_svg_path_data`
  via kurbo): relative commands accumulate in `f64`; the reader's own
  integer parser is exact and needs no rounding.
- Regenerating quick-shape outlines on every read ("parametric wins"
  literally): 32k corpus shapes have outlines `QuickShape::outline()`
  only approximates (edge templates); the stored `d` is kept as the cache.
- Normal form = "the writer's SVG with def ids stripped": it cannot see
  what the writer drops (the Opaque subtrees were only found once the
  normal form listed the model's own tree) and breaks on every hoisting
  change. The tree is model-level; only paint goes through the writer.
- Rendering both documents with `HeadlessFrame::FitDrawing`: the drawing
  bounds include line widths of unstroked objects, which the profile does
  not carry, so the two fits differ by a few pixels. Frame both on the
  original's drawing.
- A separate ad-hoc digest walker on the reader side: the writer's
  emission order (clip shapes before their ClipView, fragments after
  sidecars) is subtle; running `write_svg` is exact and costs only for
  documents with baggage.

- Carrying a JPEG8BPP palette inline (`xarast:palette="#rrggbb …"`): 2 KB
  of text per reference, repeated on every element naming the image; the
  blob is content-addressed once.
- Recognising the writer's `<filter>`s by id prefix or by shape: foreign
  filters would be dropped. The `xarast:filter` marker is the contract.

## Open TODOs

- W4 leftovers: F4.7 deletion accounting (XARA-T-0113); header data
  another editor adds — `namedview`, `<metadata>`, run-level attributes,
  foreign ids (XARA-T-0112). Round-trip gaps not in the corpus: a
  custom arrow outline is written as the name `custom` (its path is
  lost) and an arrow spec's width/height is not written (XARA-T-0103);
  per-key transparency modes are taken from the side's mode; a tinted
  palette reference on a key (`Colour::Indexed { tint: Some }`) is
  written as its 8-bit value.
- Bitmap names (`BitmapResource::name`, the gallery name) are not
  written; a reload has empty names (not part of the normal form). The
  SVG mask/pattern of a bitmap ignores `Simple` (clamp) and mirrored
  tiling — patterns always repeat — and a transparency's contone levels.
- W3 leftovers: baking/`BakeProvider`
  (XARA-T-0102), arrow markers (XARA-T-0103), PNG rendition of BMPs
  (XARA-T-0104), the conformance harness in CI (XARA-T-0106), `README.txt`
  entry (§5.9, SHOULD), split layout above 32 spreads / 8 MiB.
- `meta.xml` model (F2.6) and the full `<metadata>` mirror (F2.7):
  `save::meta_xml` writes only title, dates, generator, origin,
  statistics, page setup and comment.
- Text leftovers (XARA-T-0172): the builder's Info diagnostic "an
  attribute follows an ink node" fires on every multi-run line (text items
  count as ink in `validate`); run-level foreign attributes and elements
  are dropped (runs are not nodes); text on a path is still a straight
  line in the base SVG (`<textPath>`, W9.5); a gradient or bitmap fill on
  text is written in the spread frame, so browsers misplace it under the
  story's transform (the twin is exact); fonts are not embedded
  (`@font-face`/WOFF2, §6.7 rule 2) and no generic family is guessed from
  a name without PANOSE.
- ~~Doc model has no per-node foreign-baggage container~~ — done,
  XARA-T-0089 (`docs/memory/document-model.md` decision 32).
- F6.4 lock UX + `SIGINT`/`SIGTERM` cleanup (app), F6.5 autosave, F6.6
  journal, F6.7 recovery scan, F6.8 `xarast repair`, F6.9 partial XML
  recovery.
- F5.5 master/derived (`set_derivation` and the manifest fields exist; the
  `--no-derived` regeneration path does not), F5.7 geometry dedup, F5.9
  previews from a provider, F5.10 `data:` policy — all need W3.
- `compact` profile (zstd), `history/` (v1.0).
- Raw copies take the host system from the running OS (`zip` does not let
  `raw_copy_file_touch` set it): byte-identical re-saves hold per platform,
  not across Linux ↔ Windows.
