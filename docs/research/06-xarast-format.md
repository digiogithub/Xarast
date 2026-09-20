# `.xarast` native format specification — version 1.0

> **Clean-room notice.** This document specifies the native `.xarast` format
> and describes, only where interoperability requires it, the *behaviour* and
> the *data formats* of Xara Xtreme (GPL-2.0-only). It does not reproduce
> source code from the original; the `file:line` references point at the
> reference tree in `xara-xtreme/` and serve only to locate the logic being
> described. Xarast is implemented from this specification, not by translating
> the original.

> **Status:** NORMATIVE — freeze candidate for Xarast v0.1.
> **Document:** `docs/research/06-xarast-format.md`
> **Format version described:** `1.0`
> **Date:** 2026-09-19
> **Depends on:** `docs/00-vision-and-scope.md` (principle 3), `research/01-xar-format.md`,
> `research/02-document-model.md`.
> **Associated memory note:** `docs/memory/xarast-format.md` (to be created when the phase starts).

## Normative conventions

This document uses the following keywords with the meaning given in
RFC 2119 / RFC 8174:

| Term | RFC equivalent | Meaning |
|---|---|---|
| **MUST** / **MUST NOT** | MUST / MUST NOT | Absolute requirement. A file or implementation that breaks it is **not conformant**. |
| **SHOULD** / **SHOULD NOT** | SHOULD / SHOULD NOT | Strong recommendation. Deviating requires a documented reason. |
| **MAY** | MAY | Optional; both options are conformant. |

**“Writer”** is used for an implementation that produces `.xarast`, **“reader”** for one
that consumes it, and **“implementation”** for both.

The tag identifiers in parentheses (`TAG_...`) are references to Xara's original binary
format, defined in `/home/user/xara-xtreme/Kernel/cxftags.h`, and provide traceability
between the legacy model and this specification.

---

## Contents

