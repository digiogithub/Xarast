# image

Memory note for **`xarast-image`**: bitmap resources, safe decoding, EXIF,
the legacy `.xar` bitmap wrappings. Phase 10
(`docs/phases/phase-10-bitmaps-and-photo.md`, W10.1/W10.2; story
XARA-US-0051). The phase document calls this note `images.md`; the crate is
`xarast-image`, so the note is `image.md`.

## Current state

Round 1 (2026-09-23) built the decoding half of W10.1/W10.2 as a **leaf
crate**: it depends on `image`, `blake3`, `flate2` and `thiserror` only — not
on `xarast-doc` nor `xarast-xar`, which it reaches as dev-dependencies for
the corpus test.

- `BitmapInfo` (with `ColourSpace`), `BitmapData` (premultiplied RGBA8),
  `OriginalEncoded`, `BitmapResource`, `ImageFormat`.
- `probe` / `probe_as` (header only), `decode` / `decode_as` (sniff, guard,
  decode, orient), `decode_on_worker` (hard wall clock),
  `to_working_space` (the colour-space choke point, a no-op until phase 15).
- `xar::decode_xar_bitmap(tag, bytes, palette, limits)`: the `.xar`
  wrappings; `xar::dib_to_bmp`, `xar::snap_to_palette`.
- Formats: PNG (8/16-bit, grey, palette, APNG first frame), JPEG (baseline
  and progressive, via zune-jpeg), WebP (lossy, lossless, animated first
  frame), GIF (first frame), TIFF, BMP, headerless DIB, PNM.
- **Every bitmap in the 59-file corpus decodes** (table below).
- **The walker decodes and renders them** (XARA-T-0129, 2026-09-23):
  `images_pending == 0` on every corpus file. See "Walker integration
  (as built)".
- **Tag-68 PNG alpha is transparency** (2026-09-23): `decode_xar_bitmap(68)`
  inverts it and `xar::normalise_xar_png` rewrites such a file as a
  standard PNG, which the importer does. See "The `.xar` wrappings".
- Not yet: `xarast-doc` still has its own `BitmapResource` (SHA-256 over
  pixels + original; XARA-T-0156). Encoders (T10.2.5), photo ops and the
  gallery are later workstreams. The pyramid (XARA-US-0052) and the pixel
  budget (XARA-US-0053) were built in `xarast-render`; see below.

### Corpus census (all 59 files, `tests/corpus.rs`)

`XARAST_XAR_CORPUS=… cargo test -p xarast-image --test corpus -- --nocapture`

| Record kind [sniffed content] | Records | Decoded | Failed | Mpx | With alpha | Decode ms (test profile) |
|---|---|---|---|---|---|---|
| 61 `PREVIEWBITMAP_GIF` [gif] | 58 | 58 | 0 | 0.65 | 0 | 2.7 |
| 67 `DEFINEBITMAP_JPEG` [jpeg] | 4 | 4 | 0 | 0.71 | 0 | 3.8 |
| 68 `DEFINEBITMAP_PNG` [png] | 32 | 32 | 0 | 3.81 | 22 | 15.7 |
| 71 `DEFINEBITMAP_JPEG8BPP` [jpeg] | 8 | 8 | 0 | 2.45 | 0 | 99 (≈ 20 decode + palette snap) |

24 of the 59 files carry bitmap definitions (44 records; the importer
deduplicates them to 38 resources). No tag 60, 62–66 or 69 occurs, so the
headerless-DIB and BMPZIP paths are exercised only by synthetic tests. The
test also asserts that `probe` and `decode` agree on dimensions and that
every output pixel is premultiplied (channel ≤ alpha).

## Decisions taken (and why)

