# xarast-format

Memory note for the native **`.xarast`** format (`crates/xarast-format`).
Normative spec: `docs/research/06-xarast-format.md`. Plan:
`docs/phases/phase-06-xarast-format.md`. Epic XARA-EP-0007.

## Current state

Phase 6, round 1 (2026-09-23): the byte layer. Round 2 (2026-09-23): the
SVG profile writer (W3), the document-level save and `xarast-cli convert`.
Round 3 (2026-09-23): passes 4–5, `zlib-rs` everywhere, the path-data
separator fix; the reader (W4) is being written alongside.

| Workstream | State | Where |
|---|---|---|
| W1 container | F1.1–F1.8 done | `name.rs`, `sniff.rs`, `eocd.rs`, `reader.rs`, `writer.rs`, `limits.rs` |
| W2 manifest | F2.1–F2.5, F2.8 (diagnostics only) done; **F2.6/F2.7 `meta.xml` model open** (a minimal `meta.xml` writer exists: `save::meta_xml`) | `manifest.rs`, `digest.rs`, `reader.rs::consistency` |
| W3 SVG write | F3.1–F3.8, F3.11 done; F3.9 all eight passes done (4–5: XARA-T-0101, round 3); F3.10 baking open (XARA-T-0102) | `svg/` (`num`, `pathdata`, `frame`, `xml`, `defs`, `paint`, `style`, `emit`), `save.rs` |
| W4 SVG read + preservation | not started (the container half of F4.8 is done: unknown entries are raw-copied with their rows) | `writer.rs::carry_from` |
| W5 resources | F5.1–F5.4, F5.6 done; F5.8 contract + validation done (no provider implementation) | `resource.rs`, `policy.rs`, `thumbnail.rs` |
| W6 durability | F6.1 (`write_atomic`, `.bak` = F6.2) and F6.3 (`DocumentLock`) done; F6.4–F6.9 open | `durability/` |

Public entry points: `XarastReader::{open, open_with}`, `PackageWriter`,
`ResourceIndex`, `Manifest`, `sniff`/`sniff_bytes`, `write_atomic[_with]`,
`DocumentLock`, **`svg::write_svg`**, **`save`/`save_to`** (the spec's
`save_atomic` for a first save: SVG + resources + `meta.xml` + container,
atomically) and `meta_xml`. `xarast-cli convert <in.xar|DIR>… (-o F |
--out-dir D)` drives it; `cargo xtask svg-render <svg> <png> [width]` is
the resvg check.

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
- **Live effects**: controller `<g xarast:kind="blend|…">` with the
  parametric element first; generated children in `<g
  xarast:generated=… xarast:generated-by=… xarast:base-authoritative="true">`
  — nothing regenerates them before Phase 13, so a reader must keep them.
- **Text** (until Phase 9): `<text transform>` with the story matrix
  conjugated by the flip (`Frame::local_matrix`), one `<tspan>` per line
  (line advance = ratio × 1.2 × largest size, or the absolute spacing),
  runs split where font, size, weight, style, underline or fill change.
  Kerns are dropped; stories on a path are laid out as lines. Fill only,
  no stroke.

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

### Fuzzing

Two targets, `fuzz_xarast_open` (reader + every read path + a full re-save
of whatever opens, which must reopen) and `fuzz_xarast_manifest` (parser +
the write–parse fixed point). Seeds are generated by
`cargo run -p xarast-format --example fuzz_seeds -- <dir>`, never committed
(`crates/xarast-xar/tests/fuzz_seeds.rs` rejects any file under
`fuzz/corpus/` it does not generate itself); CI generates them before the
run. Round 1, 2026-09-23, `Limits::FUZZ`, 1 MiB max input:

| Target | Runs | Executions | Coverage | Findings |
|---|---|---|---|---|
| `fuzz_xarast_open` | 3 × 10 min (last on the final build) | 5.17 M + 2.46 M + 2.16 M | 5 292 edges | none |
| `fuzz_xarast_manifest` | 4 × 10 min (the first three stopped at a finding) | final clean run 6.69 M | 2 970 edges | 4, all fixed (below) |

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

## Open TODOs

- W4: the reader, preservation context and the re-save path
  (`from_package` + `carry_from` + raw copies) on top of `save`;
  attribute localisation (XARA-T-0105); the preservation digest as above.
- W3 leftovers: baking/`BakeProvider`
  (XARA-T-0102), arrow markers (XARA-T-0103), PNG rendition of BMPs
  (XARA-T-0104), the conformance harness in CI (XARA-T-0106), `README.txt`
  entry (§5.9, SHOULD), split layout above 32 spreads / 8 MiB.
- `meta.xml` model (F2.6) and the full `<metadata>` mirror (F2.7):
  `save::meta_xml` writes only title, dates, generator, origin,
  statistics, page setup and comment.
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
