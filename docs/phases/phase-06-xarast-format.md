# Phase 6 — Native `.xarast` format

> After this phase Xarast has a format of its own: it can save what it opened, reopen it byte-for-byte identically, survive a crash mid-edit, show a thumbnail in the file manager — and the same file drops into a browser or Inkscape and still looks right.

## Goal

Implement `research/06` in `xarast-format`: the ZIP container, the manifest, the metadata,
the SVG profile, content-addressed resource deduplication, the compression policy,
thumbnails and previews, the unknown-data preservation rule, atomic save, locking,
autosave, the journal, crash recovery, MIME and desktop integration, and the formal
schemas with CI validation.

Two properties are the point of the phase and everything else is in service of them:

- **O4 — nothing is ever lost.** A reader must preserve, in place, every piece of data it
  does not understand, and write it back on save. This is the property comparable formats
  fail (`research/06 §2.4`), and its failure turns "the document looks odd" into "the
  user's work is gone".
- **O2 — it is a real SVG.** `document.svg`, extracted from the package, must render
  acceptably in a browser engine and in Inkscape. Not as a nice gesture: as a measured,
  CI-enforced SSIM threshold.

Conformance target for this phase: **level B** for the v0.1 feature subset, **level A**
complete (`research/06 §14.2`).

---

## Scope

### In scope

| # | Item | Reference |
|---|---|---|
| S1 | ZIP container read/write, APPNOTE 6.3.x, ZIP64, UTF-8 names with the EFS flag, zip-slip rejection, duplicate-entry rejection | `research/06 §3.1` |
| S2 | Normative entry layout and ordering, with `mimetype` first, STORED, no extra field, content at offset 38 | `research/06 §3.2` |
| S3 | `META-INF/manifest.xml`: one entry per ZIP entry, roles, BLAKE3 digests, sizes, methods, refcounts, profile | `research/06 §3.4` |
| S4 | `meta.xml`: document identity, Dublin Core, units, page setup, palette, guides/grid, mirrored into the SVG `<metadata>` | `research/06 §7` |
| S5 | The SVG profile: namespaces, whitelisted subset, the prohibition list, dual representation (base + parametric) | `research/06 §5.1`–`§5.3` |
| S6 | Baked-content marking (`xarast:generated*`) and the three baking strategies, behind a `BakeProvider` trait | `research/06 §5.4` |
| S7 | Coordinate system: user unit = 1 pt, Y down, `viewBox` in points with physical `width`/`height`, 3-decimal precision | `research/06 §5.5` |
| S8 | Persistent object identity: stable `id` per object, content-hash ids for generated defs | `research/06 §5.7` |
| S9 | The eight normative serialiser passes (number normalisation, relative paths, default elision, attribute hoisting, CSS classes, short stable ids, defs dedup, indentation) | `research/06 §4.5.1` |
| S10 | Compression policy: STORED / DEFLATE / ZSTD per entry type, the 64 KiB escape heuristic, `portable` vs `compact` profiles | `research/06 §4.2`, `§4.3` |
| S11 | Content-addressed deduplication by BLAKE3-256, master/derived two-level dedup, refcount GC, geometry dedup via `<defs>` + `<use>` | `research/06 §4.4` |
| S12 | Thumbnails (`thumbnail.png`, ≤ 512 px) and per-spread previews, behind a `ThumbnailProvider` trait | `research/06 §3.2.5`, `§3.2.6` |
| S13 | Unknown-data preservation: foreign baggage per node, unknown ZIP entries, comments and PIs, the preservation digest, the edit-marking rules | `research/06 §8` |
| S14 | Atomic write (temp + fsync + rename + dir fsync), optional `.bak` rotation | `research/06 §10.1` |
| S15 | Autosave to `$XDG_STATE_HOME`, NDJSON journal with fsync policy, crash recovery scan at startup | `research/06 §10.2`, `§10.3` |
| S16 | Advisory document lock with stale detection | `research/06 §10.4` |
| S17 | Read robustness: damaged-central-directory repair, malformed-XML partial recovery, digest mismatch reporting, hard limits against zip bombs and XML attacks | `research/06 §10.5` |
| S18 | MIME registration, `.desktop` file, thumbnailer, magic bytes; Windows/macOS declarations drafted but not installed | `research/06 §9` |
| S19 | Formal schemas (`manifest.rnc`, `xarast-doc.rnc`, `meta.rnc`) plus `.rng` generation and CI validation | `research/06 §11` |
| S20 | CLI verbs: `xarast extract`, `cat`, `validate`, `repair`, `convert` | `research/06 §13.1` |
| S21 | Determinism: identical model ⇒ byte-identical file, fixed DOS timestamps, canonical ordering | `research/06 §13.4(5)` |

### Explicitly out of scope (and which phase owns it)

