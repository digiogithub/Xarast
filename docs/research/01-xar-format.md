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
    consume the `size` bytes of the payload without interpreting them
    carry on with the next record
```

That is: **skip `size` bytes and carry on**. But there are two critical nuances
(`Kernel/camfiltr.cpp:5293-5312`, `BaseCamelotFilter::UnrecognisedTag`):

* If the tag is in the **essential** list (`TAG_ESSENTIALTAGS`, tag 11) → **abort the
  import**: the file cannot be represented faithfully.
* If the tag is in the **atomic** list (`TAG_ATOMICTAGS`, tag 10) → its **entire subtree
  must be discarded too** (`StripNextSubTree()`, `Kernel/cxfile.cpp:620-632`), that is,
  ignore every record up to the `TAG_UP` that closes the `TAG_DOWN` following the unknown
  record. Reason: if you do not understand a composite node (bevel, contour, shadow,
  ClipView, live effect), its children are *derived* data that must not be inserted loose
  into the tree.
* In any other case → ignore only that record and carry on, accumulating a warning for
  the user.

The atomic list written by Xara LX (`Kernel/camfiltr.cpp:6996-7013`) is:

```
TAG_BEVEL(4052) TAG_BEVELINK(4057) TAG_CONTOURCONTROLLER(4066) TAG_CONTOUR(4067)
TAG_SHADOWCONTROLLER(4050) TAG_SHADOW(4051) TAG_CLIPVIEWCONTROLLER(4084) TAG_CLIPVIEW(4085)
TAG_CURRENTATTRIBUTES(4119) TAG_LIVE_EFFECT(4125) TAG_LOCKED_EFFECT(4126)
TAG_FEATHER_EFFECT(4127)
```

In the corpus there are **739 `TAG_ATOMICTAGS` records of 4 bytes each** (one per tag),
not a single record with the complete list: the reader must accumulate them all
(`Kernel/cxfile.cpp:2562-2585` iterates over `size/4` entries per record).

### 2.4. Primitive payload data types

All defined in `Kernel/cxfrec.cpp` (writing ~lines 597-1100, reading ~1148-1660).

| CXF name | Bytes | Encoding | Rust |
|---|---|---|---|
| `BYTE` | 1 | unsigned integer | `u8` |
| `UINT16` / `INT16` | 2 | LE | `u16` / `i16` |
| `UINT32` / `INT32` | 4 | LE | `u32` / `i32` |
| `REFERENCE` | 4 | LE, **signed**: >0 = record number; <0 = predefined reference; 0 = error/null | `i32` |
| `FLOAT` | 4 | IEEE-754 binary32 LE | `f32` |
| `DOUBLE` | 8 | IEEE-754 binary64 LE | `f64` |
| `FIXED16` | 4 | `i32` with the binary point between bits 15 and 16 → real value = `raw / 65536.0` (`Kernel/ccmaths.h:116`, `Kernel/fixed16.h`) | `i32` + helper |
| `ANGLE` | 4 | alias of `FIXED16`, in radians (`Kernel/ccmaths.h:120`) | `i32` + helper |
| `FIXED24` | 4 | `i32` with 24 fractional bits → `raw / 16777216.0` (`Kernel/fixed24.h:157,264`). Appears only in colour components | `i32` + helper |
| `DocCoord` | 8 | two `INT32`s (x, y) in millipoints (§5) | `(i32,i32)` |
| interleaved `DocCoord` | 8 | alternating x/y bytes (§7.3) | same |
| `Matrix` | 24 | `FIXED16 a, b, c, d` + `INT32 e, f` (`Kernel/cxfrec.cpp:1913-1959`) | see §5.4 |
| `ASCII-Z` | var | ASCII bytes terminated by `0x00` (`Kernel/cxfrec.cpp:1546-1562`) | `CString`-like |
| `UNICODE-Z` | var | UTF-16 **LE**, terminated by `0x0000` (2 bytes). Constant `SIZEOF_XAR_UTF16 = 2` (`Kernel/cxfile.h:127`) | `Vec<u16>` → `String` |
| `UTF16STR` | var | identical to `UNICODE-Z` but with no length limit (`Kernel/cxfrec.cpp:1034-1058`) | same |
| `CCPanose` | 10 | 10 PANOSE bytes (family, serif, weight, proportion, contrast, strokeVar, armStyle, letterform, midline, xHeight) (`Kernel/cxfrec.cpp:1416-1447`) | `[u8;10]` |
| `RGBTRIPLE` | 3 | R, G, B (only in `TAG_DEFINEBITMAP_JPEG8BPP` palettes) | `[u8;3]` |

> **Caution:** `TAG_TEXT_STRING` (2201) is the only string that carries **no** terminator:
> its length is `size / 2` UTF-16 characters. Verified in `TextDesigns/SimpleText.xar`
> (size 38 = 19 characters, "Single line of text", no NUL).

### 2.5. Rust pseudocode for the record reader

```rust
pub const XAR_MAGIC: [u8; 8] = [0x58, 0x41, 0x52, 0x41, 0xA3, 0xA3, 0x0D, 0x0A];

