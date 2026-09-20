# Phase 15 — v1.0

> After this phase Xarast opens the legacy files people actually still have, prints and
> exports PDF that a print shop accepts, manages colour end to end including CMYK, survives a
> week of continuous use without leaking or crashing, and has a published, final answer to
> "can I extend it?" — on three platforms, from one tag.

## Goal

Close the remaining gap between "a good editor" and "a program someone runs a business on".
Five things stand between those, and this phase does exactly those five: **wider import**,
**print and PDF fidelity**, **colour management including CMYK**, **stability proven by
measurement rather than by absence of reports**, and a **decided, documented extensibility
answer**. Everything else is the definition-of-done checklist that turns 0.x into 1.0.

## Scope

### In scope

1. **Import**: PDF, Adobe Illustrator, EPS, EMF, CorelDRAW — each with an explicit verdict
   (implement / delegate / decline) and the reasoning behind it.
2. **Print and PDF export fidelity**: a real PDF writer with gradients, transparency groups,
   soft masks, embedded font subsets and ICC output intents; printing implemented on top of it
   on all three platforms.
3. **Colour management and CMYK**: ICC profiles, document and display profiles, soft proofing,
   separations preview, spot colours, overprint.
4. **Stability**: long-run soak, extended fuzzing, a crash-free-session measurement, and the
   memory and handle-leak gates extended to cover every subsystem added since Phase 12.
5. **The plugin question, answered** (below: *no* native plugin ABI, *yes* to three narrower
   extension surfaces).
6. **Definition of done for 1.0** as a measurable checklist.
7. Linux **aarch64** AppImage, deferred here from Phase 12.

### Explicitly out of scope (and which phase owns it)

| Out of scope | Reason / owner |
|---|---|
| Writing `.xar` | **Permanent non-goal.** `docs/10-architecture.md` §3.5 and `docs/00-vision-and-scope.md` §4 |
| SWF, HTML + imagemap, image slicing, rollovers, animated GIF, web button bars | **Never.** `research/04 §7` note 4: ~15 % of the original's code, ~0 % of current value |
| CMX, Acorn Draw/Sprite, RISC OS formats, Photoshop/FreeHand/ArtWorks EPS dialects, legacy palette formats | **Declined.** `research/04 §5.1` marks them P3/discardable |
| PostScript/EPS *export* | **Declined.** Superseded by PDF (`research/04 §5.2`) |
| Prepress trapping, imposition, plate-making workflows | Beyond 1.0. Separations *preview* and overprint *attributes* are in scope; a prepress suite is not |
| Mac App Store, Windows on ARM, Windows Store | Post-1.0 (`docs/phases/phase-14-windows-and-macos.md`) |
| Real-time collaborative editing | Post-1.0 non-goal (`docs/00-vision-and-scope.md` §4) |
| A native plugin ABI | **Decided against in this phase.** See workstream E |
| Auto-trace (bitmap → vector) | Post-1.0; `research/04 §2.3` points at external tools |
| Additional languages in the UI | Post-1.0; Phase 12 shipped the framework, this phase does not add locales |

## Prerequisites

- **Phases 13 and 14 closed.** Live effects exist (they must survive PDF export and print) and
  the three platforms build and sign from one tag (print and colour management are
  per-platform work).
- Phase 11's PDF *export* exists in some form; this phase replaces or hardens it to fidelity
  standards rather than starting from nothing.
- `xarast-color` already models RGB/CMYK/HSV/grey, named colours, tints/shades/links
  (`docs/10-architecture.md` §2). This phase adds profile-aware conversion, not a new colour
  model.
- Phase 12's perf and licence gates run on every merge and continue to.

## Workstreams

### A. Wider import — the verdicts

The recommendation for each format, with the reason. These are decisions, not options.