| Item | Owner |
|---|---|
| Writing `.xar` | Nobody — an explicit non-goal (architecture §3.5, vision §4) |
| SVG *import* of third-party files (`usvg`-based), PNG/JPEG/WebP/PDF export | Phase 11 (`xarast-io`) — note that reading our own `document.svg` is this phase's job and is a different code path |
| Text serialisation beyond placeholders | Phase 9 — `research/06 §13.6` puts text at step 5 (v0.2) |
| Clip, mask and palette/CMYK mapping | Phase 9/Phase 8 per `research/06 §13.6` step 5 |
| Live-effect baking (`BakeProvider` implementations for shadow, bevel, contour, blend, mould) and exotic fills | Phase 13 — the trait and the marking rules land here, the implementations do not |
| `history/` snapshots and the `compact` (zstd) profile | v1.0 (`research/06 §13.6` step 7); the reader must tolerate both from day one |
| Digital signature and encryption | v1.1+ (`research/06 §14.5`) |
| Collaborative/incremental sync built on stable ids | post-1.0 |

---

## Prerequisites

| Need | Source | Hard or soft |
|---|---|---|
| `xarast-doc` with a per-node foreign-baggage container | Phase 2 | **Hard, and structural.** `research/06 §13.6` is explicit: if the model does not carry baggage from birth, retrofitting it means touching the whole model, parser and serialiser. If Phase 2 shipped without it, adding it is the first task of this phase |
| Stable persistent node ids in the model | Phase 2 | Hard |
| `.xar` importer producing real documents to save | Phase 3 | Hard (the corpus is the test material) |
| A renderer for thumbnails and for baking | Phase 4 | Soft — injected through `ThumbnailProvider`/`BakeProvider`, so `xarast-format` stays testable with no graphics stack |
| `xarast-geom` path types for the `d` serialiser | Phase 1 | Hard |

---

## Workstreams

### W1 — Container

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| F1.1 | `zip` 8.x wiring with `default-features = false, features = ["deflate", "zstd"]`, deflate backend pinned to `zlib-rs` for reproducibility | `xarast-format` | S | — |
| F1.2 | Writer: normative entry order; `mimetype` first, STORED, **no extra field** | `xarast-format` | M | F1.1 |
| F1.3 | Reader: signature sniff on 64 bytes; open without parsing `document.svg` | `xarast-format` | M | F1.1 |
| F1.4 | Entry-name validation: UTF-8 + EFS bit, no leading `/`, no `..`, no `\`, no control characters, no duplicates — reject, never sanitise | `xarast-format` | M | F1.3 |
| F1.5 | ZIP64 on either threshold (4 GiB, 65,535 entries) in both directions | `xarast-format` | M | F1.2 |
| F1.6 | `raw_copy_file` path for unchanged and preserved entries (no decompress/recompress) | `xarast-format` | M | F1.2 |
| F1.7 | Deterministic mode: fixed DOS mtime `1980-01-01`, fixed external attributes `0o644`, no extra fields, canonical order | `xarast-format` | M | F1.2 |
| F1.8 | Hard limits before allocation: 200:1 per-entry ratio, 4 GiB total, entry-count caps | `xarast-format` | M | F1.3 |

**Tricky parts.** The `mimetype` entry is the whole type-detection story and the `zip`
crate will happily add an extended-timestamp extra field that shifts the content off
offset 38. Build that one entry explicitly with
`SimpleFileOptions::default().compression_method(Stored).last_modified_time(fixed)` and
assert in a test that bytes 28..30 are `00 00` and 38..64 are the MIME string. It is a
three-line test that protects the format's identity.

`raw_copy_file` is not an optimisation either: it is the difference between re-saving a
300 MB photo document in 0.2 s and in 8 s, and it is also the only way to honour "do not
recompress preserved entries with a different method" (`research/06 §8.3` rule 2).

---

### W2 — Manifest and metadata

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| F2.1 | `Manifest` model, `quick-xml` writer/reader, namespace `https://xarast.org/ns/manifest/1.0` | `xarast-format` | M | W1 |
| F2.2 | Root `/` entry whose media type equals the `mimetype` content; one entry per ZIP entry | `xarast-format` | S | F2.1 |
| F2.3 | Roles, unknown-role tolerance (treat as `unknown`, preserve) | `xarast-format` | S | F2.1 |
| F2.4 | BLAKE3-256 digests, streaming, mandatory for `document`/`meta`/`resource` | `xarast-format` | M | F2.1 |
| F2.5 | Manifest/ZIP divergence detection → warn, offer read-only, never open silently | `xarast-format` | M | F2.4 |
| F2.6 | `meta.xml` model and serialisation: `doc-id`, revision, Dublin Core, units, pages, palette, guides and grid | `xarast-format` | L | F2.1 |
| F2.7 | Mirror of the metadata subset into `<metadata><rdf:RDF>` in the SVG, `meta.xml` authoritative on conflict | `xarast-format` | M | F2.6 |
| F2.8 | `min-reader` / capability declaration and the version-gating rules | `xarast-format` | M | F2.1 |

---

### W3 — SVG profile: write

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| F3.1 | Document tree emission: `<svg>` header, `viewBox` in points, physical `width`/`height`, namespace block, `xarast:document` with `y-axis="down"` | `xarast-format` | M | W2 |
| F3.2 | Structure: spreads, pages, layers → `<g>` with `xarast:` roles; z-order is document order | `xarast-format` | M | F3.1 |
| F3.3 | Geometry: `path`, `rect`, `circle`, `ellipse`, `line`, `polyline`, `polygon`; parametric shapes keep a `xarast:` sidecar | `xarast-format` | L | F3.2 |
| F3.4 | Path data serialiser: absolute vs relative, whichever is shorter; command collapsing; `h`/`v`/`s`/`t`; 3 decimals, no trailing zeros, no leading zero | `xarast-format` | L | F3.3 |
| F3.5 | Flat fills, strokes (caps, joins, miter limit, dashes), fill-rule | `xarast-format` | M | F3.3 |
| F3.6 | Linear and radial gradients with stops; `xarast:profile="<bias> <gain>"` plus the adaptively sampled baked stops and `xarast:stops` key list | `xarast-format` | L | F3.5 |
| F3.7 | Transparency and blend: `style="mix-blend-mode:…"` with the mandatory fallback, mirrored in `xarast:blend` | `xarast-format` | M | F3.5 |
| F3.8 | Images: `<image>` referencing `resources/…` with both `href` and `xlink:href` | `xarast-format` | M | F3.3, W5 |
| F3.9 | The eight serialiser passes as a pipeline, each independently testable and semantically neutral | `xarast-format` | L | F3.4–F3.8 |
| F3.10 | `BakeProvider` trait and the `xarast:generated*` marking discipline | `xarast-format` | M | F3.6 |
| F3.11 | Security: never emit `<script>`, `on*`, `<foreignObject>`, SMIL, external references, DTDs | `xarast-format` | S | F3.1 |

**Tricky parts.**

*Non-linear ramp profiles are the first place where "SVG cannot express this" bites.* SVG
interpolates linearly between stops; Xara applies a Schlick bias/gain curve. The rule
(`research/06 §6.4`): emit `xarast:profile="<bias> <gain>"`, plus baked intermediate stops
sampled adaptively until the maximum per-channel error against the true curve is ≤ 2/255,
with at least 9 and at most 33 stops per key-colour span — and on reading, **discard the
baked stops and recompute them**, or error accumulates across saves. The key stops go in
`xarast:stops="0:#1c3f8f 0.5:#7a2ea0 1:#ffd166"`, which is the recommended, compact form.
The curve itself is the same `Profile::map` that Phase 4 implements; share it, do not
reimplement it.

*Rule 5 of the dual-representation principle is the one people break:* if a thing is
exactly expressible in plain SVG, do **not** add a parametric twin. A flat-filled
rectangle is `<rect … fill="#c33"/>` and nothing more. Redundant `xarast:` attributes make
files bigger, diffs noisier and third-party edits more dangerous.

*The eight passes must be semantically neutral, and the round-trip test is what proves
it.* Any pass that changes the reconstructed model is a bug, no matter how much it saves.
Pass 5 (CSS classes) carries an extra constraint: CSS may only carry paint, never geometry
or structure, so that a third-party tool stripping `<style>` degrades colour and nothing
else.

---

### W4 — SVG profile: read, and preservation

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| F4.1 | `quick-xml` reader in preserving mode: `check_end_names(true)`, `trim_text(false)`, no entity expansion, DTD rejected | `xarast-format` | M | W3 |
| F4.2 | Parse the base + parametric pair; parametric wins; drop and regenerate `xarast:generated` subtrees | `xarast-format` | L | F4.1 |
| F4.3 | Orphan baked subtree handling: keep as ordinary editable geometry and warn — never delete art without a replacement | `xarast-format` | M | F4.2 |
| F4.4 | Foreign baggage capture: unknown foreign attributes, unknown foreign child subtrees (verbatim text plus sibling position), unknown SVG elements, unknown standard attributes, comments and PIs | `xarast-format` | L | F4.1 |
| F4.5 | Re-emission of baggage as raw bytes, in position, with the namespace declarations it needs | `xarast-format` | L | F4.4, W3 |
| F4.6 | Preservation digest: canonical concatenation, `xarast:foreign-digest` / `foreign-count`, verification on open with the user-facing warning | `xarast-format` | M | F4.4 |
| F4.7 | Edit-marking rules: `foreign-dirty`, `foreign-stale`, `base-authoritative`, and the deletion accounting | `xarast-format`, `xarast-doc` | M | F4.4 |
| F4.8 | Unknown ZIP entries preserved byte-identically, with their manifest rows; conservative GC that scans baggage for resource paths | `xarast-format` | M | F4.4, W1 |
| F4.9 | Duplicate-id detection and reassignment with a warning | `xarast-format` | S | F4.1 |
| F4.10 | "Open and close writes nothing" guarantee | `xarast-format` | M | F4.5 |

**Tricky parts.**

*Preservation is byte-level, not model-level.* Unknown subtrees are stored as the exact
bytes that were read, including prefixes and namespace declarations, and re-emitted as
raw (already-escaped) text — not re-serialised from a parsed tree, because re-serialising
normalises quoting and escaping and the digest then fails on a file we ourselves wrote.

