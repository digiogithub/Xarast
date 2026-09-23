# export

Memory note for **export** (`xarast-io`, the export entry of
`xarast-render`, and `xarast-cli export`). Phase 11:
`docs/phases/phase-11-export-filters.md`.

## Current state

Round 1 (2026-09-23, XARA-US-0056 + XARA-US-0057): the export model, the
filter registry and deterministic **raster** export to PNG, JPEG and WebP.
Round 2 (2026-09-23, XARA-US-0059): **vector PDF 1.7** export, one page per
export, with the fidelity ladder (section "PDF" below). SVG export (W11.3,
XARA-US-0058, still backlog), batch export (T11.1.6), export hints in the
document (T11.1.5), palette quantisation (T11.2.8), AVIF (T11.2.7) and the
dialog (T11.1.8, XARA-T-0192) are not built yet.

| Piece | Where |
|---|---|
| `ExportRequest`, `ExportArea`, `ExportSizing` (+ `SizingEdit`), `Background` | `crates/xarast-io/src/model.rs` |
| `FormatId`, `FormatOptions` and the per-format option structs (serde) | `crates/xarast-io/src/options.rs` |
| `Exporter`, `Capabilities`, `Registry`, `XAR_EXPORT_REFUSAL` | `crates/xarast-io/src/registry.rs` |
| `ExportSource` (the app seam), `SceneSource`, `Progress`, `CancelFlag` | `crates/xarast-io/src/source.rs` |
| `ExportReport`, `Compromise`, `ExportError` | `crates/xarast-io/src/report.rs` |
| The three raster exporters, atomic writes | `crates/xarast-io/src/raster.rs` |
| PNG writer (ours), parallel zlib, JPEG and WebP wrappers | `png.rs`, `deflate.rs`, `jpeg.rs`, `webp.rs` |
| The export rasteriser (strips on the CPU backend) | `crates/xarast-render/src/export.rs`, `CpuBackend::render_rows` |
| `xarast-cli export` + `SessionSource` (temporary home) | `crates/xarast-cli/src/export.rs` |
| `PdfExporter`, `plan_page`, the command translator and the ladder | `crates/xarast-io/src/pdf/mod.rs` |
| `PdfWriter` façade (the only code naming `pdf-writer`) | `crates/xarast-io/src/pdf/writer.rs` |
| Gradients → shadings, graduated transparency → soft-mask content | `crates/xarast-io/src/pdf/shading.rs` |
| Ladder step 3: object / backdrop rasters | `crates/xarast-io/src/pdf/rasterise.rs`, `DisplayList::with_commands`, `export::ListRasteriser` |
| `PdfOptions`, `PdfVersion`, `BlendFidelity` | `crates/xarast-io/src/options.rs` |

The whole 59-file corpus exports to all three formats with **zero errors**,
at 96 and at 300 dpi, and two runs are byte-identical for every file and
format (numbers in `perf.md`, "Export"). The only compromises the corpus
reports are 25 font substitutions and one text-on-path drawn straight
(plus `AlphaFlattened` for every JPEG over a transparent background).

## PDF (W11.4, XARA-US-0059)

### The T11.4.1 spike: `pdf-writer`, not `krilla`

The phase recommended `krilla` and named `pdf-writer` as the fallback. The
spike read `krilla` 0.8.2's scene model against our feature list and chose
the fallback:

| Criterion (phase order) | `krilla` 0.8.2 | `pdf-writer` 0.15 |
|---|---|---|
| Pure Rust | yes | yes |
| Licence | MIT OR Apache-2.0 | MIT OR Apache-2.0; deps `bitflags`, `itoa`, `memchr`, `ryu` |
| A baked ramp as a sampled function (type 0) | **no**: gradients are stop lists, written as stitched exponentials or PostScript (type 4) functions | yes |
| Mesh shadings (types 4–7) | **no** | yes |
| Soft masks, groups, blend modes | yes | yes |
| Shares our types | **no**: `tiny-skia-path` geometry in `f32`, not `kurbo`; pins `skrifa` 0.42 (we have 0.44) | n/a (we pass numbers) |
| MSRV | **1.92** (workspace declares 1.90) | fine |
| Font subsetting | built in (`subsetter`) | not included — T11.4.7 adds `subsetter` directly |

So the phase's premise ("same `kurbo`/`skrifa` family, no conversion
layer") does not hold for 0.8, and `krilla` cannot carry the two things
the ladder most needs (the sampled ramp and the meshes). The façade in
`pdf/writer.rs` keeps the choice replaceable.

### The fidelity matrix (what each feature becomes)

