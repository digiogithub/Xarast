# Phase 10 — Bitmaps and photo

> After this phase a photograph is a first-class document resource: imported
> once, stored once no matter how many times it is placed, rendered at the
> right quality for the zoom, adjusted without ever touching the original
> pixels, and incapable of taking the application down by being large.

## Goal

Build `xarast-image` and wire it into the document, the renderer and the UI:

- a **bitmap resource model** with content-addressed deduplication, shared by
  placed bitmaps, bitmap fills, bitmap transparency and layer backgrounds;
- **decode and encode** paths for the formats that matter, with the original
  encoded bytes retained so a JPEG never gets recompressed;
- **bitmap fills** with their on-canvas handles, tiling modes and contone;
- **quality-controlled resampling** tied to the renderer's Draft/Final split;
- a **bitmap gallery**, import, drag-and-drop placement;
- a **bounded memory budget** with out-of-core handling for images larger than
  the budget;
- **EXIF orientation** applied at import, and **decompression-bomb guards**
  treated as the security feature they are;
- the **non-destructive photo pipeline**: a chain of parameterised operations
  over a preserved master, with a small op set implemented now and the rest
  deferred to v0.3.

## Scope

### In scope

**Resource model (`xarast-doc`, `xarast-image`)**

- `BitmapResource` per `research/02 §10.12`: name, `BitmapInfo`, `Arc<BitmapData>`
  pixels (copy-on-write), the **original encoded bytes** when there are any,
  procedural provenance, palette transparency index, and a greyscale cache.
- **Deduplication by content hash** at import (`BLAKE3`, 32 bytes) — the
  replacement for `KernelBitmap::TryAndUseExistingBitmap`. Importing the same
  file twice, or pasting a bitmap between documents, yields one resource.
- **Usage is a sweep, not a refcount.** `collect_unused` walks the reachable
  tree plus the nodes retained by the history, and runs on save and on history
  prune — never on every edit. This is what replaces the original's
  `KernelBitmapRef::RemoveFromTree`/`AddtoTree` dance.
- `BitmapInfo`: pixel dimensions, depth, palette entry count, recommended
  width in millipoints, horizontal and vertical DPI, and — new, reserved from
  day one per `research/05 §6` — the **colour space**.

**Decode and encode**

- Decode: **PNG** (`png`, incl. 16-bit and APNG first frame), **JPEG**
  (`zune-jpeg`, with `jpeg-decoder` as the fallback for progressive oddities),
  **WebP** (`image-webp`, lossy + lossless + animated first frame), **GIF**
  (first frame; sequences deferred), **TIFF**, **BMP/DIB**, **PNM**.
- Encode for resource storage in `.xarast`: PNG (lossless, alpha), JPEG
  (when the original was JPEG — see below), WebP lossless.
- **Embedded JPEG is never recompressed.** `BitmapResource::original` keeps the
  source bytes; writing the document re-emits them verbatim. This is
  `KernelBitmap::IsLossy`/`SetAsLossy` (`bitmap.h:561`, `:575`) done properly.
  It also means a JPEG imported and saved ten times is bit-identical to the
  original.
- The `.xar` **8-bpp-JPEG quirk**: the legacy format stores an 8-bpp JPEG as a
  24-bpp JPEG plus a separate palette record, reconstructing the 8-bpp image on
  import (`research/02 §8.5`). Phase 3 parses the records; phase 10 is where the
  reconstruction is verified and the palette is retained.
- **EXIF**: read with `kamadak-exif`; apply the orientation tag at import so
  that everything downstream sees upright pixels; retain the original
  orientation value and the rest of the EXIF block as document metadata so it
  can be re-emitted.

**Rendering and fills (`xarast-render`)**

- `FillGeometry::Bitmap` becomes editable: origin / x-axis / y-axis handles,
  the four repeat modes (`Simple`, `Repeat`, `RepeatInverted`, and the
  high-quality repeat variant), DPI, and **contone** (recolouring a greyscale
  bitmap between two colours, with the `FillEffect` choosing RGB / HSV-short /
  HSV-long interpolation).
- `FillGeometry::Bitmap` for **transparency** as well as colour — the same
  geometry with an 8-bit payload.
- `NodeBitmap` as a parallelogram carrying a bitmap fill, exactly as the
  original does (`research/02 §8.6`): a placed bitmap **is** a rectangle whose
  fill references the resource. There is no second code path.
- The fast path for axis-aligned, unrotated, unscaled placement
  (`HasSimpleOrientation`): blit instead of sampling.
- **Resampling quality** tied to the renderer's Draft/Final levels
  (`research/03 §3.5`):
  - `Draft` → nearest neighbour;
  - `Final`, magnification → bilinear (or the chosen high-quality kernel);
  - `Final`, minification beyond 2× → prefiltered (mip pyramid) then
    high-quality kernel, to avoid aliasing;
  - export and print → always `Final`.
  The exact high-quality kernel is chosen in this phase (see W10.4).
- `image-rendering` intent per document (`TAG_DOCUMENTBITMAPSMOOTHING` 4116) and
  per bitmap.

**Memory and out-of-core**

- A global **pixel budget** for decoded bitmap data, configurable, defaulting to
  a fraction of physical RAM with a hard cap.
- Images that exceed a threshold are kept as a **mip pyramid with the base level
  evictable**: the screen-resolution proxy stays resident, the full-resolution
  base is re-decoded from `original` bytes (or from a spill file) on demand.