*The conservative GC rule is deliberately over-inclusive:* a resource referenced only from
inside an unknown blob must not be collected, so the reader scans baggage text for strings
matching manifest entry paths and marks them referenced. Dragging one unnecessary resource
along is strictly better than breaking a document.

*Do not normalise what you did not touch* (`research/06 §8.6`): no re-indenting, no
`<rect>`→`<path>` conversion, no attribute reordering of untouched elements, no dropping
of apparently unused namespace declarations, no child reordering. F4.10 tests the strong
form of this: open a file and close it, and the bytes on disk must not change at all.

---

### W5 — Resources: dedup, compression, thumbnails

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| F5.1 | `ResourceId` = BLAKE3-256; streaming hashing on import | `xarast-format` | M | W1 |
| F5.2 | Resource index `hash → (path, refcount)` held for the document's lifetime; no rehashing of unchanged resources on save | `xarast-format` | M | F5.1 |
| F5.3 | Hash-derived names `b3-<hash32>.<ext>` in the normative subdirectories | `xarast-format` | S | F5.1 |
| F5.4 | Refcount GC with the preservation and history exemptions | `xarast-format` | M | F5.2, F4.8 |
| F5.5 | Master/derived two-level dedup with `mf:derived-from` and `mf:derivation`; `--no-derived` regeneration path | `xarast-format` | M | F5.2 |
| F5.6 | Compression policy table plus the 64 KiB / ratio-1.05 escape heuristic | `xarast-format` | M | W1 |
| F5.7 | Geometry dedup: identical subtrees modulo an affine transform → `<defs>` + `<use>`, thresholds ≥ 3 repeats or ≥ 512 bytes of `d` | `xarast-format` | L | W3 |
| F5.8 | `ThumbnailProvider` trait; thumbnail ≤ 512 px (256 px preferred), RGBA8, transparent background with page colour composited beneath | `xarast-format` | M | W1 |
| F5.9 | Per-spread previews, optional, `--no-previews` | `xarast-format` | S | F5.8 |
| F5.10 | `data:` URI policy: allowed under 4 KiB, never above | `xarast-format` | S | W3 |

---

### W6 — Durability: atomic save, lock, autosave, journal, recovery

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| F6.1 | `save_atomic`: temp in the same directory, fsync file, rename, fsync directory, remove autosave and journal; original untouched on failure | `xarast-format` | M | W1 |
| F6.2 | Optional single-rotation `.bak` | `xarast-format` | S | F6.1 |
| F6.3 | `DocumentLock`: `O_CREAT|O_EXCL` plus advisory `flock`, the documented key/value contents, stale detection by host + boot-id + pid | `xarast-format` | M | — |
| F6.4 | Lock UX contract: read-only / open a copy / force, and read-only directories must still open | `xarast-format`, `xarast-app` | M | F6.3 |
| F6.5 | `AutosaveSession`: full valid `.xarast` snapshots to `$XDG_STATE_HOME/xarast/autosave/<doc-id>/`, every 5 minutes or 50 undo operations, off the UI thread, atomic | `xarast-format` | L | F6.1 |
| F6.6 | NDJSON journal, append-only, fsync per line or batched at ≤ 250 ms; truncate on autosave, delete on real save; non-representable ops force an immediate snapshot | `xarast-format` | L | F6.5 |
| F6.7 | Startup recovery scan: dead-pid / other-host detection, date comparison against the on-disk file, snapshot + journal replay | `xarast-app` | M | F6.6 |
| F6.8 | `xarast repair`: local-header scan when the central directory is damaged, digest validation, rebuild | `xarast-cli` | L | W1 |
| F6.9 | Malformed-XML partial recovery, read-only open, corrupt resources replaced by a visible marker | `xarast-format` | M | F4.1 |

**Tricky parts.** The autosave must serialise from an *immutable snapshot* of the model on
another thread — which is exactly why architecture §3.1 keeps `imbl` checkpoints over the
arena. If autosave blocks the UI for a second every five minutes, users will turn it off,
and then the feature is worse than absent.

Journal replay must be idempotent and must detect a journal that does not belong to its
snapshot (`seq` continuity), or recovery turns into corruption.

---

### W7 — Integration, schemas, tooling

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| F7.1 | `packaging/linux/xarast.xml` shared-mime-info with the three-level magic at offsets 0/30/38, priority 80, `sub-class-of application/zip` | `packaging/` | S | W1 |
| F7.2 | `org.xarast.Xarast.desktop` with the MIME list and the New Document action | `packaging/` | S | F7.1 |
| F7.3 | `xarast-thumbnailer`: extracts and rescales `thumbnail.png` only — never opens or renders the document | `xarast-cli` | M | F5.8 |
| F7.4 | MIME + desktop + thumbnailer registration inside the AppImage | `packaging/` | M | F7.1–F7.3 |
| F7.5 | Windows registry and macOS UTI declarations, drafted and committed but not installed | `packaging/` | S | F7.1 |
| F7.6 | `schemas/manifest.rnc`, `xarast-doc.rnc`, `meta.rnc` + `.rng` generation via `trang` | `xarast-format/schemas` | L | W2, W3 |
| F7.7 | CI validation: `xmllint --relaxng` for manifest, meta and SVG 1.1, plus `xarast-validate` for the `xarast:` vocabulary | CI | M | F7.6 |
| F7.8 | CLI: `extract`, `cat`, `validate`, `repair`, `convert` | `xarast-cli` | M | W1–W6 |