| Feature | PDF | Rung |
|---|---|---|
| Path fill, non-zero / even-odd | `f` / `f*` | native |
| Fill rule Positive / Negative | — | **rasterise** (object alone) |
| Clip, non-zero / even-odd | `W n` / `W* n` inside `q`/`Q` | native |
| Clip, Positive / Negative | drawn non-zero | approximated (reported) |
| Stroke: width, join, mitre, one cap, dashes + offset | `w J j M d`, drawn in document units under the object's transform (`cm`), coordinates relative to the path's corner | native |
| Hairline (width 0) | `0 w` | native (device-dependent by definition) |
| Different start and end caps | outline via `kurbo::stroke`, filled | workaround, exact |
| Gradient- or bitmap-painted stroke | outline, then as a fill | workaround |
| Flat colour, colour alpha, flat Mix transparency | `rg` + `ca`/`CA` | native |
| Linear, radial (elliptical, skewed) | axial / radial shading in the gradient's frame (`cm` to A/B/C) | native |
| Diamond | `\|u\|` axial everywhere + `\|v\|` axial clipped to the bow tie where `\|v\| > \|u\|` | native, exact |
| Repeat / mirror / repeat-HQ | domain widened to whole periods, parameter folded while sampling (≤ 4096 periods, else rasterise) | native |
| Ramp profile and effect space | the renderer's 256/2048 table as a sampled function | native, exact up to sampling |
| Conical | 256-wedge Gouraud fan (type 4) | **approximated** (reported) |
| Four-colour mesh | type 1 shading of a 2 × 2 sampled function; the function clamps its input as the renderer clamps (u, v) | native, exact |
| Three-colour mesh | type 1 shading, 33 × 33 grid of the renderer's formula | **approximated** (reported) |
| Perspective gradient mapping | — | **rasterise** |
| Ramp or mesh colours with alpha | — | rasterise (XARA-T-0227) |
| Graduated transparency (Mix) | luminosity soft mask painting `255 − t` with the same shading code | native |
| Bitmap transparency | — | rasterise (XARA-T-0227) |
| Families other than Mix | `Exact` (default): **rasterise with backdrop**. `PreferNative`: Stained Glass → Multiply, Bleach → Screen (reported as `BlendModeApproximated`), the rest rasterised | see below |
| Layers (`PushLayer`) with flat Mix | transparency-group form XObject, `/I` unless `Plain`, `ca` on the `Do` | native |
| Layers with other transparency | rasterise the whole layer with backdrop | rasterise |
| Bitmap fills, placed images | — | rasterise (XARA-T-0229) |
| Text | the walker's glyph outlines, as paths | native paths; embedded fonts are XARA-T-0228 |

**Blend families.** Not measured yet (T11.4.6, XARA-T-0227): so `Exact`
is the default and every non-Mix family is rasterised with its backdrop.
`PreferNative` maps only the two per-channel families whose shape is
Multiply/Screen; Darken/Lighten/Hue/Saturation/Luminosity are *not*
mapped to PDF's same-named modes (different formulas, the phase's risk
row), nor are Contrast, Brightness, Bevel.

### How it works

- **Page.** `plan_page`: the (bled) area, sized to `ExportSizing`'s
  physical size, or the area's own size (a pixel-count overflow falls back
  to the area — pixels mean nothing to a vector page). ≤ 14 400 pt a side
  (PDF 1.7 Annex C). `to_page` maps millipoints to points, y up; TrimBox
  and BleedBox are written when there is a bleed. The page is a DeviceRGB
  transparency group.
- **Translation.** `DisplayList::build` with the page as the "device"
  (points, y up, viewport = the page) — the same resolution of groups,
  transforms and paint mappings the rasterisers use; every `DrawItem` walks
  the ladder. Paint mappings arrive in page space, so a gradient's frame is
  `[C−A, B−A, A]`.