- LRU eviction weighted by decode cost, sharing the policy and the
  instrumentation with the renderer's node cache.

**Guards (security)**

- Header-only inspection before allocating: reject or refuse-to-decode when
  declared dimensions, total pixel count, or estimated decoded size exceed
  limits.
- A hard wall-clock and allocation ceiling around every decode, enforced even
  when the decoder does not offer limits itself.
- Every decoder runs with the crate's own limit API set where one exists.
- Fuzzed.

**UI (`xarast-ui`, `xarast-app`)**

- **Bitmap gallery** (`F11`): every resource in the document with a thumbnail,
  dimensions, depth, DPI, memory used, how many objects use it, plus delete
  (unused only), replace, save-a-copy, and drag-to-canvas to place.
- **Import**: file dialog, drag a file onto the canvas, paste from clipboard.
  Placement at the natural size derived from the DPI and
  `RecommendedWidth`; modifier keys to place at 1:1 pixels or to fit the page.
- **Transform with quality control**: the selector's scale/rotate/skew of a
  placed bitmap renders `Draft` during the drag and `Final` on release.
- Thumbnails generated off the main thread, cached by resource hash.

**Non-destructive photo pipeline — what is in this phase**

The *representation* is complete in this phase, because the file format has to
round-trip it (`research/06 §6.9`): a placed bitmap may carry a
`PhotoOps` chain referring to a **master** resource, with the rendered result
being a derived image that may or may not be materialised.

The *implemented operation set* for v0.1 is deliberately small:

| Op | In v0.1 (this phase) | Notes |
|---|---|---|
| `crop` | ✅ | rectangle in master pixel coordinates |
| `orient` | ✅ | 90/180/270 rotation and flips, lossless for JPEG where possible |
| `brightness` / `contrast` | ✅ | the two `GBitmap_Set*` controls Xara exposed |
| `gamma` | ✅ | |
| `saturation` | ✅ | |
| `greyscale` | ✅ | feeds contone fills |
| `levels` (input/output range per channel) | ✅ | `GBitmap_SetInputRange`/`SetOutputRange` |
| `unsharp` / `blur` / `sharpen` | ❌ → **v0.3** | needs the convolution machinery phase 13 builds for shadows and feather |
| `curves` | ❌ → **v0.3** | |
| `colour-depth reduction` with dithering | ❌ → **v0.3** | only Floyd–Steinberg and none, per `research/03 §3.9` M8 |
| named "special effects" (emboss, etc.) | ❌ → **v0.3 or never** | P3 |

Unknown op kinds read from a file are **preserved verbatim** and passed through
on save, per `research/06 §8`. An unknown op makes the chain non-editable in the
UI but does not make the document unopenable, and does not get dropped.

### Explicitly out of scope (and which phase owns it)

| Not in this phase | Owner |
|---|---|
| **Fractal and noise fills** (which are bitmaps generated procedurally) | **Phase 13** — the `ProceduralSource` slot exists in `BitmapResource` from this phase, unpopulated |
| **Bitmap tracing / auto-trace** | Post-v1.0 (P2 in `research/04 §1.8`, very high complexity) |
| **Photoshop-style bitmap plug-ins, the XPE bridge** | Never (P3) |
| **Animated bitmap sequences, animated GIF authoring, the frame gallery** | Post-v1.0 (P3) |
| **Blur, sharpen, emboss and the rest of the effect menu** | **Phase 13** (they share the convolution machinery with shadow and feather) |
| **Live effects applied to vector objects** | **Phase 13** |
| **Full ICC colour management** (profile-aware conversion, soft proofing) | **Phase 15**. This phase *reserves the slot*: every `BitmapResource` stores its colour space, and any embedded ICC profile is retained as a resource. Nothing converts yet, and nothing may assume sRGB at storage time |
| **Bitmap export options** (PNG depth/palette, JPEG quality dialog) | **Phase 11** |
| **Masks and clipping by bitmap alpha as an editing feature** | **Phase 13**; bitmap *transparency fills* are in this phase |
| **HDR / >8-bit compositing** | Not planned. Decode 16-bit sources, composite at 8-bit sRGB per `research/03 §2.10`, keep the 16-bit data only where an op needs it |
| **AVIF and HEIF decode** | Optional feature, not a gate. `ravif` is chosen for AVIF *export* in phase 11 |

## Prerequisites

| Needs | From | Specifically |
|---|---|---|
| Resource table with `BitmapId` and `DocumentResources` | Phase 2 | `research/02 §10.12` |
| `FillGeometry::Bitmap` variant and the attribute plumbing | Phase 2 / 4 | `research/02 §10.7` |
| Image paint in the renderer (`Paint::Image` with mapping, repeat, filter, contone, adjust) | Phase 4 | `research/03 §3.6` |
| Node render cache with cost-weighted LRU | Phase 4 | `research/03 §3.5` |
| Draft/Final quality split | Phase 4 | `research/03 §3.5` |
| `.xar` bitmap tags parsed (`TAG_DEFINEBITMAP_*` 65-71, 4138; `TAG_NODE_BITMAP` 198; `TAG_NODE_CONTONEDBITMAP` 199; `TAG_BITMAP_PROPERTIES` 4115; `TAG_XPE_*` 4117/4118) | Phase 3 | |
| `.xarast` `resources/images/`, `resources/derived/`, manifest digests | Phase 6 | `research/06 §3.2`, `§4.4` |
| Gallery panel infrastructure, drag-and-drop framework | Phase 7 / 8 | the DnD framework is phase 8's W8.7 |
| On-canvas handle overlay | Phase 8 | bitmap fill handles reuse it exactly |

