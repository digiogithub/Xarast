# Phase 3 — `.xar` importer

> After this phase every one of the 59 corpus files opens: the bytes are read,
> the tree is rebuilt, the tags that matter are mapped into a validated
> `Document`, and `xar-dump` can say exactly what is in any `.xar` file — while
> a fuzzer throws hostile input at the parser and it neither panics nor grows
> without bound.

## Goal

Ship `xarast-xar`, a read-only importer for the Camelot eXchange Format, and
`xar-dump`, the CLI that makes it inspectable. `.xar` is **read-only** —
`docs/10-architecture.md §3.5` overrides `research/04` on this and the non-goal
stands.

The work follows the empirical priority order of `research/01 §10.6`, which was
derived from measuring 1 390 282 records across the 59-file corpus:

1. Physical reader — magic, records, raw deflate, CRC. Validates all 59 files.
2. `DOWN`/`UP` tree and tag dump — this is `xar-dump`.
3. Colours, then relative paths, then line and flat-fill attributes. At this
   point something meaningful is already extractable.
4. Gradients and transparencies, regular shapes, groups and layers.
5. Bitmaps, then text.

Fuzzing is **part of this phase, not a follow-up**. A legacy binary format
parser is attack surface; `docs/00-vision-and-scope.md §3.5` and `CLAUDE.md`
both make it a day-one requirement.

## Scope

### In scope

- The physical layer: magic, `TAG_FILEHEADER`, record framing, raw-deflate
  state machine, CRC-32 and length verification, streamed (uncompressed)
  records, `TAG_ENDOFFILE`.
- Record numbering and the definition/reference resolution it powers.
- `DOWN`/`UP` tree reconstruction, tolerant of imbalance.
- `TAG_ATOMICTAGS` and `TAG_ESSENTIALTAGS` handling, including subtree
  stripping — implemented from the start, because files from newer Xara
  versions depend on it.
- The ~45 tags that cover 99.2 % of corpus records and appear across 100 % of
  corpus files, mapped into `xarast-doc` through `DocumentBuilder`.
- Bitmap definitions and bitmap nodes; the embedded PNG/JPEG/GIF/BMP bytes are
  preserved verbatim as `BitmapResource::original`.
- Text records mapped into the `TextStory`/`TextLine`/`TextItem` tree
  **structurally**; no shaping, no layout, no measurement.
- `xar-dump`: records, tree, statistics, tag histogram, model dump, JSON output,
  batch mode over a corpus directory.
- Five fuzz targets with explicit invariants and hard resource caps.
- A corpus harness that runs all 59 files and produces the acceptance numbers.

### Explicitly out of scope (and which phase owns it)

| Out of scope | Owner |
|---|---|
| Writing `.xar` | Nobody. Permanent non-goal (`10-architecture.md §3.5`) |
| Rendering anything | Phase 4 |
| Decoding the embedded bitmap bytes into pixels | Phase 10 (`xarast-image`); this phase stores the bytes and the declared metadata |
| Text shaping, layout, measurement, kerning resolution | Phase 9 |
| Live-object regeneration: blends, moulds, contours, shadows, bevels, ClipView. Their records are read into `NodeKind::Live`/`Opaque` and round-trip, but nothing regenerates them | Phase 13 |
| Brushes and stroke types (tags 4000–4003, 4079–4083, 4102–4113) — one corpus file, very complex, and the base path is present regardless | Phase 13 |
| Printing and imagesetting (3500–3510) | Never; skipped deliberately |
| The legacy regular-shape families (1000–1217, 1900) — zero occurrences in the corpus | Deferred; see risk 9 |
| Absolute path tags 100–103 as *top-level nodes* (zero occurrences); the absolute path **codec** is in scope because embedded paths use it | This phase (codec), deferred (tags) |
| `.web`/`CXW` and `CXM` specific behaviour beyond accepting the type byte | This phase accepts all three; nothing else differs |
| The Flare template text format (`CXaraTemplateFile`) | Out of scope permanently |

## Prerequisites

- **Phase 1** closed: `Mp` with its overflow contract, `Point`, `Matrix`,
  `Path`, `PathBuilder`, `Fixed24`, `ColourDef`, `ColourTable`, `Transparency`.
- **Phase 2** closed: `DocumentBuilder`, `BuildLimits`, `Diagnostic`,
  `DiagCode`, `NodeKind`, `AttrValue` with the finalised `AttrSlot` set,
  `FillGeometry`, `DocumentResources`, `Document::validate()`.
  In particular criterion 13 of Phase 2 — the complete `.xar` tag → `AttrSlot`
  mapping — must be done, or this phase will redo it badly.
- The corpus fixture from Phase 0, and the 59-file `corpus.lock`.

Phase 3 runs in parallel with Phase 4 (`docs/phases/00-roadmap.md`); they share
no crate.

## Workstreams

### W3.1 — The physical reader

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 3.1.1 | `Cur`: a bounds-checked little-endian payload cursor with the CXF primitive types | `xarast-xar` | M | — |
| 3.1.2 | Magic check and `TAG_FILEHEADER` parsing, tolerant of missing strings | `xarast-xar` | S | 3.1.1 |
| 3.1.3 | `RecordReader`: framing, record numbering, `TAG_ENDOFFILE` | `xarast-xar` | M | 3.1.2 |
| 3.1.4 | Deflate state machine: `TAG_STARTCOMPRESSION` / `TAG_ENDCOMPRESSION` | `xarast-xar` | L | 3.1.3 |
| 3.1.5 | CRC-32 and length verification, trailer repositioning | `xarast-xar` | M | 3.1.4 |
| 3.1.6 | Resource caps: per-record, per-block and total inflated byte limits | `xarast-xar` | M | 3.1.4 |
| 3.1.7 | Corpus validation of the physical layer alone | `xarast-xar` | S | 3.1.5 |

**Framing.** Eight bytes of magic — `58 41 52 41 A3 A3 0D 0A` — then a flat
sequence of records, each `tag: u32 LE`, `size: u32 LE`, then exactly `size`
payload bytes. No alignment, no padding, no table of contents, no offsets. The
only pointer the format has is the **record number**: the 1-based ordinal of a
record in the file, counting every record including `UP`, `DOWN`, the header
and everything inside the compressed blocks.

`#[repr(C)]` structs are useless for payloads: fields are unaligned (a `u8`
after four `i32`s in `TAG_SPREADINFORMATION`, five loose bytes before a `u32`
in `TAG_DEFINECOMPLEXCOLOUR`). Everything is read field by field through `Cur`.

**The header must be read permissively.** `Templates/animation.xar` has a
36-byte header in which all three producer strings are empty or truncated
(`research/01 §12.2`). The reader must not require them. `precompression_flags`
at offset 11 **must** be 0; any other value is a hard error, matching the
original.

**Compression.** From immediately after `TAG_STARTCOMPRESSION`'s 4-byte payload
to the end of `TAG_ENDCOMPRESSION`, the stream is **raw DEFLATE** — RFC 1951
with no zlib and no gzip wrapper, i.e. `windowBits = -15`. In Rust:
`flate2::Decompress::new(false)`. Getting the wrapper wrong is the single most
likely way to waste a day here, and the symptom is an immediate "invalid
header" on every file.

The 4-byte payload is a version word; the corpus value is always 99. The high
byte is the compression type and must be 0; anything else gets a `Warning`
diagnostic, not an abort, because the original does not validate it either.

**`TAG_ENDCOMPRESSION` is the subtle part** and it deserves the space:

1. The record *header* (`tag = 31`, `size = 8`) is written **inside** the
   deflate stream, so the reader sees it as an ordinary record while inflating.
2. The 8 bytes of payload are **not** in the deflate stream. They sit
   uncompressed in the file, immediately after the last byte the inflater
   consumed.
3. Those 8 bytes are `u32 LE crc32` and `u32 LE uncompressed_length`, both over
   the block's plain bytes — *including* the 8-byte header of
   `TAG_ENDCOMPRESSION` itself.

So on seeing tag 31 the reader must ask the inflater how many bytes of the
physical file it actually consumed (`flate2::Decompress::total_in()`), seek
there, switch back to plain mode, and read the 8 trailer bytes. The original
does the same adjustment. The CRC is the standard zlib CRC-32 (polynomial
`0xEDB88320`, init 0, final xor).

**Multiple blocks per file.** Streamed records — bitmap and sound definitions —
close the compressed block before writing and open a new one afterwards, which
is why 59 files contain 103 compressed blocks. The reader needs no special case
for streamed records: implement start/end correctly and it all falls out. This
was verified empirically (`research/01 §3.1`).

**Stopping.** `TAG_ENDOFFILE` stops the parse. The reader must not assume
`pos == len` afterwards; the corpus happens to have zero trailing bytes in all
59 files but the format does not promise it.

**Resource caps** (these are what make the fuzz invariants achievable):

- A record's payload allocation is `min(declared_size, bytes_actually_available)`.
  **Never** `Vec::with_capacity(declared_size)` — a 4 GB declared size in a
  12-byte file must cost nothing.
- Per compressed block, inflated output is capped at
  `max(64 MiB, 200 × compressed_bytes)`. Exceeding it aborts the block with
  `DiagCode::LimitExceeded`. This is the decompression-bomb defence.
- Total inflated bytes across the file are capped at `BuildLimits::max_bytes`.
- Forward progress is structural: every record consumes at least its 8-byte
  header from the stream, so a record loop cannot spin. Assert it anyway.

### W3.2 — Record tree and tag policy

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 3.2.1 | `RecordTree` from the `DOWN`/`UP` stream, tolerant of imbalance | `xarast-xar` | M | 3.1.3 |
| 3.2.2 | `TAG_ATOMICTAGS` / `TAG_ESSENTIALTAGS` accumulation | `xarast-xar` | S | 3.1.3 |
| 3.2.3 | Unknown-tag policy: skip, strip subtree, or abort | `xarast-xar` | M | 3.2.2 |
| 3.2.4 | Definition table keyed by record number; `Ref` resolution | `xarast-xar` | M | 3.1.3 |
| 3.2.5 | Coordinate origin tracking from `TAG_SPREADINFORMATION` | `xarast-xar` | M | 3.2.1 |
| 3.2.6 | Tag classification table (structural / attribute / definition / ignorable) | `xarast-xar` | M | 3.2.3 |

The file is a pre-order walk: an object record creates a node as the next
sibling at the current level; `TAG_DOWN` descends; `TAG_UP` ascends. The corpus
has exactly 201 121 of each, perfectly balanced, but a truncated file will not,
so unmatched `UP`s are ignored and unclosed `DOWN`s are closed implicitly at
end of file, each with a `DiagCode::UnbalancedScope` diagnostic.

Two special cases from the original, both worth honouring:

- A **layer** record inserted while the current context is already a layer
  becomes a *sibling*, not a child.
- **Attributes are children of the object they apply to**, emitted inside the
  object's own `DOWN`/`UP`.

**The three-way unknown-tag policy is not optional.** `research/01 §11` item 7
warns that `TAG_SPREAD_PHASE2` (4131), `TAG_CURRENTATTRIBUTES_PHASE2` (4132)
and everything above 4138 can appear in files from newer commercial versions,
and that the atomic/essential mechanism exists precisely for that. Implement it
now or corrupt trees later:

| Case | Action | Diagnostic |
|---|---|---|
| Tag is in the **essential** list and we have no handler | Abort the import | `EssentialTagMissing`, `Error` |
| Tag is in the **atomic** list and we have no handler | Skip the record **and its entire subtree** (everything up to the `UP` matching the `DOWN` that follows it) | `AtomicSubtreeDropped`, `Warning` |
| Any other unknown tag | Skip the record's `size` bytes, continue | `UnknownTag`, `Info` |

The reason atomic nodes must take their subtree with them: a shadow controller's
children are the shadow record *and a duplicate of the shadowed object*. Insert
those loose and the document gains a phantom object painted as if it were real
(`research/01 §11` item 13).

The atomic list is accumulated from **many** `TAG_ATOMICTAGS` records, not one:
the corpus has 739 such records of 4 bytes each, one tag apiece. Read `size / 4`
entries per record and union them all.

**Definitions and references.** Colours, bitmaps, fonts, paths and units are
referenced by record number. A `REFERENCE` is a signed `i32`: `> 0` is a record
number, `< 0` is a predefined built-in, `0` is null/error. Definitions are
always written before first use — the original guarantees it — so a single pass
with an incrementally filled `HashMap<u32, Definition>` is sufficient. A
dangling reference is a `DanglingReference` diagnostic and a documented
fallback (black for colours, the default bitmap for bitmaps), never a failure.

**The coordinate origin.** Record coordinates are relative to the `lo` corner
of the rectangle enclosing every page in the spread, and the origin is updated
when `TAG_SPREADINFORMATION` is processed. The subtlety that bites: *points*
get the origin added, *vectors* do not. The major and minor axes of regular
shapes are written untranslated (`research/01 §5.3`). Phase 1's separate
`transform_point`/`transform_vector` split exists for exactly this; the importer
mirrors it with `read_point()` and `read_vector()` on `Cur`, and never exposes a
single "read a coordinate" call.

### W3.3 — `xar-dump`

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 3.3.1 | CLI skeleton, argument parsing, exit codes | `xarast-cli` | S | 3.1.3 |
| 3.3.2 | `--records`, `--tree`, `--tags`, `--stats` output modes | `xarast-cli` | M | 3.2.1 |
| 3.3.3 | `--model` (the `Document` dump) and `--validate` | `xarast-cli` | M | W3.4–W3.8 |
| 3.3.4 | `--json` machine-readable output | `xarast-cli` | M | 3.3.2 |
| 3.3.5 | `--corpus` batch mode and the summary table | `xarast-cli` | M | 3.3.4 |

`xar-dump` is a second `[[bin]]` in `xarast-cli`. It is not a toy: it is how the
corpus acceptance numbers are produced, how a user reports "this file does not
open", and how a future contributor learns the format.

```
xar-dump <FILE>...            [--records] [--tree] [--tags] [--stats]
                              [--model] [--validate] [--json]
                              [--max-depth N] [--limit N] [--quiet]
xar-dump --corpus <DIR>       [--json] [--fail-on warning|error]
```

Exit codes: `0` clean; `1` the file parsed with warnings and `--fail-on
warning` was given; `2` the file failed to parse; `3` the file parsed but the
resulting document failed `validate()`.

**A clean-room constraint on output that is easy to get wrong.** Dumping the
full contents of a `Design/` file — coordinates, colour values, text strings —
and checking that into the repository as a snapshot would amount to
redistributing Xara's artwork, which
`docs/11-licensing-and-clean-room.md §3.2` forbids. Therefore:

- `--records`, `--tree` and `--model` print content and are for **interactive
  use only**. Their output is never committed.
- `--stats`, `--tags` and the `--corpus` summary print only **facts about the
  file**: record counts, tag histograms, node-kind counts, depth, block count,
  diagnostics. No coordinates, no colour components, no strings taken from the
  file. This output *is* committed as `insta` snapshots, and it is what the
  regression suite compares.

Put that rule in `xar-dump`'s `--help` text, not only in this document.

### W3.4 — Colours

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 3.4.1 | `TAG_DEFINECOMPLEXCOLOUR` (51) | `xarast-xar` | M | 3.2.4 |
| 3.4.2 | Negative built-in colour references (−1..−10) | `xarast-xar` | S | 3.2.4 |
| 3.4.3 | Tint / shade / link parent resolution into `ColourTable` | `xarast-xar` | M | 3.4.1 |
| 3.4.4 | `TAG_DEFINERGBCOLOUR` (50) | `xarast-xar` | S | 3.4.1 |

Layout of tag 51 (`research/01 §9.2`), size 29 + the name:

| Off | Type | Field |
|---|---|---|
| 0–2 | `u8`×3 | `r`, `g`, `b` — the 8-bit approximation, and the resolution fallback |
| 3 | `u8` | colour model (0–6) |
| 4 | `u8` | colour type (0 normal, 1 spot, 2 tint, 3 linked, 4 shade) |
| 5 | `u32` | entry index in the document colour list |
| 9 | `i32` | parent record number, 0 = none |
| 13,17,21,25 | `FIXED24`×4 | components |
| 29 | UTF-16 + NUL | name, possibly empty (2 bytes) |

Observed sizes run 31 to 65. Component `0xF800_0000` (FIXED24 −8.0) means
**inherit from the parent** — Phase 1's `Fixed24::INHERIT` and the
`Option<f32>` component type handle it, and reading it literally gives absurd
colours (`research/01 §11` item 9).

Tag 51 appears in **all 59 files** despite being only 0.41 % of records; it is
in the minimum viable set for that reason.

### W3.5 — Paths

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 3.5.1 | Relative interleaved decoder (`TAG_PATH_RELATIVE*`, 113–116) | `xarast-xar` | L | 3.2.5 |
| 3.5.2 | Absolute decoder (100–103 and embedded paths) | `xarast-xar` | M | 3.2.5 |
| 3.5.3 | Verb decoding into `xarast_geom::Verb` | `xarast-xar` | M | 3.5.1 |
| 3.5.4 | `TAG_PATH_FLAGS` (111) application | `xarast-xar` | S | 3.5.3 |
| 3.5.5 | `TAG_PATHREF_TRANSFORM` (118) and `TAG_PATHREF_IDENTICAL` (4013) | `xarast-xar` | M | 3.2.4 |

Only the relative forms 115 and 116 occur in the corpus (Xara always exports
relative), but absolute paths appear **embedded** inside other records —
reformed regular shapes, `TAG_MOULD_PATH`, `TAG_BLEND_PATH` — so the absolute
codec is needed regardless.

**The relative format has no point counter.** `num_points = payload_size / 9`.
If `size % 9 != 0` the record is corrupt or from an unknown variant: abort that
path with `DiagCode::TruncatedRecord` and **do not guess**
(`research/01 §11` item 4).

Per point: one verb byte, then eight bytes holding X and Y interleaved
most-significant first:

```
b0 = X>>24  b1 = Y>>24  b2 = X>>16  b3 = Y>>16
b4 = X>>8   b5 = Y>>8   b6 = X      b7 = Y
```

The interleaving exists so that the near-constant high bytes sit next to each
other and zlib compresses them; it is not an endianness quirk of the file.

**The sign trap.** Point 0 holds an absolute coordinate (to which the
coordinate origin is added). Every later point holds an **inverted delta**:

```
P[i] = P[i-1] - D
```

Not plus. `research/01 §11` item 5 flags this specifically, because getting it
wrong produces mirrored geometry that looks entirely plausible on symmetric
shapes. The worked example from `testfiles/OneLine.xar` in `research/01 §7.3`
is the unit test: verb `06`, `X = 112101`, `Y = 178899`; then verb `02`,
`D = (-171000, -142500)`, giving `P1 = (283101, 321399)`.

Verbs are a bitfield, not an enum: `verb & 0x06` gives the type (2 line, 4
cubic, 6 move), `verb & 0x01` is "this point closes the subpath". Zero is not a
valid verb. Cubics consume three consecutive `PT_BEZIERTO` points: two controls
then the endpoint. The conversion into Phase 1's segment-oriented `Path` is the
one place these two representations meet, and it relies on the identity
recorded in Phase 1: `points.len()` is the same in both.

`TAG_PATH_FLAGS` is the path record's first child, one byte per point
(`SMOOTH`/`ROTATE`/`END_POINT`), in point order — so it maps directly onto
`Path::set_flags`. If its length disagrees with the point count, apply the
prefix and warn.

The relative-path decoder uses `Mp::checked_sub` and emits
`DiagCode::CoordinateClamped` when a delta pushes a coordinate outside the
document extent, rather than silently saturating.

One theoretical risk worth knowing about and not acting on: the original
contains a non-interleaved relative variant behind a compile-time macro that
was *enabled* in Xara LX. A file written with it disabled would be unreadable
and indistinguishable by tag. No such file exists in the corpus
(`research/01 §11` item 6). Do not add heuristics for it.

### W3.6 — Attributes

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 3.6.1 | Line attributes: 151, 152, 173, 174, 175, 176, 177, 178, 193–195 | `xarast-xar` | M | 3.4.1 |
| 3.6.2 | Flat fill 150 and the shortcuts 190–192 | `xarast-xar` | S | 3.4.1 |
| 3.6.3 | Gradients: 153, 155, and then 154, 156, 200, 202, 204, 159, 4010, 4121 | `xarast-xar` | L | 3.6.2 |
| 3.6.4 | Transparencies: 166, 167, 169, and the rest of 168–172, 201, 203, 205, 4011, 4123 | `xarast-xar` | L | 3.6.3 |
| 3.6.5 | Multi-stage ramps: 4075–4078, 4088, 4122 | `xarast-xar` | M | 3.6.3 |
| 3.6.6 | Mapping, repeat and effect records: 160–165, 180–182, 206, 207 | `xarast-xar` | S | 3.6.3 |
| 3.6.7 | Dashes (183, 184, 188) and arrows (185, 186) | `xarast-xar` | M | 3.6.1 |
| 3.6.8 | Feather (4086), quality (179), user values (189) | `xarast-xar` | S | 3.6.1 |

**Never trust the declared record size.** Several records grew between versions:
`TAG_LINEARFILL` 24→40 when bias/gain was added, `TAG_TEXT_STORY_SIMPLE` 8→12,
`TAG_SHADOW` 16→24, `TAG_FEATHER` 4→20 (`research/01 §11` item 1). The reader
reads fields **while bytes remain** and fills the rest with documented defaults
(bias 0, gain 0, autokern 0). This is the single most important habit in the
whole importer, and `Cur` should make it the ergonomic path:
`cur.opt_f64().unwrap_or(0.0)` rather than a length check at every call site.

Corollary from `research/01 §11` item 2: where a declared size constant and the
actual writer disagree, **the writer is the truth**
(`TAG_VARIABLEWIDTHFUNC` declares 4 and writes 8; `TAG_BEVEL` declares nothing
and writes 24).

Gradient geometry shares a prefix — `start`, `end`, and for the
two-axis shapes `end2` — followed by the payload stops and a `f64 bias`,
`f64 gain` profile. Multi-stage variants carry `u32 n` then `n × (f64 pos,
i32 colour_ref)` and, unlike their two-colour equivalents, **have no
bias/gain**. Transparency records are the same geometry with `u8` levels and a
`u8` mode byte instead of colour references; unknown mode values map to `Mix`.

All of this lands in one `FillGeometry<Colour>` or
`FillGeometry<Transparency>` and one `AttrValue`, which is why Phase 2 built
the generic.

### W3.7 — Regular shapes

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 3.7.1 | `TAG_REGULAR_SHAPE_PHASE_2` (1901) | `xarast-xar` | L | 3.5.2 |
| 3.7.2 | `QuickShape` node payload and the ellipse/rectangle fast paths | `xarast-xar` | M | 3.7.1 |

Tag 1901 is 2.53 % of corpus records and appears in 22 files. Layout
(`research/01 §4.7.1`): `u8 flags`, `u16 num_sides`, `DocCoord major_axis`
(a **vector** — no origin translation), `DocCoord minor_axis` (likewise),
`Matrix` (24 bytes, translated), four `f64` (stellation radius and offset,
primary and secondary curvature), then two absolute-format paths (primary and
secondary edge). Flags: `0x01` circular, `0x02` stellated, `0x04` primary
curvature active, `0x08` stellation curvature active. The centre is `(0,0)` in
untransformed space; the matrix places it.

Observed sizes are 119 (35 178 records) and 155 (28 records) — useful as a
sanity check while developing, not as an assumption in the code.

### W3.8 — Document structure

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 3.8.1 | 40/41/42/43 document, chapter, spread, layer | `xarast-xar` | M | 3.2.1 |
| 3.8.2 | 45 spread information; 52/53 spread scaling; 4031 animation properties | `xarast-xar` | M | 3.8.1 |
| 3.8.3 | 48/49 layer details and guide layers; 4030 frame properties | `xarast-xar` | M | 3.8.1 |
| 3.8.4 | 46/47 grid and ruler; 112 guidelines | `xarast-xar` | S | 3.8.1 |
| 3.8.5 | 87 default units; 85/86 unit definitions and the negative unit table | `xarast-xar` | S | 3.2.4 |
| 3.8.6 | 80/82 viewport and saved view; 91/92/93 dates, undo size, flags; 4114/4116 nudge and smoothing | `xarast-xar` | S | 3.8.1 |
| 3.8.7 | 104 group; 4070/4087 sentinel and bar property | `xarast-xar` | S | 3.8.1 |
| 3.8.8 | 4119 current attributes — an atomic container whose subtree is skipped | `xarast-xar` | S | 3.2.3 |

`TAG_SPREADINFORMATION` (17 bytes): `i32 width`, `i32 height`, `i32 margin`,
`i32 bleed`, `u8 flags`. **Bit 0 is the double-page-spread flag** — the original
is self-inconsistent here (its import handler reads bit 0, its debug printer
bit 2) and we follow the handler, treating bit 2 as unknown
(`research/01 §11` item 3).

`TAG_LAYERDETAILS`: `u8 flags` then a UTF-16 NUL-terminated name. Flags are
visible `0x01`, locked `0x02`, printable `0x04`, active `0x08`, page background
`0x10`, background `0x20`.

`TAG_CURRENTATTRIBUTES` (4119) is on the atomic list and describes the
*document defaults*, not objects. Its subtree is skipped for now; whether those
defaults should populate `DefaultAttrs` rather than being discarded is **to be
determined in this phase**, decided by checking whether any corpus file's
appearance depends on it — dump the subtree for all 59 files and see whether
the values differ from `DefaultAttrs::xara_compatible()`.

### W3.9 — Bitmaps

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 3.9.1 | Definitions 65–68, 71 (BMP, GIF, JPEG, PNG, JPEG8BPP) | `xarast-xar` | M | 3.1.4 |
| 3.9.2 | `TAG_NODE_BITMAP` (198) and `TAG_BITMAP_PROPERTIES` (4115) | `xarast-xar` | M | 3.9.1 |
| 3.9.3 | Bitmap fills 157/158 and bitmap transparency 171 | `xarast-xar` | M | 3.6.3 |
| 3.9.4 | JPEG8BPP palette reconstruction metadata | `xarast-xar` | S | 3.9.1 |

Bitmap definitions are **streamed records**: uncompressed, containing a
complete PNG/JPEG/GIF/BMP file. For the reader that means "read `size` bytes",
nothing more. Those bytes go straight into
`BitmapResource::original` and are **never re-encoded** — that is what keeps an
embedded JPEG lossless across a `.xar → .xarast` conversion. Decoding into
pixels is Phase 10's job; this phase records the declared dimensions and format
and leaves `pixels` empty until then.

`TAG_NODE_BITMAP` (36 bytes) is a four-point parallelogram plus a bitmap
reference — not a rect and a matrix. Bitmaps appear in 22–24 corpus files.

### W3.10 — Text (structure only)

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 3.10.1 | Font definitions 2000/2001 and `TAG_TEXT_FONT_TYPEFACE` | `xarast-xar` | M | 3.2.4 |
| 3.10.2 | 2100/2101 story records; 2200–2206 lines, chars, kerns, tabs | `xarast-xar` | L | 3.10.1 |
| 3.10.3 | Text attributes 2900–2920, 2150/2151 | `xarast-xar` | M | 3.10.2 |
| 3.10.4 | `TAG_TEXT_STRING` (2201) — the unterminated-string special case | `xarast-xar` | S | 3.10.2 |

Text touches 22 corpus files (all of `TextDesigns/` plus a few designs). This
phase maps the records into the `TextStory` → `TextLine` → `TextItem` tree and
the text `AttrValue`s, and stops there. Nothing is shaped or measured until
Phase 9.

**The one real trap:** `TAG_TEXT_STRING` (2201) is the only string in the
format **without** a terminator — its length is `size / 2` UTF-16 code units.
Every other string is NUL-terminated. Mixing the two conventions desynchronises
the cursor inside the record and produces garbage that is hard to trace
(`research/01 §11` item 16). `Cur` therefore exposes `utf16_z()` and
`utf16_rest()` as separate methods, and the handler for 2201 is the only caller
of the latter.

Two calibration unknowns, both **to be determined in this phase** by dumping
the corpus and comparing against a reference rendering (a screenshot of the
original is a behavioural observation, which the clean-room rule permits):
`TAG_TEXT_TRACKING`'s unit, declared `MILLIPOINT` but combined with the font
size at render time (`research/01 §11` item 15); and the angle encodings, which
are inconsistent across records — `ANGLE` is FIXED16 radians,
`TAG_SHADOWCONTROLLER` uses a bespoke `i32` encoding, and `TAG_BEVEL` uses
integer degrees (item 14). Record whatever is found in
`docs/memory/xar-import.md`; do not guess in code.