| Format | Verdict | How |
|---|---|---|
| **PDF** (vector import) | **Implement** | A content-stream interpreter in `xarast-io` over a pure-Rust PDF object layer. PDF is the hub format: modern `.ai` is PDF, modern EPS workflows end in PDF, and "import this PDF" is a live daily request. It is also the only importer here whose cost is recovered twice, because the **exporter** needs the same model of shadings, transparency groups and soft masks |
| **Adobe Illustrator ≥ 9** (`.ai`) | **Implement — for free** | AI 9+ files are PDF with a private data section. Route them through the PDF importer and read the AI-specific bits only where they are cheap. `research/04 §5.1` says exactly this: "modern AI = PDF ⇒ via the PDF route" |
| **Adobe Illustrator ≤ 8, generic EPS** | **Delegate, optional, not bundled** | These are PostScript programs; importing them correctly means running a PostScript interpreter. Ghostscript is the only practical one and it is **AGPL**, which `docs/11-licensing-and-clean-room.md` §4 bans as a dependency. The resolution: if the user has Ghostscript installed, Xarast may **invoke it as an external process** to convert EPS → PDF, then use the PDF importer. It is never bundled, never linked, off unless the binary is found, and clearly labelled in the UI as using an external tool. Files that cannot be converted are declined with a clear message |
| **EMF** | **Implement a subset** | Worth it only because of one workflow: pasting vector content out of Microsoft Office on Windows, where EMF is the clipboard's vector flavour. Implement the record subset Office actually emits (paths, pens, brushes, transforms, text, bitmaps), in pure Rust, and decline the rest gracefully |
| **WMF** | **Decline** | 16-bit, effectively extinct outside archives, and not the Office clipboard format. `research/04 §5.1` marks it P3 |
| **CorelDRAW** (`.cdr`) | **Delegate, optional, not bundled** | `libcdr` (librevenge) already does this well and is MPL-2.0, which is acceptable "with care" per `docs/11-licensing-and-clean-room.md` §4. Writing a CDR importer ourselves is a multi-month reverse-engineering project for a shrinking user base. Ship it as an **out-of-process converter** the user enables, using the same external-tool mechanism as the EPS path, so the MPL code is never linked into our binary and the AppImage stays free of it |
| **CMX** | **Decline** | `research/04 §5.1`: discardable. Nine source files in the original for a format nobody sends any more |
| **PSD** | **Decline for vector; accept as raster** | Flattened-composite import via the `image` ecosystem if free; layered PSD import is a separate product |
| **TIFF, GIF, BMP, PNM, ICO** | **Implement via `image`** | Trivial; tail work |
| **SVG, PNG, JPEG, WebP, `.xar`, `.xarast`** | Already done | Phases 3, 6, 10, 11 |

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| A1 | PDF object layer: xref (classic and streams), object streams, filters (Flate, LZW, ASCIIHex, ASCII85, RunLength, DCT passthrough), encryption detection with a clear decline | `xarast-io` | L | — |
| A2 | Content-stream interpreter: graphics state stack, path construction and painting, clipping, text objects, XObjects (form and image), inline images | `xarast-io` | L | A1 |
| A3 | Shadings: types 2 and 3 → our gradient model; types 4–7 declined with a rasterised fallback | `xarast-io` | M | A2 |
| A4 | Transparency: constant alpha, blend modes mapped onto our families, transparency groups, soft masks (luminosity and alpha) | `xarast-io` | L | A2 |
| A5 | Text: embedded font extraction and subsetting-aware mapping to glyphs, ToUnicode for editable text where possible, outlines where not | `xarast-io`, `xarast-text` | L | A2 |
| A6 | Colour spaces: DeviceRGB/Gray/CMYK, ICCBased, Indexed, Separation and DeviceN (via the tint transform) | `xarast-io`, `xarast-color` | M | A2, C2 |
| A7 | Multi-page handling: import as pages/spreads, or one page at a time with a chooser | `xarast-io`, `xarast-doc` | M | A2 |
| A8 | `.ai` detection and routing through the PDF path | `xarast-io` | S | A2 |
| A9 | External-converter framework: discover an allowlisted external binary, run it sandboxed with a timeout and a size cap, convert to PDF or SVG, import the result, and report clearly in the UI which tool was used | `xarast-app`, `xarast-io` | L | A2 |
| A10 | EPS/AI≤8 via A9 + Ghostscript, disabled when absent | `xarast-io` | M | A9 |
| A11 | CDR via A9 + an out-of-process `libcdr`-based converter, distributed separately | `xarast-io`, `packaging` | M | A9 |
| A12 | EMF subset importer, plus the Windows clipboard EMF flavour | `xarast-io`, `xarast-shell` | L | — |
| A13 | Remaining raster formats via `image` | `xarast-image` | S | — |
| A14 | Fuzz targets: `fuzz_pdf_parse`, `fuzz_emf_parse`; hard limits on nesting depth, object count, memory and time | `fuzz/` | M | A2, A12 |

A9 deserves care because it is the phase's main security surface. An external converter is
another program processing hostile input on the user's behalf. The rules: an **allowlist** of
known binaries discovered on `PATH` (never a user-supplied arbitrary command line), execution
with a wall-clock timeout and an output size cap, no shell, no network, a temporary working
directory removed afterwards, and a visible UI indication that an external tool ran. The
feature is off when the binary is absent and never prompts to install anything.