Phase 10 runs **in parallel with phase 9** (roadmap). It depends on phase 8's
handle overlay and DnD framework for the bitmap-fill parts; sequence the
workstreams so W10.1–W10.3 (which do not need them) start first.

## Workstreams

### W10.1 — Resource model and deduplication

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T10.1.1 | `BitmapResource`, `BitmapInfo` (with colour space), `BitmapData`, `OriginalEncoded` | `xarast-image` | M | — |
| T10.1.2 | Content hashing (BLAKE3 over the **decoded, orientation-normalised** pixels) and the `bitmap_by_hash` index | `xarast-doc` | S | T10.1.1 |
| T10.1.3 | `collect_unused` sweep over reachable tree + retained history | `xarast-doc` | M | T10.1.1 |
| T10.1.4 | Commands: `ImportBitmap`, `ReplaceBitmap`, `DeleteBitmap`, `RenameBitmap` | `xarast-doc` | M | T10.1.2 |
| T10.1.5 | Cross-document paste sharing one resource | `xarast-app` | M | T10.1.2 |
| T10.1.6 | `.xarast` resource round-trip incl. `original` bytes and `mf:digest` | `xarast-format` | M | T10.1.1 |

**Hash the decoded pixels, not the file bytes.** Two different JPEG files can
decode to the same image (re-saved at the same quality with different metadata),
and the same file bytes can appear with different EXIF orientation. Hashing
decoded, orientation-normalised pixels means "the same picture" deduplicates.
The trade is that deduplication costs a decode — which import does anyway.
Keep a secondary index on the *file* hash so that re-importing the identical
file short-circuits before decoding at all.

**`original` and dedup interact.** If two resources dedup to one and only one of
them had `original` bytes, keep the bytes. If both had different `original`
bytes that decode identically, keep the smaller. Record this rule; it is the
kind of thing that silently bloats files.

### W10.2 — Decode, encode, EXIF and guards

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T10.2.1 | `Decoder` façade: sniff format, read header, decode with limits | `xarast-image` | M | T10.1.1 |
| T10.2.2 | Per-format decoders wired: PNG, JPEG (zune + fallback), WebP, GIF, TIFF, BMP, PNM | `xarast-image` | M | T10.2.1 |
| T10.2.3 | **Guards**: dimension, pixel-count, decoded-size, wall-clock and allocation ceilings | `xarast-image` | M | T10.2.1 |
| T10.2.4 | EXIF read, orientation normalisation, metadata retention | `xarast-image` | M | T10.2.1 |
| T10.2.5 | Encoders for resource storage: PNG, WebP lossless, JPEG passthrough | `xarast-image` | S | T10.2.1 |
| T10.2.6 | The `.xar` 8-bpp-JPEG + palette reconstruction, verified against corpus | `xarast-xar` | M | T10.2.2 |
| T10.2.7 | `cargo-fuzz` targets per decoder entry point | `xarast-image` | M | T10.2.3 |

**Decompression bombs are a security issue, and the guard has to run before the
allocation, not after.** A 200-byte PNG can declare 65,535 × 65,535 pixels;
decoding it allocates 17 GB. A 4-byte-per-pixel guard on a declared size is
therefore step one, and it must come from the **header**, before a single row is
decoded. The limits this phase sets (all configurable, these are the defaults):

| Limit | Default | Rationale |
|---|---|---|
| Max width or height | 65,535 px | format maximum for most; beyond it nothing legitimate |
| Max total pixels | 256 Mpx | a 16,000 × 16,000 image; larger needs explicit opt-in |
| Max decoded bytes | 1 GiB | 256 Mpx × 4 bytes |
| Max decode wall clock | 20 s | a progressive JPEG bomb takes time, not memory |
| Max allocation during decode | 1.25 × the estimate | catches decoders that allocate scratch beyond the image |
| Max GIF/APNG frames considered | 1 | we take the first frame; sequences are out of scope |
| Max compression ratio (decoded ÷ encoded) | 2,000:1 warn, 20,000:1 refuse | catches the classic PNG/zlib bomb |

Exceeding a limit is a **typed error the UI reports**, offering "import anyway"
only for the pixel-count and decoded-bytes limits, never for the ratio or
allocation limits. A file that trips a guard must never be partially imported
and must never leave a half-decoded buffer behind.

**EXIF orientation is applied once, at import.** Store upright pixels; store the
original orientation value in the resource so export can re-emit it if the user
asks. Do not carry orientation as a render-time transform — that way lies a
bitmap fill whose tiles are rotated and whose handles are not.

### W10.3 — Bitmap fills, contone, and the placed bitmap node

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T10.3.1 | `FillGeometry::Bitmap` editing commands (origin, axes, tiling, DPI) | `xarast-doc` | M | phase 8 W8.2 |
| T10.3.2 | Bitmap fill handles in `fill_handles`, reusing phase 8's overlay | `xarast-app` | M | phase 8 W8.3 |
| T10.3.3 | Tiling modes: `Simple` (edge colour outside), `Repeat`, `RepeatInverted` (mirrored, seamless), high-quality repeat | `xarast-render` | M | phase 4 |
| T10.3.4 | Contone: greyscale conversion + two-colour remap with `FillEffect` interpolation space | `xarast-render` | M | T10.2.2 |
| T10.3.5 | Bitmap **transparency** fills (same geometry, 8-bit payload) | `xarast-render` | M | T10.3.3 |
| T10.3.6 | `NodeBitmap` = rectangle + bitmap fill; `ApplyDefaultBitmapAttrs` equivalent | `xarast-doc` | M | T10.3.1 |
| T10.3.7 | Simple-orientation blit fast path | `xarast-render` | M | T10.3.6 |
| T10.3.8 | Drag-to-place, paste-to-place, natural-size computation from DPI | `xarast-app` | M | T10.1.4 |

`RepeatInverted` is the one worth stating precisely because it is what makes
Xara's bitmap fills seamless: the tile is mirrored on alternate repetitions in
both axes, so tile edges always meet their own reflection. Implement it in the
sampler's coordinate wrap (`u = triangle_wave(u)`), not by building a 2×2
mirrored texture — the latter quadruples memory for every tiled bitmap.

### W10.4 — Resampling quality

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T10.4.1 | Nearest, bilinear, and a high-quality kernel; one sampler, a quality enum | `xarast-render` | M | phase 4 |
| T10.4.2 | Mip pyramid generation (box, in linear light) and level selection from the Jacobian | `xarast-render` | M | T10.4.1 |
| T10.4.3 | Comparison harness: downscale and upscale a test set with each candidate kernel, measure and eyeball | `xarast-render` | S | T10.4.1 |
| T10.4.4 | Wire the Draft/Final split and the print/export override | `xarast-render` | S | T10.4.1 |
| T10.4.5 | GPU sampler parity with the CPU sampler for every mode | `xarast-render` | M | T10.4.1 |

**The high-quality kernel is chosen here, by measurement.** The research does
not name one — CDraw simply had a "filtering flag" (`research/03 §2.8`). The
candidates and how they are judged in T10.4.3: Mitchell–Netravali
(B = C = 1/3), Catmull–Rom, and Lanczos-3. Judged on a fixed test set (a
resolution chart, a photograph with fine detail, a screenshot with hard edges,
a logo with thin strokes) at 4 magnification and 4 minification ratios, scored
on ringing around hard edges, sharpness retention, and cost per output pixel.
**Default hypothesis: Mitchell–Netravali for magnification (least ringing on
synthetic content, which is most of what a vector editor places) and box-
prefilter + bilinear for minification beyond 2× (cheapest correct answer once
a pyramid exists).** If measurement says otherwise, measurement wins; record
both the choice and the numbers in `docs/memory/render.md`.

Linear-light resampling is a deliberate exception to "everything composites in
encoded sRGB" (`research/03 §2.10`): *blending* stays in encoded sRGB because
the blend LUTs are defined there, but *resampling* is a weighted average of
light, and doing it in encoded space visibly darkens downscaled images. Convert
to linear for the filter, convert back. The same exception `research/03 §3.7`
already grants to blur and high-quality scaling.

**Outcome (XARA-US-0052, 2026-09-24).** Measured in
`crates/xarast-render/tests/resampling.rs`, numbers in
`docs/memory/render.md`, "Resampling quality": Mitchell–Netravali for
magnification, as hypothesised; the tent widened by the footprint for 1–2×
minification (trilinear lost at ÷1.5 on every test image) and trilinear
over a linear-light pyramid beyond. Minification is in linear light as
stated above, but **magnification averages in encoded sRGB**: it measured
better for every kernel, with less ringing and no light leak into dark
texels of a magnified small bitmap.

### W10.5 — Memory budget and out-of-core

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T10.5.1 | Global pixel budget, accounting, and instrumentation | `xarast-image` | M | T10.1.1 |
| T10.5.2 | Proxy generation: a screen-resolution level kept resident per resource | `xarast-image` | M | T10.4.2 |
| T10.5.3 | Evict the full-resolution base; re-materialise from `original` or from a spill file | `xarast-image` | L | T10.5.1 |
| T10.5.4 | Spill directory management, cleanup on exit and on crash recovery | `xarast-image` | M | T10.5.3 |
| T10.5.5 | Async re-materialisation so the UI never blocks on it; draw the proxy meanwhile | `xarast-app` | M | T10.5.3 |
| T10.5.6 | Budget stress test: 40 × 24 Mpx images in one document | `xarast-cli` | S | T10.5.3 |

The invariant: **the proxy is never evicted.** A document with 200 large photos
must always be able to draw *something* for each of them without touching disk.
Proxy size is bounded by the largest viewport the session has seen, rounded up
to a power of two, capped at 2048 on the long edge. Total resident proxy memory
for 200 photos at 2048² RGBA is 3.2 GB, which is too much — so the proxy level
is chosen per resource from its on-canvas size, not from the viewport, and
recomputed when a resource is scaled up. Record the final rule in
`docs/memory/perf.md`.

### W10.6 — Non-destructive photo pipeline

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T10.6.1 | `PhotoOp` enum + `PhotoOps` chain, with an `Unknown` variant carrying raw preserved data | `xarast-doc` | M | T10.1.1 |
| T10.6.2 | Evaluator: master → ops → derived image, with a cache keyed by (master hash, ops hash, target resolution) | `xarast-image` | L | T10.6.1 |
| T10.6.3 | The v0.1 op set: crop, orient, brightness, contrast, gamma, saturation, greyscale, levels | `xarast-image` | M | T10.6.2 |
| T10.6.4 | LUT fusion: collapse consecutive per-channel point ops into one 256-entry LUT per channel | `xarast-image` | M | T10.6.3 |
| T10.6.5 | Commands to add / remove / reorder / edit ops, with live preview at proxy resolution | `xarast-doc`, `xarast-app` | M | T10.6.1 |
| T10.6.6 | Materialisation policy: when to bake a derived image into `resources/derived/` | `xarast-format` | M | T10.6.2 |
| T10.6.7 | Verbatim preservation of unknown ops through load → edit → save | `xarast-format` | M | T10.6.1 |
| T10.6.8 | UI: photo panel listing the chain, per-op controls, reset, and a "non-editable: unknown operation" state | `xarast-ui` | M | T10.6.5 |