---

## Public API introduced

```rust
// ─────────────────────────── crates/xarast-format/src/lib.rs
#![forbid(unsafe_code)]

pub const FORMAT_VERSION: Version = Version { major: 1, minor: 0 };
pub const MIME_TYPE: &str = "application/vnd.xarast+zip";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version { pub major: u16, pub minor: u16 }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile { Portable, Compact }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method { Stored, Deflate, Zstd }

/// BLAKE3-256 of the uncompressed resource bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceId(pub [u8; 32]);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind { Image, Derived, Baked, Font, Profile, Brush, Blob }

// ───────── reading

pub struct XarastReader<R: Read + Seek> { /* … */ }

impl<R: Read + Seek> XarastReader<R> {
    /// Validates the signature and reads manifest + meta. Does NOT parse document.svg.
    pub fn open(reader: R) -> Result<Self, ReadError>;
    /// Type detection from the first 64 bytes only.
    pub fn sniff(reader: &mut R) -> Result<bool, ReadError>;

    pub fn format_version(&self) -> Version;
    pub fn min_reader(&self) -> Version;
    pub fn profile(&self) -> Profile;
    pub fn manifest(&self) -> &Manifest;
    pub fn meta(&self) -> &DocumentMeta;

    pub fn thumbnail(&mut self) -> Result<Option<Vec<u8>>, ReadError>;
    pub fn preview(&mut self, spread: u32) -> Result<Option<Vec<u8>>, ReadError>;

    pub fn entry(&mut self, path: &str) -> Result<Vec<u8>, ReadError>;
    pub fn entry_stream(&mut self, path: &str) -> Result<impl Read + '_, ReadError>;
    pub fn resource(&mut self, id: ResourceId) -> Result<Vec<u8>, ReadError>;
    pub fn entries(&self) -> impl Iterator<Item = &FileEntry>;

    /// Parses the whole document into the model. This is where the cost is paid.
    pub fn document(&mut self, opts: &LoadOptions) -> Result<LoadedDocument, ReadError>;

    /// Everything the reader did not understand, needed to rewrite without loss.
    pub fn into_preservation(self) -> PreservationContext;
}

#[derive(Debug, Clone, Default)]
pub struct LoadOptions {
    pub rebake: bool,        // regenerate xarast:generated subtrees (default true)
    pub limits: Limits,      // zip-bomb / XML limits (§10.5)
    pub strict: bool,        // reject rather than warn on digest mismatch
}

pub struct LoadedDocument {
    pub document: xarast_doc::Document,
    pub preservation: PreservationContext,
    pub diagnostics: Vec<Diagnostic>,   // warnings, not errors
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_entry_ratio: u32,       // default 200
    pub max_total_uncompressed: u64,// default 4 GiB
    pub max_xml_depth: u32,         // default 256
    pub max_entries: u32,
}

// ───────── writing

pub struct XarastWriter<W: Write + Seek> { /* … */ }

#[derive(Debug, Clone)]
pub struct WriteOptions {
    pub profile: Profile,
    pub deflate_level: u8,          // 0..=9,  interactive 6, optimised 9
    pub zstd_level: i32,            // -7..=22, interactive 10, optimised 19
    pub deterministic: bool,        // fixed mtimes, canonical order, no extra fields
    pub fixed_mtime: Option<OffsetDateTime>,
    pub pretty: bool,               // indent the SVG for git diffs
    pub thumbnail: bool,
    pub previews: bool,
    pub embed_fonts: bool,
    pub materialize_derived: bool,
    pub history: HistoryPolicy,
}

impl<W: Write + Seek> XarastWriter<W> {
    pub fn new(writer: W, opts: WriteOptions) -> Self;
    /// Without this, the result is NOT a lossless round-trip and `finish` says so.
    pub fn with_preservation(self, ctx: PreservationContext) -> Self;
    pub fn with_thumbnail_provider(self, p: Arc<dyn ThumbnailProvider>) -> Self;
    pub fn with_bake_provider(self, p: Arc<dyn BakeProvider>) -> Self;

    pub fn write_document(&mut self, doc: &xarast_doc::Document) -> Result<(), WriteError>;
    pub fn add_resource(&mut self, kind: ResourceKind, bytes: &[u8])
        -> Result<ResourceId, WriteError>;
    pub fn finish(self) -> Result<WriteReport, WriteError>;
}

pub struct WriteReport {
    pub bytes_written: u64,
    pub entries: usize,
    pub resources_deduplicated: usize,
    pub bytes_saved_by_dedup: u64,
    pub preservation_complete: bool,
    pub warnings: Vec<Diagnostic>,
}

/// Atomic save: temp + fsync + rename + directory fsync (§10.1).
pub fn save_atomic(
    path: &Path,
    doc: &xarast_doc::Document,
    ctx: Option<&PreservationContext>,
    opts: &WriteOptions,
) -> Result<WriteReport, WriteError>;

// ───────── preservation

#[derive(Debug, Clone, Default)]
pub struct PreservationContext {
    pub node_baggage: HashMap<NodeId, ForeignBaggage>,
    pub unknown_entries: Vec<RawZipEntry>,      // compressed stream kept intact
    pub meta_baggage: Vec<ForeignFragment>,
    pub comments: Vec<PositionedNode>,
    pub foreign_digest: Option<[u8; 32]>,
    pub foreign_count: usize,
}

impl PreservationContext {
    pub fn digest(&self) -> [u8; 32];
    pub fn verify(&self, stored: &[u8; 32]) -> Result<(), PreservationLoss>;
}

#[derive(Debug, Clone, Default)]
pub struct ForeignBaggage {
    pub attrs: Vec<(String, String, String)>,   // (namespace uri, local name, value)
    pub children: Vec<PositionedFragment>,      // verbatim bytes + sibling index
    pub dirty: bool,
    pub stale: bool,
    pub base_authoritative: bool,
}

// ───────── durability

pub struct DocumentLock { /* … */ }
impl DocumentLock {
    pub fn acquire(doc_path: &Path) -> Result<Self, LockError>;
    pub fn holder(&self) -> Option<&LockHolder>;
    pub fn steal_if_stale(doc_path: &Path) -> Result<Self, LockError>;
}

pub struct AutosaveSession { /* … */ }
impl AutosaveSession {
    pub fn begin(doc_id: &DocId) -> Result<Self, IoError>;
    pub fn snapshot(&mut self, doc: &xarast_doc::Document) -> Result<(), IoError>;
    pub fn journal(&mut self, op: &JournalEntry) -> Result<(), IoError>;
    pub fn discard(self) -> Result<(), IoError>;
    pub fn recoverable() -> Result<Vec<RecoveryCandidate>, IoError>;
}

// ───────── the SVG profile (module `svg` inside xarast-format)

pub struct SvgProfile { /* serialisation options */ }

pub fn write_svg(
    doc: &Document, ctx: Option<&PreservationContext>,
    bake: &dyn BakeProvider, opts: &SvgProfile, out: &mut dyn Write,
) -> Result<SvgWriteStats, SvgError>;

pub fn read_svg(
    input: &[u8], limits: &Limits,
) -> Result<(Document, PreservationContext, Vec<Diagnostic>), SvgError>;

/// All baking (§5.4, §6) lives behind this, so the format crate needs no renderer.
pub trait BakeProvider: Send + Sync {
    fn bake_gradient_profile(&self, g: &GradientSpec) -> Vec<GradientStop>;
    fn bake_effect(&self, e: &LiveEffect, target: &Node) -> BakedSubtree;
    fn bake_fill(&self, f: &Fill, bounds: Rect) -> BakedFill;   // may rasterise
    fn bake_stroke(&self, s: &Stroke, path: &Path) -> BakedSubtree;
}

pub trait ThumbnailProvider: Send + Sync {
    fn thumbnail(&self, doc: &Document, max_px: u32) -> Result<Vec<u8>, BakeError>;
    fn preview(&self, doc: &Document, spread: u32, max_px: u32)
        -> Result<Vec<u8>, BakeError>;
}
```