**Why we write a PDF importer instead of binding pdfium.** pdfium is BSD-3 and would pass the
licence gate, but it is built to *render* PDF, not to hand back editable vector objects; using
it would give us pixels, which is not import. A content-stream interpreter is a well-bounded
amount of work against a published specification, it shares its model with the exporter
(workstream B), and it keeps us free of a large C++ build. The residual risk — exotic files we
interpret badly — is handled by declining loudly rather than importing wrongly, and by a
corpus (below) that is assembled before the interpreter is written.

### B. Print and PDF export fidelity

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| B1 | PDF writer core: object model, xref streams, compression, metadata (XMP), PDF/A-2b and PDF/X-4 conformance switches | `xarast-io` | L | — |
| B2 | Vector fidelity: paths, fill rules, strokes with our caps/joins/mitre limits, dashes | `xarast-io` | M | B1 |
| B3 | Gradients: linear and radial as shading types 2 and 3, **with our bias/gain profile baked into the function** (PDF has no profile concept, so the ramp is emitted as a sampled type-0 function at sufficient resolution) | `xarast-io` | L | B1 |
| B4 | Conical, diamond, three- and four-colour and fractal fills: no PDF equivalent ⇒ rasterise the filled region at the output resolution and emit an image, with the decision logged per object | `xarast-io` | L | B3 |
| B5 | Transparency: constant alpha, transparency groups, soft masks; map our Mix/Stained Glass/Bleach onto PDF Normal/Multiply/Screen where the semantics genuinely match | `xarast-io` | L | B1 |
| B6 | Exotic blend families (Contrast, Saturation, Luminosity, Hue, Bevel, Brightness, Darken, Lighten): **no faithful PDF equivalent** ⇒ rasterise the affected region at output resolution; never silently substitute a similar-looking PDF blend mode | `xarast-io` | L | B5 |
| B7 | Live effects in PDF: regenerate at output resolution via `regenerate_for_output(dpi)` (Phase 13 A10); emit vector where the effect is vector (contour, blend, mould), raster where it is raster (shadow, feather, bevel) | `xarast-io` | L | B1 |
| B8 | Text: embed subsetted fonts (TrueType/CFF), correct encoding and ToUnicode so the PDF is searchable and copyable; a "text as curves" switch for the paranoid case | `xarast-io`, `xarast-text` | L | B1 |
| B9 | Images: emit JPEG passthrough where the source was JPEG, Flate otherwise; downsample to a configurable ceiling; honour ICC profiles | `xarast-io`, `xarast-image` | M | B1 |
| B10 | Output intent and colour: embed the document profile, convert to the output intent when asked, emit CMYK where the document is CMYK | `xarast-io`, `xarast-color` | L | C2 |
| B11 | Print pipeline: **print = generate PDF + hand it to the platform**. Linux via CUPS (a PDF spool job is native); macOS via `NSPrintOperation` over the PDF; Windows **to be determined in this phase** (Windows has no native PDF print path) | `xarast-app`, `xarast-shell` | L | B1 |
| B12 | Print options: page range, copies, scale, fit, tiling across sheets, orientation; a print preview rendered from the same PDF | `xarast-ui` | M | B11 |
| B13 | Printer marks: crop, registration, colour bar, page information | `xarast-io` | M | B1 |
| B14 | Rasterisation-decision report: an export log listing every object that could not be emitted as vector and why, visible in the export dialog | `xarast-io`, `xarast-ui` | M | B4, B6 |

B11's Windows question is the honest gap. Linux and macOS both accept PDF as a print job
natively; Windows does not. Three candidate routes — render to the printer DC at device
resolution, drive the system's PDF print handler, or use XPS conversion — differ in fidelity,
dependency weight and licence exposure. **To be determined in this phase**, by a two-week
spike that implements the simplest (render to DC at device resolution) as the guaranteed
fallback and measures the other two against three real printers using the same test page,
scoring vector-text sharpness, gradient banding and file/spool size. The decision and its
measurements are recorded in `docs/memory/packaging.md`.

B4, B6 and B14 together are the phase's honesty mechanism. A PDF exporter that silently
substitutes a near-enough blend mode produces files that look right on screen and wrong at the
print shop. Ours rasterises what it cannot express, and **tells the user which objects it
rasterised and why**, in a report they can act on.