**LUT fusion is what makes this fast enough to be live.** Brightness, contrast,
gamma, levels and greyscale are all per-channel point operations. Composing
them at 8-bit means one 3 × 256 table; applying it to a 24 Mpx image is a table
lookup per byte, trivially parallel with `rayon`, and the preview at proxy
resolution is under a millisecond. Only crop and orient change geometry and they
commute to the front of the chain. Establish the canonical chain order —
geometry ops first, then the fused point LUT — and normalise on edit, so that
the ops hash is stable regardless of the order the user added them.

**Materialisation policy** (`research/06 §4.4`): store the master once; write a
derived image into the container only when regenerating it on open would be
expensive. The threshold this phase sets: materialise when
`estimated_regen_ms > 250` at the document's nominal resolution, or when the
chain contains an unknown op (we cannot regenerate what we cannot evaluate).
Otherwise store only the op chain. Record the measured constant.

### W10.7 — Bitmap gallery and import UX

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T10.7.1 | Gallery model: resources, usage counts, memory, thumbnails | `xarast-app` | M | T10.1.3 |
| T10.7.2 | Gallery panel with sort, filter and the delete-if-unused rule | `xarast-ui` | M | T10.7.1 |
| T10.7.3 | Thumbnail generation off-thread, cached by resource hash on disk | `xarast-image` | M | T10.1.2 |
| T10.7.4 | Import: file dialog via XDG portal, drop-file-on-canvas, paste from clipboard | `xarast-app`, `xarast-shell` | M | T10.2.1 |
| T10.7.5 | Progress reporting for slow imports, cancellable | `xarast-app` | M | T10.2.1 |
| T10.7.6 | Replace-bitmap keeping every placement and every fill referencing it | `xarast-doc` | M | T10.1.4 |

### W10.8 — Colour space plumbing (reserving the slot)

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T10.8.1 | `ColourSpace` on `BitmapInfo`; sniff from PNG `iCCP`/`sRGB`/`gAMA`, JPEG APP2, WebP `ICCP` | `xarast-image` | M | T10.2.1 |
| T10.8.2 | Retain embedded ICC profiles as document resources (`resources/profiles/`) | `xarast-format` | S | T10.8.1 |
| T10.8.3 | Assume-sRGB conversion with a recorded "assumed" flag when no profile is present | `xarast-image` | S | T10.8.1 |
| T10.8.4 | A single choke point where a non-sRGB bitmap would be converted, currently a no-op + warning | `xarast-image` | S | T10.8.1 |

Full CMS is phase 15. What this phase must not do is make phase 15 impossible,
and the way that happens is by assuming sRGB in fifty places instead of one.
T10.8.4 is the discipline: exactly one function, which today logs and returns
the input unchanged.

## Public API introduced

