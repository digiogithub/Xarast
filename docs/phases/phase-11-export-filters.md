# Phase 11 — Export filters

> After this phase a Xarast document leaves the application in a form other
> tools accept: SVG and PDF that stay vector, PNG/JPEG/WebP that are
> bit-reproducible, all driven by one export model with one deterministic
> rasteriser behind it.

## Goal

Build `xarast-io`: the export side of the filter architecture.

- **Vector out**: SVG and PDF, produced from the document model, sharing the
  mapping machinery that phase 6 built for `.xarast` but emitting a different
  profile.
- **Raster out**: PNG, JPEG, WebP, always through the **CPU backend**, because
  architecture §3.3 makes that the deterministic path.
- **One export model**: area, size, resolution, background, per-format options,
  remembered per document, driven identically from the dialog and from
  `xarast-cli`.
- **Colour fidelity** that is explicit rather than accidental.

And one thing this phase must make impossible to forget:

> **`.xar` export is an explicit non-goal.** Architecture §3.5 settles it
> against `research/04 §1.13`, which listed it as P0. Writing a format we
> understand only by observation invites silently corrupt files, and the
> interoperability need is served by SVG and PDF. `xarast-io` contains no
> `.xar` writer, `xarast-xar` is read-only, and the CLI's `export` subcommand
> rejects `--format xar` with a message pointing at this decision. If someone
> re-adds it, they are reversing an arbitrated architectural decision and must
> say so in `docs/10-architecture.md` first.

## Scope

### In scope

**Export model (`xarast-app`)**

- **Area**: selection, current page, current spread, whole drawing, or an
  explicit rectangle in document coordinates.
- **Size and resolution**: the three are linked — pixel size, physical size,
  and DPI — with one of them pinned while the other two follow, exactly as a
  print dialog behaves. Physical units follow the document's unit preference.
- **Background**: transparent (where the format allows), the page's paper
  colour, or an explicit colour. JPEG has no alpha, so it always composites
  against the chosen background; the dialog says so rather than silently
  flattening onto white.
- **Bleed / margin** around the area, in document units.
- **Anti-aliasing** on/off and the render quality level.
- **Per-format options** (below).
- **Export hints**: the last-used options per format are stored in the document
  (`Kernel/exphint.cpp` did this; `research/04 §1.13`) so re-exporting is one
  click, and they round-trip through `.xarast` in `meta.xml`.
- **Batch export**: a list of jobs run in one pass, sharing one scene build
  where the area is identical. Driven from the CLI and from the dialog's
  "export every layer / every page as a separate file" option. Cheap because
  the scene build, not the encode, is the expensive part.

**Raster formats**

| Format | Encoder | Options |
|---|---|---|
| **PNG** | `png` | bit depth (8 / 16), colour type (RGBA / RGB / palette / grey), interlace, physical DPI (`pHYs`), optional `oxipng` optimisation pass |
| **JPEG** | `jpeg-encoder` | quality 1-100, progressive, chroma subsampling, DPI in JFIF/EXIF, background compositing (mandatory) |
| **WebP** | `image-webp` | lossless (default); lossy only when the optional `webp-lossy` feature is built |
| **AVIF** | `ravif` | optional feature, off by default: encoding is slow (`research/05 §6`) |

**Vector formats**

| Format | Producer | Options |
|---|---|---|
| **SVG** | our own serialiser, sharing phase 6's mapper | profile (Interchange / Native), resource handling (inline data URIs / sidecar folder), precision, text as text or as outlines, minify, embed fonts |
| **PDF** | see W11.4 | version, page size and area, text as text or as outlines, font subsetting, image downsampling and recompression, blend-mode fidelity level, output intent |

**Rasterisation**

- All raster export goes through `vello_cpu` + our CPU compositor — never the
  GPU backend. Two runs of the same export produce **byte-identical** files.
- Banding for large outputs, reusing the renderer's band planner, so a
  20,000 × 20,000 px export does not need a 1.6 GB frame buffer.
- Progress reporting and cancellation.

**Colour fidelity**

- Compositing happens in **encoded sRGB, 8 bits per channel**
  (`research/03 §2.10`) — the same space as on screen. Export does not switch
  to linear light: that would change Stained Glass and Bleach visibly.
- Exported raster files carry an sRGB marker (`sRGB` chunk for PNG, an sRGB ICC
  profile or EXIF `ColorSpace=1` for JPEG) so viewers do not guess.
- A bitmap whose source had an embedded ICC profile is exported with that
  profile preserved where the container allows and the image is passed through
  unmodified; otherwise it is converted (a no-op today, phase 15's choke point
  from phase 10 W10.8.4).
- CMYK document colours export as their resolved sRGB in raster formats, and as
  `icc-color()` in SVG / a DeviceN or ICCBased colour in PDF **when** a profile
  is available; otherwise as their resolved sRGB with a recorded warning.

### Explicitly out of scope (and which phase owns it)