- **`image` 0.25.10 as the façade, default features off**, with exactly
  `png`, `jpeg`, `webp`, `gif`, `tiff`, `bmp`, `pnm` (`research/05 §6`).
  JPEG goes through `image`'s zune-jpeg 0.5 backend — the decoder
  `research/05` picks — so zune is not declared twice. `image` was already in
  the graph via `arboard`; enabling formats unifies into that build too.
  New crates in the graph: `gif` 0.14 (a second `gif`; resvg in `xtask` pins
  0.13), `image-webp`, `zune-jpeg`/`zune-core` 0.5, `color_quant`, `weezl`,
  `fax`, `quick-error`. All MIT / Apache-2.0 / Zlib / BSD / Unlicense
  families; `cargo deny` passes with no new exception.
- **No `kamadak-exif`.** The decoders already return the raw EXIF block and
  `image::metadata::Orientation::from_exif_chunk` parses the one tag import
  needs. The raw block is retained in `DecodedImage::exif` for re-emission.
  Revisit only if phase 11 needs to *edit* EXIF.
- **No `jpeg-decoder` fallback.** zune decoded every corpus JPEG, and a
  second JPEG decoder doubles the attack surface. Add it only on a real file
  zune rejects.
- **Output is premultiplied RGBA8** in non-linear sRGB, like the renderer's
  surfaces. `premultiply` is exactly `round(c·a/255)`; `unpremultiply`
  returns the straight value that premultiplies back to the same byte (both
  exhaustively tested). `BitmapData::to_straight_rgba8` exists because
  `xarast_render::ImageRef` takes **straight** RGBA today; it is exact for
  opaque pixels.
- **Content hash = BLAKE3** over a domain tag, width, height and the
  premultiplied, orientation-normalised pixels
  (`BitmapData::content_hash`). The same picture in PNG and TIFF, or stored
  with any of the 8 EXIF orientations, hashes the same (tested).
- **The JPEG probe does not use `image`**: its JPEG decoder copies the whole
  file before reading a header (≈ 490 µs on a 7.5 MB file, over the 200 µs
  budget). `probe` walks the segments before the first scan itself (SOF
  dimensions, JFIF density, APP1 EXIF, APP2 ICC chunks): 67 ns.
- **Resolution**: PNG `pHYs`, JPEG JFIF density, BMP/DIB pels-per-metre;
  otherwise `DEFAULT_DPI` = 96. `recommended_width` = width × 72 000 / dpi
  millipoints. TIFF/WebP/GIF resolution is not read yet.
- **Colour space**: embedded ICC → `Icc { hash }` (BLAKE3 of the profile,
  bytes in `DecodedImage::icc`); PNG `sRGB` → `Srgb`; grey PNG with `gAMA` →
  `Grey { gamma }`; else `AssumedSrgb`. Anything but the two sRGB variants
  adds `DecodeWarning::ColourSpaceNotConverted` from `to_working_space`, the
  **single** place phase 15 converts.
- **`BitmapInfo::depth` is the source depth** (PNG IHDR bits × channels,
  JPEG components × 8, DIB `biBitCount`, GIF 8), not the stored 32 bpp.

## Decode limits as shipped (`DecodeLimits::default()`)

| Limit | Default | Error | Overridable |
|---|---|---|---|
| `max_dimension` | 65 535 | `TooLarge { limit: Dimension }` | yes |
| `max_pixels` | 256 Mi px | `TooLarge { limit: Pixels }` | yes |
| `max_decoded_bytes` (w·h·4) | 1 GiB | `TooLarge { limit: DecodedBytes }` | yes |
| `refuse_ratio` (native decoded ÷ encoded) | 20 000 | `SuspiciousRatio` | **never** |
| `warn_ratio` | 2 000 | `DecodeWarning::HighRatio` | — |
| `ratio_floor_bytes` | 16 MiB | ratio ignored below it | — |
| `max_jpeg_scans` (SOS count) | 128 | `ExcessiveWork` | **never** |
| `max_alloc_factor` | 1.25 × native + 1 MiB, via `image::Limits::max_alloc` | `AllocationRefused` | **never** |
| `max_duration` | 20 s | `Timeout` | **never** |
| `max_frames` | 1 | first frame only | — |