```rust
// ─────────────────────────────── xarast-image ───────────────────────────────

/// Decoded pixels. Always 8-bit premultiplied-alpha RGBA in the resource
/// store; 16-bit sources are kept alongside only when an op needs them.
pub struct BitmapData {
    pub width: u32,
    pub height: u32,
    pub pixels: Box<[u8]>,          // width * height * 4, RGBA, premultiplied
    pub deep: Option<Box<[u16]>>,   // optional 16-bit companion
}

/// == BitmapInfo (bitmpinf.h:104), plus the colour space slot.
#[derive(Clone, PartialEq, Debug)]
pub struct BitmapInfo {
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub depth: u8,                  // source depth: 1,2,4,8,16,24,32
    pub palette_entries: u16,       // 0 = no palette
    pub recommended_width: Millipoints,
    pub hdpi: u32,
    pub vdpi: u32,
    pub colour_space: ColourSpace,
    pub has_alpha: bool,
}

/// Reserved from day one (research/05 §6). Nothing converts yet; phase 15 does.
#[derive(Clone, PartialEq, Debug)]
pub enum ColourSpace {
    /// No profile found; sRGB assumed. The flag matters for phase 15.
    AssumedSrgb,
    Srgb,
    /// An ICC profile retained as a document resource.
    Icc { profile: ProfileId },
    Grey { gamma: f32 },
}

#[derive(Clone, Debug)]
pub struct OriginalEncoded { pub format: ImageFormat, pub bytes: Arc<[u8]> }

/// Limits enforced BEFORE allocation. A bomb is a security problem.
#[derive(Clone, Copy, Debug)]
pub struct DecodeLimits {
    pub max_dimension: u32,          // default 65_535
    pub max_pixels: u64,             // default 256 << 20
    pub max_decoded_bytes: u64,      // default 1 << 30
    pub max_duration: Duration,      // default 20 s
    pub max_alloc_factor: f32,       // default 1.25
    pub warn_ratio: u32,             // default 2_000
    pub refuse_ratio: u32,           // default 20_000
}

impl Default for DecodeLimits { /* the table in W10.2 */ }

#[derive(Debug)]
pub enum DecodeError {
    /// The header alone shows this cannot be decoded within the limits.
    TooLarge { declared: (u32, u32), limit: DecodeLimits },
    /// Encoded-to-decoded ratio above `refuse_ratio`. Never overridable.
    SuspiciousRatio { ratio: u32 },
    Timeout,
    AllocationRefused { wanted: u64 },
    Unsupported(ImageFormat),
    Corrupt(Box<dyn std::error::Error + Send + Sync>),
}

impl DecodeError {
    /// True only for `TooLarge`: the UI may offer "import anyway".
    pub fn is_overridable(&self) -> bool;
}

/// Header inspection without decoding. Always call this first.
pub fn probe(bytes: &[u8]) -> Result<Probe, DecodeError>;

pub struct Probe {
    pub format: ImageFormat,
    pub info: BitmapInfo,
    pub exif_orientation: Option<Orientation>,
    pub estimated_decoded_bytes: u64,
}

/// Decode with limits and EXIF orientation applied. Returns upright pixels.
pub fn decode(bytes: &[u8], limits: &DecodeLimits)
    -> Result<DecodedImage, DecodeError>;

pub struct DecodedImage {
    pub data: BitmapData,
    pub info: BitmapInfo,
    /// The orientation that was applied, so export can re-emit it.
    pub applied_orientation: Orientation,
    pub exif: Option<Arc<[u8]>>,
    pub icc: Option<Arc<[u8]>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Orientation { Normal, FlipH, Rot180, FlipV,
                       Transpose, Rot90, Transverse, Rot270 }

// ── resampling ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Resample {
    /// Draft: during a drag, a zoom, a scroll.
    Nearest,
    Bilinear,
    /// The kernel chosen in W10.4, applied over a mip level for minification.
    HighQuality,
}

/// Mip pyramid. Level 0 may be evicted; the proxy level never is.
pub struct Pyramid { /* … */ }

impl Pyramid {
    pub fn build(base: &BitmapData, proxy_max_edge: u32) -> Self;
    /// Level whose texel density best matches the requested device scale.
    pub fn level_for(&self, scale: f32) -> u8;
    pub fn proxy(&self) -> &BitmapData;
    /// `None` when the base has been evicted and must be re-materialised.
    pub fn base(&self) -> Option<&BitmapData>;
}

// ── photo pipeline ──────────────────────────────────────────────────────────

/// One non-destructive operation. `Unknown` exists so that an op kind we do
/// not implement survives load → edit → save (research/06 §8).
#[derive(Clone, PartialEq, Debug)]
pub enum PhotoOp {
    Crop { rect: PixelRect },
    Orient(Orientation),
    Brightness(f32),                 // [-1, 1]
    Contrast(f32),                   // [-1, 1]
    Gamma(f32),                      // (0, 8]
    Saturation(f32),                 // [-1, 1]
    Greyscale,
    Levels { channel: Channel, in_lo: u8, in_hi: u8, out_lo: u8, out_hi: u8 },
    /// Preserved verbatim. Renders as if absent; blocks materialisation-free
    /// storage and marks the chain non-editable.
    Unknown { kind: Arc<str>, raw: Arc<str> },
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct PhotoOps { pub ops: Vec<PhotoOp> }

impl PhotoOps {
    /// Geometry ops first, then one fused point LUT. Idempotent.
    pub fn normalise(&mut self);
    pub fn is_editable(&self) -> bool;          // false if any Unknown
    pub fn hash(&self) -> [u8; 32];
    /// Collapse the point ops into 3 × 256 entries. `None` if there are none.
    pub fn fused_lut(&self) -> Option<Arc<[[u8; 256]; 3]>>;
    pub fn estimated_cost_ms(&self, pixels: u64) -> f32;
}

/// Evaluate a chain against a master, at a target resolution. Cached.
pub fn evaluate(master: &BitmapData, ops: &PhotoOps, target: Option<(u32, u32)>)
    -> Arc<BitmapData>;

// ──────────────────────────────── xarast-doc ────────────────────────────────

pub struct BitmapResource {
    pub name: Arc<str>,
    pub info: BitmapInfo,
    pub pixels: Arc<BitmapData>,
    /// Source bytes. When present, saving re-emits them verbatim — a JPEG is
    /// never recompressed. == KernelBitmap::IsLossy / GetOriginalSource.
    pub original: Option<Arc<OriginalEncoded>>,
    /// Generated, not stored: regenerate from parameters. Populated in phase 13.
    pub procedural: Option<ProceduralSource>,
    pub transparent_index: Option<u8>,
    pub greyscale_cache: Option<Arc<BitmapData>>,
    pub pyramid: Option<Arc<Pyramid>>,
    pub content_hash: [u8; 32],
}

/// Commands.
pub struct ImportBitmap  { pub bytes: Arc<[u8]>, pub name: Arc<str>,
                           pub limits: DecodeLimits }
pub struct ReplaceBitmap { pub id: BitmapId, pub with: BitmapId }
pub struct DeleteBitmap  { pub id: BitmapId }   // fails if still used
pub struct SetPhotoOps   { pub node: NodeId, pub ops: PhotoOps }
pub struct PlaceBitmap   { pub id: BitmapId, pub at: Point, pub sizing: Sizing }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sizing { NaturalDpi, OnePixelPerPoint, FitPage, Explicit }

/// Sweep, not refcount. Runs on save and on history prune, never per edit.
pub fn collect_unused(doc: &mut Document) -> usize;

// ──────────────────────────────── xarast-app ────────────────────────────────

pub struct BitmapGalleryModel {
    pub entries: Vec<GalleryEntry>,
    pub total_bytes: u64,
    pub budget_bytes: u64,
}

pub struct GalleryEntry {
    pub id: BitmapId,
    pub name: Arc<str>,
    pub info: BitmapInfo,
    pub uses: u32,
    pub bytes_resident: u64,
    pub thumbnail: Option<TextureId>,
}
```