### C. Colour management and CMYK

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| C1 | ICC engine: adopt Little-CMS via the `lcms2` bindings (MIT, clears the licence gate) behind our own trait, so it can be replaced | `xarast-color` | M | — |
| C2 | Profile-aware conversion: document working space, per-object source profiles, rendering intents (perceptual, relative colorimetric with black-point compensation, saturation, absolute) | `xarast-color` | L | C1 |
| C3 | Display profile: read the system display profile per monitor on all three platforms; convert on the way to the screen, cached as a 3D LUT in the compositor | `xarast-shell`, `xarast-render` | L | C2 |
| C4 | Document colour profile stored in `.xarast` (`research/06 §6.12`) and round-tripped | `xarast-format` | M | C2 |
| C5 | CMYK as a first-class document colour space: authoring, the colour editor, palettes, and conversion boundaries that do not silently round-trip through RGB | `xarast-color`, `xarast-ui` | L | C2 |
| C6 | Spot colours and tints: named separations with a tint transform and a fallback RGB/CMYK appearance | `xarast-color`, `xarast-doc` | L | C5 |
| C7 | Overprint attributes for line and fill, plus print-on-all-plates | `xarast-doc`, `xarast-io` | M | C6 |
| C8 | Separations preview on screen: composite, C, M, Y, K and each spot plate | `xarast-render`, `xarast-ui` | L | C6 |
| C9 | Soft proofing: preview the document through an output profile with gamut warning | `xarast-render`, `xarast-ui` | M | C3 |
| C10 | Colour-management preferences: working space, default profiles, missing/mismatched profile policy | `xarast-ui` | M | C2 |
| C11 | Import/export plumbing: honour embedded profiles in PNG/JPEG/TIFF/PDF/SVG on the way in and out | `xarast-image`, `xarast-io` | M | C2 |

Two constraints carried from the research and easy to violate. First, `research/03 §2.10`
records that the original's kernel used a naive CMYK conversion (`R = 1−C` and so on) and
left real UCR/GCR to CDraw's separation tables — meaning **a `.xar` file's CMYK values must not
be re-interpreted through a modern profile on import** or the document changes colour. Import
maps them with the naive rule, tags them as untagged-device CMYK, and only a deliberate user
action assigns a profile. Second, `research/03 §2.10` also fixes that all compositing happens
in 8-bit non-linear sRGB; colour management therefore sits **around** the compositor — convert
into the working space before compositing and out to the display profile after — and must not
be smuggled into the blend formulas, or Phase 13's blend families change behaviour.

The most common real-world failure in this area is not a wrong conversion but an unnoticed
one: a user opens an untagged file and gets a silent assignment. C10's missing/mismatched
policy (ask / assume working space / assume a named profile) is therefore a visible preference
with a default of *ask on mismatch, assume working space when untagged*.

### D. Stability and long-run soak

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| D1 | Soak harness: a scripted synthetic user performing a randomised but reproducible (seeded) edit stream — draw, transform, apply effects, undo, save, reopen, export — for a configurable duration | `xarast-cli` | L | — |
| D2 | 72-hour soak on each platform: zero crashes, RSS growth < 2 % over the final 48 hours, no unbounded handle/file-descriptor growth | CI + manual | M | D1 |
| D3 | Leak gates extended to every subsystem added since Phase 12 (effects caches, offscreen layers, ICC transforms, PDF import buffers, external-converter temp files) | `xarast-cli` | M | D1 |
| D4 | Extended fuzzing: all importer targets at 8 hours each per week, with corpora carried between runs; `fuzz_pdf_parse` and `fuzz_emf_parse` added | `fuzz/` | M | A14 |
| D5 | `Miri` over the `unsafe` FFI boundaries (`lcms2`, `objc2`, any converter plumbing) | CI | M | C1 |
| D6 | Crash-free-session measurement during the 1.0 beta, from voluntarily submitted crash reports plus the sentinel-file signal, with the methodology and its limitations published | docs | M | — |
| D7 | Recovery drill matrix: SIGKILL, power loss simulation, full disk, read-only target directory, network volume disconnect mid-save — for each, no lost work beyond the autosave interval and a clear message | `xarast-app` | L | — |
| D8 | Large-document stress: 1M-object synthetic document, 500 MB embedded images, 10,000-step blend, 200-page multi-page document — each must open, edit and save or fail with a clear limit message rather than dying | `xarast-cli` | M | D1 |
| D9 | Error-path audit: every `unwrap`/`expect` in non-test code reviewed; each is either justified with a comment proving the invariant or replaced | workspace | L | — |

D7 is the one users judge a 1.0 on. "Full disk while saving" must not destroy the previous
version of the file: saving is write-to-temp-then-rename within the same filesystem, with an
explicit fallback and a clear error when the rename cannot be atomic (network volumes). That
behaviour is asserted by a test that runs against a deliberately tiny filesystem image.

### E. The plugin and extension question — answered

**Should Xarast have a plugin system? No native plugin ABI. Yes to three narrower, stable
extension surfaces.**

The reasoning, stated once, so it does not get relitigated every release:

1. **Rust has no stable ABI.** A `dyn Trait` plugin loaded from a `.so`/`.dll`/`.dylib` must be
   compiled by the same compiler version with the same flags, or it is undefined behaviour. The
   workarounds — a C ABI boundary, `abi_stable`, or a versioned FFI shim — all mean designing
   and freezing a C-shaped interface over a document model built on enums, arenas and `Arc`
   copy-on-write. That interface would constrain the core's evolution permanently, and the core
   is the product.
2. **A native plugin API is a security and stability transfer.** Every in-process plugin can
   crash the editor, corrupt the arena and read the user's files, and every crash arrives as
   *our* crash report. We would be spending Phase 15's stability budget on other people's code.
3. **Binary compatibility with Xara's plugins is already an explicit non-goal**
   (`docs/00-vision-and-scope.md` §4), and `research/04 §2.3` rules out the XPE bridge for the
   same reasons. There is no installed base to serve.
4. **The demand is real but narrower than "plugins".** What people actually ask for is: open my
   weird file format, script a repetitive job, and apply a custom effect. Those are three
   different problems and only the third wants anything like a plugin.

So, three surfaces, all out-of-process or data-only, all versioned:

| Surface | What it is | Status in this phase |
|---|---|---|
| **File-format converters** | The A9 external-converter contract: a declarative manifest naming a binary, its arguments, its input and output formats, and its limits. Anyone can add a format by dropping in a manifest and a program. This is how CDR and EPS already work | **Shipped in 1.0** |
| **Headless automation** | `xarast-cli` as a documented, stable, semantically versioned command surface — convert, render, inspect, and batch-apply a command script expressed as JSON against the same command bus the UI uses. Scripting is therefore automation of real commands, not a parallel API | **Shipped in 1.0** |
| **Effect extensions in WebAssembly** | A sandboxed, out-of-arena surface: an effect receives pixels and parameters and returns pixels, with no access to the document tree, the filesystem or the network. It fits the `<xarast:effects>` chain already specified in `research/06 §6.8.7` | **Designed, not shipped.** Post-1.0, and only if the demand shows up |

`.xarast`'s unknown-data preservation (`research/06 §8`) is what makes the third one safe to
defer: a document produced by a future Xarast with WASM effects already round-trips through a
1.0 build without losing the effect.

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| E1 | Converter manifest format, discovery, validation and documentation | `xarast-app` | M | A9 |
| E2 | `xarast-cli` command surface frozen, versioned and documented; stability policy published | `xarast-cli` | M | — |
| E3 | JSON command-script runner over the existing command bus, with a dry-run mode | `xarast-cli`, `xarast-app` | L | E2 |
| E4 | `docs/extending.md`: the answer above, the two shipped surfaces, and the WASM design sketch | docs | M | E1–E3 |

### F. Remaining 1.0 tail

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| F1 | Linux aarch64 AppImage, built on an ARM runner (`linuxdeploy` cannot cross-compile) | CI | M | — |
| F2 | Flatpak/Flathub as a second Linux channel | `packaging/linux` | L | — |
| F3 | Multi-page / spreads completed if Phase 7 left them partial; page navigation UI | `xarast-doc`, `xarast-ui` | L | — |
| F4 | Documentation completion: every feature from Phases 13–15 in the manual; a migration chapter for Xara users | docs | L | — |
| F5 | 1.0 versioning switch: semantic versioning from 1.0.0 onward, with the compatibility promise for `.xarast` and for the CLI written down | docs | S | E2 |
| F6 | Performance re-baseline: all budgets re-measured and re-committed for 1.0, including the new import/export/print paths | CI | M | — |
| F7 | Licence and provenance audit repeated in full for the 1.0 artefacts on all three platforms | CI | M | — |

## Public API introduced