#[derive(Debug, Clone)]
pub struct Record {
    pub number: u32,   // 1..N, order in the file; it is the key used by references
    pub tag: u32,
    pub data: Vec<u8>, // length == size
}

/// Cursor over a record payload: EVERYTHING is little-endian.
pub struct Cur<'a> { b: &'a [u8], p: usize }

impl<'a> Cur<'a> {
    pub fn new(b: &'a [u8]) -> Self { Cur { b, p: 0 } }
    pub fn remaining(&self) -> usize { self.b.len() - self.p }
    fn take(&mut self, n: usize) -> Result<&'a [u8], Err> {
        if self.remaining() < n { return Err(Err::Eof); }
        let s = &self.b[self.p..self.p + n]; self.p += n; Ok(s)
    }
    pub fn u8(&mut self)  -> Result<u8, Err>  { Ok(self.take(1)?[0]) }
    pub fn u16(&mut self) -> Result<u16, Err> { Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap())) }
    pub fn i16(&mut self) -> Result<i16, Err> { Ok(self.u16()? as i16) }
    pub fn u32(&mut self) -> Result<u32, Err> { Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap())) }
    pub fn i32(&mut self) -> Result<i32, Err> { Ok(self.u32()? as i32) }
    pub fn f32(&mut self) -> Result<f32, Err> { Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap())) }
    pub fn f64(&mut self) -> Result<f64, Err> { Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap())) }

    pub fn fixed16(&mut self) -> Result<f64, Err> { Ok(self.i32()? as f64 / 65536.0) }
    pub fn fixed24(&mut self) -> Result<f64, Err> { Ok(self.i32()? as f64 / 16_777_216.0) }
    pub fn angle(&mut self)   -> Result<f64, Err> { self.fixed16() }          // radians
    pub fn reference(&mut self) -> Result<i32, Err> { self.i32() }

    pub fn coord(&mut self) -> Result<Coord, Err> { Ok(Coord { x: self.i32()?, y: self.i32()? }) }

    /// 8 bytes with the bytes of X and Y interleaved, from MSB to LSB.
    pub fn coord_interleaved(&mut self) -> Result<Coord, Err> {
        let b = self.take(8)?;
        let x = i32::from_be_bytes([b[0], b[2], b[4], b[6]]);
        let y = i32::from_be_bytes([b[1], b[3], b[5], b[7]]);
        Ok(Coord { x, y })
    }

    pub fn matrix(&mut self) -> Result<Matrix, Err> {
        Ok(Matrix { a: self.fixed16()?, b: self.fixed16()?, c: self.fixed16()?,
                    d: self.fixed16()?, e: self.i32()?,     f: self.i32()? })
    }

    pub fn ascii_z(&mut self) -> Result<String, Err> {
        let mut v = Vec::new();
        loop { let c = self.u8()?; if c == 0 { break } v.push(c); }
        Ok(String::from_utf8_lossy(&v).into_owned())
    }

    pub fn utf16_z(&mut self) -> Result<String, Err> {
        let mut v = Vec::new();
        loop { let c = self.u16()?; if c == 0 { break } v.push(c); }
        Ok(String::from_utf16_lossy(&v))
    }

    /// The rest of the record as UTF-16 with no terminator (TAG_TEXT_STRING).
    pub fn utf16_rest(&mut self) -> Result<String, Err> {
        let mut v = Vec::new();
        while self.remaining() >= 2 { let c = self.u16()?; if c == 0 { break } v.push(c); }
        Ok(String::from_utf16_lossy(&v))
    }
}
```

---

## 3. Compression