## Acceptance criteria

1. `cargo test -p xarast-image` passes, including a decode round-trip test per
   supported format against a fixture set.
2. **Bomb fixtures.** A directory `tests/bombs/` containing at minimum: a PNG
   declaring 65,535², a zlib-bomb PNG with a > 20,000:1 ratio, a progressive
   JPEG designed to be slow, a GIF with a huge logical screen, and a WebP with
   an inconsistent header. Every one is rejected with the right `DecodeError`
   variant, in **under 1 second**, with **peak RSS growth under 64 MiB**,
   measured by the test harness. No panics, no partial imports.
3. `cargo fuzz run decode_png` / `decode_jpeg` / `decode_webp` each run 1
   million executions in CI's nightly job with zero crashes and zero OOMs.
4. **EXIF**: an 8-image fixture set covering all 8 orientation values decodes to
   pixel-identical upright output (all 8 images compare equal to each other
   after normalisation).
5. **Deduplication**: importing the same photograph twice, and importing it once
   as JPEG and once as a PNG re-encode of the same pixels, both yield exactly
   one `BitmapResource`. Asserted on `DocumentResources::bitmaps.len()`.
6. **JPEG passthrough**: a document containing an imported JPEG, saved and
   reloaded ten times, contains a `resources/images/` entry **byte-identical**
   to the original file. Asserted by unzipping and comparing with `cmp`.
7. `xarast-cli render --backend cpu testfiles/TestBitmapFill.xar` matches its
   golden image within the phase-3 perceptual gate, and the same for the
   bitmap-bearing files in `Designs/` (`amurdove.xar`, `leafgirl.xar`,
   `feathers.xar` — the set is confirmed by `xar-dump --tags | grep 198`).
8. **Tiling**: golden images for all four repeat modes; `RepeatInverted`
   produces no visible seam, asserted by comparing the pixel columns either
   side of a tile boundary (difference ≤ 1/255).
9. **Contone**: a greyscale bitmap remapped between two colours matches a
   reference computed in the test, for all three `FillEffect` interpolation
   spaces.
10. **Resampling choice recorded**: T10.4.3's comparison table exists in
    `docs/memory/render.md` with actual numbers, and the chosen kernel is
    implemented on both backends with bit-identical `Final` output.
11. **Memory budget**: a document with 40 × 24 Mpx images opens, renders and
    pans within the budget, with resident bytes never exceeding
    `budget_bytes × 1.1`, measured by the instrumentation and asserted.
12. **Out-of-core**: with the budget set to 256 MiB, the same document still
    pans at ≤ 16 ms per frame, drawing proxies where the base has been evicted,
    and a full-resolution re-materialisation never blocks the main thread
    (asserted: no main-thread stall > 4 ms in the trace).
13. **Photo ops**: applying brightness + contrast + gamma to a 24 Mpx image and
    reading back pixels matches a reference computed with the fused LUT; and
    the same chain applied in three different orders produces the same
    normalised `PhotoOps::hash()`.
14. **Unknown-op preservation**: a hand-written `.xarast` containing
    `<xarast:op xarast:kind="curves" …>` loads, the object is marked
    non-editable, the document is edited elsewhere and saved, and the `curves`
    op is present in the output **character for character**. This is
    `research/06 §8.7`'s conformance test applied to photo ops.
15. **Gallery**: usage counts are correct after grouping, ungrouping, deleting
    and undoing; `DeleteBitmap` on a used resource fails; `collect_unused`
    after undoing an import reclaims nothing (the history still holds it) and
    after a history prune reclaims it.
16. `cargo clippy --workspace -- -D warnings` and `cargo deny check licenses`
    pass. If `qcms` (MPL-2.0) enters the tree in T10.8.2, `deny.toml` carries a
    written justification as CLAUDE.md requires.

## Performance budgets

| Budget | Target | How measured |
|---|---|---|
| Probe (header only), any format | ≤ 200 µs | `criterion` |
| Decode a 24 Mpx baseline JPEG | ≤ 400 ms | `criterion`, `zune-jpeg` |
| Decode a 24 Mpx PNG | ≤ 600 ms | `criterion` |
| BLAKE3 hash of 100 MB of decoded pixels | ≤ 200 ms | `criterion` |
| Import → placed on canvas → first paint (24 Mpx) | ≤ 700 ms, of which ≤ 100 ms after the proxy exists | trace |
| Mip pyramid build, 24 Mpx | ≤ 250 ms, on the pool | `criterion` |
| Pan a document with 50 large bitmaps | ≤ 16 ms/frame | matches the roadmap's global budget |
| Fused photo-op LUT applied to a 2048² proxy | ≤ 8 ms, `rayon` | `criterion` |
| Fused photo-op LUT applied to 24 Mpx | ≤ 200 ms, `rayon` | `criterion` |
| Live preview latency while dragging a brightness slider | ≤ 33 ms | trace |
| Gallery thumbnail generation, 100 resources | ≤ 1.5 s total, off the main thread | trace |
| Resident bitmap memory | ≤ configured budget × 1.1 | instrumentation |