```rust
// xarast-io
pub fn import_pdf(bytes: &[u8], opts: &PdfImportOptions) -> Result<DocumentBuild, ImportError>;
pub struct PdfImportOptions { pub pages: PageSelection, pub text: TextPolicy, pub max_objects: usize }
pub enum TextPolicy { Editable, Outlines, EditableWhereMappable }

pub fn import_emf(bytes: &[u8]) -> Result<DocumentBuild, ImportError>;

pub struct PdfExportOptions {
    pub conformance: Option<PdfConformance>,   // PdfA2b | PdfX4
    pub output_intent: Option<IccProfileRef>,
    pub text_as_curves: bool,
    pub raster_dpi: f64,
    pub image_ceiling_dpi: Option<f64>,
    pub printer_marks: PrinterMarks,
}
pub fn export_pdf(doc: &Document, opts: &PdfExportOptions) -> Result<(Vec<u8>, ExportReport), ExportError>;
pub struct ExportReport { pub rasterised: Vec<RasterisedRegion>, pub substitutions: Vec<Substitution>, pub warnings: Vec<String> }
pub struct RasterisedRegion { pub node: NodeId, pub reason: RasterReason, pub bounds: Rect, pub dpi: f64 }

// xarast-color
pub trait ColourEngine {
    fn transform(&self, from: &Profile, to: &Profile, intent: Intent, bpc: bool) -> Box<dyn Transform>;
}
pub enum Intent { Perceptual, RelativeColorimetric, Saturation, AbsoluteColorimetric }
pub struct Profile;                 // ICC, loaded or built-in
pub struct SpotColour { pub name: String, pub tint_transform: TintTransform, pub fallback: Colour }
pub struct ColourPolicy { pub working: Profile, pub on_missing: MissingPolicy, pub on_mismatch: MismatchPolicy }

// xarast-app
pub struct PrintJob { pub pages: PageSelection, pub copies: u16, pub scale: PrintScale, pub tiling: Option<Tiling> }
pub fn print(doc: &Document, job: &PrintJob, shell: &dyn PlatformPrint) -> Result<(), PrintError>;
pub trait PlatformPrint { fn submit_pdf(&self, pdf: &[u8], job: &PrintJob) -> Result<(), PrintError>; }

pub struct ConverterManifest { /* name, binary, args, input_formats, output_format, limits */ }
pub fn discover_converters(dirs: &AppDirs) -> Vec<ConverterManifest>;

// xarast-cli  (frozen and versioned at 1.0)
// xarast-cli convert|render|inspect|script|bench|soak|diagnose
```

## Acceptance criteria

1. **PDF import corpus**: a 200-file corpus (assembled before the interpreter is written,
   covering vector art, transparency, shadings, embedded fonts, CMYK, multi-page, and
   deliberately malformed files) imports with **≥ 95 %** rendering within mean ΔE₀₀ < 3.0 of a
   reference rasterisation, **0** panics and **0** hangs
   (`cargo xtask corpus pdf --report`).
2. Every remaining PDF corpus file either imports or is **declined with a specific message**;
   zero files import silently wrong (manually reviewed and recorded, file by file).
3. `.ai` files of version 9 and later import through the PDF path: 20-file corpus, same
   thresholds as criterion 1.
4. EMF: content copied from Word, Excel and PowerPoint pastes into Xarast as editable vector
   objects on Windows; 20-case corpus with mean ΔE₀₀ < 4.0 against a reference render.
5. External converters: with Ghostscript absent the EPS option is hidden and the app never
   prompts; with it present, a 20-file EPS corpus converts and imports; the converter runs
   under the timeout and size caps and leaves no temporary files
   (`cargo nextest run -p xarast-app converters`).
6. **PDF export fidelity**: every document in the golden corpus exports to PDF and, when
   rasterised by an independent renderer at 300 dpi, matches Xarast's own render at the same
   resolution within mean ΔE₀₀ < 2.0 and p99 < 6.0 (`cargo xtask check-pdf-fidelity`).
7. Exported PDFs validate: `veraPDF` (or an equivalent checker) reports conformance for the
   PDF/A-2b and PDF/X-4 switches; text is selectable and searchable when `text_as_curves` is
   false.
8. The export report lists **every** rasterised region with a reason, and a document using
   only PDF-expressible features produces a report with **zero** rasterised regions.
9. **Print**: a test page prints correctly on at least one physical printer per platform,
   recorded with a photograph or a scan; page range, copies, scale, fit and tiling each verified.
10. **Colour management**: a round trip RGB → CMYK → RGB through known profiles matches
    Little-CMS's own result bit-for-bit (`cargo nextest run -p xarast-color icc`); the display
    profile is picked up on each platform and changing it changes on-screen colour.
11. `.xar` CMYK values import unchanged through the naive rule and are tagged as untagged
    device CMYK, asserted by a test over the corpus files that use CMYK.
12. Separations preview shows the correct plate for a CMYK + 2-spot document, and overprint
    attributes round-trip through `.xarast` and into PDF.
13. **72-hour soak** on each platform: 0 crashes, RSS growth < 2 % over the final 48 hours, no
    file-descriptor growth (`cargo xtask soak --hours 72 --report`).
14. Leak gate over every subsystem: `cargo xtask leakcheck --full` within 5 %.
15. Fuzzing: 8 hours per target per week for four consecutive weeks with **0** new crashes or
    timeouts across all importer targets.