- **Rasterising** (`pdf/rasterise.rs`). A second display list is built for
  the image's pixel grid over the object's bounds and commands are picked
  by **scene op index** (`DisplayList::op_of`), so the pixels are the raster
  exporter's. *Object* mode draws the command alone, twice — over opaque
  black and opaque white — and unmixes coverage and colour. That worked
  around XARA-T-0231, which is fixed (2026-09-23): a single render over
  transparency and `unpremultiply_rgba_in_place` would now give the same
  pixels at half the cost; the switch is left for a PDF task. *Backdrop* mode draws every command up to it, closing open
  clips and layers (`PopLayer { layer: u32::MAX }` composites opaquely;
  the group's own opacity is applied by the PDF group around the image),
  over the **paper** when the page is transparent: a blend reads a colour,
  and that is what the editor showed under it. Such a rectangle is opaque
  in the file (the report says "over the paper").
- **Determinism.** Object numbers in call order; `BTreeMap`/`BTreeSet`
  for interning and resource dictionaries; zlib level 6 through the
  workspace's `flate2`; no `CreationDate`, no `/ID`. Functions and
  shadings are interned by value, which took the corpus from 214 MB to
  121 MB (`20000GradFilledShapes` 20 MB → 3.2 MB).

### Validation (2026-09-23)

- `crates/xarast-io/tests/pdf.rs`: all 120 synthetic cases export twice
  byte-identically, compressed and not, and pass a no-tool structural check
  (xref offsets, `startxref`, catalog); MediaBox = area to 1/1000 pt; the
  criterion-10 sheet gives exactly three `Rasterised` entries naming nodes
  100/101/102; bleed boxes; cancellation leaves no file.
- Render comparison against our own PNG export (72 dpi, paper) through
  Poppler's `pdftoppm` when it is on the `PATH`: every vector case has mean
  |Δ| < 4/255 and < 3 % of pixels off by more than 48. Worst: conical
  3.6/255 (Poppler does not antialias clipped mesh shadings), diamond 2.2.
  Rasterised cases are excluded (Poppler resamples placed images; the
  pixels are ours by construction) and the hairline case too.
- The 59-file corpus (`--background paper`): 59/59 exported, 121 MB,
  6.5 s render in release; Poppler and Ghostscript read all 59 with no
  message. `qpdf` is not installed on this machine (XARA-T-0230).

## Decisions taken (and why)

- **`.xar` export is permanently out of scope** (architecture §3.5; the
  phase-11 banner). Enforced three ways: `Registry` has no `.xar` entry and
  `for_extension("xar")` is `None` (test
  `registry::tests::xar_is_never_an_export_format`); `FormatId` has no `Xar`
  variant; `xarast-cli export --format xar …` and `-o out.xar` exit non-zero
  printing `XAR_EXPORT_REFUSAL`, which names §3.5 (CLI test
  `export_refuses_xar_with_the_architecture_reason`). Re-adding it means
  first reversing the decision in `docs/10-architecture.md`.
- **The export model lives in `xarast-io`, not `xarast-app`.** The phase
  document put `ExportRequest` and friends in the app, but `xarast-app`
  depends on `xarast-io`, so the `Exporter` trait could not name them from
  there. The types need only `xarast-doc` (`NodeId`), `xarast-geom` and
  `xarast-render`, so they sit with the trait. What does need the app —
  turning a selection or a page into a rectangle and walking the document
  into a scene — is the `ExportSource` trait, which the app implements
  (XARA-T-0192). Until then `xarast-cli` carries `SessionSource`.
- **Export geometry is exact.** `export_transform` maps the area's top-left
  to pixel (0, 0) and its bottom-right to (w, h), independently in x and y
  (non-uniform when the aspect lock is off). The headless `render` path
  frames with a margin and centres, so the two differ by sub-pixel shifts
  (mean |Δ| 0.17/255 on `10000GradFilledShapes`); that is expected, not a
  bug. A vertically flipped comparison gives 42/255, which is how the y
  flip was checked.
- **Sizing.** `pixels = physical × dpi` in `f64` on millipoints, pixel
  counts rounded half away from zero (`f64::round`), never below 1. One
  axis is pinned; an edit of a non-pinned axis moves the third; an edit of
  the pinned axis moves its default follower (dpi → pixels, pixels → dpi,
  physical → pixels). A physical size of zero means "the area's own size".
  100 mm at 300 dpi = 1181 px; the 20-triple table is in `model.rs`.
- **Background.** `Transparent` keeps alpha where the options do
  (`FormatOptions::keeps_alpha`: PNG RGBA/grey-alpha/palette, WebP);
  otherwise the surface is cleared to the paper colour (a translucent
  explicit colour is composited over the paper first) and the report gets
  `Compromise::AlphaFlattened`. The export renders *onto* that colour
  rather than flattening afterwards, because blend modes over white and
  over transparency differ, and the former is what the user sees.
- **Straight alpha in image files (XARA-T-0231).** Render surfaces are
  premultiplied; PNG and WebP store straight colour. `raster.rs` converts
  every strip with `xarast_render::unpremultiply_rgba_in_place` before
  encoding (a fully transparent pixel becomes `[0, 0, 0, 0]`), and
  `render_export_strips` premultiplies `ExportJob::background`, which is
  straight. Before, premultiplied bytes were written as straight and every
  partly transparent pixel came out darkened by its own alpha, on top of
  the compositor's own darkening. Guarded by
  `tests/raster.rs::a_half_transparent_square_exports_straight_not_darkened`.
- **One rasteriser, CPU only.** `render_export_strips` constructs
  `CpuBackend::new(CpuConfig::deterministic())` itself; it takes no backend
  argument, so the GPU cannot leak in.
- **Band and strip geometry are functions of the output size only.**
  `export_band_lines(w, h)` aims for 64 bands (≥ 16 rows, ≤ 1 MiB), and
  strips (`export_strip_lines`, 64 MiB budget) are whole numbers of bands
  starting on the band grid, so strips change memory and never pixels.
  This is what fixed the parallelism half of XARA-T-0038: a 766 px image is
  48 bands, not 3.
- **PNG is our own writer**, not the `png` crate: `png` 0.18 cannot write
  Adam7 data (it only sets the IHDR flag), compresses with `fdeflate` instead
  of the workspace's one DEFLATE backend, and cannot stream. Ours writes
  IHDR, `sRGB` (perceptual), `pHYs`, `PLTE`/`tRNS`, IDAT in 256 KiB chunks
  and IEND; filters are chosen per row by minimum sum of absolute signed
  bytes. Non-interlaced, non-palette PNGs stream strip by strip.
- **Parallel DEFLATE, deterministic.** `deflate::ChunkedZlib` cuts the
  filtered stream at fixed 1 MiB offsets of *uncompressed* data, compresses
  each piece independently (sync flush; the last piece carries the final
  block) and combines the Adler-32s. Cut points never depend on strips,
  batches or threads. Cost: the lost dictionary at each cut (< 1 %). Level 6
  was ~4× the rasterising time before this.
- **JPEG: `jpeg-encoder`** (quality, progressive, 4:4:4/4:2:2/4:2:0, JFIF
  density, APP segments), `simd` feature **off** (it would pick AVX2 at run
  time). Every file carries an APP1 EXIF block with `ColorSpace = 1` (sRGB),
  44 bytes, built in `jpeg::exif_srgb`. **Standard Annex K Huffman tables**,
  not optimised ones: see dead ends.
- **WebP: lossless only**, through `image-webp` (pure Rust, already in the
  graph). Opaque images are written as RGB. No `ICCP` chunk: WebP without
  one is sRGB. `WebPMode::Lossy` returns `FeatureNotBuilt`.
- **`oxipng` is an optional feature** (`xarast-io/oxipng`, forwarded as
  `xarast-cli/oxipng`), off by default. It builds libdeflate from C, which
  the default AppImage does not want. `parallel` is off so its trial order,
  and therefore its output, is fixed; `StripChunks::None` keeps our `sRGB`
  and `pHYs`. It runs on its own thread; cancelling **abandons** it (the
  call returns within 20 ms, the worker finishes and drops its result),
  because `oxipng`'s own `timeout` would make the bytes depend on the clock.
- **Atomic writes.** Every exporter writes `.<name>.<pid>.part` next to the
  destination and renames it into place; a failure or cancellation removes
  the temporary. Test: `a_cancelled_export_leaves_nothing_behind`.
- **Defaults and why.** PNG: RGBA, 8-bit, not interlaced, `pHYs` written,
  zlib level 6, no `oxipng` (lossless, alpha, the common case). JPEG: q90,
  baseline, 4:2:0, density written (what every viewer expects; q90 is the
  usual "high"). WebP: lossless (the only mode built). CLI: 96 dpi (one
  pixel per screen pixel at 100 %), transparent background, the drawing.
- **Licences.** `jpeg-encoder` is `(MIT OR Apache-2.0) AND IJG`; IJG is
  allowed for that crate only in `deny.toml`, and its acknowledgement
  sentence is in `docs/11-licensing-and-clean-room.md` §4.1 (shipping it in
  the About box and AppImage notices: XARA-T-0194). `image-webp` is
  `MIT OR Apache-2.0`; `oxipng` is MIT, `libdeflater`/`libdeflate-sys` are
  Apache-2.0 (libdeflate upstream MIT). `oxipng` adds a second `rustc-hash`
  to the graph (a `cargo deny` duplicate warning, not an error).

## Invariants that must not be broken

- **Determinism contract**: same `ExportRequest` + same document → same
  bytes, across runs, processes, thread counts and strip budgets.
  Guarded by `xarast-render/tests/export.rs` (runs, strip budgets, 1 vs 8
  threads, over the whole synthetic corpus),
  `xarast-io/tests/raster.rs::every_format_is_byte_identical_across_runs`,
  `deflate::tests` (feed pattern and thread count), and the CLI's
  `export_is_byte_identical_across_processes`. Anything new in the path
  (a filter heuristic, a chunk size, an encoder option) must be a function
  of the input only.
- **New lossy behaviour adds a `Compromise` variant** and is reported; it is
  never silent. Taxonomy today: `AlphaFlattened { onto }`,
  `NotRendered { what, count }` (walker shortfalls: text with no font,
  text on a path drawn on a straight baseline, quick shapes, images, live
  effects, unsupported clips), `FontSubstituted`,
  `WidenedFrom8Bit` (16-bit PNG from the 8-bit render), and for vector
  formats `Rasterised { node, reason, dpi }`, `Approximated { node, what }`
  and `BlendModeApproximated { node, ours, theirs }`.
- The export band height must stay a function of `(width, height)` only, and
  strips must start on the band grid.
- Nothing in `xarast-io` writes `.xar`.
- **PDF: only `pdf/writer.rs` names `pdf-writer`.** Everything else speaks
  `Canvas`/`Resource`/`GState`.
- **PDF numbers never carry `NaN`** (`writer::num` writes zero); geometry
  is passed in page points or path-local document units, never absolute
  millipoints, so the `f32` the file holds keeps 0.001 pt.
- **A soft mask's graphics state is set in page space** (before any `cm`):
  the mask form is interpreted in the coordinate system current at `gs`.
- **Rasterised pixels come from the raster exporter's code path**: a display
  list built from the same scene, commands matched by scene op index,
  `ListRasteriser` on `CpuConfig::deterministic()`.
- **Every rasterised or approximated object is reported**, per object, with
  its scene node (the document tag).

## Dead ends (do not retry)

- **`png` crate for interlaced output**: sets the IHDR interlace bit over
  non-interlaced data, producing corrupt files.
- **Optimised Huffman tables in JPEG** (`set_optimized_huffman_tables(true)`):
  `zune-jpeg` 0.5 — our own importer's decoder — misdecodes the 4:2:2 and
  4:2:0 files (libjpeg, Pillow and ImageMagick read them fine). Standard
  tables cost ~5 % in size. XARA-T-0193.
- **`image`'s built-in JPEG encoder**: fixed subsampling, no progressive.
- **Lossy WebP**: every usable encoder binds libwebp (C); none is pure Rust.
  Revisit only as a packaging decision (phase document risk table).
- **`oxipng`'s `timeout` for cancellation**: output would depend on timing.
- **`krilla` for PDF** (0.8.2): see the spike table — no sampled ramps, no
  meshes, `f32` `tiny-skia` geometry, a second `skrifa`, MSRV 1.92.
- ~~Rasterising an object over a transparent surface and un-premultiplying~~:
  a dead end only while the renderer darkened partly transparent Mix over
  nothing. Fixed by XARA-T-0231; it is valid now.
- **Rasterising a blend's backdrop over a transparent page**: the image
  composites a second time over the vector content under it. Render the
  backdrop over the paper.
- **`StreamShadingType` import**: `pdf-writer` 0.15 does not re-export it;
  the `/ShadingType 4` key is written by hand.
- **Comparing Poppler renders of placed images pixel for pixel**: Poppler
  resamples even at 1:1 (mean |Δ| 5–8/255 on the image cases); assert the
  report instead.

## Open TODOs

- XARA-T-0192: `impl ExportSource for Session` in `xarast-app`, File ›
  Export dialog from `Capabilities`, export off the UI thread with a
  `CancelFlag`.
- XARA-T-0038 remainder: the per-pixel cost of gradient fills (~30 ns/px)
  and the on-screen/headless path's three bands; export itself is done.
- T11.1.5 export hints in `meta.xml` (the option structs are serde-ready and
  `#[serde(default)]`), T11.1.6 batch export, T11.2.8 palette quantisation
  (PNG `Palette` is exact-only and refuses > N colours), T11.2.7 AVIF,
  T11.5.2 ICC passthrough, W11.3 SVG (XARA-US-0058).
- PDF follow-ups: XARA-T-0227 (bitmap transparency, ramp alpha, layer
  masks, per-family ΔE), XARA-T-0228 (embedded subset fonts), XARA-T-0229
  (images: DCT passthrough), XARA-T-0230 (multi-page, XMP, output intent,
  `qpdf` in CI), XARA-T-0232 (ladder cost and file size). XARA-T-0231 is
  done: the raster exporters now honour `cap_end` and the dash offset as
  PDF does, so PDF and raster agree on both.
- A 20 000 × 20 000 WebP or JPEG holds the whole image (JPEG ≤ 65 535 px a
  side, WebP ≤ 16 383); only PNG streams. `MAX_EXPORT_PIXELS` is 2²⁹.