`DecodeLimits::tight()` (fuzzing, untrusted previews): 4096², 16 Mpx,
64 MiB, 2 s.

Order, which is the security property: sniff → header → size check →
ratio → JPEG scan count → set the allocation ceiling → allocate
(`try_reserve_exact`) → decode → deadline check → convert → orient.

- **Ratio uses the decoder's native size** (`total_bytes()`), not the RGBA
  output: a 1-bit PNG legitimately expands 32× on conversion, and would
  otherwise trip the ratio at ~1 000:1 deflate.
- **Wall clock, two layers.** Cooperative: every decoder reads through a
  `DeadlineReader` that fails with `TimedOut` after the deadline, and
  `decode_as` turns *any* result that arrives late into `Timeout` (a result
  is never returned half-trusted). Hard: `decode_on_worker` runs the decode
  on a thread and returns `Timeout` when `recv_timeout` expires; the
  abandoned thread finishes under the same memory limits. JPEG decodes in one
  call after reading everything, so for JPEG the scan cap is the real time
  guard.
- `image`'s own dimension limit is **off** at header time so that
  `check_size` can report the declared dimensions in `TooLarge`.

## Bomb fixtures (`tests/bombs.rs`; this list only grows)

All generated in the test, none on disk. Each is refused in < 1 s (measured
µs), and the whole suite grows peak RSS by < 64 MiB (measured 32 MiB, most
of it the BMPZIP inflate) — measured by resetting `VmHWM` through
`/proc/self/clear_refs` (Linux; skipped elsewhere). Tests in that file take a
mutex so their RSS figures are not polluted by each other.

| Fixture | Targets | Refused as |
|---|---|---|
| PNG declaring 65 535² | declared-size bomb | `TooLarge(Pixels)` |
| PNG 16 000² grey with a 5 KB IDAT | zlib/declared-size ratio (238 805:1) | `SuspiciousRatio` |
| JPEG with 5 000 empty scans | progressive time bomb | `ExcessiveWork` |
| GIF with a 65 535² logical screen | huge screen | `TooLarge(Pixels)` |
| WebP whose RIFF and VP8L headers disagree | inconsistent header | `Corrupt` |
| WebP VP8X declaring 16 777 216² | huge canvas | `Corrupt` (image-webp refuses) |
| TIFF declaring 65 535² | huge strips | `TooLarge(Pixels)` |
| BMP declaring 60 000² at 32 bpp | huge DIB | `TooLarge(Pixels)` |
| PPM declaring 4 000 000 000 × 1 | huge dimension | `TooLarge(Dimension)` |
| Truncated PNG | early EOF | `Corrupt` |
| BMPZIP inflating 64 MiB under a 16 MiB limit | inflate bomb | `SuspiciousRatio` |

Plus: a zero `max_duration` gives `Timeout`, never pixels; `decode_on_worker`
returns `Timeout` within 500 ms on a 1 ms ceiling; raising `max_pixels` turns
a `TooLarge` into a successful decode ("import anyway").

## The `.xar` wrappings (`xar.rs`; facts in `research/01 §4.5`)

- Sniff first: a recognisable file decodes as itself whatever the tag says.
- Tag 65: headerless DIB → synthesise `BITMAPFILEHEADER` (`dib_to_bmp`).
- Tag 69: zlib or raw DEFLATE, inflated with a cap of
  `max_decoded_bytes + 1 MiB`, then sniff or DIB.
- Tag 71: reconstruct only when the result is an **opaque JPEG** and the
  palette has 1–256 entries (as the original); nearest squared-RGB entry,
  lowest index on ties, no dithering, a 64 Ki-entry exact-key cache. Sets
  `depth = 8`, `palette_entries = n`.