### W3.11 — Fuzzing

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 3.11.1 | `fuzz_xar_records` | `fuzz` | M | 3.1.5 |
| 3.11.2 | `fuzz_xar_tree` | `fuzz` | S | 3.2.1 |
| 3.11.3 | `fuzz_xar_import` | `fuzz` | M | W3.4–W3.10 |
| 3.11.4 | `fuzz_xar_path` and `fuzz_xar_colour` (record-payload targets) | `fuzz` | M | 3.5.1, 3.4.1 |
| 3.11.5 | Seed corpus construction — synthetic only | `fuzz` | M | 3.11.1 |
| 3.11.6 | CI nightly integration, crash artifact upload, corpus caching | — | S | 3.11.3 |

The five targets and what each one is for:

| Target | Input | Exercises |
|---|---|---|
| `fuzz_xar_records` | Arbitrary bytes | Magic, framing, the deflate state machine, CRC, the trailer reposition. The highest-value target: it is where pointer arithmetic and length handling live |
| `fuzz_xar_tree` | Arbitrary bytes | `DOWN`/`UP` imbalance, atomic subtree stripping, deep nesting, record numbering |
| `fuzz_xar_import` | Arbitrary bytes | The whole pipeline into `DocumentBuilder`, including reference resolution and every handler |
| `fuzz_xar_path` | Arbitrary bytes as a single path payload | The `% 9` check, the interleave, the delta sign, the extent clamp |
| `fuzz_xar_colour` | Arbitrary bytes as a tag-51 payload plus a synthetic parent chain | Inherit sentinels, parent cycles, the depth limit |

**The invariants, stated so they can be asserted rather than hoped for:**

1. **Never panic.** No unwrap on untrusted data, no slice index without a bound
   check, no arithmetic overflow panic in a debug build. `Mp`'s saturating
   contract and `Cur`'s bounds checking are what make this achievable; the
   fuzzer is what proves it. `xarast-xar` carries
   `#![deny(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used,
   clippy::panic, clippy::arithmetic_side_effects)]`.
2. **Never OOM.** Peak allocation is bounded by
   `min(BuildLimits::max_bytes, 200 × input_len + 64 MiB)`, enforced by the caps
   in W3.1.6 and verified by running the fuzzer under
   `-rss_limit_mb=2048 -malloc_limit_mb=512`.
3. **Bounded allocation on hostile input.** Stronger than (2) and the one that
   catches the real bug class: *allocation must be a function of bytes actually
   read, never of a declared length.* A 12-byte file claiming a 4 GB record must
   allocate nothing. Asserted by a direct unit test per length field, and by
   a fuzz post-condition comparing peak allocation against input length through
   a counting global allocator in the fuzz build.
4. **Terminate.** Every record consumes at least 8 stream bytes, so the loop is
   structurally finite. The fuzzer enforces `-timeout=5`.
5. **Valid or nothing.** Any `Ok(Document)` passes `Document::validate()` with
   zero errors. There is no third outcome.

**Seed corpus.** The 59 real files are the obvious seeds and they **must not be
copied into the repository** (`docs/11-licensing-and-clean-room.md §3.2`). The
committed seed corpus under `fuzz/corpus/xar_records/` is therefore
**synthetic**: small files we generate that exercise each structural feature —
a bare header; a header plus `ENDOFFILE`; one empty compressed block; two
compressed blocks with an uncompressed record between them; a truncated deflate
stream; a bad CRC; a bad length; an unbalanced `DOWN`; a 256-level nesting; a
record declaring `0xFFFFFFFF`; each of the ~45 tags with a minimal valid
payload. A developer with the corpus present can seed from it locally via
`XARAST_XAR_CORPUS`; CI cannot and does not.

**Minimisation and regression.** Every crash found is minimised with
`cargo fuzz tmin`, added to `fuzz/artifacts/` as a committed regression case
(it is synthetic by then, so this is clean-room safe), and turned into a unit
test in `xarast-xar` so that it is checked on every push rather than only
nightly.

### W3.12 — Corpus validation

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| 3.12.1 | The corpus harness producing the acceptance table | `xarast-xar` | M | 3.3.5 |
| 3.12.2 | Per-file statistics snapshots (`insta`, facts only) | `xarast-xar` | M | 3.12.1 |
| 3.12.3 | Tag-coverage report against the 157 distinct tags observed | `xarast-xar` | S | 3.12.1 |
| 3.12.4 | Import performance benchmarks | `xarast-xar` | S | 3.12.1 |

The harness runs every corpus file through four levels and produces one row per
file: physical records read, compressed blocks and their CRC status, `DOWN`/`UP`
balance, distinct tags, handled/skipped/stripped/unknown counts, nodes built,
diagnostics by severity, `validate()` result, and wall-clock time. It prints a
table and, with `--json`, a machine-readable summary that CI archives.

## Public API introduced

```rust
// ═══ xarast-xar ══════════════════════════════════════════════════════════════

pub const XAR_MAGIC: [u8; 8] = [0x58, 0x41, 0x52, 0x41, 0xA3, 0xA3, 0x0D, 0x0A];

// ─── Physical layer ──────────────────────────────────────────────────────────

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum FileType { Native, Web, MinimalWeb }   // CXN / CXW / CXM

#[derive(Clone, Debug)]
pub struct FileHeader {
    pub file_type: FileType,
    /// Total uncompressed size the writer recorded. Advisory only.
    pub declared_size: u32,
    pub native_web_link_id: u32,
    /// Must be 0; any other value is a hard error.
    pub precompression_flags: u32,
    /// All three may be absent or truncated (`Templates/animation.xar`).
    pub producer: Option<String>,
    pub producer_version: Option<String>,
    pub producer_build: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Record {
    /// 1-based ordinal over EVERY record in the file, including UP/DOWN, the
    /// header and compressed records. This is the format's only pointer.
    pub number: u32,
    pub tag: u32,
    pub data: Vec<u8>,
    /// Byte offset of the record header in the physical file, for diagnostics.
    pub file_offset: u64,
    pub compressed: bool,
}

#[derive(Clone, Debug)]
pub struct ReaderLimits {
    /// Per compressed block: `max(64 MiB, ratio * compressed_bytes)`.
    pub max_inflated_per_block: usize,
    pub inflation_ratio: usize,        // 200
    pub max_total_inflated: usize,
    pub max_record_size: usize,
    pub max_records: u32,
}
impl Default for ReaderLimits { /* the values above */ }

#[derive(Debug, thiserror::Error)]
pub enum XarError {
    #[error("not a Xara file: bad magic")]                     BadMagic,
    #[error("truncated at offset {0}")]                        Truncated(u64),
    #[error("first record is not TAG_FILEHEADER")]             MissingHeader,
    #[error("unsupported precompression flags {0:#x}")]        BadPrecompression(u32),
    #[error("deflate error at offset {offset}: {source}")]
    Inflate { offset: u64, #[source] source: std::io::Error },
    #[error("compressed block CRC mismatch: file {file:#010x}, computed {computed:#010x}")]
    Crc { file: u32, computed: u32 },
    #[error("compressed block length mismatch: file {file}, computed {computed}")]
    BlockLength { file: u32, computed: u32 },
    #[error("essential tag {0} has no handler; the file cannot be represented")]
    EssentialTag(u32),
    #[error("limit exceeded: {0}")]                            Limit(&'static str),
    #[error(transparent)]                                      Build(#[from] xarast_doc::BuildError),
}

/// Streaming record reader. Handles raw deflate, the CRC trailer and streamed
/// records transparently; the caller just sees a record sequence.
#[derive(Debug)]
pub struct RecordReader<'a> { /* ... */ }

impl<'a> RecordReader<'a> {
    pub fn new(bytes: &'a [u8], limits: ReaderLimits) -> Result<RecordReader<'a>, XarError>;
    pub fn header(&self) -> &FileHeader;
    /// `None` at `TAG_ENDOFFILE` or end of input.
    pub fn next_record(&mut self) -> Option<Result<Record, XarError>>;
    pub fn diagnostics(&self) -> &[xarast_doc::Diagnostic];
    /// One entry per START/END pair, with its verification result.
    pub fn blocks(&self) -> &[BlockReport];
    pub fn records_read(&self) -> u32;
    pub fn trailing_bytes(&self) -> u64;
}

#[derive(Copy, Clone, Debug)]
pub struct BlockReport {
    pub start_offset: u64,
    pub compressed_bytes: u64,
    pub inflated_bytes: u32,
    pub crc_file: u32,
    pub crc_computed: u32,
    pub length_file: u32,
    pub ok: bool,
}

/// Bounds-checked little-endian cursor over a record payload. Every read is
/// fallible; nothing indexes without checking.
#[derive(Debug)]
pub struct Cur<'a> { /* ... */ }

impl<'a> Cur<'a> {
    pub fn new(b: &'a [u8]) -> Cur<'a>;
    pub fn remaining(&self) -> usize;
    pub fn u8(&mut self) -> Result<u8, XarError>;
    pub fn u16(&mut self) -> Result<u16, XarError>;
    pub fn i16(&mut self) -> Result<i16, XarError>;
    pub fn u32(&mut self) -> Result<u32, XarError>;
    pub fn i32(&mut self) -> Result<i32, XarError>;
    pub fn f32(&mut self) -> Result<f32, XarError>;
    pub fn f64(&mut self) -> Result<f64, XarError>;
    pub fn fixed16(&mut self) -> Result<f64, XarError>;
    pub fn fixed24(&mut self) -> Result<xarast_color::Fixed24, XarError>;
    pub fn angle(&mut self) -> Result<f64, XarError>;          // FIXED16 radians
    pub fn reference(&mut self) -> Result<Ref, XarError>;

    /// A POINT: the spread coordinate origin IS added.
    pub fn point(&mut self, origin: xarast_geom::Point) -> Result<xarast_geom::Point, XarError>;
    /// A VECTOR: the origin is NOT added. Regular-shape axes use this
    /// (`research/01 §5.3`); confusing the two is a real bug class.
    pub fn vector(&mut self) -> Result<xarast_geom::Vector, XarError>;
    pub fn point_interleaved(&mut self) -> Result<(i32, i32), XarError>;
    pub fn matrix(&mut self, origin: xarast_geom::Point) -> Result<xarast_geom::Matrix, XarError>;

    pub fn ascii_z(&mut self) -> Result<String, XarError>;
    pub fn utf16_z(&mut self) -> Result<String, XarError>;
    /// The rest of the record as UTF-16 with NO terminator. `TAG_TEXT_STRING`
    /// (2201) is the only caller; every other string is NUL-terminated.
    pub fn utf16_rest(&mut self) -> Result<String, XarError>;

    // ── Tolerant reads. Records grew between versions; never assume the
    //    declared size. These return `None` when the bytes ran out. ──
    pub fn opt_i32(&mut self) -> Option<i32>;
    pub fn opt_f64(&mut self) -> Option<f64>;
    pub fn opt_u8(&mut self) -> Option<u8>;
}

/// A `.xar` REFERENCE: `> 0` record number, `< 0` predefined, `0` null.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Ref { None, Builtin(i32), Record(u32) }
impl Ref { pub fn parse(v: i32) -> Ref; }

// ─── Record tree ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct RecordNode { pub record: Record, pub children: Vec<RecordNode> }

#[derive(Clone, Debug, Default)]
pub struct RecordTree {
    pub roots: Vec<RecordNode>,
    pub down_count: u32,
    pub up_count: u32,
    pub max_depth: usize,
}

pub fn build_record_tree(reader: RecordReader<'_>, limits: &ReaderLimits)
    -> Result<(RecordTree, Vec<xarast_doc::Diagnostic>), XarError>;

// ─── Tag policy ──────────────────────────────────────────────────────────────

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum TagClass {
    /// Builds or closes tree structure; not understanding it corrupts the tree.
    Structural,
    /// Pushes an attribute into the current scope.
    Attribute,
    /// Registers a referenceable definition.
    Definition,
    /// An object node.
    Object,
    /// Safe to skip: printing, view state, editor hints.
    Ignorable,
}

#[derive(Debug, Default)]
pub struct TagPolicy {
    /// Accumulated from ALL `TAG_ATOMICTAGS` records (739 of them, 4 bytes
    /// each, in the corpus) — not from one record with a list.
    atomic: std::collections::HashSet<u32>,
    essential: std::collections::HashSet<u32>,
}

impl TagPolicy {
    pub fn absorb_atomic(&mut self, payload: &[u8]);
    pub fn absorb_essential(&mut self, payload: &[u8]);
    pub fn is_atomic(&self, tag: u32) -> bool;
    pub fn is_essential(&self, tag: u32) -> bool;
    /// The static classification of every tag we know of.
    pub fn class_of(tag: u32) -> Option<TagClass>;
    pub fn name_of(tag: u32) -> Option<&'static str>;
    /// The three-way decision for a tag with no handler.
    pub fn unknown_action(&self, tag: u32) -> UnknownAction;
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum UnknownAction { Skip, StripSubtree, Abort }

// ─── Import ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct ImportOptions {
    pub reader: ReaderLimits,
    pub build: xarast_doc::BuildLimits,
    /// Stop at the first `Error` diagnostic instead of continuing.
    pub strict: bool,
    /// Skip text records entirely (useful for bisecting a failing file).
    pub skip_text: bool,
    /// Skip bitmap payloads, keeping only their metadata.
    pub skip_bitmaps: bool,
}
impl Default for ImportOptions { /* non-strict, everything enabled */ }

#[derive(Debug)]
pub struct ImportReport {
    pub header: FileHeader,
    pub records_read: u32,
    pub records_handled: u32,
    pub records_skipped: u32,
    pub records_stripped: u32,
    pub distinct_tags: std::collections::BTreeMap<u32, u32>,
    pub blocks: Vec<BlockReport>,
    pub max_depth: usize,
    pub nodes_built: usize,
    pub diagnostics: Vec<xarast_doc::Diagnostic>,
    pub duration: std::time::Duration,
}

impl ImportReport {
    pub fn errors(&self) -> usize;
    pub fn warnings(&self) -> usize;
    /// Unknown tags whose class is `Structural`, or that are essential. This is
    /// the number the phase's acceptance criteria count.
    pub fn unknown_structural_tags(&self) -> Vec<u32>;
}

/// The importer's whole public surface. It never touches the arena: everything
/// goes through `DocumentBuilder`.
pub fn import(bytes: &[u8], opts: &ImportOptions)
    -> Result<(xarast_doc::Document, ImportReport), XarError>;

/// Header-only probe: format detection without inflating anything.
pub fn probe(bytes: &[u8]) -> Result<FileHeader, XarError>;

// ═══ xarast-cli: the `xar-dump` binary ═══════════════════════════════════════
//
// xar-dump <FILE>...      [--records] [--tree] [--tags] [--stats]
//                         [--model] [--validate] [--json]
//                         [--max-depth N] [--limit N] [--quiet]
// xar-dump --corpus <DIR> [--json] [--fail-on warning|error]
//
// Exit codes: 0 clean, 1 warnings with --fail-on warning, 2 parse failure,
//             3 parsed but the document failed validation.
//
// --records/--tree/--model print file CONTENT and are for interactive use; the
// output is never committed. --stats/--tags/--corpus print only FACTS about the
// file and are what the snapshot tests compare. See the clean-room rule.
```

## Acceptance criteria

All corpus figures are measured by `xar-dump --corpus $XARAST_XAR_CORPUS --json`
with `XARAST_CORPUS_REQUIRED=1`, over the 59 files of `corpus.lock`.

1. **59 of 59** files pass the physical layer: `RecordReader` drains every file
   to `TAG_ENDOFFILE` with zero `XarError`.
2. Exactly **1 390 282** records are read in total across the 59 files, and
   **157** distinct tags are observed — reproducing the measurement in
   `research/01 §12.2`. A different number means either the corpus or the
   reader is wrong, and the harness says which files disagree.
3. **103 of 103** compressed blocks verify: CRC-32 and uncompressed length both
   match the trailer, in every file.
4. **0 trailing bytes** after `TAG_ENDOFFILE` in all 59 files.
5. `DOWN` and `UP` counts are equal in every file and total **201 121** each.
6. **59 of 59** files build a `Document` with **zero unknown-structural-tag
   errors**, where "unknown structural tag" means a tag with no handler whose
   `TagClass` is `Structural`, or a tag on the essential list. Reported by
   `ImportReport::unknown_structural_tags()`.
7. **0 of 59** files abort with `XarError::EssentialTag`.
8. **≥ 99.2 %** of all records across the corpus are either handled by a
   registered handler or classified `Ignorable`. The remainder are reported
   per tag with counts, and each one is listed with a reason in
   `docs/memory/xar-import.md`.
9. All **45** tags of the minimum viable set are implemented and exercised by
   the corpus:
   structural `0,1,2,3,10,30,31,40,41,42,43,45,46,47,48,80,82,87,91,92,93,4070,4087,4114,4116,4031`;
   colour `51`; geometry `111,115,116,1901,104`;
   attributes `150,151,152,153,155,166,167,169,173,174,175,176,193`.
   A test asserts each appears in `ImportReport::distinct_tags` for at least
   one file and that each has a handler.
10. **59 of 59** resulting documents pass `Document::validate()` with zero
    errors. Warnings (notably `AttrAfterInk`) are permitted and counted.
11. Every document contains at least one spread, at least one layer, and at
    least one ink node.
12. The `OneLine.xar` worked example from `research/01 §12.2` reproduces
    exactly: 88 records, depth 5, the path decoding to
    `MoveTo(112101, 178899) → LineTo(283101, 321399)`, the colour at record 31
    being CMYK "Black", and line width 500 mp. One unit test, asserted field by
    field — this is the end-to-end oracle for the sign convention, the
    interleave and the reference resolution all at once.
13. `xar-dump --stats --json` output for all 59 files matches committed `insta`
    snapshots containing **facts only** — counts, histograms, depths,
    diagnostics — and **no coordinates, colour values or strings taken from the
    files**. A test greps the snapshots for anything resembling file content and
    fails if it finds it.
14. Bitmaps: every corpus file containing bitmap definitions produces
    `BitmapResource` entries whose `original` bytes are byte-identical to the
    corresponding record payload, and whose declared format matches the payload
    magic (`\x89PNG`, `\xFF\xD8\xFF`, `GIF8`, `BM`).
15. Text: all 14 `TextDesigns/*.xar` files produce a `TextStory` subtree whose
    `TextItem` count equals the sum of the `TAG_TEXT_STRING` code-unit counts
    plus the non-string item records, and `SimpleText.xar` yields the 19
    characters of its single line with no NUL and no truncation.
16. Round-trip of the record layer: for all 59 files, re-serialising the parsed
    record sequence (framing only, no compression) and re-parsing it yields an
    identical record sequence, proving the framing is lossless.
17. `cargo +nightly fuzz run fuzz_xar_records -- -runs=5000000 -timeout=5
    -rss_limit_mb=2048 -malloc_limit_mb=512` finds no crash, no timeout and no
    OOM. Same for `fuzz_xar_tree`, `fuzz_xar_path` and `fuzz_xar_colour`.
18. `cargo +nightly fuzz run fuzz_xar_import -- -runs=2000000 -timeout=5
    -rss_limit_mb=2048` finds no crash, and every `Ok` result passes
    `Document::validate()` with zero errors (asserted inside the target).
19. Bounded allocation, tested directly rather than only by fuzzing: for each
    of the seven length-bearing fields (record `size`, relative-path implied
    count, `TAG_ATOMICTAGS` count, ramp stop count, string lengths, bitmap
    payload size, `TAG_TAGDESCRIPTION` count), a crafted 64-byte input
    declaring `0xFFFFFFFF` allocates **less than 1 MiB**, measured by a counting
    global allocator.
20. A 1 KiB input that inflates to 4 GiB is rejected with
    `XarError::Limit("inflation ratio")` and peak allocation stays under
    128 MiB.
21. `xarast-xar` compiles with
    `#![deny(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used,
    clippy::panic, clippy::arithmetic_side_effects)]` and no `#[allow]` outside
    tests.
22. `xar-dump` exit codes behave as documented, verified by one test per code.
23. Import performance budgets below are met and recorded.

## Performance budgets

Measured on the reference machine, `release` profile, warm page cache, over the
corpus. `testfiles/ProbeX16.xar` is the largest file at 7.4 MB with a tree depth
of 13; `testfiles/20000GradFilledShapes.xar` (331 KB) is the densest in objects.

| Operation | Input | Budget |
|---|---|---|
| Physical record iteration (no model building) | inflated stream | ≥ 250 MB/s |
| CRC-32 verification | inflated stream | ≥ 1 GB/s (does not dominate) |
| Full import to `Document` | `ProbeX16.xar` (7.4 MB) | ≤ 350 ms |
| Full import to `Document` | `20000GradFilledShapes.xar` | ≤ 120 ms |
| Full import to `Document` | median corpus file | ≤ 25 ms |
| Whole corpus, 59 files, sequentially | 12 MB | ≤ 3 s |
| Relative-path decode | 100 000 points | ≤ 4 ms |
| Peak RSS during import | `ProbeX16.xar` | ≤ 8 × file size |

The 350 ms figure is deliberately inside the roadmap's *500 ms to first paint
for a 5 MB `.xar`*, which Phase 5 measures end to end: parsing must leave the
renderer room.

## Risks and mitigations

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| 1 | The relative-path delta sign is implemented as `+` instead of `−`, producing mirrored geometry that looks plausible | **High** | **High** | Criterion 12's `OneLine.xar` byte-level oracle; an asymmetric synthetic path in the unit tests, because a symmetric one would pass either way |
| 2 | The `TAG_ENDCOMPRESSION` trailer reposition is wrong, so everything after the first bitmap desynchronises | **High** | **High** | Criterion 3 checks all 103 blocks; the multi-block synthetic seed file is in the fuzz corpus and in the unit tests |
| 3 | A record is parsed against its declared size rather than its actual bytes, so older or newer files fail | **High** | Medium | `Cur::opt_*` is the ergonomic path; a test feeds every fixed-size record truncated by 1, 4 and 8 bytes and asserts documented defaults |
| 4 | Atomic subtree stripping is not implemented, so files from newer Xara versions import with phantom objects | Medium | **High** | Implemented in W3.2 from the start, not deferred; a synthetic file with an unknown atomic tag and a two-node subtree is a unit test |
| 5 | A malicious file causes an OOM or a hang in a user's editor | Medium | **High** | The three caps in W3.1.6; fuzz invariants 2–4; criteria 17–20 |
| 6 | Corpus `.xar` content leaks into the repository through a snapshot or a fuzz seed | Medium | **Severe** (licence) | Criterion 13's grep; synthetic-only fuzz seeds; the Phase 0 `git ls-files '*.xar'` check |
| 7 | The 157-tag / 1 390 282-record figures do not reproduce, meaning the corpus differs from the one measured | Low | Medium | `corpus.lock` hashes catch a changed corpus before the counts do; criterion 2 names the disagreeing files |
| 8 | Text tracking units and the inconsistent angle encodings are guessed wrong, and text or shadows are subtly misplaced | Medium | Medium | Both are marked "to be determined in this phase" with a stated method; whatever is found goes in the memory note. Nothing is guessed in code |
| 9 | Legacy regular shapes (1000–1217, 1900) appear in a user file even though the corpus has none | Low | Low | They fall through to `UnknownTag`, `Info`, and the object is missing rather than wrong. Implement on the first real report; `research/01 §4.7.1` has the layout rules |
| 10 | The non-interleaved relative-path variant exists in some file in the wild and is indistinguishable by tag | Very low | Medium | Do nothing. Adding a heuristic risks misreading correct files to accommodate a file nobody has seen |
| 11 | `TAG_CURRENTATTRIBUTES` really does carry document defaults some file depends on, and skipping it changes appearance | Low | Medium | The W3.8.8 investigation answers it with data before Phase 4 renders anything |
| 12 | Colour parent chains in a corrupt file cycle and the resolver hangs | Low | Medium | Phase 1's `MAX_PARENT_DEPTH` and cycle detection; `fuzz_xar_colour` targets it specifically |

## Test plan

**Unit.** Every primitive in `Cur`, including the truncation path for each.
Magic acceptance and rejection (including the `A3 A3 0D 0A` tail flipped by a
CRLF conversion, which is what that tail exists to catch). The `OneLine.xar`
record-by-record oracle (criterion 12). Verb decoding for all eight byte values
`0x01..0x07`. Relative path with `size % 9 != 0`. `TAG_PATH_FLAGS` longer and
shorter than the point count. Each colour type resolving against the worked
examples. Each fill and transparency record at both its historic and current
size. `TAG_TEXT_STRING` with and without a trailing NUL byte in the data (it
must be treated as a character, not a terminator).

**Synthetic file tests.** A generator builds minimal `.xar` files in memory —
header, optional compressed blocks, an arbitrary record sequence,
`TAG_ENDOFFILE` — so that the physical layer can be tested exhaustively without
the corpus. Every structural edge case gets one: empty block, two blocks,
streamed record between blocks, bad CRC, bad length, truncated deflate,
unbalanced `DOWN`, unbalanced `UP`, 256-deep nesting, unknown essential tag,
unknown atomic tag with a subtree, record size beyond end of file. **CI runs
these**; the corpus tests skip when the corpus is absent, and these do not.

**Corpus.** The harness of W3.12, producing criteria 1–15. Run locally and in
any environment with the corpus; skipped elsewhere with a printed notice.

**Snapshot (`insta`).** Per-file statistics (facts only). The tag
classification table. The list of unhandled tags with counts and reasons — so
that implementing a new tag shows up as a visible, reviewable diff in the
coverage snapshot.

**Property (`proptest`).** Record framing round-trip (criterion 16).
Relative-path encode/decode round-trip against a synthetic encoder written for
the test (writing `.xar` is a non-goal for the product, but a test-only encoder
is the cleanest way to prove the decoder). Reference resolution: any sequence of
definitions and references resolves or diagnoses, never panics.

**Fuzz.** The five targets of W3.11, nightly at 30 minutes each, with the
corpus cached between runs and crashes uploaded as artifacts. Every minimised
crash becomes a committed regression case and a unit test.

**Benchmarks.** The budget table, plus a per-tag-family breakdown so that a
regression can be attributed to a handler rather than to "the importer".

## Memory note

Create **`docs/memory/xar-import.md`** from the template in
`docs/memory/INDEX.md` (it is listed there but does not exist yet).

Record:

- **Current state.** Which tags have handlers, grouped by the priority levels
  of `research/01 §10`. The corpus acceptance table with the measured numbers
  for criteria 1–15. Which of the 157 observed tags remain unhandled, with
  counts and a one-line reason each.
- **Decisions taken (and why).** Raw deflate with `windowBits = -15`. The
  `TAG_ENDCOMPRESSION` trailer reposition via `Decompress::total_in`. The
  three-way unknown-tag policy, implemented from the start rather than deferred.
  Following the import handler (bit 0) for the double-page-spread flag. Reading
  tolerantly rather than by declared size, and "the writer is the truth" where
  size constants disagree. Points translated, vectors not. Bitmap bytes
  preserved verbatim rather than re-encoded. Text mapped structurally only. The
  three resource caps and their values. Facts-only snapshots as a clean-room
  measure. Whatever the two open investigations return: the `TAG_TEXT_TRACKING`
  unit and the angle encodings, and whether `TAG_CURRENTATTRIBUTES` feeds
  `DefaultAttrs`.
- **Invariants that must not be broken.** `P[i] = P[i-1] − D`, not plus. Record
  numbering counts every record from 1, including `UP`/`DOWN` and compressed
  ones. `TAG_TEXT_STRING` has no terminator and every other string does. Never
  allocate from a declared length. Never panic on any input. The parser never
  touches the arena — everything goes through `DocumentBuilder`. No corpus bytes
  are ever committed to this repository.
- **Dead ends (do not retry).** Zlib-wrapped inflate (`windowBits = 15`).
  Assuming `pos == len` after `TAG_ENDOFFILE`. Assuming one `TAG_ATOMICTAGS`
  record holds the whole list. `#[repr(C)]` payload structs. Heuristics for the
  non-interleaved relative-path variant. Guessing at the unit of
  `TAG_TEXT_TRACKING`.
- **Open TODOs.** Legacy regular shapes (1000–1217, 1900). Brushes and stroke
  types. Blends, moulds, contours, shadows, bevels and ClipView beyond
  structural round-trip (Phase 13). Absolute path tags as top-level nodes.
  `TAG_PATHREF_*` deduplication, which the corpus never exercises. Whether the
  perceptual or exact golden gate is right, now that there is real content to
  render — `10-architecture.md §7` question 5, which Phase 4 answers and which
  this phase feeds with the first real documents.

Append the import benchmarks to `docs/memory/perf.md`, and add the
`xar-import.md` row to `docs/memory/INDEX.md` if it is not already listed.