16. `cargo miri test` passes over the crates containing `unsafe` FFI.
17. `cargo xtask audit-unwrap` reports **0** unjustified `unwrap`/`expect` in non-test code.
18. Stress: the four D8 documents each open, edit, save and reopen — or fail with a clear,
    documented limit message. No silent truncation, no hang.
19. `docs/extending.md` is published, and both shipped extension surfaces work end to end: a
    third-party converter manifest adds a format without recompiling, and a JSON command script
    performs a batch edit identically to the same actions in the UI.
20. All three platforms build, sign, notarise and publish from one tag, plus the aarch64
    AppImage; `cargo deny check` and the SBOM pass per target (Phase 12 I, Phase 14 D7).
21. Every Phase 12 and Phase 13 performance budget still passes, and the 1.0 budget table
    (below) is met.
22. The definition-of-done checklist is complete with every box ticked and dated.

### Definition of done for 1.0 — the checklist

| # | Item | How it is checked |
|---|---|---|
| 1 | Every acceptance criterion of Phases 12–15 is met | Each phase document's criteria, re-run at the 1.0 tag |
| 2 | `.xar` corpus: 100 % of files parse, 100 % render, and each is reviewed against the original | `xar-dump` + the comparison harness |
| 3 | `.xarast` round trips byte-identically for the whole golden corpus, including unknown data | `cargo nextest run -p xarast-format roundtrip` |
| 4 | Golden corpus ≥ 250 cases, all exact-matching on the CPU backend | `research/05 §13.1` asked for ≥ 120 before beta; 1.0 doubles it |
| 5 | GPU↔CPU parity within tolerance on all three platforms | Nightly parity job |
| 6 | 0 open issues labelled `severity:crash` or `severity:data-loss` | Issue tracker query, recorded |
| 7 | Crash-free sessions ≥ 99.5 % over the final beta, methodology published | D6 |
| 8 | 72-hour soak green on all three platforms | D2 |
| 9 | All performance budgets met on the reference machine | `cargo xtask bench --check-budgets` |
| 10 | Accessibility checklist complete with Orca, Narrator and VoiceOver | Phases 12 and 14 checklists |
| 11 | Licence and provenance audit green per target; SBOM published; clean-room attestation signed | Phase 12 I, repeated |
| 12 | Manual complete, keyboard reference generated, migration chapter written | `mdbook build` + review |
| 13 | Signed and verifiable artefacts for Linux x86_64 + aarch64, Windows, macOS universal | Verification commands from Phases 12 and 14 |
| 14 | `docs/extending.md` published and both surfaces demonstrated | Criterion 19 |
| 15 | `.xarast` format version and compatibility promise published | `research/06 §7.4` + F5 |
| 16 | Every `docs/memory/*.md` note current as of the tag | Review at release |

## Performance budgets

Phase 12 and 13 budgets carry forward unchanged and remain gates. New for 1.0:

| Budget | Target | Notes |
|---|---|---|
| Import a 10 MB, 50-page PDF (first page visible) | ≤ 3 s | Lazy per-page import is acceptable and preferred |
| Import a 2 MB single-page vector PDF, complete | ≤ 1.5 s | |
| Export a 10k-object document to PDF | ≤ 5 s | Excluding rasterised regions |
| Export the same with 20 % of objects rasterised at 300 dpi | ≤ 15 s | Rasterisation dominates; measured separately so the split is visible |
| Print submission (PDF generated and handed to the platform), one ISO A4 page | ≤ 4 s | |
| ICC transform of a full-screen 4K frame | ≤ 3 ms | 3D-LUT cached; this is per-frame cost |
| Separations preview switch | ≤ 100 ms | |
| Soak: RSS growth over 72 hours | < 2 % over the final 48 h | Tighter than Phase 12's 60-minute gate |
| Crash-free sessions | ≥ 99.5 % | Measured over the final beta |
| Open the 1M-object stress document | ≤ 30 s, or a clear limit message | Documented either way |
| aarch64 AppImage: pan/zoom p95, 100k objects | ≤ 24 ms | Slower target acknowledged; measured on a reference ARM board named in `docs/memory/perf.md` |

Numbers here are first estimates, calibrated in the phase's first two weeks by the Phase 12
rule: measure the first working implementation, add 20 %, commit the number.

## Risks and mitigations