- Tag 68: a PNG of colour type 4 (grey + alpha) or 6 (RGBA) stores
  **transparency** in its alpha channel, 0 = opaque (facts and
  `file:line` in `research/01 §4.5`). `decode_xar_bitmap(68, …)` flips the
  alpha samples in the decoder's native buffer (bitwise NOT, so 8 and 16
  bit alike) **before** premultiplying — flipping after would have
  premultiplied the colour of every really-opaque pixel by zero.
  `normalise_xar_png` does the same flip and re-encodes with `image`'s PNG
  encoder at the same depth and colour type (lossless, never interlaced),
  then splices the original's ancillary chunks (`pHYs`, `iCCP`, `sRGB`,
  text…) around the new `IHDR`/`IDAT` in their original order. Palette
  PNGs are left alone: the original writes ≤ 8 bpp with a single
  transparent index, which means what it says. `png_alpha_is_transparency`
  is the IHDR test (colour type byte at offset 25).
- 60–64 (previews): plain files.

## Invariants that must not be broken

1. No pixel buffer is allocated before `check_size`, the ratio and the scan
   count have passed. The two big allocations use `try_reserve_exact`.
2. A `DecodeError` means no pixels; a late result is `Timeout`.
3. Output: `width·height·4` bytes, every channel ≤ alpha, dimensions after
   orientation. Asserted by `fuzz_image_decode` and the corpus test.