| Not in this phase | Owner |
|---|---|
| **`.xar` export** | **Never.** Architecture §3.5. See the banner above |
| **EPS / PostScript export** | **Never.** Superseded by PDF (`research/04 §5.2`) |
| **Flash / SWF, HTML + imagemap, image slicing with rollovers** | **Never.** `research/04 §5.2` marks all three "no reimplementar" |
| **GIF export, animated GIF** | Post-v1.0 (P1/P3). Palette quantisation and animation frames are a project of their own |
| **Image slicing** (cutting the drawing into pieces by named objects) | **Never** — the name gallery it depends on is itself P2/P3 |
| **Colour separations, registration marks, imagesetting, overprint** | **Phase 15** (print/prepress). Stored in `meta.xml` by phase 6, ignored by export here |
| **Printing** (to a printer, as opposed to producing a PDF) | **Phase 15**. PDF export is the v0.1 answer to "I need to print this" |
| **Export preview with file-size estimation and A/B comparison** | **Phase 12** polish, if it earns its place. The model supports it; the panel is not built here |
| **TIFF, BMP, PNM, ICO export** | Post-v1.0 (P2/P3). The `Encoder` trait makes adding one a day's work |
| **Palette export (`.gpl` / `.ase`)** | Post-v1.0 (P3) |
| **PDF/A and PDF/X conformance** | **Phase 15**. This phase emits well-formed PDF 1.7 and records what would block conformance |
| **PDF *import*** | Post-v1.0 (P2 in `research/04 §5.1`) |

## Prerequisites

| Needs | From | Specifically |
|---|---|---|
| CPU backend producing correct, deterministic pixels | Phase 4 | architecture §3.3 |
| Band planner for large surfaces | Phase 4 | `research/03 §3.5 (3)` |
| The SVG profile mapper: elements, gradients, ramp baking, masks, blend modes, bitmap references | **Phase 6** | `research/06 §5`, §6 |
| `.xarast` resource store and digests | Phase 6 | `research/06 §4.4` |
| Document model complete enough to export: paths, groups, layers, pages, fills, strokes | Phases 2, 7, 8 | |
| Text layout and glyph outlines | **Phase 9** | needed for text-as-text *and* text-as-outlines |
| Bitmap resources with original bytes and colour space | **Phase 10** | needed to avoid recompressing a JPEG on export |
| `xarast-cli` command plumbing | Phase 3 | |

Phase 11 is listed as depending on 4 and 6 and running in parallel with 10. In
practice the *raster* half needs only 4 and 6; the *vector* half needs 9 and 10
for text and images. Sequence W11.1–W11.3 first so the phase delivers something
before 9 and 10 close.

## Workstreams

### W11.1 — The export model and the filter registry

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T11.1.1 | `ExportRequest` / `ExportArea` / `ExportSizing` / `Background` types and their validation | `xarast-app` | M | — |
| T11.1.2 | The size↔DPI↔physical-size linkage with one pinned axis | `xarast-app` | M | T11.1.1 |
| T11.1.3 | `Exporter` trait and a registry keyed by format, with capability flags | `xarast-io` | M | T11.1.1 |
| T11.1.4 | Per-format option enums, serialisable, with defaults | `xarast-io` | M | T11.1.3 |
| T11.1.5 | Export hints: store last-used options per format in the document; round-trip in `meta.xml` | `xarast-doc`, `xarast-format` | M | T11.1.4 |
| T11.1.6 | Batch runner: N jobs, shared scene build when areas match, progress, cancellation | `xarast-io` | M | T11.1.3 |
| T11.1.7 | `xarast-cli export` with every option exposed as a flag; `--format xar` rejected with a pointer to architecture §3.5 | `xarast-cli` | M | T11.1.3 |
| T11.1.8 | Export dialog UI bound to the model | `xarast-ui` | L | T11.1.4 |

**The size/DPI linkage is the fiddly bit and it is worth specifying.** Three
quantities — pixel size `P`, physical size `S`, resolution `R` — satisfy
`P = S × R`. The user pins one and edits another; the third follows. Aspect
ratio is locked by default. All arithmetic happens in `f64` on the document's
millipoint area, and the pixel size is `round_half_away_from_zero`, matching the
renderer's rounding rule (`research/03 §3.7`). An export of a 100 mm square at
300 dpi must give exactly 1181 × 1181 px, every time, on every platform.

**`Exporter` capability flags** tell the dialog what to show: does the format
support alpha, multiple pages, vector content, embedded fonts, a DPI field?
The dialog is generated from those flags rather than switched on the format,
which is what keeps adding TIFF later a day's work.

### W11.2 — Raster export

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T11.2.1 | `render_to_surface(doc, area, px_size, background, quality)` on the **CPU backend**, banded | `xarast-render` | M | phase 4 |
| T11.2.2 | Determinism harness: same input → byte-identical output, asserted across two runs and two machines | `xarast-render` | S | T11.2.1 |
| T11.2.3 | PNG encoder with depth, colour type, interlace, `pHYs`, `sRGB` chunk | `xarast-io` | M | T11.2.1 |
| T11.2.4 | Optional `oxipng` pass, off the main thread, cancellable | `xarast-io` | S | T11.2.3 |
| T11.2.5 | JPEG encoder with quality, progressive, subsampling, DPI, mandatory background compositing | `xarast-io` | M | T11.2.1 |
| T11.2.6 | WebP lossless; lossy behind the `webp-lossy` feature | `xarast-io` | M | T11.2.1 |
| T11.2.7 | AVIF behind the `avif` feature, `ravif`, on the pool | `xarast-io` | S | T11.2.1 |
| T11.2.8 | Palette quantisation for 8-bit PNG (median cut + Floyd–Steinberg, per `research/03 §3.9` M8) | `xarast-io` | M | T11.2.3 |
| T11.2.9 | Memory ceiling: bands sized so peak RSS stays under a configured cap regardless of output size | `xarast-io` | M | T11.2.1 |

