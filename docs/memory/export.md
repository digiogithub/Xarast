# export

Memory note for **export** (`xarast-io`, the export entry of
`xarast-render`, and `xarast-cli export`). Phase 11:
`docs/phases/phase-11-export-filters.md`.

## Current state

Round 1 (2026-09-23, XARA-US-0056 + XARA-US-0057): the export model, the
filter registry and deterministic **raster** export to PNG, JPEG and WebP.
SVG and PDF export (W11.3, W11.4), batch export (T11.1.6), export hints in
the document (T11.1.5), palette quantisation (T11.2.8), AVIF (T11.2.7) and
the dialog (T11.1.8, XARA-T-0192) are not built yet.

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

The whole 59-file corpus exports to all three formats with **zero errors**,
at 96 and at 300 dpi, and two runs are byte-identical for every file and
format (numbers in `perf.md`, "Export"). The only compromises the corpus
reports are 25 font substitutions and one text-on-path drawn straight
(plus `AlphaFlattened` for every JPEG over a transparent background).

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
  `WidenedFrom8Bit` (16-bit PNG from the 8-bit render).
- The export band height must stay a function of `(width, height)` only, and
  strips must start on the band grid.
- Nothing in `xarast-io` writes `.xar`.

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

## Open TODOs

- XARA-T-0192: `impl ExportSource for Session` in `xarast-app`, File ›
  Export dialog from `Capabilities`, export off the UI thread with a
  `CancelFlag`.
- XARA-T-0038 remainder: the per-pixel cost of gradient fills (~30 ns/px)
  and the on-screen/headless path's three bands; export itself is done.
- T11.1.5 export hints in `meta.xml` (the option structs are serde-ready and
  `#[serde(default)]`), T11.1.6 batch export, T11.2.8 palette quantisation
  (PNG `Palette` is exact-only and refuses > N colours), T11.2.7 AVIF,
  T11.5.2 ICC passthrough, W11.3 SVG, W11.4 PDF.
- A 20 000 × 20 000 WebP or JPEG holds the whole image (JPEG ≤ 65 535 px a
  side, WebP ≤ 16 383); only PNG streams. `MAX_EXPORT_PIXELS` is 2²⁹.