1. [Design goals and non-goals](#1-design-goals-and-non-goals)
2. [Prior research: lessons from other formats](#2-prior-research-lessons-from-other-formats)
3. [Container structure](#3-container-structure)
4. [Compression and deduplication strategy](#4-compression-and-deduplication-strategy)
5. [The Xarast SVG profile](#5-the-xarast-svg-profile)
6. [Complete mapping table: Xara → SVG + `xarast:`](#6-complete-mapping-table)
7. [Document metadata](#7-document-metadata)
8. [Unknown-data preservation (lossless round-trip)](#8-unknown-data-preservation)
9. [Identification: magic bytes, extension, MIME, desktop integration](#9-identification-magic-bytes-extension-mime)
10. [Failure recovery: autosave, journal, locking](#10-failure-recovery)
11. [Formal schemas](#11-formal-schemas)
12. [Complete example of a minimal `.xarast`](#12-complete-example)
13. [Rust implementation plan](#13-rust-implementation-plan)
14. [Appendices](#14-appendices)

---

## 1. Design goals and non-goals

### 1.1 Goals (in decreasing order of priority)

**O1 — Full fidelity within Xarast.** A document saved and reopened in the same version
of Xarast MUST be indistinguishable from the original: same geometry (down to millipoint
precision), same attributes, same *editable* live effects, same undo-equivalent document
state. Saving MUST NEVER destroy an object's parametricity (a QuickShape remains a
QuickShape, a blend remains a live blend).

**O2 — Graceful degradation outside Xarast.** The `document.svg` file contained in the
package MUST be a **valid, self-contained SVG 1.1 document**, renderable by any modern
browser or by Inkscape with a *visually approximate* result. “Approximate” is defined
measurably in §5.6: SSIM ≥ 0.90 against Xarast's native render for the reference corpus.
Nothing proprietary to Xarast MUST prevent the base render.

**O3 — Space efficiency comparable to `.xar`.** The format MUST apply the same
strategies that made the original `.xar` compact — reuse of bitmap definitions, relative
coordinates, elision of default values, attribute inheritance and stream compression —
translated to the ZIP+SVG domain (§4). Quantitative target: a `.xarast` SHOULD NOT exceed
the equivalent `.xar` by more than **40 %** for predominantly vector documents, and
SHOULD be **equal or smaller** for bitmap-dominated documents (thanks to content-based
deduplication).

**O4 — Lossless round-trip across versions.** A reader of version *N* that opens a file
of version *N+k*, edits it partially and saves it MUST preserve in full the data it does
not understand (§8). This is the property that fails most critically in competing
formats and is a first-class requirement here.

**O5 — Inspectable and repairable with standard tools.** `unzip`, `xmllint`, `git diff`,
`grep` and a text editor MUST suffice to audit, diagnose and — in simple cases — repair a
document. This is a deliberate advantage over opaque binary formats and a guarantee of
archival longevity.

**O6 — Incremental, cheap reading.** Opening the thumbnail, the metadata or the layer
list MUST NOT require decompressing or parsing the whole document. The ZIP central
directory and `META-INF/manifest.xml` MUST suffice.

**O7 — Atomic, failure-resistant writing.** A power cut during saving MUST NEVER leave
the user without the previous document (§10).

**O8 — Stable under version control.** Two saves with no semantic changes SHOULD produce
byte-identical files (deterministic writing: no varying timestamps in the ZIP unless
explicitly requested, fixed entry order, fixed XML attribute order, stable IDs). This
makes `git` and diffs useful.

### 1.2 Non-goals (explicit)

**N1 — Not a universal interchange format.** The goal is not for other applications to
*edit* `.xarast` with fidelity. For interchange there are plain SVG, PDF and — in the
future — dedicated exporters.

**N2 — No binary `.xar` compatibility on write.** Xarast reads `.xar`; it does not write
it (already declared a non-goal in `00-vision-and-scope.md`). `.xarast` shares not a
single byte of structure with `.xar`.

**N3 — No real-time collaborative editing and no CRDTs in v1.0.** The container reserves
space (`history/`, `META-INF/`) for future extensions, but v1.0 assumes a single writer.

**N4 — No encryption and no digital signatures in v1.0.** The names
`META-INF/encryption.xml` and `META-INF/signatures.xml` are reserved; their semantics are
defined in a later version. A v1.0 reader that encounters these entries MUST reject the
file with a clear error rather than opening it partially.

**N5 — Not a raster image format.** The rasterised document is not stored as the primary
representation; only derived thumbnails and previews.

**N6 — Not “pure” SVG without extensions.** The approach of trying to express everything
in standard SVG is explicitly rejected: it would lose parametricity (O1). Exporting to
plain SVG is a separate, lossy operation.

**N7 — No streaming partial writes over the network.** The file is written in full and
renamed atomically.

---

## 2. Prior research: lessons from other formats

Nine comparable formats have been analysed. This section summarises what is copied and
what is avoided, with the concrete design decision derived from each lesson.

### 2.1 ODF / OpenDocument Graphics (`.odg`) — OASIS

ZIP container with `mimetype` as the **first entry, stored uncompressed and with no
extra field**, so that the MIME type string always falls at fixed offset 38 (30 bytes of
local header + 8 for the name `mimetype`). A `META-INF/manifest.xml` enumerates every
entry with its `media-type`. The content lives in `content.xml`, the styles in
`styles.xml`, the metadata in `meta.xml` and the binaries in `Pictures/`.

- **Copied:** the `mimetype` trick (detection by *magic* without decompressing), the
  explicit manifest, the metadata/content separation, the standard thumbnail.
- **Copied:** the rule that the manifest's `/` entry MUST match the contents of
  `mimetype`.
- **Avoided:** the fragmentation into `content.xml` + `styles.xml` + `settings.xml`,
  which forces cross-references between four XML trees to be resolved in order to paint
  one object. Xarast concentrates the document into a **single** `document.svg`.
- **Avoided:** the bespoke XML vocabulary (`draw:`, `svg:`, `style:`) that *looks* like
  SVG but is not, and therefore cannot be opened in any viewer. That is precisely the
  failure O2 corrects.

### 2.2 Krita (`.kra`)

ZIP with `mimetype`, `maindoc.xml` (layer tree and properties), `documentinfo.xml`,
`preview.png`, `mergedimage.png` and a `layers/` directory holding the binary pixels.

- **Copied:** `mergedimage.png` is an excellent idea — a *flat, always-readable*
  representation of the result, which lets file managers, simple importers and the
  application itself show the document without understanding its model. Xarast adopts
  the vector equivalent: `document.svg` is itself the “merged document”, and there are
  also `thumbnail.png` and `previews/`.
- **Copied:** the radical separation between the tree (small XML, quick to parse) and
  the heavy data (independent binary entries).
- **Avoided:** the opaque per-layer binary (`layers/layerN`), which makes the format
  unauditable and dependent on the exact version of Krita.

### 2.3 Scribus (`.sla`)

Plain XML (optionally gzipped as `.sla.gz`), with a single tree where `<PAGEOBJECT>`
elements are appended in creation order and reference their page by attribute.

- **Avoided:** appearance order decoupled from paint/page order. It is an inexhaustible
  source of bugs and makes it impossible to reason about Z order by reading the file.
  In Xarast **document order IS Z order**, as in SVG.
- **Avoided:** the monolithic XML file with no container: it forces every image to be
  embedded as base64 or depends on external absolute paths (Scribus links images by
  filesystem path: documents break when moved).
  **Derived decision:** in Xarast every referenced resource MUST reside inside the
  package; external absolute references are forbidden (§5.8).
- **Copied:** the readability of the XML and the ease of transforming it with
  XSLT/scripts.

### 2.4 Inkscape SVG (`sodipodi:` / `inkscape:`)

Standard SVG enriched with attributes and elements from two bespoke namespaces
(`http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd` and
`http://www.inkscape.org/namespaces/inkscape`). Layers as
`<g inkscape:groupmode="layer" inkscape:label="...">`, guides as `<sodipodi:guide>`
inside `<sodipodi:namedview>`, parametric shapes as `<path sodipodi:type="star"
sodipodi:sides="5" ... d="...">`.

- **Copied, and the central inspiration of the design:** the *dual representation*
  technique — the standard attribute (`d`) holds the “baked” geometry that anyone can
  render, and the bespoke-namespace attributes hold the **parameters** that allow it to
  be regenerated. The SVG 1.1 specification guarantees that a user agent “shall include
  unknown attributes in the DOM but shall otherwise ignore them”, which makes this safe
  by construction.
- **Copied:** the deliberate compatibility of layers: emitting `inkscape:groupmode` and
  `inkscape:label` costs ~40 bytes per layer and makes Inkscape open the document with
  its layers intact. It is the highest value/cost ratio in the whole format.
- **Avoided:** the lack of versioning of the bespoke vocabulary. Inkscape does not
  record which version wrote each extension, which has caused historical breakage.
  **Derived decision:** the whole `xarast:` vocabulary is versioned in the namespace URI
  and, in addition, every document declares `xarast:version` and `xarast:min-reader`.
- **Avoided:** the loss of extensions when passing through third-party tools. It is a
  known and well-documented problem that SVG cleaners and parsing libraries strip
  `sodipodi:`/`inkscape:` and leave the file unrecoverable for the editor.
  **Derived decision:** §8 makes preservation a normative obligation and adds a digest
  to *detect* that someone has destroyed the data, instead of failing silently.

### 2.5 Figma (`.fig`)

Binary format based on **Kiwi**, a Protocol Buffers-like binary schema. The document is
serialised as a **flat array of `nodeChanges`** with a reference to the parent, not as a
tree; the tree is reconstructed on load. Each file **embeds its own schema** (more than
500 type definitions), which makes it self-describing.

- **Copied (conceptually):** the idea of self-description. Xarast implements it far more
  cheaply: the file declares which vocabulary version it uses and which capabilities it
  requires of a reader (`xarast:min-reader`, `xarast:requires`).
- **Copied:** the flat model with references is very good for incremental
  synchronisation. It is **not** adopted in v1.0 (N3), but the stable-ID design (§5.7) is
  a precondition for adopting it later without breaking the format.
- **Avoided:** the opaque binary. It contradicts O5 directly. The cost is size and
  parsing speed; this is offset by compression (§4) and by streaming SAX parsing.

### 2.6 SVGZ

Simply SVG compressed with gzip. Widely supported (browsers decompress it if the server
sends `Content-Encoding`, and Inkscape/libxml2 do it on the fly).

- **Avoided as a native format:** a single gzip stream allows no random access, no cheap
  thumbnail (O6), no storing binaries without recompressing them, and no deduplication.
  It would force base64 for every image: **+33 % size** on already-compressed data.
- **Kept as an export format:** Xarast SHOULD offer “Export → compressed SVG (`.svgz`)”,
  producing a single self-contained file with `data:` URIs.

### 2.7 Adobe Illustrator (modern `.ai`)

A valid PDF with Illustrator's proprietary data embedded in a `PieceInfo`/`AIPrivateData`
stream inside the PDF itself.

- **Copied:** exactly the same principle as O2/§5.1 — a standard, widely readable format
  with the rich data attached on a channel that standard readers ignore. It is the
  real-world validation that the strategy works at industrial scale over decades.
- **Avoided:** the “private channel” being an opaque binary blob. In `.ai`, if
  Illustrator cannot read its `AIPrivateData`, the user is left with a flat,
  non-editable PDF, and there is no way to diagnose it. In Xarast the private channel is
  readable XML.
- **Avoided:** the total duplication of geometry. `.ai` stores the artwork twice (PDF +
  private), almost doubling the size. Xarast **does not duplicate geometry** when
  standard SVG suffices; it only bakes alternative representations for what SVG cannot
  express, and those baked subtrees are marked with `xarast:generated` and **can be
  discarded and regenerated** (§5.4).

### 2.8 Affinity (`.afdesign`, `.afphoto`, `.afpub`)

Proprietary, undocumented binary format (header starting with `00 FF 4B 41`).

- **Everything avoided.** It is cited as a counter-example: no external tool reads it,
  there is no possible recovery of damaged files, and interoperability depends entirely
  on the vendor. It is the scenario Xarast exists in order not to repeat.

### 2.9 Xara's own `.xar`

A format of `(tag: u32, size: u32, data)` records with two stages of compression: “rich”
data types that encode high-level graphical information in very few bytes, and zlib
compression of the record stream between `TAG_STARTCOMPRESSION` (30) and
`TAG_ENDCOMPRESSION` (31). There are four concrete efficiency lessons, and all four are
carried over in §4.5:

1. **Shared definitions referenced by index.** `TAG_DEFINEBITMAP_*` (65-71) defines a
   bitmap once; each `TAG_NODE_BITMAP` (198) node references it. The same goes for
   `TAG_DEFINERGBCOLOUR` (50) and `TAG_DEFINECOMPLEXCOLOUR` (51).
2. **Relative coordinates.** `TAG_PATH_RELATIVE*` (113-116) and `TAG_PATHREF_TRANSLATE`
   (4014) / `TAG_PATHREF_IDENTICAL` (4013) avoid repeating absolute coordinates.
3. **“Simple” vs “complex” variants of the same record.** `TAG_ELLIPSE_SIMPLE` (1000) vs
   `TAG_ELLIPSE_COMPLEX` (1001), `TAG_RECTANGLE_SIMPLE` (1100) vs the 16
   rounded/starred/reshaped variants: the frequent case does not pay for the rare case's
   fields. Also `TAG_FLATFILL_NONE/BLACK/WHITE` (190-192): 0-byte records for the most
   common values.
4. **Attribute inheritance by position in the tree.** Attributes apply to the following
   subtree (`TAG_UP`/`TAG_DOWN`), so they are not repeated per object. This is exactly
   the inheritance semantics of SVG presentation properties on `<g>`.

- **Avoided:** the global tag registry frozen into numeric ranges (`0-3999`
  Camelot 1.5, `4000-4999` Camelot 2.0), which forced comments of the kind
  “*DO NOT define tags outside this range. Ask Mark Neves*”. XML namespaces eliminate the
  problem of centralised identifier allocation.

### 2.10 Synthesis: the five structural decisions

| # | Decision | Derived from |
|---|---|---|
| D1 | **ZIP** container with `mimetype` STORED in first position | ODF, KRA, EPUB |
| D2 | A **single** `document.svg`, valid SVG 1.1, with Z order = document order | Against ODF/Scribus; in favour of SVG |
| D3 | **Dual representation**: baked standard SVG + `xarast:` parameters | Inkscape, `.ai` |
| D4 | **Content-based deduplication** (BLAKE3) and aggressive elision in the SVG serialiser | `.xar` |
| D5 | **Mandatory preservation** of the unknown, with a verification digest | Against Inkscape/everyone |

---

## 3. Container structure

### 3.1 Base format

A `.xarast` file **MUST** be a ZIP archive conforming to *PKWARE APPNOTE 6.3.x*
(compatible with the subset implemented by `zip`/`unzip`, `libarchive`, `libzip`,
`java.util.zip` and Rust's `zip` crate).

- The archive **MUST** use the classic “End of Central Directory” where it fits; it
  **MUST** use ZIP64 when the archive exceeds 4 GiB or 65,535 entries, and readers
  **MUST** support ZIP64.
- Entry names **MUST** be encoded in **UTF-8** with bit 11 of the general purpose flag
  (*language encoding flag*, EFS) set.
- Names **MUST** use `/` as the separator, **MUST NOT** begin with `/`, and **MUST NOT**
  contain `..`, `\`, control characters or invalid UTF-8 sequences. A reader **MUST**
  reject (not silently sanitise) any entry that breaks this: it is the *zip-slip*
  mitigation.
- The archive **MUST NOT** contain duplicate entries. A reader that finds them **MUST**
  reject the file.
- Directory entries (name ending in `/`, size 0) **MAY** be omitted; readers **MUST
  NOT** depend on their existence.
- ZIP encryption **MUST NOT** be used in v1.0 (N4).

### 3.2 Normative entry layout

The order of entries in the archive **MUST** be as follows (absent entries are simply
skipped; the relative order of those present is mandatory):

```
 1. mimetype                          MANDATORY, first, STORED, no extra field
 2. META-INF/manifest.xml             MANDATORY
 3. meta.xml                          MANDATORY
 4. document.svg                      MANDATORY
 5. thumbnail.png                     RECOMMENDED
 6. previews/<...>.png                OPTIONAL
 7. resources/<...>                   OPTIONAL
 8. history/<...>                     OPTIONAL
 9. extensions/<...>                  OPTIONAL (third-party extensions)
10. any other entry                   PRESERVED (§8.3)
```

Within each group, entries **SHOULD** be sorted lexicographically by name (UTF-8 byte
order) to satisfy O8 (determinism).

#### 3.2.1 `mimetype`

- **MUST** be the first entry in the archive.
- **MUST** be stored with method `STORED` (0), uncompressed.
- **MUST NOT** have an *extra* field in the local header (neither for alignment nor for
  extended timestamps): this guarantees that the content starts exactly at **offset 38**
  (30 bytes of local header + 8 bytes for the name `mimetype`).
- Its content **MUST** be exactly the 26 ASCII bytes `application/vnd.xarast+zip`,
  **with no** trailing newline and **no** BOM.

#### 3.2.2 `META-INF/manifest.xml`

Normative inventory of all entries. Details and schema in §3.4 and §11.1.
It **MUST** exist. It **MUST** describe *every* entry in the archive, including
preserved unknown ones (§8.3) and `mimetype` itself.

#### 3.2.3 `meta.xml`

Document metadata (author, dates, units, page size, palette…). §7.
A separate file is chosen — instead of putting everything in the SVG — to satisfy O6:
a file manager can read `mimetype` + `meta.xml` + `thumbnail.png` (typically < 4 KiB
uncompressed) without touching a 20 MiB `document.svg`.

#### 3.2.4 `document.svg`

**The document.** A valid, self-contained SVG 1.1. §5.
- It **MUST NOT** be gzip-compressed *inside* the ZIP (pointless double compression):
  the ZIP already compresses it. The entry is called `.svg`, not `.svgz`.
- It **MUST** begin with the XML declaration `<?xml version="1.0" encoding="UTF-8"?>`.
- It **SHOULD** end with a newline.

#### 3.2.5 `thumbnail.png`

Document thumbnail (first *spread*, page area).
- It **SHOULD** exist in every interactively saved file.
- It **MUST** be a valid PNG, RGBA at 8 bits per channel.
- Its longer side **SHOULD** be **256 px**; it **MUST NOT** exceed 512 px.
- It **MUST** represent the document on a transparent background, with the page colour
  composited underneath if the page has an opaque background.
- It sits at the root (not in `Thumbnails/` as in ODF) for simplicity and because the
  manifest declares its role explicitly.

#### 3.2.6 `previews/`

Per-*spread* previews, for quick navigation in multi-page documents and for the
“versions” dialogue.

```
previews/spread-1.png
previews/spread-2.png
```

- The name **MUST** be `previews/spread-<n>.png`, with `<n>` the 1-based index of the
  spread in document order.
- The longer side **SHOULD** be 512 px and **MUST NOT** exceed 1024 px.
- They are **derived**: a reader **MUST** be able to work without them, and a writer
  **MAY** omit them (e.g. in non-interactive saving or from the CLI with
  `--no-previews`).

#### 3.2.7 `resources/`

All the binaries referenced by the document. Normative subdirectories:

| Subdirectory | Content | Naming |
|---|---|---|
| `resources/images/` | *Master* bitmaps: the original pixels the user imported | `b3-<hash32>.<ext>` |
| `resources/derived/` | Derived renditions (crops, baked photographic adjustments) | `b3-<hash32>.<ext>` |
| `resources/baked/` | Fallback rasterisations for fills/effects that SVG cannot express (§5.4) | `b3-<hash32>.png` |
| `resources/fonts/` | Embedded font subsets (WOFF2) | `b3-<hash32>.woff2` |
| `resources/profiles/` | ICC colour profiles | `b3-<hash32>.icc` |
| `resources/brushes/` | Brush/stroke definitions (`TAG_BRUSHDEFINITION`, 4080) | `b3-<hash32>.xml` |
| `resources/blobs/` | Any other opaque binary (includes preserved data) | `b3-<hash32>.bin` |

- `<hash32>` **MUST** be the **first 32 lowercase hexadecimal characters** of the
  BLAKE3-256 hash of the **uncompressed** content of the resource (128 bits: negligible
  collision risk for this domain). The full 64-hex hash **MUST** appear in the manifest.
- `<ext>` **MUST** correspond to the actual type of the content (`png`, `jpg`, `webp`,
  `avif`, `jxl`, `tiff`, `gif`, `svg`, `woff2`, `icc`, `bin`).
- A writer **MUST NOT** keep entries in `resources/` that are not referenced by the
  document **unless** they are marked as preserved (§8.3) or as history.

#### 3.2.8 `history/` (optional)

Snapshots of earlier versions of the document, for “undo across sessions” and for the
history dialogue.

```
history/index.xml
history/0007/document.svg
history/0007/meta.xml
```

- It **MUST** be disabled by default in v1.0 (enabled by preference).
- `history/index.xml` **MUST** list each snapshot with `id`, `timestamp` (RFC 3339, UTC),
  an optional `label`, and the cumulative size.
- Binary resources **MUST NOT** be duplicated: snapshots reference the same `resources/`
  paths (this is exactly the payoff that makes deduplication worthwhile).
- The writer **MUST** enforce a configurable limit (by default: 10 snapshots or 25 % of
  the document size, whichever is reached first) and prune the oldest ones.

#### 3.2.9 `extensions/`

Space reserved for plugin or third-party application data. A writer **MUST NOT** modify
or remove entries under `extensions/` that it did not create itself (§8.3).

### 3.3 Cross-entry reference rules

- All references from `document.svg` to resources **MUST** be **paths relative to the
  directory of `document.svg` itself**, that is, to the package root
  (`resources/images/b3-....png`).
- References **MUST NOT** be absolute (`/resources/...`), nor `file://`, nor `http(s)://`,
  and **MUST NOT** contain `..`. A reader **MUST** treat an external reference as a
  missing resource and **MUST** warn the user rather than load it (mitigation against
  exfiltration and against documents that break when moved, the Scribus failure, §2.3).
- Explicit exception: `data:` URIs **MAY** be used for resources smaller than 4 KiB
  (e.g. a tiny pattern), but **SHOULD NOT** be used above that threshold (§4.3).
- Useful consequence of relative paths: when the package is **unzipped into a
  directory**, `document.svg` opens in a browser (`file://`) and resolves all its images
  correctly. This is the concrete operational route to satisfying O2 (§5.9).

### 3.4 `META-INF/manifest.xml`

Namespace: `https://xarast.org/ns/manifest/1.0`, conventional prefix `mf`.

Example (the complete example is in §12):

```xml
<?xml version="1.0" encoding="UTF-8"?>
<mf:manifest xmlns:mf="https://xarast.org/ns/manifest/1.0"
             mf:version="1.0"
             mf:min-reader="1.0"
             mf:generator="Xarast/0.1.0 (linux; x86_64)"
             mf:profile="portable">
  <mf:file-entry mf:full-path="/" mf:media-type="application/vnd.xarast+zip"/>
  <mf:file-entry mf:full-path="mimetype"
                 mf:media-type="text/plain"
                 mf:role="mimetype"
                 mf:size="26" mf:method="stored"/>
  <mf:file-entry mf:full-path="meta.xml"
                 mf:media-type="application/xml" mf:role="meta"
                 mf:size="1184" mf:method="deflate"
                 mf:digest="blake3-256" mf:digest-value="a3c9...e1"/>
  <mf:file-entry mf:full-path="document.svg"
                 mf:media-type="image/svg+xml" mf:role="document"
                 mf:size="20714" mf:method="deflate"
                 mf:digest="blake3-256" mf:digest-value="7f10...bd"/>
  <mf:file-entry mf:full-path="thumbnail.png"
                 mf:media-type="image/png" mf:role="thumbnail"
                 mf:size="9210" mf:method="stored"/>
  <mf:file-entry mf:full-path="resources/images/b3-4a91c0de5f73b8a2.png"
                 mf:media-type="image/png" mf:role="resource"
                 mf:size="482113" mf:method="stored"
                 mf:digest="blake3-256" mf:digest-value="4a91c0de5f73b8a2...9c"
                 mf:refcount="3"/>
</mf:manifest>
```

Normative rules for the manifest:

1. There **MUST** be exactly one `<mf:file-entry>` with `mf:full-path="/"`; its
   `mf:media-type` **MUST** be identical to the content of the `mimetype` entry (rule
   inherited from ODF).
2. There **MUST** be one `<mf:file-entry>` for **every** ZIP entry (except directory
   entries).
3. `mf:role` **MUST** be one of: `mimetype`, `manifest`, `meta`, `document`,
   `thumbnail`, `preview`, `resource`, `history`, `extension`, `unknown`.
   A reader that encounters an unknown `mf:role` **MUST** treat it as `unknown` and
   preserve the entry.
4. `mf:digest`/`mf:digest-value` **MUST** be present for `document`, `meta` and every
   `resource`; they **MAY** be omitted for `mimetype` and previews.
   The only algorithm defined in v1.0 is `blake3-256` (64 hex, lowercase).
5. `mf:size` is the **uncompressed** size in bytes.
6. `mf:method` is informative (`stored`, `deflate`, `zstd`); the ZIP is the source of
   truth. It serves to audit without opening every local header.
7. `mf:refcount` is informative: the number of references to the resource from
   `document.svg`. Useful for diagnosing how effective deduplication is.
8. `mf:profile` **MUST** be `portable` (only `stored`+`deflate`) or `compact` (allows
   `zstd`). See §4.2.
9. If a reader detects a discrepancy between the manifest and the real ZIP content
   (missing entry, extra entry, or a digest that does not match), it **MUST** warn the
   user and **SHOULD** offer to open in read-only mode. It **MUST NOT** open silently.

### 3.5 Why XML and not JSON for the manifest

`META-INF/manifest.json` was considered. XML is chosen for three concrete reasons:

1. **Homogeneity of the toolchain.** The document is already XML (SVG) and so is the
   metadata. A single parser (`quick-xml`) covers all three; with JSON two would be
   needed, plus two different escaping/encoding models.
2. **Precedent and tooling.** `xmllint --relaxng` validates the manifest in CI without
   writing any code; the ODF/OPC ecosystem has validated this design over 20 years.
3. **Namespaces.** Third-party extensibility with non-collision guarantees is native to
   XML. In JSON a prefix convention has to be invented.

The drawback is acknowledged (verbosity, ~1.6× against JSON) and considered irrelevant:
the typical manifest takes 0.5-3 KiB before compression and < 500 bytes afterwards,
against documents of hundreds of KiB.

---

## 4. Compression and deduplication strategy

### 4.1 Guiding principle

**Do not recompress what is already compressed, compress text aggressively, and do not
store the same thing twice.** The three rules, applied together, reproduce in the
ZIP+SVG domain what `.xar` achieved with rich records and zlib (§2.9).

### 4.2 Permitted compression methods

| ZIP method | ID | `portable` profile | `compact` profile | Use |
|---|---|---|---|---|
| `Stored` | 0 | **MUST** be supported | **MUST** be supported | `mimetype`, already-compressed binaries |
| `Deflate` | 8 | **MUST** be supported | **MUST** be supported | XML, SVG, text |
| `Zstandard` | **93** | **MUST NOT** be used | **MAY** be used | Large XML/SVG, incompressible but not already-compressed binaries |

Normative notes:

- A **conformant reader MUST** support `Stored` and `Deflate`, and **SHOULD** support
  `Zstandard` (method 93).
- A **writer MUST** use the `portable` profile by default. The `compact` profile
  **MUST** be enabled explicitly (preference or `--profile compact`), **MUST** be
  declared in `mf:profile`, and **MUST** raise `mf:min-reader` to `1.0` with the `zstd`
  capability in `<mf:requires>` (§8.4).
- ID **93** is used and **NOT** 20. ID 20 was assigned to Zstandard in APPNOTE 6.3.7 and
  **replaced by 93** in 6.3.8 to avoid conflicts; 93 is what WinZip, libzip, libarchive,
  7-Zip and Python's `zipfile` module write. A reader **MAY** accept 20 on reading out
  of tolerance, but **MUST NEVER** write it.
- BZip2, LZMA, XZ, PPMd, Deflate64 and Shrink/Implode **MUST NOT** be used. Reason: poor
  interoperability and marginal gain over zstd.
- Recommended level: `Deflate` level 6 on interactive save, level 9 on
  “Save as… → optimised”; `Zstandard` level 10 interactive, 19 optimised.
  The level does **NOT** affect the conformance of the file.

### 4.3 What to compress and what not (normative table)

| Entry type | Method | Rationale |
|---|---|---|
| `mimetype` | **STORED** (mandatory) | Requirement for fixed-offset detection (§3.2.1) |
| `document.svg`, `meta.xml`, `META-INF/manifest.xml`, `history/**/*.xml`, `*.svg` | **DEFLATE** (or ZSTD in `compact`) | Text: typical ratio 6:1 to 12:1 |
| PNG, JPEG, WebP (lossy and lossless), AVIF, JXL, GIF | **STORED** | They already carry Deflate/DCT/VP8L/AV1 inside. Recompressing costs CPU and saves **nothing**: measured, `deflate -6` over a typical JPEG saves 0.1-0.5 % and over a PNG 0.0-0.2 %. |
| WOFF2 | **STORED** | Internal Brotli |
| ICC | **DEFLATE** | Profiles with repetitive tables: 2:1 to 4:1 |
| Uncompressed TIFF, BMP, RAW | **DEFLATE** (or ZSTD) | Uncompressed pixels. **But**: a writer **SHOULD** convert them to PNG/JPEG on import instead of storing them raw |
| `resources/blobs/*.bin` (preserved opaque) | **DEFLATE** | Unknown: the heuristic in §4.4 decides |
| `thumbnail.png`, `previews/*.png` | **STORED** | PNG |

**Escape rule (mandatory heuristic).** For any entry not explicitly covered above, the
writer **MUST** apply:

1. If the MIME type is in the “already compressed” list → `STORED`.
2. Otherwise, compress the first **64 KiB** with `Deflate` level 1. If the resulting
   ratio is **< 1.05** (less than 5 % saving) → store the whole entry as `STORED`.
   Otherwise → compress it entirely with the profile's method and level.

This avoids the pathological pattern of “naive” ZIPs that spend minutes recompressing
gigabytes of JPEG to save kilobytes.

**On `data:` URIs.** Embedding an image in the SVG as base64 inflates it by **33 %**
*and* puts it inside a stream that does get recompressed, destroying the two previous
optimisations *and* making cross-document deduplication and random access impossible.
That is why §3.3 limits them to 4 KiB.

### 4.4 Content-based deduplication

**Mechanism.** Every binary resource is identified by the **BLAKE3-256 of its
uncompressed content**. BLAKE3 is chosen over SHA-256 for speed (on the order of
1-3 GB/s per core, with internal Merkle-tree parallelisation), which allows hashing on
the fly during import at no perceptible cost, and because it is cryptographically sound
— ruling out adversarial collisions, not just accidental ones.

Rules:

1. On importing a binary, the writer **MUST** compute its BLAKE3-256 and consult the
   document's resource index. If a resource with that hash already exists, it **MUST**
   reuse it and increment its `refcount` instead of adding a new entry.
2. The entry name **MUST** be derived from the hash (§3.2.7), which makes deduplication
   **structural**: it is impossible to have two entries with the same content and
   different names within the same folder.
3. On saving, the writer **MUST** walk the document, count real references and **omit**
   resources with `refcount == 0` (garbage collection), except those referenced by
   `history/` or marked as preserved.
4. The writer **SHOULD** keep an in-memory index `hash → (path, refcount)` while the
   document is open, and **SHOULD NOT** rehash unchanged resources when saving (saving a
   300 MB document must not read 300 MB).

**Second-level deduplication: master + derived.** This is the direct translation of
Xara's `TAG_DEFINEBITMAP_*` + `TAG_XPE_BITMAP_PROPERTIES` (4117) +
`TAG_DEFINEBITMAP_XPE` (4118), where the bitmap is stored once and the processed
variants are regenerated on the fly from the master plus the processing parameters.

- The **master** (the pixels as the user imported them) goes in `resources/images/`.
- Each **derived** rendition (crop, baked brightness/contrast adjustment, rescaled
  screen version) goes in `resources/derived/` and **MUST** declare in the manifest
  `mf:derived-from="<hash of the master>"` and `mf:derivation="<id of the op chain>"`.
- A writer **MAY** omit derived renditions entirely (`--no-derived`) if the operation
  chain is deterministic and described in `<xarast:photo-ops>` (§6.9): the reader
  regenerates them. It is an explicit size/opening-time trade-off.
- Consequence: 8 uses of the same photo with 3 different crops take **1 master +
  3 derived renditions**, not 8 copies.

**Geometry deduplication.** Besides binaries, the writer **SHOULD** deduplicate
identical vector subtrees: if two or more objects have identical geometry and attributes
modulo an affine transformation, it **SHOULD** emit one in `<defs>` and reference it
with `<use>`. This is the SVG equivalent of `TAG_PATHREF_IDENTICAL` (4013) and
`TAG_PATHREF_TRANSLATE` (4014). The recommended threshold is **≥ 3 repetitions** or
**≥ 512 bytes** of repeated `d`, so that the indirection pays off.

### 4.5 The four `.xar` optimisations, translated

| `.xar` technique | Translation in `.xarast` | Measured/estimated saving |
|---|---|---|
| Bitmap definitions referenced by index (`TAG_DEFINEBITMAP_*`) | BLAKE3 deduplication + reused `<pattern>`/`<image>` (§4.4) | Up to **N×** in documents with repetition |
| Relative coordinates (`TAG_PATH_RELATIVE*`) | Relative path commands (`m`,`l`,`c`,`s`,`h`,`v`) + quantisation to 3 decimals | **18-30 %** over absolute `d` |
| “Simple/complex” variants and empty records (`TAG_FLATFILL_BLACK`) | Elision of attributes with SVG default values; presentation attributes instead of `style=`; short IDs | **10-20 %** of the SVG |
| Inheritance by position in the tree (`TAG_UP`/`TAG_DOWN`) | Hoisting of common attributes to the parent `<g>` and use of `<style>` with classes when ≥ 8 objects share paint | **8-25 %** of the SVG |
| Stream compression (`TAG_STARTCOMPRESSION`) | Deflate/zstd of the `document.svg` entry | **6:1 to 12:1** |

#### 4.5.1 Normative passes of the SVG serialiser

The writer **MUST** implement, in this order, the following passes when emitting
`document.svg`. Each one **MUST** be semantically neutral (verifiable by the round-trip
test in §13.5).

1. **Number normalisation.** Coordinates in user units = PostScript points. They are
   emitted with **up to 3 decimals** (exactly 1 millipoint, Xara's internal unit) and
   **without trailing zeros** (`12.5`, not `12.500`). The leading `0` is dropped (`.5`,
   not `0.5`). Exponents only if they shorten the value.
2. **Relative paths.** For each path the absolute and relative variants are generated
   and the shorter is emitted. Repeated commands are collapsed (`l 1,0 l 2,0` →
   `l 1,0 2,0`), `h`/`v` are used for axial segments and `s`/`t` for curves with a
   reflected control point.
3. **Elision of default values.** `fill="black"`, `fill-opacity="1"`, `stroke="none"`,
   `stroke-width="1"`, `opacity="1"`, `stroke-linecap="butt"`,
   `stroke-linejoin="miter"`, `stroke-miterlimit="4"`, `fill-rule="nonzero"`,
   `transform="matrix(1,0,0,1,0,0)"` and `preserveAspectRatio="xMidYMid meet"` are not
   emitted.
   **Exception:** if the SVG default value differs from the value inherited from the
   parent `<g>`, it **MUST** be emitted explicitly.
4. **Attribute hoisting.** If all the direct children of a `<g>` share the same value
   for an inheritable property, it is hoisted to the `<g>` and deleted from the children.
5. **CSS classes.** If ≥ 8 elements share the same set of ≥ 3 paint properties, a rule
   is emitted in a single `<style type="text/css">` inside `<defs>` and they are
   replaced by `class="cN"`. CSS **MUST NOT** be used for anything affecting geometry or
   model semantics (paint only), so that removal of the `<style>` by a third-party tool
   degrades colour, never structure.
6. **Short, stable IDs.** The IDs emitted **MUST** be the model's persistent IDs (§5.7),
   not positional indices. For auto-generated internal elements (gradients, markers,
   clips) short IDs derived from the content hash are used (`g7a3`, `m1f`, `c22`), which
   automatically deduplicates identical definitions.
7. **`<defs>` deduplication.** Two gradients, patterns, markers, filters or `clipPath`
   elements with identical content **MUST** be collapsed into one.
8. **Minimal indentation.** On normal save, no indentation (one newline per top-level
   element). With `--pretty` it is indented by 1 space for git diffs.
   Indentation affects size **before** compressing far more than after (deflate eats
   whitespace almost for free: ~2 % of the compressed size), so it is a user option, not
   a requirement.

### 4.6 Size estimates

Figures estimated over the `xara-xtreme/Designs/` and `testfiles/` corpus, with this
methodology: SVG generated by the 8 passes of §4.5.1, deflate-9, binaries STORED.

#### Case A — Medium vector illustration (≈ 1,800 objects, 14 layers, no bitmaps)

| Component | Uncompressed | Inside the `.xarast` |
|---|---|---|
| `document.svg` | 1,240 KiB | 118 KiB (deflate-9) / 84 KiB (zstd-19) |
| `meta.xml` | 2 KiB | 0.7 KiB |
| `META-INF/manifest.xml` | 1 KiB | 0.4 KiB |
| `thumbnail.png` | 11 KiB | 11 KiB (STORED) |
| `previews/spread-1.png` | 42 KiB | 42 KiB (STORED) |
| ZIP overhead (5 entries × ~110 B) | — | 0.6 KiB |
| **Total** | | **≈ 173 KiB** (portable) / **139 KiB** (compact) |
| Equivalent reference `.xar` | | ≈ 126 KiB |

Ratio against `.xar`: **1.37×** in the portable profile, **1.10×** in compact. Within
goal O3 (≤ 1.40×). The SVG without the 8 optimisation passes would weigh ~2,100 KiB
uncompressed and ~186 KiB compressed: **the passes save 37 % of the final size**.

#### Case B — Photographic document (one 12 MP JPEG, 3.2 MiB, used 8 times with 3 crops)

| Strategy | Size |
|---|---|
| Naive (8 copies embedded as base64, SVG deflate) | ≈ 34.1 MiB |
| Naive (8 copies as ZIP entries, deflate) | ≈ 25.5 MiB |
| 8 copies as ZIP entries, STORED | ≈ 25.6 MiB |
| **`.xarast`: 1 master + 3 derived renditions, all STORED** | **≈ 4.9 MiB** |
| `.xarast` with `--no-derived` (derived renditions regenerated on open) | ≈ 3.3 MiB |

Deduplication contributes a factor of **5.2×** here. This is the case where the format
*beats* the original `.xar`, which deduplicated the master bitmap but stored every
processed XPE variant.

#### Case C — Document with live effects (40-step blend + 3 shadows + 2 contours)

| Component | Note |
|---|---|
| Source geometry (2 blend objects + 5 base objects) | 3 KiB |
| `xarast:` parameters of the effects | 1.2 KiB |
| **Baked subtrees** (40 blend steps + contours as paths) | 96 KiB uncompressed |
| Total `document.svg` | 104 KiB → **9.1 KiB** compressed |

Baking is what dominates the size *before* compression, but it is extremely repetitive
text (40 nearly identical paths) and deflate reduces it ~11:1. **Normative
alternative:** if the baked subtree exceeds **256 KiB** uncompressed, the writer
**SHOULD** replace it with a rasterisation in `resources/baked/` referenced with
`<image>` (§5.4.3), whose size is bounded by the chosen resolution.

#### Case D — Incremental saving with `history/` enabled (10 snapshots)

Since binaries are not duplicated (§3.2.8), the cost of 10 snapshots of a Case A
document is ≈ 10 × 118 KiB = 1.15 MiB, that is **7.8× the base document**. That is why
`history/` is disabled by default and has mandatory pruning. A version 1.1 of the format
**SHOULD** store deltas (`history/0007/document.svg.vcdiff`) instead of full copies; the
entry name is already provided for.

---

## 5. The Xarast SVG profile

### 5.1 The dual-representation principle

**It is the central architectural decision of the format.** Every Xarast object or
attribute is written into `document.svg` as the conjunction of two things:

- **(a) The base representation**: standard, self-sufficient SVG 1.1 that any conformant
  renderer paints reasonably. It may be *exact* (a path with a flat fill) or *baked* (an
  approximation generated by Xarast: 40 paths for a blend, a `<filter>` for a shadow, a
  rasterisation for a fractal fill).
- **(b) The parametric representation**: attributes and elements from the `xarast:`
  namespace, which describe the object *as the Xarast model understands it*, with all
  its editing parameters.

Normative rules of the principle:

1. The base representation **MUST** always exist. There **MUST NEVER** be an object
   visible in Xarast that is invisible or empty in a standard SVG renderer.
2. The parametric representation, where it exists, is the **source of truth** for
   Xarast. On opening, Xarast **MUST** reconstruct the object from (b) and **MUST**
   discard and regenerate (a).
3. Every SVG subtree that is purely a product of baking **MUST** be marked with
   `xarast:generated="<type>"` on its root element. This tells Xarast “this is derived,
   delete it and recompute it”, and tells an analysis tool “this is not authored
   content”.
4. When (a) and (b) disagree (because a third party edited the SVG), **(b) wins**,
   unless the element carries `xarast:base-authoritative="true"` (§8.5).
5. If an object is expressible **exactly** in standard SVG, the writer **MUST NOT** add
   a redundant parametric representation. A rectangle with a flat fill is written
   `<rect ... fill="#c33"/>` and nothing more. The extension is only paid for when it
   contributes something.

### 5.2 Namespaces

```xml
<svg xmlns="http://www.w3.org/2000/svg"
     xmlns:xlink="http://www.w3.org/1999/xlink"
     xmlns:xarast="https://xarast.org/ns/document/1.0"
     xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape"
     xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd"
     xmlns:dc="http://purl.org/dc/elements/1.1/"
     xmlns:cc="http://creativecommons.org/ns#"
     xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
```

| Prefix | URI | Requirement |
|---|---|---|
| (default) | `http://www.w3.org/2000/svg` | **MUST** |
| `xarast` | `https://xarast.org/ns/document/1.0` | **MUST** |
| `xlink` | `http://www.w3.org/1999/xlink` | **MUST** (SVG 1.1 needs it for `xlink:href`) |
| `inkscape`, `sodipodi` | (see above) | **SHOULD** (interoperability with Inkscape) |
| `dc`, `cc`, `rdf` | (see above) | **SHOULD** (metadata in `<metadata>`) |

- The URI of the `xarast` namespace **MUST** contain the **major** version of the
  vocabulary (`/1.0`). An incompatible version 2 would use
  `https://xarast.org/ns/document/2.0`; compatible additions within the 1.x series do
  **NOT** change the URI.
- The **prefix** is conventional: a reader **MUST** resolve by URI, not by prefix.
- `xlink:href` and `href` (SVG2): the writer **MUST** emit **both** on the elements that
  use them (`<use>`, `<image>`, `<textPath>`, `<mpath>`, gradient references), because
  SVG 1.1 and older renderers only understand `xlink:href` and some modern sanitisers
  only keep `href`. Cost: ~15 bytes per reference, which deflate reduces to almost
  nothing.

### 5.3 SVG subset used

**Base profile: SVG 1.1 Second Edition, Full profile.** The writer **MUST NOT** use
features outside this whitelist without marking them as optional:

**Structure:** `svg`, `g`, `defs`, `symbol`, `use`, `switch`, `title`, `desc`,
`metadata`, `style`.
**Geometry:** `path`, `rect`, `circle`, `ellipse`, `line`, `polyline`, `polygon`.
**Paint:** `linearGradient`, `radialGradient`, `stop`, `pattern`, `marker`,
`solidColor` (SVG2 — only with a fallback).
**Image:** `image`.
**Text:** `text`, `tspan`, `textPath`.
**Clipping and masking:** `clipPath`, `mask`.
**Filters:** `filter` and the primitives `feGaussianBlur`, `feOffset`, `feFlood`,
`feComposite`, `feMerge`, `feMergeNode`, `feBlend`, `feColorMatrix`,
`feComponentTransfer` (+ `feFuncR/G/B/A`), `feTurbulence`, `feDisplacementMap`,
`feSpecularLighting`, `feDiffuseLighting`, `feDistantLight`, `fePointLight`,
`feMorphology`, `feTile`, `feImage`.

**Explicitly forbidden (the writer MUST NOT emit it):**

| Element/mechanism | Reason |
|---|---|
| `<script>`, `on*` | Security: a document must not be able to execute code. A reader **MUST** strip it on opening and warn. |
| `<foreignObject>` | Not predictably renderable; contributes nothing to the model. |
| `<animate>`, `<set>`, `<animateTransform>`, SMIL | Outside the v1.0 model. |
| `<font>`, `<glyph>` (SVG fonts) | Obsolete and unsupported in browsers. Use WOFF2. |
| `<meshgradient>`, `<hatch>` (SVG 2) | Removed from SVG 2 and moved to `svg-next`; no browser implementation. 3/4-colour fills are baked (§6.4). |
| External references (`http(s)://`, `file://`, absolute paths) | §3.3 |
| External XML entities, DTD, `<!ENTITY>` | XXE. The reader **MUST** reject any DOCTYPE with entities. |

**Usable with a mandatory fallback (SVG 2 / CSS Compositing):**

| Feature | Required fallback |
|---|---|
| `style="mix-blend-mode:<mode>"` | The element **MUST** additionally have a plausible `opacity`/colour; the blend mode **MUST** be repeated in `xarast:blend`. Supported by all current browsers, not by all viewers. |
| `style="isolation:isolate"` | Without it the render degrades, but does not break. |
| `href` without `xlink:` | Always accompanied by `xlink:href` (§5.2). |
| `var(--...)` for named colours | Only if `xarast:colour-refs="var"`; by default literals are emitted (§6.12). |
| `paint-order` | Affects the fill/stroke order of text; degrades acceptably. |

### 5.4 Marking baked content

```xml
<g xarast:generated="blend" xarast:generated-by="x:blend-7" xarast:generated-rev="3">
  <!-- 40 intermediate steps, pure standard SVG -->
</g>
```

| Attribute | Mandatory | Meaning |
|---|---|---|
| `xarast:generated` | **MUST** | Generator type: `blend`, `contour`, `shadow`, `bevel`, `mould`, `fill-bake`, `stroke-outline`, `text-outline`, `clone-expand`, `effect` |
| `xarast:generated-by` | **MUST** | ID of the parametric element that produced it |
| `xarast:generated-rev` | **SHOULD** | Monotonic counter; allows stale baking to be detected |
| `xarast:generated-hash` | **MAY** | BLAKE3 of the input parameters; if it does not match on opening, it is regenerated |

Rules:

1. On opening, Xarast **MUST** delete every subtree with `xarast:generated` whose
   `xarast:generated-by` resolves to a parametric element that is present, and
   regenerate it.
2. If `xarast:generated-by` does **not** resolve (the parametric element was deleted by
   a third party), Xarast **MUST** keep the baked subtree as normal editable geometry
   and warn that the live effect has been lost. **Never** delete artwork without a
   replacement.
3. A baked subtree **MUST NOT** contain nested parametric elements: baking is a leaf of
   the editing tree.

#### 5.4.1 Three baking strategies, in order of preference

1. **Equivalent geometry** (preferred): the effect is expressed with SVG
   paths/gradients.
   Applies to: blend, contour, mould, variable stroke, expanded clones.
   *Advantage:* scales losslessly, and is editable if all else fails.
2. **SVG filter** (second choice): `<filter>` with standard primitives.
   Applies to: shadow, feathering, bevel, blurs, photographic adjustments, fractals
   (`feTurbulence` is a remarkably good approximation of Xara's cloud and plasma fills).
   *Advantage:* very compact. *Risk:* the exact result depends on the renderer.
3. **Rasterisation** (last resort): PNG in `resources/baked/` referenced with `<image>`.
   Applies to: high-quality conical/diamond/3-4-colour fills, effects whose geometric
   baking would exceed 256 KiB (§4.6 Case C), and anything new that SVG cannot express.
   The writer **MUST** rasterise at **2× the object's nominal resolution in the
   document**, with a maximum of 4096 px per side, and **MUST** annotate
   `xarast:baked-dpi`.

### 5.5 Coordinate system and units

- **SVG user unit = 1 PostScript point = 1/72 inch.** The point is chosen rather than
  the CSS pixel because Xara's internal unit is the **millipoint** (1/1000 pt,
  `Kernel/doccoord.h`), so that 3 decimals in the SVG represent **exactly** the
  internal precision with no rounding error. The document **MUST** declare it:

```xml
<svg width="210mm" height="297mm" viewBox="0 0 595.276 841.89" ...>
```

  `viewBox` in points, `width`/`height` in the real physical unit, so that a browser and
  a printer get the size right.

- **Y axis:** SVG grows downwards; Xara grows upwards. The document is stored in
  **native SVG convention (Y downwards)**, with the origin at the **top-left corner of
  the spread**. The alternative of applying a global `transform="scale(1,-1)"` is
  explicitly rejected: it breaks text, gradients, filters and readability. The Y
  conversion is done in the importer/exporter, once only.
  Xarast **MUST** save `<xarast:document xarast:y-axis="down">` to make this explicit
  and allow a future `up`.

- **The user's working units** (mm, cm, in, pt, px, and user-defined units with a
  prefix/suffix, `TAG_DEFINE_PREFIXUSERUNIT` 85 / `TAG_DEFINE_SUFFIXUSERUNIT` 86) are a
  **presentation preference**, live in `meta.xml` (§7) and do **NOT** affect the numbers
  in the SVG.

- **Precision.** The writer **MUST** emit coordinates with up to 3 decimals and **MUST
  NOT** emit more. The reader **MUST** accept any valid SVG number, including
  exponential notation.

### 5.6 Measurable criterion for a “reasonable render” (O2)

An automatable conformance test is defined:

1. The document is rendered with Xarast to PNG at 96 dpi → *reference*.
2. `document.svg` is extracted and rendered with **resvg**, with **headless Chromium**
   and with **Inkscape** (`--export-type=png`) → *candidates*.
3. The SSIM of each candidate against the reference is computed.

**Normative thresholds** for the conformance corpus (§13.5):

| Document class | Minimum SSIM | Note |
|---|---|---|
| Geometry only, flat fills and linear/radial gradients | **0.99** | Must be near-exact |
| With transparencies, clips, masks, text | **0.95** | Rasterisation and font differences |
| With live effects (shadow, bevel, feathering, contour, blend) | **0.90** | SVG filters approximate |
| With fractal or conical fills | **0.85** | Baked; more deviation is accepted |
| **Mean over the whole corpus** | **≥ 0.90** | This is the number that closes O2 |

A threshold failure **MUST** break CI.

### 5.7 Object identity

Every object in the model **MUST** have a persistent identifier emitted as the `id`
attribute of the corresponding SVG element.

- The `id` **MUST** be a valid XML name, stable throughout the object's life: it does
  not change on moving, reordering, editing or regrouping. Only a new object gets a new
  `id`.
- Recommended format: `x` + 11 characters of a lowercase base32 alphabet derived from a
  truncated ULID/UUIDv7 (`xk3m9q2vr7t`). It is short (12 bytes), sortable by creation
  time and collides with negligible probability.
- The IDs of generated auxiliary elements (gradients, filters, clips, markers)
  **SHOULD** be derived from the content hash (§4.5.1, pass 6), which makes them both
  deduplicable and stable.
- A reader that finds duplicate `id`s **MUST** reassign the duplicates and warn; a
  document with duplicate `id`s is **not conformant**.
- IDs are the foundation on which incremental synchronisation and collaborative editing
  will be built in the future (§2.5, N3).

### 5.8 Document tree structure

```xml
<svg …>                                        <!-- document -->
  <title>…</title>
  <desc>…</desc>
  <metadata>  <rdf:RDF>…</rdf:RDF>  </metadata>
  <defs>
    <style type="text/css">…</style>           <!-- paint classes (§4.5.1) -->
    <xarast:document …/>                       <!-- global properties -->
    <xarast:palette …>…</xarast:palette>       <!-- named colours -->
    <xarast:units …/>                          <!-- user units -->
    <!-- gradients, patterns, filters, markers, clipPaths, symbols -->
  </defs>
  <sodipodi:namedview …>                       <!-- Inkscape-compatible guides/grid -->
    <sodipodi:guide …/>
  </sodipodi:namedview>

  <g id="xSPREAD1" xarast:spread="1" xarast:kind="spread" …>
     <xarast:page …/>                          <!-- geometry of the spread's pages -->
     <g id="xL1" inkscape:groupmode="layer" inkscape:label="Background"
        xarast:kind="layer" …>  …objects…  </g>
     <g id="xL2" inkscape:groupmode="layer" inkscape:label="Text"
        xarast:kind="layer" …>  …objects…  </g>
     <g id="xLG" inkscape:groupmode="layer" inkscape:label="Guides"
        xarast:kind="layer" xarast:layer-kind="guide" style="display:none">…</g>
  </g>

  <svg id="xSPREAD2" x="0" y="900" width="595.276" height="841.89"
       viewBox="0 0 595.276 841.89" xarast:spread="2" xarast:kind="spread">
     …
  </svg>
</svg>
```

#### 5.8.1 Pages and spreads

Xara's model is `Document → Chapter → Spread → (Page*, Layer*)`
(`TAG_DOCUMENT` 40, `TAG_CHAPTER` 41, `TAG_SPREAD` 42, `TAG_PAGE` 44, `TAG_LAYER` 43).

**Decision:** all spreads live in **a single `document.svg`**.

- The **first** spread is emitted as a `<g>` directly under the root `<svg>`, at the
  origin of the root `viewBox`. Consequence: **a browser opening `document.svg` shows
  the first page, correctly framed**. This is the desired degradation.
- The **second and subsequent** spreads are emitted as **nested** `<svg>` elements with
  `x`, `y`, `width`, `height` and their own `viewBox`, laid out vertically below the
  first with a gap of one page unit. They fall outside the root `viewBox`, so a browser
  clips them: it sees page 1, not a confusing collage.
- The alternatives are rejected: (i) one SVG per spread in `spreads/` would break direct
  opening in a browser and multiply the shared `<defs>`; (ii) all spreads inside the
  same `viewBox` would produce an illegible initial render.
- **Chapters** (`TAG_CHAPTER`) are represented as a logical grouping in
  `<xarast:document>`, not as a level of the SVG tree, because they have no geometry.
- Each spread **MUST** declare its pages with `<xarast:page>` (position, size, bleed,
  whether it is a double page, spread scaling `TAG_SPREADSCALING_*` 52/53).

For documents with **more than 32 spreads or more than 8 MiB of SVG**, the writer
**MAY** use the split layout (`xarast:layout="split"`): `document.svg` contains the
first spread and a list of `<xarast:spread-ref xarast:href="spreads/spread-N.svgpart"/>`;
the remaining spreads live in `spreads/`. A reader **MUST** support both layouts. The
writer **MUST** annotate `mf:profile` accordingly and **MUST** warn that opening
directly in a browser will only show the first spread.

#### 5.8.2 Layers

A layer is a `<g>` that is a direct child of the spread, **never** nested inside another
layer.

```xml
<g id="xL2"
   inkscape:groupmode="layer" inkscape:label="Text"
   xarast:kind="layer"
   xarast:layer-kind="normal"
   xarast:visible="true"
   xarast:locked="false"
   xarast:printable="true"
   xarast:solid="false"
   xarast:active="true"
   xarast:frame-delay="0"
   style="display:inline;opacity:1">
```

| Attribute | Source in Xara | SVG equivalent | Notes |
|---|---|---|---|
| `inkscape:groupmode="layer"` | — | — | Inkscape compatibility; cost ~28 B |
| `inkscape:label` | `TAG_LAYERDETAILS` (48) | — | Visible name; **MUST** match `xarast:label` if both are present |
| `xarast:visible` | `TAG_LAYERDETAILS` | `style="display:inline|none"` | Both **MUST** be emitted; `display` is the base representation |
| `xarast:locked` | `TAG_LAYERDETAILS` | — | SVG has no locking; only `xarast:` (+ `sodipodi:insensitive="true"` for Inkscape) |
| `xarast:printable` | `TAG_LAYERDETAILS` | — | A non-printable layer **MUST** still be visible on screen |
| `xarast:layer-kind` | `TAG_GUIDELAYERDETAILS` (49) | — | `normal`, `guide`, `background`, `frame` |
| Layer opacity | — | `opacity` on the `<g>` | Xara has no native layer opacity; Xarast adds it and uses standard `opacity` |
| `xarast:frame-*` | `TAG_LAYER_FRAMEPROPS` (4030) | — | Frame animation; outside the scope of v1.0 rendering |

- The **guide layer** **MUST** be emitted with `style="display:none"` so that it does not
  appear in an external viewer, and with `xarast:layer-kind="guide"`.
- **Z order** is document order: the first layer `<g>` is painted first (it ends up at
  the back). This inverts the usual convention of layer panels (where the top layer is
  at the front): the UI **MUST** show the list reversed. It is documented here because
  it is the number one source of bugs in implementations of graphics formats (§2.3).
- The `xarast:solid` attribute corresponds to Xara's concept of a layer with a solid
  background that hides those below it.

### 5.9 How “open it in a browser” is satisfied in practice

It is worth being precise, because there is a real limitation: **a browser does not open
a ZIP**. Requirement O2 is satisfied by four routes, all of them normative:

1. **Extraction.** `unzip documento.xarast -d /tmp/doc && xdg-open /tmp/doc/document.svg`.
   Since resource references are relative to the package (§3.3), everything resolves.
   This is the canonical route and **MUST** always work.
2. **Inkscape.** The same: the extracted `document.svg` is opened. Layers, guides and
   grid appear correctly thanks to `inkscape:groupmode` and `sodipodi:namedview`
   (§5.8.2). Inkscape **preserves** unknown `xarast:` attributes when saving, so the
   Xarast → Inkscape → Xarast round-trip keeps the extensions (§8).
3. **`xarast extract`** (CLI): the Xarast binary **MUST** offer
   `xarast extract doc.xarast -o dir/` and `xarast cat doc.xarast document.svg`.
4. **Export to self-contained plain SVG.** Xarast **MUST** offer “Export → SVG” with
   three variants:
   - `SVG (with extensions)`: `document.svg` as-is, resources as sibling files.
   - `SVG (self-contained)`: resources embedded as `data:` URIs; `xarast:` extensions
     preserved. A single file that opens anywhere.
   - `SVG (plain / Inkscape / web)`: extensions stripped, baking consolidated,
     `<style>` expanded. **Lossy, and labelled as such in the UI.**
   The self-contained variant **MAY** additionally be compressed to `.svgz`.

In addition, the writer **SHOULD** include at the package root a `README.txt` entry
(≈ 700 bytes, deflating to ~350) explaining to the human user what the file is and how
to extract it. Precedent: well-made `.epub` files do this. It is the format's best
investment of 350 bytes.

---

## 6. Complete mapping table

Table notation: **Base** = what is emitted in standard SVG (representation (a));
**Extension** = what is emitted in the `xarast:` namespace (representation (b));
**Loss without (b)** = what degrades in an external viewer.

### 6.1 Geometric objects

| Xara concept | Original tag | SVG base | `xarast:` extension | Loss without (b) |
|---|---|---|---|---|
| Path (fill / stroke / both) | `TAG_PATH*` 100-103, 113-116 | `<path d="…"/>` with `fill`/`stroke` | — (exact) | None |
| Path flags (smooth, rotational, end point) | `TAG_PATH_FLAGS` 111 | — | `xarast:node-flags="s r e …"` (one letter per node) | None visually; node-editing behaviour is lost |
| Group | `TAG_GROUP` 104 | `<g>` | `xarast:kind="group"` | None |
| Group with transparency | `TAG_GROUPTRANSP` 4063 | `<g opacity="…" style="isolation:isolate">` | `xarast:group-transp="true"` | Slightly different compositing |
| Compound group (cached render) | `TAG_COMPOUNDRENDER` 4128, `TAG_CACHEBMP` 4064 | plain `<g>` | `xarast:compound="true"` | None |
| Simple ellipse | `TAG_ELLIPSE_SIMPLE` 1000 | `<ellipse cx cy rx ry/>` | — | None |
| Complex ellipse (rotated/skewed) | `TAG_ELLIPSE_COMPLEX` 1001 | `<ellipse …  transform="matrix(…)"/>` | — | None |
| Simple rectangle | `TAG_RECTANGLE_SIMPLE` 1100 | `<rect x y width height/>` | — | None |
| Rounded rectangle | `TAG_RECTANGLE_SIMPLE_ROUNDED` 1104 | `<rect rx="…"/>` | `xarast:corner-ratio` if the radius is proportional | None |
| Starred / reshaped rectangle and the 12 variants | 1101-1115 | baked `<path d="…">` | `<xarast:quickshape>` (§6.2) | It stops being parametric |
| QuickShape (polygon/star) | `TAG_REGULAR_SHAPE_PHASE_1/2` 1900/1901 | baked `<path d="…">` | `<xarast:quickshape>` (§6.2) | It stops being parametric |
| Dimension line | `TAG_DIMENSION` 4130 | `<g>` with path + text | `<xarast:dimension>` | It stops updating |
| Web address on an object | `TAG_WEBADDRESS` 4020 | `<a xlink:href="…">` wrapping the object | `xarast:web-bbox` | None |

### 6.2 Parametric shapes (QuickShapes)

Xara's `NodeRegularShape` node (`Kernel/nodershp.h`) parameterises rectangles, polygons,
stars and ellipses with a single model. The extension mirrors it field by field:

```xml
<path id="xq4" d="M 120,20 L 149.4,80.6 …Z"
      xarast:shape="quick"
      fill="#e8a33d">
  <xarast:quickshape
      xarast:sides="5"
      xarast:circular="false"
      xarast:stellated="true"
      xarast:primary-curvature="false"
      xarast:stellation-curvature="false"
      xarast:stell-radius-ratio="0.382"
      xarast:primary-curve-ratio="0"
      xarast:stell-curve-ratio="0"
      xarast:stell-offset-ratio="0"
      xarast:centre="120 90"
      xarast:major-axis="0 -70"
      xarast:minor-axis="70 0"
      xarast:matrix="1 0 0 1 0 0"
      xarast:reformed="false"/>
</path>
```

| Attribute | Field in `NodeRegularShape` | Type | Notes |
|---|---|---|---|
| `xarast:sides` | `NumSides` | integer ≥ 3 | Ignored if `circular` |
| `xarast:circular` | `Circular` | bool | The shape is based on a circle |
| `xarast:stellated` | `Stellated` | bool | Star |
| `xarast:primary-curvature` | `PrimaryCurvature` | bool | Curved sides |
| `xarast:stellation-curvature` | `StellationCurvature` | bool | Curved points |
| `xarast:stell-radius-ratio` | `StellRadiusToPrimary` | double | Inner / outer radius |
| `xarast:primary-curve-ratio` | `PrimaryCurveToPrimary` | double | Curvature of the side |
| `xarast:stell-curve-ratio` | `StellCurveToStell` | double | Curvature of the point |
| `xarast:stell-offset-ratio` | `StellOffsetRatio` | double | ±0.5 = 360/N degrees of rotation of the points |
| `xarast:centre` | `UTCentrePoint` | `x y` | In untransformed coordinates |
| `xarast:major-axis`, `xarast:minor-axis` | `UTMajorAxes`, `UTMinorAxes` | `x y` | Vectors from the centre |
| `xarast:matrix` | `TransformMatrix` | 6 doubles | Transformation of the shape |
| `xarast:reformed` | `IsReformed()` | bool | The sides have been edited by hand |
| `<xarast:edge-path>` (child, ×2) | `EdgePath1`, `EdgePath2` | path `d` | **MUST** be emitted only if `reformed` |

Rule: if `xarast:reformed="true"`, the shape has hand-edited sides and the two
`<xarast:edge-path>` elements are mandatory; otherwise the path is regenerated from the
parameters and they **MUST NOT** be emitted.

### 6.3 Fills

All fills additionally carry, where applicable, the common attributes:

| Common attribute | Tag | Values | SVG base |
|---|---|---|---|
| `xarast:fill-repeat` | `TAG_FILL_REPEATING` 163 / `NONREPEATING` 164 / `REPEATINGINVERTED` 165 / `_EXTRA` 206 | `simple`, `repeat`, `reflect`, `repeat-extra` | `spreadMethod="pad|repeat|reflect"` |
| `xarast:fill-profile` | `TAG_BLENDPROFILES` 4072 and ramp profiles | `bias gain` (2 doubles in [-1,1]) | Baked into stops (§6.5) |
| `xarast:fill-effect` | `TAG_FILLEFFECT_FADE` 160 / `RAINBOW` 161 / `ALTRAINBOW` 162 | `fade`, `rainbow`, `alt-rainbow` | HSV interpolation baked into stops |

| Xara fill | Tag | SVG base | Extension | Loss without (b) |
|---|---|---|---|---|
| Flat | `TAG_FLATFILL` 150 (+190-192) | `fill="#rrggbb"` | — | None |
| No fill | `TAG_FLATFILL_NONE` 190 | `fill="none"` | — | None |
| Linear (2 points) | `TAG_LINEARFILL` 153 | `<linearGradient x1 y1 x2 y2 gradientUnits="userSpaceOnUse">` | `xarast:fill="linear"` only if there is a profile | Ramp profile |
| Multi-stage linear | `TAG_LINEARFILLMULTISTAGE` 4075 | Same, with N `<stop>` elements | ditto | Profile |
| 3-point linear | `TAG_LINEARFILL3POINT` 4121, `…MULTISTAGE3POINT` 4122 | `<linearGradient>` + `gradientTransform` reproducing the skew | `xarast:fill="linear3" xarast:p0/p1/p2` | None visually if the skew is affine |
| Circular | `TAG_CIRCULARFILL` 154 | `<radialGradient cx cy r fx=cx fy=cy>` | — | Profile |
| Elliptical | `TAG_ELLIPTICALFILL` 155 | `<radialGradient>` + `gradientTransform="matrix(…)"` | — | Profile |
| Conical | `TAG_CONICALFILL` 156, `…MULTISTAGE` 4078 | **Baked**: a `<g>` of ≤ 96 wedges with flat fills, **or** a `<pattern>` with a rasterised `<image>` | `<xarast:fill xarast:type="conical" xarast:centre xarast:start-angle xarast:end-angle>` + stops | Appears faceted or as a bitmap |
| Diamond / square | `TAG_SQUAREFILL` 200, `…MULTISTAGE` 4088 | **Baked** as for conical, or a `<radialGradient>` with a `gradientTransform` rotated 45° (acceptable approximation) | `<xarast:fill xarast:type="diamond">` | Rounded rather than straight corners |
| 3 colours | `TAG_THREECOLFILL` 202 | **Baked** to an `<image>` (mesh gradients were withdrawn from SVG 2) | `<xarast:fill xarast:type="three-point">` with 3 `<xarast:point x y colour>` | Becomes raster |
| 4 colours | `TAG_FOURCOLFILL` 204 | ditto | `xarast:type="four-point"` | ditto |
| Bitmap | `TAG_BITMAPFILL` 157 | `<pattern patternUnits="userSpaceOnUse" patternTransform="matrix(…)"><image …/></pattern>` | `xarast:fill="bitmap"` + `xarast:tile-mode` | None if it is a simple tiling |
| Contone bitmap (duotone) | `TAG_CONTONEBITMAPFILL` 158 | `<pattern>` + `<filter>` with a luminance `feColorMatrix` + `feComponentTransfer` interpolating start→end | `xarast:contone-start`, `xarast:contone-end` | Slight difference in the tonal curve |
| Fractal — clouds | `TAG_FRACTALFILL` 159 (`FILLSHAPE_CLOUDS` 9) | `<filter>` with `feTurbulence type="fractalNoise"` + `feColorMatrix` + `feComponentTransfer` mapping to the 2 colours, applied to a `<rect>` clipped by the shape | `<xarast:fill xarast:type="fractal-clouds" xarast:seed xarast:graininess xarast:octaves xarast:squash xarast:dpi>` | Texture **similar but not identical**; very acceptable |
| Fractal — plasma | `TAG_FRACTALFILL` 159 (`FILLSHAPE_PLASMA` 10) | ditto with `type="turbulence"` | `xarast:type="fractal-plasma"` | ditto |
| Noise | `TAG_NOISEFILL` 4010 | `feTurbulence` + `feComposite` | `xarast:type="noise"` | ditto |
| Bevel fill | `TAG_BEVEL`-related, `bevfill.cpp` | Part of the bevel filter (§6.8) | — | — |

**Fill rasterisation rule (normative).** When the writer bakes a fill to a bitmap, it
**MUST**:
- rasterise at **twice** the object's nominal resolution (min. 96 dpi, max. 4096 px per
  side);
- store the PNG in `resources/baked/` (deduplicated by hash);
- reference it with a `<pattern>` whose `patternTransform` reproduces the fill geometry
  exactly;
- annotate `xarast:baked-dpi` and `xarast:generated="fill-bake"`.

### 6.4 Non-linear ramp profiles

Xara allows a *bias/gain* curve over the interpolation of any gradient and over the
distribution of a blend or contour. SVG only interpolates linearly between `<stop>`
elements.

**Solution:** baking by adaptive sampling.

```xml
<linearGradient id="g7a3" gradientUnits="userSpaceOnUse" x1="20" y1="20" x2="220" y2="20"
                xarast:profile="0.42 -0.15">
  <stop offset="0"     stop-color="#1c3f8f"/>
  <stop offset=".0625" stop-color="#23499a"/>
  …
  <stop offset="1"     stop-color="#ffd166"/>
</linearGradient>
```

- The writer **MUST** emit `xarast:profile="<bias> <gain>"` (two doubles in [-1, 1];
  `0 0` = linear) and **MUST** omit it when it is linear.
- The writer **MUST** bake the curve by emitting intermediate stops. Number of stops:
  adaptive sampling until the maximum colour error between the real curve and the
  piecewise linear interpolation is **≤ 2/255** in each channel. Minimum **9**, maximum
  **33** stops per span between key colours.
- The reader **MUST** discard the intermediate stops when `xarast:profile` is present
  and recompute them, so as not to accumulate error over successive saves.
- The original key stops are marked with `xarast:key="true"` so they can be
  distinguished from the baked ones; alternatively the writer **MAY** list the key
  colours and their positions in `xarast:stops="0:#1c3f8f 0.5:#7a2ea0 1:#ffd166"`, which
  is more compact and is the **recommended** form.

### 6.5 Transparencies and blend modes

Xara models transparency as a “transparency fill” with the same variety of geometric
shapes as the colour fills (`TAG_*TRANSPARENTFILL`, 166-172, 180-182, 201, 203, 205,
4011), plus a transparency **type** which is in practice a blend mode
(`enum TranspType` in `Kernel/fillval.h`).

#### 6.5.1 Transparency geometry

| Xara transparency | Tag | SVG base | Extension |
|---|---|---|---|
| Flat | `TAG_FLATTRANSPARENTFILL` 166 | `fill-opacity` / `stroke-opacity` / `opacity` | — |
| Linear | `TAG_LINEARTRANSPARENTFILL` 167 | `<mask maskUnits="userSpaceOnUse">` with a `<rect>` painted by a greyscale `<linearGradient>` | `<xarast:transparency xarast:type="linear" …>` |
| Circular / elliptical | 168 / 169 | `<mask>` with a greyscale `<radialGradient>` | `xarast:type="circular|elliptical"` |
| Conical / square / 3-4 col. | 170 / 201 / 203 / 205 | `<mask>` with the same baking as the equivalent fill | ditto |
| Bitmap | `TAG_BITMAPTRANSPARENTFILL` 171 | `<mask>` with a `<pattern>` of the image in greyscale | `xarast:type="bitmap"` |
| Fractal | `TAG_FRACTALTRANSPARENTFILL` 172 | `<mask>` with `feTurbulence` | `xarast:type="fractal-*"` |
| Line transparency | `TAG_LINETRANSPARENCY` 173 | `stroke-opacity` | — |
| 3-point transparency | `TAG_LINEARTRANSPARENTFILL3POINT` 4123 | `<mask>` + `gradientTransform` | `xarast:type="linear3"` |

**Important note on masks.** The luminance value of a `<mask>` in SVG 1.1 is computed
with the linear luminance formula (`linearRGB` by default). The writer **MUST** emit
`color-interpolation="sRGB"` on the `<mask>` and on the gradient so that the result
matches Xara's model (direct alpha, with no gamma conversion). Omitting this produces
visibly different transparencies in a browser. It is a classic mistake.

#### 6.5.2 Transparency types → blend modes

`TranspType` in Xara (`Kernel/fillval.h`) includes: `Mix`, `StainGlass`, `Bleach`,
`Contrast`, `Saturation`, `Darken`, `Lighten`, `Brightness`, `Luminosity`, `Hue`,
`Bevel` and three “special” modes (additive, subtractive, lookup table).

| `TranspType` | Value | SVG/CSS base | Fallback accuracy | Extension |
|---|---|---|---|---|
| `TT_Mix` | 1 | normal alpha (`opacity`) | **Exact** | — (default) |
| `TT_StainGlass` | 2 | `style="mix-blend-mode:multiply"` | **Exact** | `xarast:blend="stained-glass"` |
| `TT_Bleach` | 3 | `style="mix-blend-mode:screen"` | **Exact** | `xarast:blend="bleach"` |
| `TT_DARKEN` | — | `mix-blend-mode:darken` | Exact | `xarast:blend="darken"` |
| `TT_LIGHTEN` | — | `mix-blend-mode:lighten` | Exact | `xarast:blend="lighten"` |
| `TT_SATURATION` | — | `mix-blend-mode:saturation` | Approximate (CSS HSL model vs Xara's HSV) | `xarast:blend="saturation"` |
| `TT_LUMINOSITY` | — | `mix-blend-mode:luminosity` | Approximate | `xarast:blend="luminosity"` |
| `TT_HUE` | — | `mix-blend-mode:hue` | Approximate | `xarast:blend="hue"` |
| `TT_CONTRAST` | — | `<filter>` with `feComponentTransfer type="linear"` around the midpoint | Approximate | `xarast:blend="contrast" xarast:amount="…"` |
| `TT_BRIGHTNESS` | — | `<filter>` with `feComponentTransfer type="linear" intercept` | Approximate | `xarast:blend="brightness"` |
| `TT_BEVEL` | — | Resolved as part of the bevel (§6.8) | — | `xarast:blend="bevel"` |
| Special additive | `T_SPECIAL_1` | `mix-blend-mode:plus-lighter` | Good | `xarast:blend="additive"` |
| Special subtractive | `T_SPECIAL_2` | `mix-blend-mode:multiply` (approx.) | Poor | `xarast:blend="subtractive"` |
| Lookup table | `T_SPECIAL_3` | `<filter>` with `feComponentTransfer type="table"` and the table included | Good | `xarast:blend="lut"` + `<xarast:lut>` |

Rules:

- The writer **MUST** always emit `xarast:blend` when the mode is not `Mix`, even where
  an exact `mix-blend-mode` exists: the canonical name of the mode is Xarast's, and this
  way the reader does not depend on parsing CSS.
- The writer **MUST** emit the `mix-blend-mode` inside `style=` and **NOT** as a
  presentation attribute, because it is a CSS property only.
- Every group containing children with `mix-blend-mode` **SHOULD** carry
  `style="isolation:isolate"` to confine the blending to the group, replicating Xara's
  transparency group semantics (`TAG_GROUPTRANSP` 4063).

### 6.6 Stroke

| Concept | Tag | SVG base | Extension | Loss |
|---|---|---|---|---|
| Line colour | `TAG_LINECOLOUR` 151 (+193-195) | `stroke="#rrggbb"` / `none` | — | — |
| Width | — | `stroke-width` | — | — |
| Dashes | — | `stroke-dasharray`, `stroke-dashoffset` | `xarast:dash-name="…"` (name of the gallery pattern) | The name is lost, not the appearance |
| Caps | — | `stroke-linecap="butt|round|square"` | — | — |
| Joins | — | `stroke-linejoin="miter|round|bevel"`, `stroke-miterlimit` | — | — |
| Stroke scaling with the object | — | — | `xarast:stroke-scales="true"` | The width is not rescaled on transform |
| Arrows / terminators | `arrows.cpp` | `<marker>` + `marker-start`/`marker-end` | `xarast:arrow-start="<name>"`, `xarast:arrow-end`, `xarast:arrow-scale-with-width` | The name and the automatic rescaling are lost |
| Variable-width stroke | `TAG_VARIABLEWIDTHFUNC` 4000, `TAG_VARIABLEWIDTHTABLE` 4001 | **Baked**: a closed `<path>` with `fill` (the stroke outline), `stroke="none"`, `xarast:generated="stroke-outline"` | `<xarast:stroke xarast:type="variable">` + `<xarast:width-table>` (space-separated list of `t:w`) | It stops being editable as a stroke |
| Stroke type / definition | `TAG_STROKETYPE` 4002, `TAG_STROKEDEFINITION` 4003 | — | `xarast:stroke-type="<id>"`, definition in `resources/brushes/` | — |
| Airbrush | `TAG_STROKEAIRBRUSH` 4004 | Baked to an `<image>` or to a filter | `xarast:stroke-type="airbrush"` + parameters | Becomes raster |
| Brush | `TAG_BRUSHATTR` 4079, `TAG_BRUSHDEFINITION` 4080, `TAG_BRUSHDATA` 4081 … 4113 | **Baked**: `<g xarast:generated="stroke-outline">` with the brush instances along the path | `<xarast:brush>` with a reference to `resources/brushes/b3-….xml`, plus the spacing, rotation, scaling, randomness and colour data | It stops being a live brush |
| Tablet pressure | `TAG_BRUSHPRESSUREINFO` 4105, `TAG_BRUSHPRESSUREDATA` 4106, `TAG_BRUSHPRESSURESAMPLEDATA` 4109 | (included in the baking) | `<xarast:pressure>` with the samples in base64 (normalised u16 LE) or a list of decimals | — |
| Line/fill overprint | `TAG_OVERPRINTLINEON` 3500 … 3503 | — | `xarast:overprint-stroke="true"`, `xarast:overprint-fill="true"` | Only affects colour separation |
| Print on all plates | `TAG_PRINTONALLPLATESON` 3504 | — | `xarast:all-plates="true"` | ditto |

Example of a variable-width stroke:

```xml
<g xarast:kind="stroke-variable" id="xv9">
  <xarast:stroke xarast:type="variable" xarast:base-width="4.5"
                 xarast:profile="0.2 0">
    <xarast:width-table>0:0.1 0.15:0.7 0.5:1 0.85:0.7 1:0.1</xarast:width-table>
    <xarast:source-path d="M 10,50 C 60,10 140,90 190,50"/>
  </xarast:stroke>
  <path xarast:generated="stroke-outline" xarast:generated-by="xv9"
        d="M 10,49.8 C …Z" fill="#222"/>
</g>
```

### 6.7 Text

Xara's text model (`TAG_TEXT_STORY_*` 2100-2117, `TAG_TEXT_LINE` 2200, `TAG_TEXT_CHAR`
2202, attributes 2900-2920, and the XaraLX 0.6 extensions: `TAG_TEXT_TAB` 4200,
indentation 4201-4204, linked stories 4205-4207).

| Concept | Tag | SVG base | Extension | Loss |
|---|---|---|---|---|
| Simple text (one line) | `TAG_TEXT_STORY_SIMPLE` 2100 | `<text x y>…</text>` | `xarast:story="simple"` | None |
| Paragraph / area text | `TAG_TEXT_STORY_COMPLEX` 2101 | `<text>` with one `<tspan x y>` **per line**, with baked positions | `<xarast:text-area>` with the containing shape, the indents and the reflow | The text stops reflowing when edited |
| Text on a path | 2110-2117 | `<textPath xlink:href="#p">` (SVG 1.1 supports it) | `<xarast:text-path xarast:start-offset xarast:side xarast:reverse xarast:justify-along>` | Placement differences between renderers |
| Linked stories | `TAG_TEXT_STORY_LINK_INFO` 4206 | Each story is an independent `<text>` | `xarast:story-next="#idNext"`, `xarast:story-prev` | Reflow between frames is lost |
| Justification | 2902-2905 | `text-anchor="start|middle|end"`; full justification **baked** word by word | `xarast:justify="left|centre|right|full"` | Full justification is frozen |
| Font size | `TAG_TEXT_FONT_SIZE` 2906 | `font-size` (in pt) | — | — |
| Typeface | `TAG_TEXT_FONT_TYPEFACE` 2907 | `font-family="'Name', <fallbacks>"` | `xarast:font-id`, `xarast:font-ref="resources/fonts/b3-….woff2"` | Font substitution |
| Bold / italic | 2908-2911 | `font-weight`, `font-style` | — | — |
| Underline | 2912/2913 | `text-decoration="underline"` | — | — |
| Super/subscript, explicit script | 2914-2917 | `<tspan baseline-shift="…" font-size="…">` | `xarast:script="super|sub|explicit"` + offset and size | — |
| Tracking | `TAG_TEXT_TRACKING` 2918 | `letter-spacing` (converted from thousandths of an em to pt) | `xarast:tracking="<thousandths of an em>"` | Rounding |
| Aspect ratio | `TAG_TEXT_ASPECT_RATIO` 2919 | Per-glyph positions baked into the `<tspan>`'s `x="…"` | `xarast:aspect="1.2"` | — |
| Baseline shift | `TAG_TEXT_BASELINE` 2920 | `baseline-shift` or `dy` | — | — |
| Line spacing | 2900 (ratio) / 2901 (absolute) | explicit `y`/`dy` per line | `xarast:line-spacing="ratio:1.2"` or `"abs:14pt"` | It is frozen |
| Manual kerning | `TAG_TEXT_KERN` 2204 | `dx` on the `<tspan>` | — | — |
| Tab stops and ruler | `TAG_TEXT_TAB` 4200, `TAG_TEXT_RULER` 4204 | Baked positions | `<xarast:tabs>` and `<xarast:ruler>` | They are frozen |
| Indents | 4201-4203 | Baked into `x` | `xarast:indent-left/first/right` | They are frozen |
| Text converted to curves | — | `<path>` | `xarast:was-text="true"` + `<xarast:text-source>` with the original text (accessibility and search) | — |

**Fonts.** Normative rules:

1. The writer **MUST** always record family, weight, style and a stable
   `xarast:font-id`.
2. The writer **SHOULD** embed a **WOFF2 subset** of every font used in
   `resources/fonts/`, and **MUST** emit the corresponding `@font-face` rule in the
   `<style>` inside `<defs>`, so that a browser opening the extracted SVG renders with
   the correct typeface.
3. The writer **MUST** respect the licence: if the font forbids embedding (restrictive
   `fsType` bit in `OS/2`), it **MUST NOT** embed it and **MUST** annotate
   `xarast:font-embed="denied"`.
4. The writer **MUST** emit a generic fallback chain
   (`font-family="'Xara Sans', 'DejaVu Sans', sans-serif"`).
5. In “archival” mode (conformance profile C, §14.2) the writer **MUST** additionally
   emit a copy of the text converted to curves inside a
   `<g xarast:generated="text-outline" style="display:none">`, which the reader can
   enable if the font is unavailable. High cost in size; that is why it is a separate
   profile.

### 6.8 Live effects

This is the group where the dual-representation principle earns its keep. In every
case: the parametric `<xarast:*>` element lives next to the source object, and the baked
representation lives in a sibling marked with `xarast:generated`.

#### 6.8.1 Shadow (`TAG_SHADOWCONTROLLER` 4050, `TAG_SHADOW` 4051)

```xml
<g id="xs3" xarast:kind="shadow-group">
  <xarast:shadow xarast:type="wall" xarast:blur="6.2" xarast:offset="8 8"
                 xarast:angle="315" xarast:darkness="0.55" xarast:scale="1"
                 xarast:colour="#000000" xarast:penumbra="4"/>
  <g xarast:generated="shadow" xarast:generated-by="xs3"
     filter="url(#fsh3)" opacity="0.55">
    <use xlink:href="#xobj7" href="#xobj7"/>
  </g>
  <g id="xobj7"> <!-- the real object --> </g>
</g>
```

With the filter:

```xml
<filter id="fsh3" x="-30%" y="-30%" width="180%" height="180%"
        color-interpolation-filters="sRGB">
  <feGaussianBlur in="SourceAlpha" stdDeviation="3.1"/>
  <feOffset dx="8" dy="8"/>
  <feFlood flood-color="#000000"/>
  <feComposite in2="SourceAlpha" operator="in"/>
</filter>
```

| Type | `xarast:type` | Baking |
|---|---|---|
| Wall shadow | `wall` | Filter (offset + blur) |
| Floor shadow | `floor` | Copy transformed with perspective/skew + filter. The full `transform` **MUST** be emitted |
| Glow | `glow` | Filter with no offset, with optional `feMorphology` |
| Inner shadow | `inner` | Filter with `feComposite operator="out"` |

`color-interpolation-filters="sRGB"` is **mandatory** on every filter emitted: the SVG
default is `linearRGB`, which produces blurs and shadows visibly different from Xara's.

#### 6.8.2 Bevel (`TAG_BEVEL` 4052, attributes `TAG_BEVATTR_*` 4053-4056, `TAG_BEVELINK` 4057)

```xml
<xarast:bevel xarast:type="round" xarast:indent="6" xarast:light-angle="135"
              xarast:light-elevation="45" xarast:contrast="0.5"
              xarast:direction="outer" xarast:join="round"
              xarast:light-colour="#ffffff" xarast:shadow-colour="#000000"/>
```

Baking: a `<filter>` with the classic chain
`feGaussianBlur` (over `SourceAlpha`, radius = indent) →
`feSpecularLighting` with `feDistantLight azimuth elevation` →
`feComposite operator="in"` →
`feComposite operator="arithmetic"` to compose light and shadow over `SourceGraphic`.
When the bevel carries its own fill (`bevfill.cpp`), the writer **MUST** additionally
bake the “inking node” (`TAG_BEVELINK`) as a sibling `<path>`.

| `xarast:type` | Correspondence in Xara |
|---|---|
| `round` | Rounded |
| `flat` | Flat |
| `chisel` | Chisel / Angled |
| `ridge` | Ridge |
| `mesa` | Mesa / Plateau |

#### 6.8.3 Contour (`TAG_CONTOURCONTROLLER` 4066, `TAG_CONTOUR` 4067)

```xml
<g id="xc5" xarast:kind="contour">
  <xarast:contour xarast:width="4" xarast:steps="6" xarast:direction="outer"
                  xarast:join="round" xarast:profile="0.3 0"
                  xarast:colour-profile="0 0" xarast:insets="false"/>
  <g xarast:generated="contour" xarast:generated-by="xc5">
    <path d="…"/> <!-- step 6, the outermost, painted first -->
    …
  </g>
  <path id="xc5src" d="…"/>   <!-- source object -->
</g>
```

Baking is mandatory as real offset paths (strategy 1 of §5.4.1). An increasing
`stroke-width` **SHOULD NOT** be used as an approximation: it fails at the joins and on
inner contours.

#### 6.8.4 Blend (`TAG_BLEND` 105, `TAG_BLENDER` 106, `TAG_BLEND_PATH` 4061, `TAG_BLENDPROFILES` 4072, `TAG_BLENDERADDITIONAL` 4073, `TAG_BLENDER_CURVEPROP` 4060, `TAG_BLENDER_CURVEANGLES` 4062, `TAG_NODEBLENDPATH_FILLED` 4074)

```xml
<g id="xb2" xarast:kind="blend">
  <xarast:blend xarast:steps="24"
                xarast:from="#xb2a" xarast:to="#xb2b"
                xarast:position-profile="0.15 0"
                xarast:attribute-profile="0 0"
                xarast:one-to-one="false"
                xarast:antialias="true"
                xarast:path="#xb2path"
                xarast:rotate-along-path="true"
                xarast:start-angle="0" xarast:end-angle="90"
                xarast:tangential="true"/>
  <defs>
    <path id="xb2path" d="M 20,200 C 120,40 260,40 360,200"/>
  </defs>
  <g xarast:generated="blend" xarast:generated-by="xb2">
    <path d="…"/>   <!-- 24 intermediate steps -->
  </g>
  <path id="xb2a" d="…" fill="#e33"/>
  <path id="xb2b" d="…" fill="#33e"/>
</g>
```

- The start and end objects **MUST** be emitted as normal, visible elements: they are
  the user's artwork, not baking.
- The intermediate steps **MUST** go inside a `<g xarast:generated="blend">`.
- For blends with many steps (> 64) the writer **SHOULD** apply the rasterisation rule
  of §4.6 (Case C).
- A “one-to-one” blend between groups (`one-to-one`) **MUST** record the correspondence
  of sub-objects with `<xarast:blend-map>` so that the effect is reconstructed exactly.

#### 6.8.5 Moulds (`TAG_MOULD_ENVELOPE` 107, `TAG_MOULD_PERSPECTIVE` 108, `TAG_MOULD_GROUP` 109, `TAG_MOULD_PATH` 110, `TAG_MOULD_BOUNDS` 4012)

```xml
<g id="xm1" xarast:kind="mould">
  <xarast:mould xarast:type="envelope" xarast:bounds="0 0 200 120">
    <xarast:mould-shape d="M 0,0 C 60,-30 140,30 200,0 L 200,120 C 140,150 60,90 0,120 Z"/>
    <xarast:mould-source>
      <!-- the ORIGINAL, undeformed subtree, so that it can still be edited -->
      <g> <path d="…"/> <text …>…</text> </g>
    </xarast:mould-source>
  </xarast:mould>
  <g xarast:generated="mould" xarast:generated-by="xm1">
    <!-- deformed geometry: paths with the nodes already transformed -->
  </g>
</g>
```

- A mould **MUST** store the **undeformed source** inside `<xarast:mould-source>`: it is
  the only way for the effect to remain reversible. This duplicates that geometry, and
  it is an accepted and bounded cost (the source is usually much simpler than the
  result).
- `<xarast:mould-source>` **MUST NOT** be rendered: it sits inside a foreign-namespace
  element, which SVG renderers ignore entirely. It does not require `display:none`.
- For `xarast:type="perspective"` the baking of a **rectilinear** subtree **MAY** be
  reduced to a single `transform="matrix(…)"` when the perspective degenerates into an
  affine transformation.
- For text under a mould, the baking **MUST** convert the text to curves (there is no
  way to deform `<text>` in SVG); the original text stays in `<xarast:mould-source>`.

#### 6.8.6 Feathering (`TAG_FEATHER` 4086, `TAG_FEATHER_EFFECT` 4127)

```xml
<xarast:feather xarast:width="5" xarast:profile="0 0"/>
```
Baking: a `filter` with `feGaussianBlur` over the alpha channel (a
`feColorMatrix type="matrix"` that isolates alpha) + `feComposite`. The profile is baked
with `feComponentTransfer type="table"` over `feFuncA`.

#### 6.8.7 Generic live effects (`TAG_LIVE_EFFECT` 4125, `TAG_LOCKED_EFFECT` 4126)

Xara delegated these effects to external plugins. In Xarast they are modelled as a
declarative **effect chain**:

```xml
<xarast:effects>
  <xarast:effect xarast:id="blur1" xarast:kind="gaussian-blur" xarast:radius="3"/>
  <xarast:effect xarast:id="lv1" xarast:kind="levels"
                 xarast:black="0.05" xarast:white="0.92" xarast:gamma="1.1"/>
</xarast:effects>
```
- Every known `xarast:kind` **MUST** have a baking to an SVG filter defined in the
  effect registry (§14.3).
- An `xarast:kind` that is **unknown** to the reader **MUST** be preserved (§8) and its
  baking **MUST** be kept as-is (the reader cannot regenerate it, so `xarast:generated`
  is treated as authoritative: `xarast:base-authoritative="true"` is emitted as well).
- A “locked” effect (`TAG_LOCKED_EFFECT`) is marked with `xarast:locked="true"`: its
  baking is never regenerated.

### 6.9 Bitmaps and photography

| Concept | Tag | SVG base | Extension |
|---|---|---|---|
| Bitmap node | `TAG_NODE_BITMAP` 198 | `<image xlink:href="resources/images/b3-….png" x y width height preserveAspectRatio="none" transform="matrix(…)"/>` | `xarast:bitmap-id="b3-…"` |
| Duotone bitmap | `TAG_NODE_CONTONEDBITMAP` 199 | `<image>` + duotone `filter` | `xarast:contone-start/end` |
| Bitmap definition | `TAG_DEFINEBITMAP_*` 65-71, 4138 | Entry in `resources/images/` | `mf:digest` in the manifest |
| Bitmap properties | `TAG_BITMAP_PROPERTIES` 4115 | — | `<xarast:bitmap-props xarast:dpi xarast:interpolate xarast:transparent-index>` |
| Document smoothing | `TAG_DOCUMENTBITMAPSMOOTHING` 4116 | `image-rendering="auto|pixelated"` | `xarast:bitmap-smoothing` |
| Non-destructive processing (XPE) | `TAG_XPE_BITMAP_PROPERTIES` 4117, `TAG_DEFINEBITMAP_XPE` 4118 | `<image>` to the derived rendition **or** `<image>` to the master + `filter` | `<xarast:photo-ops>` (operation chain) + `mf:derived-from` |
| Embedded sound | `TAG_DEFINESOUND_WAV` 70 | — | `resources/blobs/` + `<xarast:media>` (not played back in v1.0) |

```xml
<image id="xi4" xlink:href="resources/derived/b3-8f2c….jpg" href="resources/derived/b3-8f2c….jpg"
       x="0" y="0" width="240" height="160" preserveAspectRatio="none"
       transform="matrix(1,0,0,1,60,40)"
       xarast:bitmap-id="b3-4a91c0de5f73b8a2">
  <xarast:photo-ops xarast:master="resources/images/b3-4a91c0de5f73b8a2.jpg">
    <xarast:op xarast:kind="crop" xarast:rect="120 80 1800 1200"/>
    <xarast:op xarast:kind="brightness-contrast" xarast:brightness="0.08" xarast:contrast="0.15"/>
    <xarast:op xarast:kind="unsharp" xarast:radius="1.2" xarast:amount="0.4"/>
  </xarast:photo-ops>
</image>
```

This is the mechanism that reproduces Xara's efficiency with bitmaps: **the master is
stored once**, the variants are described with parameters, and a derived rendition is
only materialised when the cost of regenerating it on opening would be high (§4.4).

### 6.10 Clipping and masks

| Concept | Tag | SVG base | Extension |
|---|---|---|---|
| ClipView (live clipping) | `TAG_CLIPVIEWCONTROLLER` 4084, `TAG_CLIPVIEW` 4085, `TAG_CLIPVIEW_PATH` 4137 | `<g clip-path="url(#cpN)">` + `<clipPath id="cpN">` with the path | `xarast:clipview="true"`, `xarast:clip-shape="#…"`, `xarast:clip-keep-shape="true"` |
| Clipping by path (static) | — | `clip-path` | — |
| Mask by transparency | see §6.5 | `mask` | `<xarast:transparency>` |
| Fill rule | — | `fill-rule="nonzero|evenodd"`, `clip-rule` | — |

Xara's ClipView keeps the clipping shape as an editable object. That is why the writer
**MUST** emit the shape **twice**: inside the `<clipPath>` (where it is neither visible
nor editable) and — if `xarast:clip-keep-shape="true"` — as an element with a referenced
`id`, so that Xarast restores it as a first-class object. The second copy **SHOULD** be
implemented with `<use>` from the `<clipPath>` so as not to duplicate the `d`.

### 6.11 Clones, symbols and sets

| Concept | Tag | SVG base | Extension |
|---|---|---|---|
| Live clone (changes with the original) | — | `<use xlink:href="#xorig" transform="…"/>` | `xarast:clone-of="#xorig"`, `xarast:clone-mode="live"` |
| Independent copy | — | Duplicated subtree | — (not a clone) |
| Reusable symbol | — | `<symbol id="…">` in `<defs>` + `<use>` | `xarast:symbol="true"` |
| Clone with its own attributes | — | `<use>` with overriding `fill`/`stroke` | `xarast:clone-override="fill stroke"` |
| Set | `TAG_SETSENTINEL` 4070, `TAG_SETPROPERTY` 4071 | — | `<xarast:set xarast:id="…" xarast:members="#a #b #c">` in `<defs>` |
| Object name (Name Gallery) | `TAG_NAMEGAL_DOCCOMP` 95 | `<title>` inside the element | `xarast:names="button background highlight"` (space-separated list) |
| Duplication offset | `TAG_DUPLICATIONOFFSET` 4124 | — | `xarast:duplicate-offset="10 10"` on `<xarast:document>` |
| Style/template (WizOp) | `TAG_WIZOP` 4040, `TAG_WIZOP_STYLE` 4041, `TAG_WIZOP_STYLEREF` 4042 | — | `<xarast:style-def>` / `xarast:style-ref` |
| Bar property | `TAG_BARPROPERTY` 4087 | — | `<xarast:bar>` |

`<title>` deserves a note: it is the standard, accessible way of naming an object in SVG
(screen readers announce it, browsers show it as a tooltip). The writer **SHOULD** emit
a `<title>` with the object's primary name **in addition to** `xarast:names`, because it
costs little and improves the accessibility of the exported SVG.

### 6.12 Colour: palettes, named colours, CMYK and spot colours

Xara distinguishes **direct RGB** colour (`TAG_DEFINERGBCOLOUR` 50) from **complex**
colour (`TAG_DEFINECOMPLEXCOLOUR` 51), which can be CMYK, HSV, a spot colour, or a