**Determinism is a feature, not a side effect.** T11.2.2 is the test that
protects it: render the same document twice in the same process, once more in a
fresh process, hash the output, compare. If the GPU backend ever leaks into the
export path — through a cached texture, a shared paint cache, anything — this
test catches it. The CPU/GPU parity tests of phase 4 are the other half: they
say the deterministic path is also the correct one.

`oxipng` is optional and off by default: it is slow, and an export that takes
eight seconds instead of one surprises people. Expose it as "optimise
(slower)".

### W11.3 — SVG export: what is shared with phase 6 and what is not

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T11.3.1 | Add an `SvgDialect` field to phase 6's `svg::SvgProfile` options struct and thread it through the mapper | `xarast-format` | L | phase 6 |
| T11.3.2 | `SvgDialect::Interchange`: no `xarast:` namespace, no preservation payloads, no unknown-data re-emission | `xarast-io` | M | T11.3.1 |
| T11.3.3 | Resource handling: inline `data:` URIs vs a sidecar folder vs referencing an existing path | `xarast-io` | M | T11.3.1 |
| T11.3.4 | Font handling: `@font-face` with a WOFF2 subset, or text converted to outlines | `xarast-io` | M | phase 9 W9.6 |
| T11.3.5 | Coordinate precision and number formatting (shortest round-tripping representation at N decimals) | `xarast-format` | M | T11.3.1 |
| T11.3.6 | Optional minification: strip ids nothing references, merge `<g>` chains, shorten paths | `xarast-io` | M | T11.3.2 |
| T11.3.7 | Validation in CI: every exported SVG parses with `usvg` and renders with `resvg` | `xarast-io` | M | T11.3.2 |

**Shared with phase 6 — exactly this list.** These are the same code, called
with a different profile:

- The element mapping: `<path>`, `<rect>`/`<ellipse>` for QuickShapes,
  `<g>` for groups and layers, `<use>` for clones, `<image>` for bitmaps
  (`research/06 §6.1`, §6.2, §6.9, §6.11).
- Gradient emission: `<linearGradient>` / `<radialGradient>` with
  `gradientUnits="userSpaceOnUse"` and `gradientTransform`, `spreadMethod` from
  the tiling mode (`research/06 §6.3`).
- **Ramp-profile baking**: adaptive sampling of the bias/gain curve into stops
  until the maximum per-channel error is ≤ 2/255, minimum 9 and maximum 33
  stops per key-colour segment (`research/06 §6.4`). Same code, same
  thresholds.
- **The bake ladder** for what SVG cannot express: conical and diamond
  gradients as ≤ 96 wedges or a rasterised `<pattern>`; three- and four-colour
  fills as an `<image>`; fractal fills as `feTurbulence`; variable-width strokes
  and brushes as filled outlines (`research/06 §6.3`, §6.6). Including the
  normative rule that a baked raster is generated at twice the object's nominal
  resolution, minimum 96 dpi, maximum 4096 px on a side.
- Transparency as `<mask>` with `color-interpolation="sRGB"` — *the* classic
  mistake, and it is fixed once, in the shared code (`research/06 §6.5.1`).
- Blend modes to `mix-blend-mode` with `isolation:isolate` on containing groups
  (`research/06 §6.5.2`).
- Clipping to `<clipPath>`, and text to `<text>` / `<tspan>` / `<textPath>`
  (`research/06 §6.7`, §6.10).
- Colour emission including `icc-color()` for CMYK (`research/06 §6.12.3`).

**Not shared — this is what `SvgDialect::Interchange` changes.** Phase 11 adds
these; phase 6 must not grow them:

| Aspect | `Native` (phase 6, inside `.xarast`) | `Interchange` (phase 11, standalone `.svg`) |
|---|---|---|
| `xarast:` namespace attributes | Emitted, they are the fidelity layer | **Omitted entirely** — an interchange file must not carry our private vocabulary |
| Preserved unknown data from a previous load | Re-emitted verbatim (`research/06 §8`) | **Dropped**, and the user is told how many objects lost data |
| Resources | Referenced into the ZIP (`resources/images/…`) | Inlined as `data:` URIs, or written to a sidecar folder next to the `.svg`, or (when exporting over an earlier export) left as relative paths |
| Object ids | Stable across saves, part of the identity model (`research/06 §5.7`) | Regenerated, minifiable, droppable |
| `meta.xml`, manifest, thumbnail, preview | Separate ZIP entries | Not produced; a minimal `<title>`/`<desc>` and `<metadata>` block instead |
| Fonts | WOFF2 subset in `resources/fonts/` + `@font-face` | Same subset **inlined** as a `data:` URI, or text converted to outlines |
| Baked content | Marked `xarast:generated="…"` so the reader can discard it | Unmarked — nothing will ever read it back |
| Conformance profile C (hidden outline copy of all text) | Available | Not offered; "text as outlines" replaces the text instead of duplicating it |
| Precision | Full, lossless round-trip is the point | User-selectable; default 3 decimals in user units |
| Validation target | Our own reader | `usvg` parse + `resvg` render, in CI |

The seam is a single parameter threaded through the mapper, not a fork of it.
If a future change needs a second fork, that is the signal that the mapper
should be split properly — record it rather than copy the file.