---

## Acceptance criteria

1. **Signature at fixed offsets.** `cargo test -p xarast-format magic_bytes` asserts, on a
   freshly written empty document: bytes 0..4 = `PK\x03\x04`, 28..30 = `00 00` (no extra
   field), 30..38 = `mimetype`, 38..64 = `application/vnd.xarast+zip`.
2. **Byte-level round trip.** `cargo test -p xarast-format roundtrip_bytes` asserts, for
   every corpus document, that `save_deterministic(load(save_deterministic(doc)))` equals
   the first save **byte for byte** — a fixed point on the first re-save.
3. **Open and close writes nothing.** `cargo test -p xarast-format open_close_no_write`
   asserts the file's mtime and content hash are unchanged after opening and closing
   without edits.
4. **Structural round trip.** `cargo test -p xarast-format roundtrip_model` asserts
   `load(save(d)).canonical() == d.canonical()` over the corpus.
5. **100 % preservation.** `cargo test -p xarast-format preservation` injects attributes
   and elements from `urn:test:future` into every node of every corpus document, opens,
   applies a battery of edits (move, recolour, group, change layer, undo, redo), saves,
   and asserts every injected item is still present, on the same element, in the same
   relative position. Also with unknown ZIP entries and unknown `meta.xml` sections.
   **Zero tolerance.**
6. **Preservation digest detects third-party loss.** A test strips one foreign attribute
   externally, reopens, and asserts the reader raises `PreservationLoss` with the correct
   count.