| Risk | Probability | Impact | Mitigation |
|---|---|---|---|
| The PDF importer is a bottomless pit | **High** | High | Scope it by corpus, not by specification: the 200-file corpus is assembled *first* and defines done. Anything outside it is declined with a message, not chased. Criterion 2 makes "declines loudly" an accepted outcome |
| PDF export fidelity regressions are invisible until a print shop complains | Medium | High | Criterion 6 rasterises exported PDFs with an independent renderer and diffs them — an automated proxy for the print shop, on every merge |
| Colour management is bolted on and changes existing rendering | Medium | High | Colour management sits around the compositor, never inside it (workstream C); a test asserts that with a null profile chain every Phase 13 golden is bit-identical |
| Windows printing has no good route (B11) | Medium | Medium | The DC-rendering fallback is implemented first and always available; the spike only decides whether to do better |
| Ghostscript's AGPL licence is misunderstood as a dependency | Medium | High | Documented in `deny.toml` comments, in `docs/extending.md` and in the UI: an optional external program the user already has, never bundled, never linked. The `cargo deny` graph stays clean because there is nothing to declare |
| An out-of-process converter becomes a security incident | Low | High | Allowlisted binaries only, no shell, timeouts, size caps, temp-directory isolation, and a visible indication in the UI (A9) |
| The 72-hour soak finds a slow leak two weeks before release | Medium | High | Run the soak continuously from the phase's start on the Linux reference machine, not once at the end |
| Scope creep from "wider import" into every legacy format | Medium | Medium | Workstream A's verdict table is the scope. Adding a format requires amending this document |
| The plugin decision gets relitigated | Medium | Low | `docs/extending.md` states the decision and the reasoning once; future requests are answered with a link |
| 1.0 slips indefinitely because the checklist grows | Medium | High | The definition-of-done checklist is frozen at the start of the phase. Anything discovered later is 1.0.x or 1.1, unless it is a crash or data-loss defect |

## Test plan

**Corpus-driven**
- PDF: the 200-file corpus (criteria 1–2), run on every merge, with a per-file pass/decline/fail
  report published as a CI artefact.
- AI (20 files), EPS (20 files, converter path), EMF (20 cases), CDR (10 files, converter path).
- `.xar` corpus re-verified in full at the 1.0 tag.

**Round trip and fidelity**
- Export every golden document to PDF, rasterise with an independent renderer at 300 dpi, diff
  against Xarast's own render (criterion 6).
- `veraPDF` conformance for the PDF/A and PDF/X switches.
- ICC round trips against Little-CMS as the oracle.
- `.xarast` round trips including colour profiles, spot colours and overprint.

**Stability**
- 72-hour soak on each platform, started at the phase's beginning and repeated at the release
  candidate.
- Weekly 8-hour fuzz runs per importer target, corpora carried forward.
- Recovery drill matrix (D7), scripted where possible: SIGKILL, tiny-filesystem save, read-only
  target, disconnected network volume.
- `Miri` over the FFI crates; `cargo xtask audit-unwrap`.
- Stress documents (D8) run nightly.

**Print and colour, manual and recorded**
- One physical printer per platform, test page with text, gradients, transparency, a live
  effect and a CMYK swatch bar; result photographed or scanned and attached to
  `docs/memory/packaging.md`.
- Soft-proofing compared against a printed proof on at least one calibrated setup.
- Display-profile change on each platform reflected on screen.

**Extension surfaces**
- A third-party converter manifest, written by someone who has not read the code, adds a format
  successfully (criterion 19).
- A JSON command script and the equivalent UI actions produce byte-identical `.xarast` output.

## Memory note

On close, update:

- **`docs/memory/INDEX.md`** — register the new notes: `io-import.md`, `print-colour.md` and
  `extending.md` (if the last is kept as a memory note alongside the user-facing document).
- **`docs/memory/io-import.md`** (new) — the verdict table as shipped; the PDF importer's
  architecture and its declared limits; the corpus and its pass/decline breakdown; the
  external-converter contract and its security rules; what EMF records are understood and what
  are not.
- **`docs/memory/print-colour.md`** (new) — the PDF writer's feature map, which constructs are
  emitted as vector and which are rasterised and why; the Windows print decision from B11 with
  its measurements; the ICC architecture and where transforms happen relative to the
  compositor; the `.xar` CMYK import rule and why it must not change; separations and overprint
  semantics.
- **`docs/memory/render.md`** — the colour-management boundary around the compositor and the
  test that protects it; any rendering change forced by output-resolution regeneration.
- **`docs/memory/perf.md`** — the 1.0 re-baseline for every budget, the aarch64 reference
  board, and the soak results.
- **`docs/memory/packaging.md`** — the aarch64 AppImage build, the Flatpak channel, and the
  printed-output records.
- **`docs/memory/xarast-format.md`** — colour profile, spot colour and overprint storage; the
  1.0 compatibility promise.