### W11.4 — PDF export

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T11.4.1 | Spike: express the Xarast feature matrix in the candidate crate; record what does not fit | `xarast-io` | M | phase 4 |
| T11.4.2 | Adopt the winner; `PdfWriter` façade so the crate choice stays replaceable | `xarast-io` | M | T11.4.1 |
| T11.4.3 | Geometry: paths, fill rules, strokes, caps/joins/miter, dashes, clipping | `xarast-io` | M | T11.4.2 |
| T11.4.4 | Gradients: axial and radial shadings; ramp profile baked into the function; conical/diamond/mesh via the bake ladder | `xarast-io` | L | T11.4.3 |
| T11.4.5 | Transparency: constant alpha, soft masks from graduated transparency, transparency groups | `xarast-io` | L | T11.4.3 |
| T11.4.6 | Blend modes: map the exactly-expressible ones to PDF blend modes; rasterise the rest | `xarast-io` | M | T11.4.5 |
| T11.4.7 | Text: embedded subset fonts with a correct `ToUnicode` map, or outlines | `xarast-io` | L | phase 9 |
| T11.4.8 | Images: JPEG passthrough via `DCTDecode`, PNG via `FlateDecode`, alpha as an `SMask`, optional downsampling | `xarast-io` | M | phase 10 |
| T11.4.9 | Pages, page boxes, bleed, document metadata (XMP), output intent | `xarast-io` | M | T11.4.2 |
| T11.4.10 | Validation in CI: `qpdf --check`, structural assertions, and a render comparison against our own raster export | `xarast-io` | M | T11.4.3 |