## Risks and mitigations

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| A decompression bomb reaches production | Low | **High** (it is a CVE) | Header-first guards, defaults tabled in W10.2, a fuzz target per decoder, and a bomb fixture suite that runs in every CI job — not nightly. The refuse-ratio and allocation limits are **not** user-overridable |
| Deduplication by decoded pixels makes import slow | Medium | Medium | Short-circuit on file-bytes hash first; hash on the pool concurrently with the rest of import. Budgeted above |
| The memory budget makes large-document editing feel sluggish | Medium | High | Proxy-never-evicted invariant, async re-materialisation, and the ≤ 16 ms pan budget as a hard gate. If it cannot be met, reduce the default proxy cap before reducing the budget |
| `Arc<BitmapData>` copy-on-write silently doubles memory on an edit | Medium | Medium | Photo ops never mutate the master — they produce a derived image, which is the whole point of the design. Any code path that clones a `BitmapData` is a review flag |
| Resampling in linear light contradicts the encoded-sRGB compositing rule and someone "fixes" it | Medium | Medium | Document the exception in `docs/memory/render.md` next to the rule, with the reason (resampling averages light, blending does not) and a golden image that regresses if someone changes it |
| JPEG passthrough conflicts with photo ops: the derived image is not the original | Medium | Medium | `original` belongs to the **master** resource; a derived image is a separate stored entry with its own encoding. The master's bytes are never touched |
| `image-webp` cannot encode lossy WebP (`research/05 §6`) | Certain | Low | Lossless only by default; lossy behind the optional `webp-lossy` feature using libwebp, which is **not** in the default AppImage build. Phase 11 owns the export-side decision |
| ICC gets designed in now "because it is easy" and blocks the phase | Low | Medium | T10.8.4's single choke point is the whole of ICC in this phase. Resist |
| Out-of-core spill files leak on crash | Medium | Low | Spill directory is per-process under `XDG_RUNTIME_DIR` with the pid in the name; cleanup on start removes directories whose pid is gone |

## Test plan

**Unit.** Per-format decode against fixtures with known pixel values. Probe
correctness (declared vs actual dimensions). Each guard triggered individually.
All 8 EXIF orientations. Orientation composition (`Orient` op on top of an
applied EXIF orientation). Pyramid level selection. Each photo op against a
hand-computed reference. `PhotoOps::normalise` idempotence and hash stability.
Tiling coordinate wrap for all four modes.

**Property.** Import → export → import of a lossless format returns identical
pixels. `evaluate` is deterministic. Random op chains normalise to a stable
hash regardless of insertion order. `collect_unused` never removes a reachable
resource (random tree + random history).

**Security.** The bomb fixture suite (criterion 2), run in every CI job. Fuzz
targets per decoder, run nightly with a 1-million-execution budget and a
corpus checked into the repository. A fuzz finding is a release blocker.

**Golden images.** Bitmap fills: 4 repeat modes × {axis-aligned, rotated,
skewed} × {Nearest, Bilinear, HighQuality}. Contone: 3 interpolation spaces.
Placed bitmaps at 10 %, 100 % and 800 % zoom, in Draft and Final. Photo-op
chain applied at proxy and at full resolution (must agree within 1/255).

**GPU/CPU parity.** Every bitmap golden image on both backends; `Final` bit for
bit. The sampler is the most likely place for the two backends to diverge —
this comparison is the main reason it is worth having both.

**Corpus.** Every `.xar` in `testfiles/` and `Designs/` containing a bitmap tag,
rendered and compared; `.xar` → `.xarast` → reload → render with zero differing
pixels; JPEG byte-identity after ten save cycles.

**Stress.** 40 × 24 Mpx document with the budget at 256 MiB, 1 GiB and
unlimited: open, render, pan, zoom, save, reload. Assert frame times, resident
bytes and no main-thread stalls.

## Memory note

Update **`docs/memory/render.md`** with:

- The resampling kernel chosen in T10.4.3, the comparison numbers that chose it,
  and the mip-level selection rule.
- The **linear-light resampling exception** to encoded-sRGB compositing, and why
  it is not a contradiction.
- The four tiling modes' exact sampler semantics, especially the
  `RepeatInverted` triangle-wave wrap and why it is not a 2×2 mirrored texture.
- Contone's interpolation spaces and how they map to `FillEffect`.
- The simple-orientation blit fast path's exact precondition.

Update **`docs/memory/perf.md`** with:

- The bitmap memory budget's default, how it is derived from physical RAM, and
  the measured numbers from the 40-image stress test.
- The proxy sizing rule as finally implemented (per-resource from on-canvas
  size, not from the viewport) and the cap.
- The materialisation threshold for derived images and the measurement behind
  it.

Create **`docs/memory/images.md`** from the template in `docs/memory/INDEX.md`
and add its row to that index. It must record:

- The decode limits table as shipped, and the rule that ratio and allocation
  limits are never user-overridable.
- Every bomb fixture and what it targets — this list only grows.
- The deduplication rule (hash decoded, orientation-normalised pixels; secondary
  file-bytes index) and the `original`-bytes tie-break.
- The JPEG passthrough invariant and where it is enforced.
- The v0.1 photo-op set, the canonical chain order, and the LUT fusion rule.
- The unknown-op preservation contract.
- The single ICC choke point (T10.8.4) that phase 15 will fill in.
- Dead ends: decoders tried and rejected, guard approaches that did not catch a
  fixture.