7. **Browser rendering.** `cargo test --test svg_conformance -- --engine chromium`
   extracts `document.svg` from each corpus file, renders it with headless Chromium, and
   asserts the SSIM thresholds of `research/06 §5.6`: ≥ 0.99 geometry-and-simple-gradients,
   ≥ 0.95 with transparency/clip/mask/text, ≥ 0.90 with live effects, ≥ 0.85 with fractal
   or conical fills, and **corpus mean ≥ 0.90**. Below threshold fails CI.
8. **Inkscape rendering.** The same harness with `inkscape --export-type=png` meets the
   same thresholds. When Inkscape is not installed, the job is skipped **loudly** and the
   release checklist requires a manual run.
9. **resvg rendering.** The same harness with `resvg` — this one always runs, since it is
   a Rust dependency and needs no external binary.
10. **Schema validation.** `make validate-corpus` runs `xmllint --relaxng` over
    `META-INF/manifest.xml`, `meta.xml` and `document.svg` (against the official SVG 1.1
    schema) for every generated file, plus `xarast-validate` over the `xarast:` subtree.
    Zero errors.
11. **Deduplication.** `cargo test -p xarast-format dedup` builds a document using one
    image eight times and asserts exactly one entry under `resources/images/` and
    `bytes_saved_by_dedup > 7 × image_len − 4096`.
12. **Compression policy.** `cargo test -p xarast-format compression` asserts JPEG, PNG,
    WebP and WOFF2 resources are `Stored`; that XML and SVG are `Deflate`; and that an
    incompressible unknown blob takes the STORED branch of the 64 KiB heuristic.
13. **Atomic save.** A fault-injection test that fails the write at each of ten points
    asserts the original file is byte-unchanged in every case and that no `.tmp-` file
    survives a clean failure path.
14. **Crash recovery.** A test kills the process (SIGKILL) mid-edit, restarts, and asserts
    the recovery scan offers the snapshot, that journal replay reproduces the operations
    after the snapshot, and that the recovered document matches the pre-kill model.
15. **Locking.** Two processes opening the same file: the second is offered read-only /
    copy / force; a lock whose pid is dead on the same host and boot-id is reclaimed
    automatically; a read-only directory still opens the document.
16. **Robustness limits.** `cargo test -p xarast-format limits` asserts rejection of: a
    zip-slip name, a duplicate entry, an entry exceeding 200:1, a total above 4 GiB, XML
    nested beyond 256, and any DOCTYPE or entity.
17. **Repair works.** `xarast repair` reconstructs a package whose central directory has
    been truncated, and the repaired file passes criteria 1, 4 and 10.
18. **Fuzzing.** `fuzz_open`, `fuzz_document` and `fuzz_roundtrip` each run 24 h with no
    crash, no panic, no OOM and no timeout before the phase closes; corpora are cached in
    CI and the run is logged in the phase note.
19. **Desktop integration.** In a clean container, installing the AppImage's MIME and
    desktop files makes `xdg-mime query filetype doc.xarast` report
    `application/vnd.xarast+zip`, and `xarast-thumbnailer` produces a PNG **without**
    loading the document (asserted by tracing that no SVG parse happens).
20. **Save budget.** `cargo bench -p xarast-format -- save_20mb` reports ≤ 1 s for a 20 MB
    document (roadmap budget), and a re-save of a 300 MB photo document with unchanged
    resources completes in ≤ 1 s thanks to `raw_copy_file`.
21. **No unsafe.** `#![forbid(unsafe_code)]` is present in the crate and CI enforces it.

---

## Performance budgets

| Budget | Target | Measured how |
|---|---|---|
| Save a 20 MB `.xarast` | ≤ 1 s | `criterion`; roadmap budget |
| Re-save a 300 MB document with unchanged resources | ≤ 1 s | `criterion`; depends on `raw_copy_file` and on not rehashing |
| Open: signature + manifest + meta + thumbnail | ≤ 15 ms for a 20 MB file | `criterion`; this is what O6 (cheap open) means |
| Full parse of `document.svg`, 1,800 objects | ≤ 120 ms | `criterion` |
| Full parse, 50,000 objects | ≤ 2 s | `criterion` |
| Autosave snapshot of a 20 MB document, off-thread | ≤ 700 ms, 0 ms of UI stall | instrumented test |
| Journal append with fsync | ≤ 2 ms per entry, or batched at ≤ 250 ms | instrumented test |
| BLAKE3 hashing throughput | ≥ 1 GB/s per core | `criterion` |
| Thumbnail extraction by `xarast-thumbnailer` | ≤ 20 ms | `hyperfine` |
| SVG size overhead of the eight passes | ≥ 35 % smaller than naive emission, before compression | reported by `SvgWriteStats` over the corpus |

---

## Risks and mitigations

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| K1 | The document model arrives without per-node foreign baggage | Medium | **Very high** | Checked as the first task of the phase; if absent, adding it precedes everything else. `research/06 §13.6` is explicit that retrofitting costs the whole model, parser and serialiser |
| K2 | Byte-identical re-save is defeated by library-inserted extra fields or timestamps | High | Medium | Deterministic mode with fixed mtime and attributes; a byte-diff test on every corpus file; the `mimetype` extra-field assertion |
| K3 | `quick-xml` loses information needed for verbatim re-emission | Medium | High | Preserving-mode configuration verified by a dedicated test that reads and rewrites a file containing exotic quoting, CDATA, comments and PIs; baggage stored as raw bytes, never re-serialised |
| K4 | SSIM thresholds are unreachable for some feature | Medium | Medium | The three baking strategies are graded (geometry → filter → raster); a feature that cannot hit its band gets rasterised at 2× nominal resolution, which always can. Record the fallback per feature in memory |
| K5 | External renderers (Chromium, Inkscape) are unavailable or non-deterministic in CI | High | Medium | `resvg` is the always-on gate; Chromium is pinned by container digest; Inkscape is best-effort in CI and mandatory on the release checklist |
| K6 | Zstd in the `compact` profile produces files other tools cannot read | Medium | Low | `portable` (stored + deflate only) is the default and must be explicitly overridden; method 93 only, never 20; `mf:profile` and `min-reader` declare it |
| K7 | Autosave stalls the UI | Medium | High | Serialise from an immutable snapshot on the I/O thread; assert 0 ms UI stall in an instrumented test |
| K8 | Journal replay corrupts a recovered document | Low | Very high | `seq` continuity checks, idempotent application, and a recovery test that compares against the pre-crash model exactly |
| K9 | The conservative GC keeps growing files that never shrink | Medium | Low | Report retained-but-unreferenced bytes in `WriteReport`; surface them in `xarast validate`; offer an explicit "purge unknown resources" that requires confirmation |
| K10 | The `xarast:` vocabulary drifts from what the model actually stores | Medium | Medium | The RELAX NG schema is the contract and CI validates every generated file against it; adding a model field without a schema change fails the build |

---

## Test plan

**Unit.** Entry-name validation table (valid/invalid pairs); manifest round-trip;
digest computation; number formatting (`12.5` not `12.500`, `.5` not `0.5`, exponent only
when shorter); path-data serialiser against hand-computed expectations, including the
relative-vs-absolute choice; each of the eight passes in isolation, with a before/after
model equality assertion; compression heuristic decisions per MIME type; profile ramp
baking error bound (≤ 2/255, 9–33 stops).

**Property (`proptest`).** Generate random documents from the model and assert
`load(save(d)) == d`, `save(load(save(d))) == save(d)`, and that no combination of
`WriteOptions` changes the recovered model (only the bytes).

**Snapshot (`insta`).** The serialised SVG for a curated set of small documents, so that
any change to the emitter shows up as a reviewable text diff rather than as a silent
behaviour change.

**Conformance.** The SSIM harness against `resvg` (always), Chromium (pinned container)
and Inkscape (best effort, mandatory at release); the schema validation sweep; the
`research/06 §14.4` checklist automated as `xarast validate --strict`.

**Durability.** Fault injection at ten points in the save sequence; SIGKILL recovery;
two-process locking; stale-lock reclamation; read-only directory.

**Fuzzing.** `fuzz_open`, `fuzz_document`, `fuzz_roundtrip`, 24 h each before the phase
closes and 30 min per target nightly thereafter, with cached corpora.

**Interoperability (manual, per release).** Open in Inkscape, move an object, save, reopen
in Xarast: layers, guides and **every** `xarast:` extension still present. This is the
real-world check that §8.4 exists for, and no automated test substitutes for it.

---

## Memory note

Update **`docs/memory/xarast-format.md`** (create it from the `INDEX.md` template). By the
end of the phase it must record:

- **Current state:** which parts of the SVG profile are implemented, which mapping table
  rows of `research/06 §6` are still unmapped, and the current conformance level reached
  (target: B for the v0.1 subset, A complete).
- **Decisions:** `portable` as the default profile; deflate backend pinned for
  reproducibility; the deterministic-write recipe; the verbatim-bytes baggage strategy and
  why re-serialising fails; the conservative GC rule; which features are baked with which
  of the three strategies, and the SSIM each achieves.
- **Invariants that must not be broken:** `mimetype` first, STORED, no extra field,
  content at offset 38; one manifest entry per ZIP entry; every resource reference
  relative and inside the package; unknown data is preserved in place and re-emitted;
  open-and-close writes nothing; never normalise untouched markup; no `<script>`, no DTD,
  no external references; ids are stable for the lifetime of an object.
- **Dead ends:** JSON manifest (rejected, `§3.5`); `data:` URIs above 4 KiB; recompressing
  already-compressed resources; `roxmltree` as the main parser (discards what the
  round-trip needs); zstd method id 20.
- **Open TODOs:** text, clip/mask, palette and CMYK mapping (v0.2); live-effect baking and
  exotic fills (v0.3); `history/` and the `compact` profile (v1.0); the IANA registration
  request; Windows/macOS integration.

Also note in **`docs/memory/document-model.md`** that the foreign-baggage container is now
load-bearing and may not be removed, and in **`docs/memory/packaging.md`** the MIME,
desktop and thumbnailer files the AppImage now ships.