4. `probe(b).info == decode(b).info` except `has_alpha` (probe reports the
   layout's alpha channel, decode whether any pixel is translucent).
5. Only `TooLarge` is overridable.
6. Only `to_working_space` may convert colour spaces.

## Fuzzing

`fuzz/fuzz_targets/fuzz_image_decode.rs`: first byte picks the façade
(`probe` + `decode`) or the `.xar` wrapper (tag selector, palette length,
palette, image). `DecodeLimits::tight()`. Seeds come from
`cargo run -p xarast-image --example fuzz_seeds -- <dir>` (12 synthetic
seeds: every format plus DIB, BMPZIP and JPEG8BPP); CI generates them, none
are committed. Nightly in `.github/workflows/fuzz.yml`.

- 2026-09-23, first run: **one finding** after 1.14 M executions — a
  translucent WebP under tag 71 was palette-snapped, leaving colour above
  alpha. Fixed (reconstruct only an opaque JPEG; `snap_to_palette` now
  matches on the straight colour and premultiplies back) with the regression
  test `jpeg8bpp_leaves_a_translucent_non_jpeg_alone`.
- Second run after the fix, 601 s on the grown corpus: **4 694 765
  executions, clean** (7 811 exec/s, peak RSS 658 MB with
  `-malloc_limit_mb=1024`, slowest unit < 1 s, 14 523 new units).

## Walker integration (as built, XARA-T-0129)

`SceneWalker::register_images` (`xarast-app/src/walker.rs`), run once per
frame before the walk:

1. A resource whose `pixels` are `w·h·4` bytes is registered as is (the
   native path). One with empty `pixels` and an `original` is decoded.
2. Decoding (`decode_resource`), `DecodeLimits::default()`:
   `Jpeg` **with a palette** → `decode_xar_bitmap(71, …, palette)`;
   `Png | Jpeg | Gif` → `decode`; `Bmp` → `decode_xar_bitmap(65)`;
   `Unknown` (the importer's BMPZIP) → `decode_xar_bitmap(69)`. PNGs need
   no tag-68 handling here: the importer already normalised them.
3. Registered as `ImageRef::new(d.width, d.height, d.to_straight_rgba8())`
   — straight alpha, decoded dimensions (the importer leaves `info` zeroed).
4. All pending resources of a frame decode together on scoped threads
   (`decode_all`, at most `available_parallelism`, work-stealing by an
   atomic index), results applied in input order so registration — and
   the scene — stay deterministic. A panicking decoder is a failure.
5. Failures go into `failed: HashSet<BitmapId>` (cleared by `reset`) and
   are never retried by that walker. Objects referring to them count in
   `WalkStats::images_failed`; objects whose bitmap has neither pixels nor
   bytes count in `images_pending`. A bitmap **transparency** whose image
   is missing counts too (it composites opaque: a shadow turns into a
   black box). `is_complete` requires both to be 0.
6. Cost (release, this machine): the per-file first-walk increase is
   ≤ 38 ms (leafgirl, a 649×430 JPEG8BPP snap; scope3 27 ms); every other
   bitmap file ≤ 8 ms. Open-to-first-paint in the real window is unchanged
   within noise (≈ 330–370 ms for the bitmap files, both before and after;
   `perf.md`). Moving decode off the frame path entirely is T10.5.5.
7. `build_scene(&Session)` makes a fresh walker, so it re-decodes every
   call (thumbnails, tests, export). Fine for the corpus; a per-document
   decoded-image cache belongs with T10.5.5.

Bitmap **fills** render too, and tile by the fill-mapping attribute
(XARA-T-0054, `paint.rs::bitmap_repeat`): the fill's own `tiling` wins
when set (a `.xarast` fill may carry one), otherwise the attribute, and
unset means repeat — the original's default. `Fill Types simple.xar`'s
bitmap row shows single, repeating and mirrored tiles, contone and
duotone.

`.xarast` round trip: bitmaps survive — the package stores the
(normalised) original bytes and, for tag 71, the snap palette as a blob
(`xarast:palette`, XARA-T-0154). The render round trip is pixel-identical
for 59/59; the writer gaps bitmaps first exposed (palette, contone,
fill mapping, transparency image) are all closed. Bitmap fills and
transparencies used to be drawn upside down (XARA-T-0171, found against
resvg): a fill's start point is the image's **bottom**-left corner, while
the renderer samples decoded (top-down) rows from `v = 0`.
`paint::bitmap_frame` now starts the frame at the top edge; facts,
`file:line` and SSIM in `render.md`, "Bitmap fill orientation". Contone
and duotone tiles and the mirrored mapping are upright too.

### Resampling (XARA-US-0052, 2026-09-24)

How a decoded bitmap is *sampled* is the renderer's
(`xarast-render/src/resample.rs`; decisions, harness numbers and the
kernel choice in `render.md`, "Resampling quality"). What matters on this
side:

- `ImageRef` keeps **straight** RGBA8, as registered by the walker; the
  sampler premultiplies (and linearises, for minification) itself, so there is no reason to
  hand it premultiplied data.
- The **mip pyramid** (+⅓ of the image's bytes) is shared by every clone
  of the `ImageRef`. Since XARA-US-0053 the walker builds it on its decode
  threads (`ImageRef::prepare`, right after the decode), so the render
  thread no longer pays 26 ms per 2048² on the first minified frame; an
  image nobody prepared still builds it lazily on first minification.
- Contone (duotone) is applied per texel through a 256-entry table, before
  filtering, as the original does in its sampler; bitmap transparencies
  are filtered like colour fills (`TranspSource::Image` has a `Filter`).
- Not wired: the document's smoothing flag (XARA-T-0273).

### Editing bitmap fills and placing bitmaps (XARA-T-0271, XARA-T-0272)

Decisions in `tools.md` 64–69; what matters on this side:

- **A placed image is decoded once, at placement**
  (`xarast-app/src/place.rs`), under `DecodeLimits::default()`, only to
  refuse what is not an image and to learn its size and resolution. The
  decoded pixels are thrown away: the document keeps the encoded bytes
  (`pixels` empty), and the walker decodes them like any opened file's.
- **Stored bytes**: PNG, JPEG and GIF as they arrived (the formats the
  document's `ImageFormat` and the walker's `decode_resource` know);
  WebP, TIFF, BMP and PNM are converted once to a lossless 8-bit RGBA PNG
  through `xarast_io::png::encode_png`, keeping the resolution in `pHYs`
  (no `xarast-image` encoder exists yet, T10.2.5). A clipboard picture
  (straight RGBA from `arboard`) becomes a PNG with no `pHYs`.
- **Natural size** = `pixels × 72 000 / dpi` per axis
  (`xarast_doc::bitmap_fill::natural_length`, the same rule as
  `BitmapInfo::natural_width`); no resolution means 96 dpi. The document's
  `BitmapInfo` of a placed image is filled in (size, 32 bpp, dpi), unlike
  the `.xar` importer's; `place::bitmap_pixels` falls back to a header
  probe of the bytes when it is empty.
- The resource is added outside the undo history and deduplicated by
  content; `collect_unused` keeps it while a redo step holds its object.
- The object gets the original's default bitmap attributes — no line
  colour, no fill colour, zero line width
  (`xarast_doc::default_bitmap_attrs`).
- Not done: EXIF orientation is applied by every decode of the stored
  JPEG (the walker's included), so a rotated JPEG places upright but its
  natural size is read from the oriented layout — consistent, but not
  checked against a real rotated file.

### Pixel memory budget (XARA-US-0053, 2026-09-24)

The budget itself lives in `xarast-render` (`pixel_budget.rs`,
`spill.rs`; design, proofs and numbers in `render.md`, "Pixel memory
budget", and the proxy rule in `perf.md`) because the levels it budgets
are `ImageRef`'s. This crate is unchanged. The walker's side
(`make_image`, `Encoded` in `xarast-app/src/walker.rs`):

- Every registered image gets a `PixelSource`. **Native pixels**
  (`w·h·4` bytes in the document): a *cheap* source copying the
  document's `Arc<[u8]>`, so an evicted base is dropped and copied back,
  never spilled. **Encoded originals**: an *expensive* source that runs
  the same decode again (`Encoded::decode`: tag 71 with the palette, 65,
  69, or the façade) and checks the dimensions; the budget spills such a
  base on its first eviction and reads the spill file back after that.
  Both re-produce the registered bytes exactly: every decoder here is
  deterministic (tested over the 24 corpus files with bitmaps, byte for
  byte, spilling and re-decoding).
- Native and encoded resources now go through the same worker pool
  (`decode_all`), which also calls `prepare`; results are still applied
  in input order, so registration is deterministic.
- `SceneWalker::with_pixel_budget` registers under a budget other than
  the process-wide one; `headless::render_with_walker` renders with such
  a walker.
- The document's own bytes (`BitmapResource::pixels`, `original`) are
  **not** in the budget; only the renderer's decoded levels are.

## Dead ends (do not retry)

- Inverting a tag-68 PNG's alpha **after** `decode` (on premultiplied
  output): every pixel the file marks 0 (= opaque) has already had its
  colour multiplied by zero. The flip must happen on the native buffer.

- Probing JPEG through `image::codecs::jpeg::JpegDecoder`: copies the whole
  input first; 7× over the probe budget on a 24 Mpx photograph.
- Counting JPEG scans in `probe`: linear in the file (≈ 3 ms on 7.5 MB);
  only `decode` counts them.
- A per-pixel full palette search for tag 71: 358 ms on the corpus's
  2.45 Mpx; the exact-key cache brings it to ≈ 80 ms.

## Open TODOs

- Reconcile `xarast-doc::BitmapResource` (SHA-256 over pixels + original)
  with this crate's (BLAKE3 over pixels) — T10.1.2, XARA-T-0156. It did not
  block the walker, which never stores decoded pixels in the document.
- Palette PNGs under tag 68 whose `tRNS` has several non-opaque entries:
  the original expands them to RGBA and so reads their alpha inverted too;
  we leave palette PNGs alone. No corpus file has one, and the original's
  writer never produces one.
- Encoders for resource storage (T10.2.5).
- `build_scene` makes a fresh walker, so it re-decodes (and now
  re-prepares) every bitmap per call; a per-document decoded-image cache
  is part of XARA-T-0281 (T10.5.5).
- TIFF/WebP/GIF resolution; PNG `iCCP`-vs-`sRGB` precedence when both exist.
- A faster tag-71 snap (k-d tree or a 32³ pre-quantised grid) if a real
  document is dominated by it.
- `DecodeLimits::max_frames` is informational: every decoder already takes
  the first frame only.
