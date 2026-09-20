# The `.xar` file format (CXF / Camelot eXchange Format) — technical specification

> **Clean-room notice.** This document describes the *behaviour* and the *data
> formats* of Xara Xtreme (GPL-2.0-only) for interoperability purposes: binary
> layout of the `.xar` container, tag numbering and the semantics of each
> record. It does not reproduce source code from the original; the
> `file:line` references point at the reference tree in `xara-xtreme/` and
> serve only to locate the logic being described. Xarast is implemented from
> this specification, not by translating the original.

> **Status:** normative reference document for implementing the `.xar` importer in Rust.
> **Origin:** reverse engineering of the original **Xara Xtreme / Xara LX** source code
> (`/home/user/xara-xtreme`, C++, GPL v2), validated against 59 real `.xar` files from
> that same repository (`testfiles/`, `Designs/`, `Templates/`, `TextDesigns/`).
> All cross-references take the form `file:line` relative to the root of the
> original tree (for example `Kernel/cxfile.cpp:1699`).
>
> **Nomenclature:** the format is internally called the *v2 file format* or *CXF*. The
> `.xar` extension corresponds to the native format (`CXN`) and `.web` to the web format
> (`CXW`/`CXM`); both share **exactly** the same binary structure (see §1.4).

---

## Contents