**The crate choice, and why.** `research/05` does not cover PDF writing — PDF
export is a modern addition (`research/04 §5.2` marks it P1, "no existía
nativamente"). So this phase chooses, against these criteria, in this order:

1. **Pure Rust, no C toolchain.** Non-negotiable: architecture §3.3 rejected
   `skia-safe` for exactly this reason, and the AppImage budget is 80 MB.
2. **Licence** compatible with `MIT OR Apache-2.0` and passing
   `cargo deny check licenses` without an exception.
3. Expresses, natively: axial and radial **shadings** with arbitrary function
   ramps, **soft masks**, **transparency groups**, **blend modes**, **font
   subsetting** with `ToUnicode`, and image XObjects with `SMask`.
4. Shares types with our stack (`kurbo`, `skrifa`) so there is no conversion
   layer.
5. Maintained, and small enough to read when it misbehaves.

**Recommendation: `krilla`**, the high-level PDF writer from the Typst
ecosystem, built on `pdf-writer`. It is pure Rust, permissively licensed, and
it exists precisely to turn a 2D scene — paths, clips, soft masks, blend modes,
gradients, glyph runs — into PDF, with font subsetting already solved. It uses
the same `kurbo`/`skrifa` family we already depend on, which removes an entire
conversion layer.

**Fallback: `pdf-writer` directly.** If the spike finds that `krilla`'s scene
model cannot express something we need (the most likely candidate is a shading
type or an unusual soft-mask arrangement), drop to `pdf-writer` and emit the
content stream ourselves. That is more code but no new dependency, since
`krilla` sits on `pdf-writer` anyway — so the fallback is a partial retreat, not
a rewrite.

**Rejected without a spike**: anything binding a C or C++ PDF library
(`pdfium`, `mupdf`, `cairo`) — same argument as `skia-safe`; and
`printpdf`, which is oriented at document/report generation rather than exact
vector graphics reproduction.

T11.4.1 must produce a written matrix: for each of our features, "native /
workaround / rasterise", with the workaround named. That matrix, not the crate's
README, is what decides.

**The fidelity ladder for PDF**, in order — the same discipline as the SVG bake
ladder:

1. **Native.** Flat fills, axial and radial shadings (with the ramp profile
   baked into a sampled `FunctionType 0`), strokes, dashes, clips, constant
   alpha, transparency groups, the PDF-standard blend modes (`Multiply`,
   `Screen`, `Darken`, `Lighten`, `Hue`, `Saturation`, `Luminosity`), embedded
   subset fonts, `DCTDecode` images.
2. **Workaround, still vector.** Conical and diamond gradients as a fan of
   shaded wedges (the same ≤ 96 wedges as SVG). Graduated transparency as a
   luminosity `SMask` built from a shading. Three/four-colour fills as a
   `ShadingType 4`/`6` mesh if the writer supports it, else step 3.
3. **Rasterise that object.** Xara blend modes with no PDF equivalent
   (Stained Glass, Bleach, Contrast, Brightness, Bevel), fractal fills, and
   anything the matrix marks "rasterise". The object and everything it blends
   with are rendered to an image XObject at a stated DPI (default 300,
   configurable) through the **CPU backend**, and placed. The export report
   lists every object this happened to, with its name and the reason.

Note that `Darken`/`Lighten`/`Hue`/`Saturation`/`Luminosity` map to PDF blend
modes with the *same names* but **not the same formulas**: PDF's are the
PDF/CSS separable and non-separable definitions; Xara's are the LUT families of
`research/03 §2.7.2`. The spike must measure the difference on a test patch and
decide per family whether "close enough" (map natively) or "visibly different"
(rasterise). Record the per-family decision and the measured ΔE in
`docs/memory/render.md`. Do not assume the names mean the same thing — that
assumption is the most likely way this phase ships something subtly wrong.

### W11.5 — Colour fidelity across formats

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T11.5.1 | sRGB markers on every raster output (`sRGB` chunk, EXIF `ColorSpace`, WebP `ICCP`) | `xarast-io` | S | T11.2.3 |
| T11.5.2 | Pass through an image's embedded ICC profile when the image itself passes through unmodified | `xarast-io` | M | phase 10 W10.8 |
| T11.5.3 | CMYK colours: `icc-color()` in SVG, ICCBased/DeviceN in PDF, resolved sRGB in raster | `xarast-io` | M | phase 8 |
| T11.5.4 | Export report listing every fidelity compromise made (rasterised objects, dropped profiles, substituted fonts, lost unknown data) | `xarast-io` | M | T11.3.2, T11.4.6 |
| T11.5.5 | A colour-fidelity test sheet: known patches exported to every format and read back | `xarast-io` | M | T11.5.1 |

The **export report** is worth building properly. Every lossy decision this
phase can make — an object rasterised because PDF cannot express its blend
mode, a font not embedded because `fsType` forbade it, unknown data dropped
because the SVG profile is Interchange, a CMYK colour flattened because there
was no profile — is appended to a structured report that the dialog shows and
the CLI prints. Silent loss is the thing users of a vector editor complain about
most, and it costs almost nothing to be honest.

### W11.6 — Corpus and regression

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T11.6.1 | Export every corpus file to all five formats; assert no errors | `xarast-cli` | M | T11.2.x, T11.3.x, T11.4.x |
| T11.6.2 | Round-trip check: export SVG → import with `usvg` → render → compare against our own raster export | `xarast-io` | M | T11.3.7 |
| T11.6.3 | PDF check: `qpdf --check` clean; render the PDF with an external tool and compare | `xarast-io` | M | T11.4.10 |
| T11.6.4 | Byte-reproducibility check across two runs and two CI machines | `xarast-cli` | S | T11.2.2 |
| T11.6.5 | Export-hint round-trip through `.xarast` | `xarast-format` | S | T11.1.5 |

## Public API introduced

```rust
// ──────────────────────────────── xarast-app ────────────────────────────────

/// What part of the document is exported.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ExportArea {
    Selection,
    Page(PageId),
    Spread(SpreadId),
    /// Union of every visible, exportable object's bounds.
    Drawing,
    Rect(Rect),          // document coordinates, millipoints
}

/// Pixel size, physical size and resolution are linked by `P = S × R`.
/// Exactly one is pinned; the caller edits another; the third follows.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ExportSizing {
    pub pinned: SizingAxis,
    pub pixels: (u32, u32),
    pub physical: (Millipoints, Millipoints),
    pub dpi: f64,
    pub lock_aspect: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SizingAxis { Pixels, Physical, Dpi }

impl ExportSizing {
    /// Recompute the two unpinned quantities from the pinned one and `area`.
    /// Pixel counts use round-half-away-from-zero, matching research/03 §3.7.
    pub fn resolve(&mut self, area: Rect) -> Result<(), SizingError>;
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Background {
    /// Only honoured by formats whose `Capabilities::alpha` is true.
    Transparent,
    Paper,
    Colour(Rgba8),
}

#[derive(Clone, PartialEq, Debug)]
pub struct ExportRequest {
    pub area: ExportArea,
    pub sizing: ExportSizing,
    pub background: Background,
    pub bleed: Millipoints,
    pub antialias: bool,
    pub quality: RenderQuality,      // always Final for export
    pub options: FormatOptions,
    pub destination: PathBuf,
}

/// One entry per format. The dialog is generated from `Capabilities`,
/// not switched on the variant.
#[derive(Clone, PartialEq, Debug)]
pub enum FormatOptions {
    Png(PngOptions),
    Jpeg(JpegOptions),
    WebP(WebPOptions),
    Avif(AvifOptions),
    Svg(SvgOptions),
    Pdf(PdfOptions),
}

// ───────────────────────────────── xarast-io ────────────────────────────────

/// Every export filter implements this. There is no `.xar` implementation,
/// and there must never be one: architecture §3.5.
pub trait Exporter {
    fn id(&self) -> FormatId;
    fn capabilities(&self) -> Capabilities;
    fn default_options(&self) -> FormatOptions;
    fn export(&self, doc: &Document, req: &ExportRequest,
              progress: &dyn Progress) -> Result<ExportReport, ExportError>;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Capabilities {
    pub vector: bool,
    pub alpha: bool,
    pub multipage: bool,
    pub embeds_fonts: bool,
    pub has_dpi: bool,
    pub lossy: bool,
    pub deterministic: bool,     // true for every format in this phase
}

pub struct Registry { /* … */ }

impl Registry {
    pub fn with_builtin() -> Self;
    pub fn get(&self, id: FormatId) -> Option<&dyn Exporter>;
    pub fn for_extension(&self, ext: &str) -> Option<&dyn Exporter>;
    pub fn all(&self) -> impl Iterator<Item = &dyn Exporter>;
}

/// Honest accounting of what the export could not reproduce exactly.
#[derive(Clone, Default, Debug)]
pub struct ExportReport {
    pub bytes_written: u64,
    pub duration: Duration,
    pub compromises: Vec<Compromise>,
}

#[derive(Clone, Debug)]
pub enum Compromise {
    /// An object could not be expressed and was rendered to an image.
    Rasterised { node: NodeId, reason: Arc<str>, dpi: f64 },
    /// A gradient or effect was approximated (wedge fan, baked stops, filter).
    Approximated { node: NodeId, what: Arc<str>, max_error: Option<f32> },
    FontNotEmbedded { family: Arc<str>, reason: Arc<str> },
    FontSubstituted { requested: Arc<str>, used: Arc<str> },
    /// `SvgDialect::Interchange` drops preserved unknown data.
    UnknownDataDropped { count: usize },
    ColourProfileDropped { node: NodeId },
    /// A blend mode mapped to a same-named but differently-defined mode.
    BlendModeApproximated { node: NodeId, ours: TranspMode, theirs: Arc<str>,
                            measured_delta_e: Option<f32> },
}

/// Batch: N jobs in one pass. Jobs sharing an identical area and sizing
/// share one scene build.
pub fn export_batch(doc: &Document, jobs: &[ExportRequest],
                    registry: &Registry, progress: &dyn Progress)
    -> Vec<Result<ExportReport, ExportError>>;

// ── raster ──────────────────────────────────────────────────────────────────

/// THE export rasteriser. CPU backend only — architecture §3.3. Banded
/// internally so peak memory is bounded regardless of output size.
pub fn render_for_export(doc: &Document, area: Rect, px: (u32, u32),
                         background: Background, antialias: bool,
                         band_budget_bytes: usize,
                         progress: &dyn Progress)
    -> Result<Surface8, ExportError>;

#[derive(Clone, PartialEq, Debug)]
pub struct PngOptions {
    pub bit_depth: PngDepth,          // Eight | Sixteen
    pub colour: PngColour,            // Rgba | Rgb | Palette { max: u16 } | Grey
    pub interlace: bool,
    pub write_dpi: bool,
    pub optimise: bool,               // oxipng pass, off by default
}

#[derive(Clone, PartialEq, Debug)]
pub struct JpegOptions {
    pub quality: u8,                  // 1..=100
    pub progressive: bool,
    pub subsampling: Subsampling,     // S444 | S422 | S420
    pub write_dpi: bool,
}

#[derive(Clone, PartialEq, Debug)]
pub struct WebPOptions {
    /// `Lossy` requires the `webp-lossy` feature; otherwise `export` returns
    /// `ExportError::FeatureNotBuilt`.
    pub mode: WebPMode,               // Lossless | Lossy { quality: u8 }
}

// ── vector ──────────────────────────────────────────────────────────────────

/// The one parameter that separates phase 6's writer from phase 11's.
/// Phase 6 already owns `xarast_format::svg::SvgProfile`, the struct of
/// serialisation options; this is a NEW FIELD on that struct, not a rival type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SvgDialect {
    /// Inside `.xarast`: `xarast:` extensions, stable ids, preservation.
    Native,
    /// Standalone `.svg`: no private namespace, no preservation, minifiable.
    Interchange,
}

#[derive(Clone, PartialEq, Debug)]
pub struct SvgOptions {
    pub dialect: SvgDialect,          // Interchange for File > Export
    pub resources: SvgResources,      // Inline | Sidecar { dir } | Reference
    pub text: TextOutput,             // AsText { embed_fonts: bool } | AsOutlines
    pub decimals: u8,                 // default 3
    pub minify: bool,
    pub bake_dpi: f64,                // for the bake ladder; default 2× nominal
}

#[derive(Clone, PartialEq, Debug)]
pub struct PdfOptions {
    pub version: PdfVersion,          // V1_7 default
    pub text: TextOutput,
    pub image_policy: PdfImagePolicy, // Passthrough | Recompress { quality }
                                      // | Downsample { max_dpi, quality }
    pub rasterise_dpi: f64,           // fidelity ladder step 3; default 300
    pub blend_fidelity: BlendFidelity,
    pub embed_output_intent: bool,
}

/// How hard to try before rasterising an object whose blend mode PDF names
/// but defines differently.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlendFidelity {
    /// Map to the same-named PDF mode. Smaller files, slight colour shift.
    PreferNative,
    /// Rasterise anything that is not exactly reproducible. Bigger, exact.
    Exact,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextOutput { AsText { embed_fonts: bool }, AsOutlines }

#[derive(Debug)]
pub enum ExportError {
    /// Returned for `.xar`. Not a TODO: architecture §3.5 makes it permanent.
    UnsupportedFormat { id: FormatId, reason: &'static str },
    FeatureNotBuilt(&'static str),
    Sizing(SizingError),
    Render(RenderError),
    Encode(Box<dyn std::error::Error + Send + Sync>),
    Io(std::io::Error),
    Cancelled,
}
```

## Acceptance criteria

1. `cargo test -p xarast-io` passes.
2. **Determinism.** Exporting `Designs/Spitfire.xar` to PNG twice in one
   process, and once in a fresh process, produces three byte-identical files
   (SHA-256 compared). The same holds on a second CI runner with a different
   CPU. Repeated for JPEG, WebP, SVG and PDF.
3. **The `.xar` non-goal is enforced in code.**
   `xarast-cli export --format xar in.xarast out.xar` exits non-zero with a
   message naming architecture §3.5; `Registry::for_extension("xar")` returns
   `None`; a test asserts that no type in `xarast-io` implements `Exporter`
   with `FormatId::Xar`, and `grep -ri 'xar.*writer\|write_xar' crates/xarast-io`
   finds nothing.
4. **Sizing arithmetic.** A 100 mm square area at 300 dpi exports at exactly
   1181 × 1181 px. A table of 20 (area, dpi, expected-pixels) triples is
   asserted, including cases that land exactly on .5.
5. **Every corpus file exports without error** to PNG, JPEG, WebP, SVG and PDF:
   all 59 `.xar` files from `testfiles/` and `Designs/`, plus the 14 from
   `TextDesigns/`. Zero errors, and every `ExportReport` is captured.
6. **SVG validity.** Every exported SVG parses with `usvg` with no errors, and
   its `resvg` render differs from our own PNG export of the same area by no
   more than the phase-3 perceptual gate **for the features
   `research/06 §6.13` lists as "looks the same"**. Features in the
   "approximate" and "different but reasonable" columns are excluded from the
   gate and are instead asserted to appear in the `ExportReport` as
   `Compromise::Approximated` or `::Rasterised`.
7. **SVG interchange profile is clean.** An exported `.svg` contains **zero**
   occurrences of the string `xarast:` and no `xmlns:xarast` declaration.
   Asserted by grep over every file from criterion 5.
8. **PDF validity.** Every exported PDF passes `qpdf --check` with no warnings,
   and a structural test asserts: one page per exported page, the declared
   MediaBox matches the requested size to within 1/1000 pt, every font used is
   embedded and subset, and every image XObject that came from a JPEG uses
   `DCTDecode` with the **original bytes** (asserted by extracting the stream
   and comparing to the source file).
9. **PDF fidelity ladder is recorded.** T11.4.1's matrix exists in
   `docs/memory/render.md`, listing every Xarast feature as
   native / workaround / rasterise, and for each blend-mode family the measured
   ΔE between Xara's definition and PDF's same-named mode.
10. **Rasterised objects are reported.** A document deliberately using Stained
    Glass, Bleach and Contrast exports to PDF with exactly three
    `Compromise::Rasterised` entries, each naming the object.
11. **JPEG passthrough survives export.** A document containing an imported
    JPEG, exported to PDF, contains that JPEG's bytes unmodified (criterion 8);
    exported to SVG with inline resources, contains the same bytes base64-
    encoded.
12. **Background handling.** Exporting a document with a transparent background
    to PNG yields alpha; to JPEG yields the chosen background colour composited,
    with an `ExportReport` entry noting it; the pixel at a known transparent
    location equals the chosen colour exactly.
13. **Batch.** Exporting 20 jobs over the same area in one `export_batch` call
    builds the scene once — asserted by an instrumentation counter — and takes
    less than 1.5× the time of the single slowest job plus the encode time of
    the rest.
14. **Export hints round-trip.** Set non-default PNG and PDF options, save the
    `.xarast`, reload, and the options come back identical.
15. **Memory ceiling.** A 20,000 × 20,000 px PNG export completes with peak RSS
    below 1.5 GB, measured by the harness.
16. `cargo clippy --workspace -- -D warnings` and `cargo deny check licenses`
    pass, including whichever PDF crate is adopted.

## Performance budgets

| Budget | Target | How measured |
|---|---|---|
| PNG export, A4 at 300 dpi (2480 × 3508), 10k-object document | ≤ 3 s, CPU backend | `criterion` |
| PNG export, same, with `optimise` | ≤ 12 s, off the main thread | timing harness |
| JPEG export, same area, quality 90 | ≤ 2.5 s | `criterion` |
| WebP lossless, same area | ≤ 4 s | `criterion` |
| SVG export, 100k-object document | ≤ 5 s | `criterion` |
| SVG export, 10k-object document | ≤ 600 ms | `criterion` |
| PDF export, 10k-object document, no rasterised objects | ≤ 3 s | `criterion` |
| PDF export with 20 rasterised objects at 300 dpi | ≤ 8 s | `criterion` |
| Peak RSS, 20,000 × 20,000 px export | ≤ 1.5 GB | harness, banded |
| Peak RSS, A4 at 300 dpi | ≤ 300 MB | harness |
| Batch of 20 same-area jobs vs 20 separate exports | ≥ 3× faster | `criterion` |
| Export dialog: recomputing size/DPI on every keystroke | ≤ 100 µs | `criterion` |
| Cancellation latency | ≤ 200 ms | harness |

## Risks and mitigations

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| The chosen PDF crate cannot express a needed construct and the fallback is a large rewrite | Medium | High | T11.4.1 is a **spike before adoption**, and the fallback (`pdf-writer`) is the layer the recommendation already sits on, so retreating is partial. The `PdfWriter` façade in T11.4.2 keeps the choice replaceable |
| PDF's `Darken`/`Lighten`/`Hue`/`Saturation`/`Luminosity` are assumed equivalent to Xara's same-named modes and are not | **High** | High | Explicitly called out in W11.4; the spike measures ΔE per family and the result is recorded. `BlendFidelity::Exact` rasterises rather than guess. This is the single most likely way this phase ships something wrong |
| Someone re-adds `.xar` export because `research/04 §1.13` lists it as P0 | Medium | Medium | The banner at the top of this document, the `UnsupportedFormat` error with a permanent reason string, and acceptance criterion 3's grep test |
| SVG export drifts from `.xarast` writing, so two code paths grow apart | Medium | High | They are one code path with an `SvgDialect` field on phase 6's `SvgProfile` (T11.3.1). Criterion 7 and the shared-machinery list in W11.3 are the contract. A second fork requires a recorded decision |
| `usvg`/`resvg` used as the SVG validation oracle disagrees with browsers | Medium | Low | `resvg` is the CI gate because it is scriptable; a manual browser spot-check of a fixture sheet happens once per phase, and `research/06 §5.9` already sets the "renders reasonably in a browser" criterion |
| The GPU backend leaks into the export path | Low | High | Criterion 2's cross-process determinism test, plus a compile-time guard: `render_for_export` takes the CPU backend type, not a trait object |
| Banding produces visible seams at band boundaries | Medium | High | The band planner is phase 4's, already exercised by on-screen rendering; the export test set includes a full-page gradient with graduated transparency, which is where a seam would show. Assert zero column-to-column discontinuity across every band boundary |
| `oxipng` or `ravif` makes the export feel broken by taking minutes | Medium | Low | Both off by default, both on the pool, both cancellable, both with a progress bar |
| `webp-lossy` pulls libwebp (C) into the AppImage | Medium | Medium | Not a default feature; the shipped AppImage is built without it. If users demand lossy WebP, revisit as a packaging decision with `deny.toml` justification, not as a code change |
| Font embedding in PDF violates a font's licence | Low | Medium | Same `fsType` check as phase 9's `.xarast` embedding, sharing the same function. When embedding is denied, fall back to outlines and record a `Compromise::FontNotEmbedded` |

## Test plan

**Unit.** `ExportSizing::resolve` against the 20-triple table, including
half-pixel rounding. Capability-driven dialog generation (each format's
`Capabilities` produces the expected field set). Format-option serialisation
round-trip. `Registry` lookup by extension and by id, including the `.xar`
rejection.

**Determinism.** Criterion 2's triple-hash comparison, per format, run in every
CI job on two runners.

**Corpus.** All 73 corpus files (59 + 14 text) × 5 formats = 365 exports, in a
nightly job; errors fail the build, and every `ExportReport` is archived as a
CI artefact so compromises can be diffed between commits. A compromise appearing
where there was none is a regression even if nothing crashed.

**SVG.** Parse with `usvg`, render with `resvg`, compare against our own raster
export for the "looks the same" feature set. Grep for `xarast:` in Interchange
output. Minification idempotence (minify twice → identical). A fixture sheet
opened manually in a browser and in Inkscape once per phase, per
`research/06 §5.9`.

**PDF.** `qpdf --check` on every output. Structural assertions (page count,
MediaBox, font embedding, `DCTDecode` passthrough). Render the PDF with an
external renderer available in CI and compare against our raster export at the
same DPI, with the "rasterise" ladder entries excluded and separately asserted
to be present in the report.

**Raster.** Per-format encode/decode round-trip with known pixel patches.
Background compositing at known coordinates. Palette quantisation error bound.
DPI metadata read back. sRGB marker present.

**Colour fidelity sheet.** A document of labelled patches: sRGB primaries and
greys, a CMYK patch with and without a profile, a named colour and two of its
tints, a gradient with a non-identity profile, each of the ten transparency
modes over a known backdrop. Exported to all five formats, read back, compared
against expected values with a stated tolerance per format. This one sheet
catches most fidelity regressions.

**Banding.** A full-page linear gradient under a graduated transparency,
exported at a size that forces at least 8 bands; assert no discontinuity greater
than 1/255 across any band boundary.

**Stress and memory.** 20,000 × 20,000 px export; 100k-object SVG; a document
with 50 large bitmaps to PDF with each `PdfImagePolicy`.

**Cancellation.** Start each long export and cancel at 10 %, 50 %, 90 %;
assert the call returns within 200 ms, no partial file is left behind, and no
thread is leaked.

## Memory note

Update **`docs/memory/render.md`** with:

- The **PDF fidelity matrix** from T11.4.1: every feature as
  native / workaround / rasterise, with the workaround named.
- The **per-blend-family decision** for PDF: which of Xara's families map to a
  PDF blend mode, the measured ΔE for each, and which are rasterised instead.
  This is the phase's most reusable finding.
- Confirmation that export runs on the CPU backend only, and where that is
  enforced.
- The band-boundary test and what it protects.

Update **`docs/memory/xarast-format.md`** with:

- The `SvgDialect` seam on phase 6's `SvgProfile`: the exact list of what
  `Native` and `Interchange`
  differ in (the table in W11.3), so a future change to the mapper knows which
  side it belongs on.
- The rule that the mapper is one code path, and what would justify forking it.
- Precision and number-formatting rules for exported SVG.

Create **`docs/memory/export.md`** from the template in `docs/memory/INDEX.md`
and add its row to that index. It must record:

- **`.xar` export is permanently out of scope** (architecture §3.5), how that is
  enforced in code, and the test that guards it — so this decision does not get
  relitigated every year.
- The PDF crate chosen, the spike results that chose it, and what the fallback
  would cost.
- The determinism contract and the cross-process hash test.
- The `ExportReport`/`Compromise` taxonomy, and the rule that new lossy
  behaviour must add a `Compromise` variant rather than be silent.
- The default option set per format and why each default was chosen.
- Optional features (`webp-lossy`, `avif`, `oxipng`) and their packaging
  consequences for the AppImage.
- Dead ends: encoders or PDF approaches tried and abandoned.

Update **`docs/memory/perf.md`** with the measured export timings and the peak
RSS numbers per output size, and the band budget that produced them.
