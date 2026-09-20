# `.xar` importer

Subsystem note for `crates/xarast-xar`, the `xar-dump` binary in
`crates/xarast-cli` and the fuzz targets in `fuzz/`.
Specification: `docs/phases/phase-03-xar-importer.md`.
Normative format reference: `docs/research/01-xar-format.md`.

## Current state

Phase 3 **up to, but not including, the mapping into `xarast-doc`**. The
crate turns a `.xar` byte stream into a typed, model-independent
representation and stops there.

| Layer | State |
|---|---|
| Physical reader: magic, framing, raw deflate, CRC trailer, streamed records | Done |
| Record numbering and `Ref` resolution | Done |
| `DOWN`/`UP` tree, tolerant of imbalance, depth-capped | Done |
| `TAG_ATOMICTAGS` / `TAG_ESSENTIALTAGS` and the three-way unknown-tag policy | Done |
| Typed decoding of 167 tags (the ~45-tag minimum set and the families that share its codecs) | Done |
| `xar-dump`, facts-only snapshots, corpus harness | Done |
| Five fuzz targets, synthetic seed corpus, bounded-allocation tests | Done |
| Mapping into `xarast-doc` (`DocumentBuilder`, `Document::validate`) | **Not started — a separate agent owns that crate** |

### Corpus acceptance, measured

`XARAST_CORPUS_REQUIRED=1 cargo test -p xarast-xar --test corpus`, over the
59 files of `tests/corpus/corpus.lock`:

| Criterion | Target | Measured |
|---|---|---|
| 1 files parsed | 59 | **59** |
| 2 records / distinct tags | 1 390 282 / 157 | **1 390 282 / 157** |
| 3 compressed blocks verified | 103 / 103 | **103 / 103** |
| 4 trailing bytes after `TAG_ENDOFFILE` | 0 | **0** |
| 5 `DOWN` / `UP` | 201 121 each, balanced per file | **201 121 / 201 121** |
| 6 unknown structural tags | 0 | **0** |
| 7 `XarError::EssentialTag` aborts | 0 | **0** |
| 8 records handled | ≥ 99.2 % | **99.621 %** (1 385 008) |
| 9 minimum viable tag set | 45 implemented and exercised | **45** |
| 12 `OneLine.xar` oracle | exact | **exact**: 88 records, depth 5, `MoveTo(112101,178899) → LineTo(283101,321399)`, record 31 CMYK "Black", line width 500 mp |
| 13 facts-only snapshots | committed + leak grep | **done** (`tests/snapshots/`) |
| 14 bitmap bytes verbatim, magic matches | all | **all** |
| 15 text decodes | 14/14 `TextDesigns` | **14/14**; `SimpleText.xar` yields the 19-character line |
| 16 record framing round trip | lossless | **lossless** |
| 19 bounded allocation per length field | < 1 MiB | **done**, 9 fields |
| 20 decompression bomb | rejected, < 128 MiB peak | **done** |
| 22 `xar-dump` exit codes | one test each | **done** |

Criteria 10, 11, 17, 18, 21 and 23 are addressed below under **Open TODOs**
or **Deviations**.

Timing, `release`, warm cache, parse **and** decode every record — no
document model built, so these are not the phase's full-import budgets:

| What | Budget | Measured |
|---|---|---|
| Whole corpus, 59 files, 12 MB | ≤ 3 s | **≈ 0.39 s**, four `xar-dump` processes included |
| `ProbeX16.xar`, 7.4 MB, 870 433 records, depth 13 | ≤ 350 ms for a full import | **≈ 0.29 s** including process start |

`criterion` benchmarks and the per-tag-family breakdown were not written,
and `docs/memory/perf.md` does not exist yet.

### The 5 274 records (0.379 %) with no decoder

Every one is deliberate, and every one is a phase that owns it:

| Tags | Count | Why not |
|---|---:|---|
| 105, 106, 4072, 4073 blends | 1 976 | Live objects, Phase 13 |
| 107–110, 4012 moulds | 788 | Live objects, Phase 13 |
| 4050, 4051 shadows; 4052, 4057 bevels; 4066, 4067 contours | 248 | Live objects, Phase 13. All six are on the corpus **atomic** list, so their subtrees are stripped — which is the format's own rule, not a shortcut |
| 4079–4113 brushes | 24 | One corpus file, very complex, base path present regardless; Phase 13 |
| 3506, 3507, 3509 printing | 344 | Deliberately never: irrelevant to rendering |
| 4015 export hint, 4040 WizOp, 4128 compound render | 19 | Editor and export hints, no rendering effect |

## Decisions taken (and why)

- **This crate does not depend on `xarast-doc`.** Phase 3 was split so that
  the byte layer could land while the document model was still being
  written. Everything stops at [`Decoded`], a flat, model-free enum. The
  mapping is a thin layer on top; see **What the mapping stage needs**.
- **Raw DEFLATE, `windowBits = -15`** — `flate2::Decompress::new(false)`.
  A zlib-wrapped inflater fails on the first byte of every `.xar` in
  existence.
- **The `TAG_ENDCOMPRESSION` trailer is repositioned via
  `Decompress::total_in()`.** Its record *header* is inside the deflate
  stream; its eight payload bytes are not. On meeting tag 31 the reader
  drives the inflater to stream end, seeks to `stream_start + total_in`,
  switches to plain mode and reads the trailer there.
- **The CRC is accumulated over bytes *consumed*, not bytes produced.** The
  reader inflates ahead in 16 KiB chunks; charging the CRC at production
  time would include read-ahead past the block's logical end. Consumption
  ends exactly at the tag-31 header, which is exactly what the trailer
  covers.
- **The inflated window is compacted as it is consumed**, so a 64 MiB block
  costs a 64 KiB window rather than 64 MiB of buffer.
- **The three-way unknown-tag policy is implemented from the start**, not
  deferred: essential → abort, atomic → strip the record *and* the subtree
  of the `DOWN` that follows it, anything else → skip. Newer commercial
  Xara versions write tags above 4138 and this is the only thing that keeps
  those files readable.
- **`TAG_SPREADINFORMATION` bit 0 is the double-page-spread flag.** The
  original contradicts itself (import handler bit 0, debug printer bit 2);
  we follow the handler.
- **Tolerant reads everywhere.** `Cur::opt_i32`/`opt_f64`/`opt_u8`/
  `opt_profile` return `None` at end of payload and the caller substitutes
  a documented default. Where a size constant in the original's headers
  disagrees with its writer, the writer is the truth.
- **Points are translated by the coordinate origin, vectors are not.**
  `Cur::point(origin)` and `Cur::vector()` are separate calls with
  different signatures so the two cannot be confused. Regular-shape axes
  use `vector()`; the matrix translation uses `point`-style translation.
- **Bitmap bytes are never copied or re-encoded.** `BitmapDefinition.image`
  is a `Range<usize>` into the record payload, so an embedded JPEG stays
  bit-identical and costs nothing to "decode".
- **Text is structural only.** Strings, lines, kerns, stories and the
  2900–2920 attributes are decoded; nothing is shaped, measured or
  positioned.
- **Diagnostics carry numbers, never text taken from the file.** A
  `Diagnostic` is a code, a severity, a record number, a tag and one `u64`.
  That is what makes the facts-only snapshot rule structural rather than a
  review item. `DiagSink` caps stored diagnostics at 4096 and counts the
  rest, because a hostile file can produce one per record.
- **Resource caps** (`ReaderLimits`): 64 MiB per block or 200 × the
  compressed bytes available, whichever is larger; 256 MiB inflated per
  file; 64 MiB per record; 8 000 000 records; 1024 levels of nesting.
- **A record budget proportional to the input was added** beyond the phase
  document: a record costs at least eight stream bytes and the stream is at
  most `inflation_ratio ×` the file, so a 1 KiB input can never produce
  millions of records. Without it a tiny bomb of empty records still cost a
  gigabyte of *tree* while respecting every cap the document named.
- **The depth cap exists for `Drop`, not for parsing.** A `RecordTree` is
  dropped recursively; a million nested `DOWN`s would overflow the stack
  *after* a successful parse. Deeper nodes are flattened onto the deepest
  level allowed, with a `DepthLimit` diagnostic.

## Invariants that must not be broken

1. **`P[i] = P[i-1] − D`.** Relative path deltas are *subtracted*. A sign
   error mirrors geometry and looks entirely plausible on symmetric shapes.
   The `OneLine.xar` oracle and an asymmetric synthetic path both guard it.
2. **Record numbering counts every record from 1**, including `UP`, `DOWN`,
   `TAG_FILEHEADER` and everything inside compressed blocks. It is the
   format's only pointer; `fuzz_xar_records` asserts the sequence is dense.
3. **`TAG_TEXT_STRING` (2201) has no terminator; every other string does.**
   Its length is `size / 2` code units and an embedded NUL is a character.
   `Cur::utf16_rest` is its only caller.
4. **Never allocate from a declared length.** Allocation follows bytes
   actually read. Nine length-bearing fields are tested directly.
5. **Never panic on any input.** The crate denies
   `clippy::indexing_slicing`, `unwrap_used`, `expect_used`, `panic` and
   `arithmetic_side_effects`, with no `#[allow]` outside `#[cfg(test)]`.
6. **Relative paths whose `size % 9 != 0` are dropped, never guessed at.**
7. **No corpus bytes are ever committed here.** Not as a test fixture, not
   as a fuzz seed, not inside a snapshot. Three tests enforce it:
   `no_snapshot_leaks_file_content`,
   `the_committed_seed_corpus_contains_nothing_from_the_real_corpus`, and
   `the_fact_modes_print_only_facts` in `xarast-cli`.

## Deviations from the phase document, and why

- **`fuzz_xar_import` is `fuzz_xar_decode`.** The named target needs
  `DocumentBuilder`. The substitute exercises the same handlers, the same
  reference resolution and the same "valid or nothing" property at the
  level this crate reaches: every decoded path passes
  `Path::validate`, every bitmap range lies inside its payload, every
  resolved colour channel is finite.
- **`Diagnostic`, `DiagCode` and `Severity` are defined here** rather than
  imported from `xarast-doc`. When the two meet, one of them should become
  a re-export; the `DiagCode` names are already the ones the phase document
  specifies.
- **Snapshots are plain committed text files, not `insta`.** Adding a
  snapshot framework for two files did not pay for itself, and a bespoke
  comparison let the leak grep be field-aware rather than a regex.
- **`XarError` has three variants the document did not name**:
  `BadFileType`, `ShortRecord(usize)` (a record ran out of bytes, as
  distinct from the file doing so) and no `Build(BuildError)`, which has
  nothing to wrap yet.
- **`xar-dump --model` and `--validate` exit 2 with a message** saying the
  document model is not wired up. They are in the usage text so that the
  gap is visible rather than silent.
- **The research note's master table is missing two tags.**
  `research/01 §4.1` lists 300 tags, but omits `TAG_PATH_FLAGS` (111) and
  `TAG_TEXT_FONT_SIZE` (2906) — both of which the same document describes
  elsewhere (§7.5 and §4.12.4), and 111 is **10.96 % of every record in the
  corpus**. `src/tags_table.rs` carries 302 entries for that reason. Worth
  correcting in the research note.
- **Criterion 15's expectation was wrong.** `SimpleText.xar` holds three
  `TAG_TEXT_STRING` records (19, 18 and 20 code units), not one. The
  38-byte / 19-character record the research document measures is there and
  is what the test asserts.

## What the mapping stage will need

Everything it needs is already typed; the work is translation, not parsing.

- **Walk `FileAnalysis::tree`** and call `decode` per node, exactly as
  `report::DecodePass` does. The tree already has atomic subtrees stripped
  and `DOWN`/`UP` resolved into parent/child.
- **Attribute scope is the tree**: attributes are children of the object
  they apply to, so pushing a context on descent and popping on ascent is
  all the inheritance model needs. There is no attribute stack in the file.
- **Colours**: `ColourRegistry` already maps record numbers onto
  `xarast_color::ColourId` and resolves tints, shades and links. It needs
  to be fed in record order, which the tree walk does.
- **The coordinate origin is currently always `(0, 0)`.** `Cur::point` takes
  it and the plumbing is complete, but nothing computes it: the original
  derives it from the spread's page rectangle when it processes
  `TAG_SPREADINFORMATION`, and in every corpus file that rectangle starts
  at the document origin. The mapping stage owns spreads, so it should
  derive the origin there and pass it down. **Verify against a file with a
  non-zero pasteboard offset before assuming zero is always right.**
- **`Decoded::PathFlags`** arrives as a sibling record — the path's first
  child — so the mapping needs to remember the last path it built within
  the current scope and call `apply_path_flags`.
- **`Decoded::FillMapping`, `TransparencyMapping` and `FillEffect`** modify
  the *preceding* fill, so they need the same "last attribute" memory.
- **`Decoded::CurrentAttributes` (4119)** is kept in the tree rather than
  stripped, because whether its subtree should populate `DefaultAttrs` is
  still open (below). It is on the corpus atomic list, so dropping it later
  costs nothing.
- **Unhandled records** reach the tree as nodes with no decoder;
  `NodeKind::Opaque` should carry them so they round-trip.

## Dead ends (do not retry)

- Zlib-wrapped inflate (`windowBits = 15`). The stream is raw.
- Assuming `pos == len` after `TAG_ENDOFFILE`.
- Assuming one `TAG_ATOMICTAGS` record holds the whole list. The corpus has
  739 of them, four bytes each, one tag apiece.
- `#[repr(C)]` or `#[repr(C, packed)]` payload structs. Fields are
  unaligned and lengths are variable; read field by field.
- Heuristics for the non-interleaved relative-path variant. It is a
  compile-time alternative in the original that was *enabled*, no file
  using the other branch has ever been seen, and a heuristic would risk
  misreading correct files.
- Guessing the unit of `TAG_TEXT_TRACKING` (below).
- Reserving from a declared count, anywhere, ever.

## Open questions this phase did not settle

1. **`TAG_TEXT_TRACKING`'s unit.** The original's type is `MILLIPOINT`, but
   the value is combined with the font size at render time. `TextAttr::
   Tracking(i32)` carries the raw value unconverted. Settle it against a
   reference rendering in Phase 9, not by guessing.
2. **The inconsistent angle encodings.** `ANGLE` is `FIXED16` radians, but
   `TAG_SHADOWCONTROLLER` uses a bespoke integer encoding and `TAG_BEVEL`
   integer degrees. None of those three records has a decoder yet, so
   nothing depends on it; whoever writes them in Phase 13 must check case
   by case.
3. **Whether `TAG_CURRENTATTRIBUTES` should feed `DefaultAttrs`.** The
   investigation the phase document asks for — dump the subtree for all 59
   files and compare against `DefaultAttrs::xara_compatible()` — needs the
   document model's defaults to compare against, so it belongs to the
   mapping stage. The records are decoded and kept in the tree so that it
   can be answered with data.
4. **The coordinate origin for a non-zero pasteboard**, as above.

## Open TODOs

- Mapping into `xarast-doc`: criteria 10, 11 and 18 cannot be met until it
  exists.
- Run the fuzz targets for real (criteria 17 and 18): `cargo fuzz` needs
  nightly, which was not available here. The targets compile and their
  seeds are committed; `fuzz/README.md` has the exact command lines.
- `criterion` benchmarks and the per-tag-family breakdown; append them to
  `docs/memory/perf.md`, which does not exist yet.
- Legacy regular shapes (1000–1217, 1900): zero occurrences in the corpus,
  layout rules in `research/01 §4.7.1`. They currently fall through to
  `UnknownTag`/`Info`, so the object goes missing rather than coming out
  wrong.
- Absolute path tags 100–103 as top-level nodes, and `TAG_PATHREF_*` (118,
  4013) deduplication: the codecs exist, the corpus never exercises them.
- `TAG_DEFINESOUND_WAV` (70), contoned bitmap nodes (199) and XPE bitmap
  properties (4117/4118).