1. [Physical structure of the file](#1-physical-structure-of-the-file)
2. [Record structure](#2-record-structure)
3. [Compression](#3-compression)
4. [Complete table of tags and layouts](#4-complete-table-of-tags-and-layouts)
5. [Coordinate system and units](#5-coordinate-system-and-units)
6. [Tree model, definitions and references](#6-tree-model-definitions-and-references)
7. [Path representation](#7-path-representation)
8. [Attributes and their inheritance model](#8-attributes-and-their-inheritance-model)
9. [Colours](#9-colours)
10. [Implementation priorities](#10-implementation-priorities)
11. [Known risks and ambiguities](#11-known-risks-and-ambiguities)
12. [Appendices](#12-appendices)

---

## 1. Physical structure of the file

### 1.1. Overview

A `.xar` file is:

```
+-------------------------------------------------------------+
| 8 bytes of signature (magic)                                 |
+-------------------------------------------------------------+
| Sequence of records, each one: tag u32, size u32, data       |
|   ...                                                        |
|   TAG_STARTCOMPRESSION  -> from here on, a deflate stream    |
|   ...compressed records...                                   |
|   TAG_ENDCOMPRESSION    -> followed by 8 bytes CRC+size      |
|   ...                                                        |
|   TAG_ENDOFFILE                                              |
+-------------------------------------------------------------+
```

There is no table of contents, no index and no absolute offsets: **the file is a
sequential stream of records**. The only "pointer" in the format is the *record number*
(the ordinal position within the file), used to reference definitions (§6.3).

### 1.2. Signature / magic bytes

Definition: `Kernel/cxfdefs.h:109-110`

| Constant | Value (`u32`) | Defined in |
|---|---|---|
| `CXF_IDWORD1` | `0x41524158` | `Kernel/cxfdefs.h:109` |
| `CXF_IDWORD2` | `0x0A0DA3A3` | `Kernel/cxfdefs.h:110` |

They are written as two **little-endian** `u32` values (`Kernel/cxfile.cpp:241-242`), so
that the first 8 bytes of the file are exactly:

```
offset 0: 58 41 52 41 A3 A3 0D 0A        ("XARA" + A3 A3 CR LF)
```

Validated across all 59 files in the corpus. Format detection (`HowCompatible`,
`Kernel/camfiltr.cpp:1639-1690`) requires these 8 bytes; it optionally checks that the
next record is `TAG_FILEHEADER` (2) with `size > 3` and reads the first 3 bytes of its
payload as a type identifier (`CXN`/`CXW`/`CXM`).

The sequence `A3 A3 0D 0A` is the classic anti-ASCII-transfer trap (if an FTP client in
text mode converts CRLF↔LF, the signature stops validating).

### 1.3. Byte order (endianness)

**The entire format is little-endian**, without exception, in every multi-byte type:

* `Kernel/cxfile.cpp:875-905` — `CXaraFile::Read(UINT32*)` applies `LEtoNative()`.
* `Kernel/cxfrec.cpp:597-676` — `WriteUINT32/INT32/UINT16/INT16/FLOAT/DOUBLE/FIXED16/ANGLE`
  apply `NativetoLE()`.
* `Kernel/cxfrec.cpp:678-686` — `WriteWCHAR` writes UTF-16 **LE**.

The one "odd ordering" exception: the *interleaved* coordinates of relative paths, which
write the bytes of X and Y alternately from most significant to least significant (§7.3).
This is not big-endian in the file: it is a deliberate permutation within an 8-byte field,
done to improve zlib compression (`Kernel/cxfrec.cpp:752-790`).

### 1.4. Logical header: `TAG_FILEHEADER` (tag 2)

Writing: `Kernel/camfiltr.cpp:3749-3768` · Reading: `Kernel/cxfile.cpp:2520-2547`

It is the **first record** in the file, always **uncompressed**.

| Offset | Type | Field | Notes |
|---|---|---|---|
| 0 | `[u8;3]` | `file_type` | ASCII, **with no terminator**: `"CXN"`, `"CXW"` or `"CXM"` |
| 3 | `u32` | `file_size` | Total **uncompressed** size of the file, in bytes (used for the progress bar). It is written as 0 and patched at the end of the export: `Kernel/camfiltr.cpp:4115-4125` |
| 7 | `u32` | `native_web_link_id` | Link identifier between the native file and its web version. Always 0 in the corpus |
| 11 | `u32` | `precompression_flags` | "Precompression" flags. **Must be 0**; any other value makes the importer abort with `IDS_UNKNOWN_COMPRESSION` (`Kernel/camfiltr.cpp:832-845`) |
| 15 | ASCII-Z | `producer` | e.g. `"Xara X"` |
| … | ASCII-Z | `producer_version` | e.g. `"3.0"` |
| … | ASCII-Z | `producer_build` | e.g. `"0.2704 (MarkG)"` |

The three strings are ASCII terminated by `NUL` (§2.4). The record size is variable
(36–47 bytes in the corpus).

A real example (`testfiles/OneLine.xar`, record #1, size 41):

```
43 58 4E | 9C 0F 00 00 | 00 00 00 00 | 00 00 00 00 |
"Xara X\0" "3.0\0" "0.2704 (MarkG)\0"
 -> type="CXN", file_size=3996, link=0, precomp=0
```

### 1.5. Versions

The format **has no global version number**. The "version" is deduced from three things:

1. **The tag range.** `Kernel/cxftags.h:109-114` documents the assigned ranges:
   `0–3999` = Camelot v1.5 (the first version of the format, **frozen**),
   `4000–4999` = Camelot v2.0 and later (Xara X, Xara X1/X2, XaraLX).
2. **The `TAG_FILEHEADER` strings** (`producer`, `producer_version`, `producer_build`):
   this is the only "program version" stored, and it is purely informational.
3. **The actual size of each record.** Several records grew between versions (e.g.
   `TAG_LINEARFILL` went from 24 to 40 bytes when the bias/gain profile was added, see
   `Kernel/cxfdefs.h:253`; `TAG_TEXT_STORY_SIMPLE` from 8 to 12 when the autokern flag was
   added, `Kernel/cxfdefs.h:470`). **The reader must tolerate records shorter than
   expected and fill in default values**: the original code does exactly that by means of
   "noError" reads (`ReadINT32noError`, `ReadDOUBLEnoError`,
   `Kernel/cxfrec.cpp:1290,1342`; used in `Kernel/rechtext.cpp:756-770`).

There is also a version **of the compression subsystem** inside
`TAG_STARTCOMPRESSION` (§3.2).

### 1.6. Differences between `.xar` (native) and `.web`

Identifiers (`Kernel/camfiltr.h:143-145`):

| Constant in the original | Value (3 ASCII characters) | Format it identifies |
|---|---|---|
| `EXPORT_FILETYPE_WEB` | `CXW` | web format (`.web`) |
| `EXPORT_FILETYPE_MIN` | `CXM` | "minimal" web format |
| `EXPORT_FILETYPE_NATIVE` | `CXN` | native format (`.xar`) |

This identifier is the `type` field of `TAG_FILEHEADER` (§1.4).

* **The binary structure is identical.** The same reader serves all three.
* The difference is *which* records are emitted. The exporter calls
  `WritePreChildrenWeb()` or `WritePreChildrenNative()` depending on the filter
  (`Kernel/camfiltr.cpp:4532-4545`). In the vast majority of classes,
  `WritePreChildrenNative()` simply delegates to the Web version
  (e.g. `Kernel/fillattr.cpp:5981-5987`), so the content matches.
* What appears in native **only**: document information that does not affect rendering
  (e.g. `TAG_DOCUMENTFLAGS`, `Kernel/infocomp.cpp:736-738` checks `IsWebFilter()` and does
  not write it in web), undo size, comments, views, and so on.
* The `CXM` format ("minimal web") indicates that the document needs a default template
  when it is loaded (`Kernel/webfiltr.cpp:383-390`).
* There is also a **text format** (`CXaraTemplateFile`, `Kernel/cxftfile.h:106-140`) used
  for Flare templates: it translates each record into text and the binary data into
  BinHex. **Outside the scope of this document.**

**Recommendation for Rust:** accept `CXN`, `CXW` and `CXM` with the same parser and expose
the detected type in the API.

---

## 2. Record structure

### 2.1. Binary layout

Writing: `Kernel/cxfile.cpp:1699-1717` and `Kernel/cxfile.cpp:1636-1657`
Reading: `Kernel/cxfile.cpp:1855-1866`

```
+--------+--------+-------------------------------+
| tag:u32| size:u32|  payload: size bytes          |
+--------+--------+-------------------------------+
```

* `tag`: little-endian `u32`. Identifies the type of the record.
* `size`: little-endian `u32`. **Size of the payload in bytes, not counting the 8 header
  bytes.** It may be 0.
* `payload`: exactly `size` bytes.

**There is no alignment and no padding of any kind.** Records follow one another with no
gaps, and the fields within the payload are not aligned either (e.g.
`TAG_SPREADINFORMATION` has a `u8` at the end of four `i32`s;
`TAG_DEFINECOMPLEXCOLOUR` starts with 5 loose bytes followed by a `u32`). **A Rust
`#[repr(C)]` is NO use for mapping payloads**: they have to be read field by field (or use
`#[repr(C, packed)]` with great care and explicit endianness conversion).

The value `CXF_UNKNOWN_SIZE = -1` (`Kernel/cxfdefs.h:111`) **never appears in the file**:
it is an internal marker in the writer meaning "the size is computed when the record is
closed". In the file the size is always the real one.

### 2.2. Variable-size records

Three mechanisms produce a variable size:

1. **Embedded strings** (layer, colour, font names, URLs…): NUL-terminated, and the record
   size includes them.
2. **Arrays with an explicit count**: e.g. the colour ramp of multi-stage gradients
   (`u32 n` followed by `n` pairs), `Kernel/fillattr.cpp:6837-6843`.
3. **Arrays *without* a count, deduced from the record size**: the most important case is
   that of relative paths, where `num_coords = size / 9`
   (`Kernel/cxfrec.cpp:1837`). Also `TAG_ATOMICTAGS` (`size/4` tags,
   `Kernel/cxfile.cpp:2574`) and `TAG_PATH_FLAGS` (`size` = number of points).

### 2.3. Unknown records: how they are ignored

`Kernel/cxfile.cpp:1997-2065` (`CXaraFile::ReadNextRecord`):

```text
if no handler is registered for the tag that was read:
