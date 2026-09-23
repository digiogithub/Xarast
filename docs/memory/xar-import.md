# `.xar` importer

Subsystem note for `crates/xarast-xar`, the `xar-dump` binary in
`crates/xarast-cli` and the fuzz targets in `fuzz/`.
Specification: `docs/phases/phase-03-xar-importer.md`.
Normative format reference: `docs/research/01-xar-format.md`.

## Current state

**Phase 3 is complete**, byte stream to validated `Document`.

| Layer | State |
|---|---|
| Physical reader: magic, framing, raw deflate, CRC trailer, streamed records | Done |
| Record numbering and `Ref` resolution | Done |
| `DOWN`/`UP` tree, tolerant of imbalance, depth-capped | Done |
| `TAG_ATOMICTAGS` / `TAG_ESSENTIALTAGS` and the three-way unknown-tag policy | Done |
| Typed decoding of 167 tags (the ~45-tag minimum set and the families that share its codecs) | Done |
| **Mapping into `xarast-doc` (`src/import.rs`)** | **Done** |
| `xar-dump`, including `--model` and `--validate` | Done |
| Facts-only snapshots, corpus harness | Done |
| Six fuzz targets, synthetic seed corpus, bounded-allocation tests | Done |

### What the mapping stage produces, measured

`XARAST_CORPUS_REQUIRED=1 cargo test -p xarast-xar --test corpus`, over the
59 files. The per-file table is `tests/snapshots/import-model.txt`; these
are its column totals.

| | |
|---|---:|
| Records read | 1 390 282 |
| … mapped (a node, an attribute, a resource, or a scope) | 1 384 408 |
| … skipped (view state, printing, metrics, editor hints, current attributes) | 3 712 |
| … stripped with an atomic subtree | 2 162 |
| … kept verbatim as `NodeKind::Opaque` so they round-trip | 1 594 |
| Document nodes built | 830 533 |
| `Document::validate()` errors | **0** |
| `Document::validate()` warnings, all `AttrAfterInk` | 267 |

Nodes by kind: 59 documents, 59 chapters, 59 spreads, **60 pages** (one
file is a double page spread), 115 layers, 59 grids, 151 991 paths,
35 183 quick shapes, 25 bitmaps, 19 057 groups, 170 text stories, 326 text
lines, 8 116 text items, 613 660 attributes, 1 594 opaque.
Resources: 5 760 palette entries, 38 bitmaps.

Round trip, asserted per file rather than sampled: all **151 991 path
point vectors and 5 760 palette entries compare equal** to what the reader
decoded, and every embedded bitmap's bytes are byte-identical.

Timing, `test` profile (`opt-level = 2`), warm cache: parse-only over the
whole corpus **0.38 s**; full import to `Document` over the whole corpus
**1.98 s** for 830 533 nodes. The phase's 3 s budget is for a release
build, so this is comfortably inside it; a release measurement and the
per-file budgets still belong in `docs/memory/perf.md`, which does not
exist yet.

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
| 10 documents passing `validate()` | 59 | **59**, zero errors |
| 11 spread, layer and ink node present | 59 | **59** spreads and layers; ink where there is ink, see below |
| 12 `OneLine.xar` oracle | exact | **exact**: 88 records, depth 5, `MoveTo(112101,178899) → LineTo(283101,321399)`, record 31 CMYK "Black", line width 500 mp |
| 13 facts-only snapshots | committed + leak grep | **done** (`tests/snapshots/`, three files) |
| 14 bitmap bytes verbatim, magic matches | all | **all** |
| 15 text decodes | 14/14 `TextDesigns` | **14/14**; `SimpleText.xar` yields the 19-character line |
| 16 record framing round trip | lossless | **lossless** |
| 19 bounded allocation per length field | < 1 MiB | **done**, 9 fields |
| 20 decompression bomb | rejected, < 128 MiB peak | **done** |
| 18 `fuzz_xar_import`'s "valid or nothing" | asserted in the target | **asserted and run** (2026-09-23): clean after the three fixes below |
| 22 `xar-dump` exit codes | one test each | **done**, including 3 for a document that fails validation |

Criteria 17 and 18 are met: every target has run on nightly (below,
"Fuzzing, first runs") and runs every night in CI. Criterion 23 is
addressed under **Open TODOs**.

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

- **Only `import` depends on `xarast-doc`.** Phase 3 was split so that the
  byte layer could land while the document model was still being written,
  and the split is still worth keeping: `reader`, `tree` and `decode` know
  nothing about the model, which is what lets them be fuzzed and
  snapshot-tested on their own. `import` sits on top and is the only
  module that reaches for `DocumentBuilder`.
- **`Severity` is `xarast-doc`'s, re-exported; `DiagCode` and `Diagnostic`
  stay here.** One three-valued severity serves everybody. `DiagCode`'s
  members name conditions of *this wire format* — a CRC, a deflate block, a
  nine-byte path stride — which would be noise in a crate that also serves
  SVG and `.xarast`, so it stays, with `DiagCode::shared()` projecting it
  onto the model's vocabulary. `Diagnostic` stays too, and stays `Copy` and
  **text-free**: `xarast_doc::Diagnostic` carries a `String` message, which
  is right for explaining a repair to a user and wrong for a value a
  committed snapshot is generated from. `From<Diagnostic>` converts at the
  boundary, building the message from constants and numbers.
- **The importer never touches the arena.** Every node, attribute and
  resource goes through `DocumentBuilder`; nothing in `import.rs` can reach
  `Tree`'s mutating methods, because they are `pub(crate)` in another
  crate. That is what makes "an importer cannot produce an inconsistent
  document" structural rather than a review item.
- **Attribute scope needs no stack.** `.xar` writes an object's attributes
  as its children and the model's rule is "an attribute applies to its
  following siblings and to its parent's own ink", which is the same rule.
  `push_scope` on descent and `pop_scope` on ascent is the whole mapping.
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
- **The spread coordinate origin is `(page_margin, page_margin)`**, derived
  rather than assumed, and checked against the files. See finding 1 above.
- **Pages are synthesised.** `.xar` has no page record: a spread implies
  one page, or two side by side when bit 0 is set. The mapper emits them
  from `TAG_SPREADINFORMATION` so that the model's canonical
  document → chapter → spread → (page, grid, layer) shape holds.
- **`TAG_DOCUMENT` creates no node.** `DocumentBuilder`'s root *is* the
  document node, so tag 40 is consumed and its children stay at the root
  level. Pushing a scope for it would emit an `UnbalancedScope` warning on
  every single file.
- **"No colour" is the transparent colour.** The model's `Paint` always
  holds a colour, and `TAG_FLATFILL_NONE` means *do not paint*, which a
  flat fill of fully transparent black expresses exactly.
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
   `the_fact_modes_print_only_facts` in `xarast-cli`. The snapshot guard is
   field-aware: every field of a fact line must be a decimal number, a
   `TAG_*` name, a class name, `yes`/`NO`, or the file's own path.
8. **The mapping goes through `DocumentBuilder` and nothing else.** No
   `&mut Tree`, no arena access, no "amend the node I just made" escape
   hatch. Where the format describes a node from its children, the mapper
   reads ahead instead.
9. **`records_mapped + records_skipped + records_stripped ==
   records_read`.** Asserted per file in the corpus harness and inside
   `fuzz_xar_import`. It is not bookkeeping for its own sake: it can only
   balance if every node of the record tree was visited exactly once, so a
   double-visit or a missed subtree is a test failure rather than a
   silently wrong document.
10. **The origin is derived from the spread, never assumed.** `(0, 0)` is
    right only for a file whose pasteboard margin is zero, and one corpus
    file out of 59 is like that.
11. **A second `DOWN` into the same node appends.** `N DOWN a UP DOWN b UP`
    gives `N` the children `a, b`: each `DOWN` makes the following records
    children of the last node inserted. Assigning instead of appending lost
    `a` from the tree while still counting it (fuzz finding, below).
12. **A subtree the mapper does not descend into is counted as skipped.**
    Grid records (46/47) and the defaults under `TAG_CURRENTATTRIBUTES` are
    single records in every real file; anything a corrupt file nests under
    them is `count_subtree`-ed into `records_skipped`, or invariant 9 breaks.

## Fuzzing, first runs (2026-09-23)

Nightly and `cargo-fuzz` 0.13.2 became available, and all six targets ran
for ten minutes each, seeded with the synthetic corpus plus, locally only,
the 59 real files (passed as an extra read-only corpus directory, never
copied into `fuzz/`). Numbers from one run on a shared 24-core machine:

| Target | Execs | exec/s | Findings |
|---|---|---|---|
| `fuzz_xar_records` | 299 k | 497 | none |
| `fuzz_xar_tree` | 237 k (clean rerun) | 394 | the `DOWN` append bug |
| `fuzz_xar_decode` | 523 k | 870 | none |
| `fuzz_xar_import` | 389 k (clean rerun) | 646 | three accounting bugs |
| `fuzz_xar_path` | 70.2 M | 116 750 | none |
| `fuzz_xar_colour` | 5.8 M | 9 706 | none |

The findings, each now a test in `tests/fuzz_regressions.rs`:

- The committed `every-tag` seed failed `fuzz_xar_import` on its first
  execution: an unresolved `TAG_NODE_BITMAP` became an opaque node and was
  counted as mapped twice.
- Grid records' and defaults' children were never counted (invariant 12).
- The tree builder's `DOWN` assignment (invariant 11), found by both
  `fuzz_xar_tree` and `fuzz_xar_import`.

Nothing panicked, overflowed or ran out of memory: the `clippy` deny list
of invariant 5 held. Every finding was an accounting or structure bug that
only the stronger assertions — record accounting, walked nodes equal to
counted nodes — could see.

Minimising: `cargo fuzz tmin` then rebuild the input by hand with
`XarBuilder`. The minimised bytes of an input that started from a real
file are not committed even when they look synthetic.

## Deviations from the phase document, and why

- **`fuzz_xar_import` now exists**, and asserts the phase document's
  "valid or nothing" in its strong form: either `import` fails, or the
  document has zero `validate()` errors. It also asserts the record
  accounting balances, which is how a handler that visits a node twice — or
  not at all — shows up as a finding rather than as a quietly wrong
  document. `fuzz_xar_decode` stays as the narrower target, so that a crash
  in it points at the decoders rather than at the builder.
- **Snapshots are plain committed text files, not `insta`.** Adding a
  snapshot framework for two files did not pay for itself, and a bespoke
  comparison let the leak grep be field-aware rather than a regex.
- **`XarError` has three variants the document did not name**:
  `BadFileType`, `ShortRecord(usize)` (a record ran out of bytes, as
  distinct from the file doing so) and no `Build(BuildError)`, which has
  nothing to wrap yet.
- **`xar-dump --model` and `--validate` are wired up.** `--model` prints
  the document tree and is *file content*, so it is interactive-only and
  never committed; `--validate` prints a node census and the invariant
  result, which are facts, and exits 3 when the document fails validation.
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

## What the mapping stage taught us

Everything below was learned by writing `src/import.rs` and running it over
the corpus. Where it contradicts the handover note it replaces, the
contradiction is called out.

### 1. The coordinate origin is the pasteboard margin, and it is not zero

The previous note said the origin was hard-coded to `(0, 0)` and that this
"happens to be right for all 59 corpus files". **It is wrong for 57 of
them.** The derivation, now implemented in
[`import::spread_origin`] and written up in `research/01 §5.3`:

```
page_margin = if margin < bleed { margin + bleed } else { margin }
origin      = (page_margin, page_margin)
```

A spread places its first page's `lo` corner at `(page_margin,
page_margin)` in spread coordinates and, for a double page spread, the
second one page-width to the right, so the union's `lo` corner — which is
what `research/01 §5.3` defines the origin to be — is the first page's
either way.

**The corpus does distinguish the two hypotheses**, and the previous note's
"verify against a file with a non-zero pasteboard offset" turns out to be
answerable without one. Two records are written by subtracting the origin
from an *empty* `DocRect`:

* every empty `Templates/*.xar` writes `TAG_VIEWPORT` (the drawing's
  bounding box) as `(-margin, -margin, -margin, -margin)`;
* all 295 degenerate `TAG_CURRENTATTRIBUTEBOUNDS` records in the corpus are
  `(-margin, -margin)`.

`margin` is 576 000 or 566 931 millipoints in 57 files, 432 000, 144 000 or
72 000 in three more, and 0 only in `animation.xar` — whose viewport is
correspondingly `(0, 0, 0, 0)`. `the_spread_origin_is_the_one_the_files_imply`
asserts exactly this, so the origin is checked against the files rather
than asserted by us.

What the corpus **cannot** distinguish is `margin` from
`margin < bleed ? margin + bleed : margin`, because every one of the 59
files has `bleed == 0`. So that branch gets a file of its own:
`a_synthetic_file_with_a_bleed_wider_than_its_margin_shifts_the_origin`
builds a complete `.xar` with `synth::XarBuilder` — header, spread, layer
and a two-point path, `margin = 10 000`, `bleed = 25 000` — imports it,
and asserts the page and the path both land at 35 000. Synthetic, so
nothing of Xara's is copied to get it. A second test,
`a_bleed_larger_than_the_margin_widens_the_origin`, pins the arithmetic
without the file.

Consequence for anyone reading a dump: **an object's document coordinates
are its file coordinates plus the margin**, and the page rectangle moves
with them, so page-relative geometry is unchanged. `OneLine.xar`'s path is
at (112 101, 178 899) in the file and (688 101, 754 899) in the document.

### 2. `TAG_VIEWPORT` does not round trip, and that is the original's bug

It is *written* with the origin subtracted (`Kernel/viewcomp.cpp:512-515`)
and *read* with `ReadCoordTrans(..., 0, 0)`, i.e. untranslated
(`:849-851`). We follow the reader, so `decode` no longer translates tag
80. `TAG_DOCUMENTVIEW` (82) uses plain `ReadCoord` and *is* translated.
Nothing in the model depends on either; the asymmetry mattered only
because it is what makes the origin measurable.

### 3. `TAG_CURRENTATTRIBUTES` does **not** carry document defaults

**Corrected by XARA-T-0037 (2026-09-23).** This finding used to say the
opposite, and it was wrong. The block holds the editor's *current*
attributes, which the original applies to the **next object the user
draws**: its loader switches into a "make current" insert mode for the
subtree (`Kernel/rechdoc.cpp:1942-1966`). An object with no attribute in
the file inherits the factory default, which no file overrides. The default
fill is "no colour" (`research/01 §8.1`). Feeding the block into
`DefaultAttrs` gave every unfilled path the file's current fill.
`Designs/SimpleSphere.xar` shows it: the current fill is black, and its
last object, a filled-and-stroked frame with no fill attribute, then
covered the whole design in black. The block's `TAG_FILL_REPEATING` also
became every gradient's default mapping, which is half of why its gradients
wrapped (see `app-core.md` decision 31).

The block is rich: across the corpus it holds 816 attributes, 434 of them
different from the factory defaults, in 53 of 59 files. They are now
counted (`ImportReport::current_attributes` / `current_differing`) and
**skipped**. The model has no current-attribute store yet; that is Phase 7
T2.7.

The trap in it, which cost a wrong colour palette before it was found:
**the subtree also contains definitions** — 26 `TAG_DEFINECOMPLEXCOLOUR`
and 27 `TAG_FONT_DEF_TRUETYPE` records — and the default fill immediately
references them by record number. A handler that only looks for attributes
there loses those colours from the palette and every later reference to
them resolves to black. `Mapper::definition` is called from both the
object walk and the defaults walk for that reason.

### 4. `TAG_CURRENTATTRIBUTES` is not an object

`tags_table.rs` classified tag 4119 as `TagClass::Object`. It is a
container of document defaults; calling it an object makes the acceptance
check "a file with object records must have ink nodes" fail on the eight
empty templates, whose only "objects" are their two current-attribute
blocks. Now `Structural`.

### 5. Criterion 11 of the phase document is wrong

It asks for "at least one ink node" in every file. The eight
`Templates/*.xar` are empty documents by definition — a spread, a layer, a
palette and no drawing — so they have none. What the test asserts instead
is the implication: a file with at least one `TagClass::Object` record
produces at least one ink node, and one without produces none.

### 6. `FillMapping` and `FillEffect` need no "last fill" memory

The handover said `PathFlags`, `FillMapping` and `FillEffect` all need the
mapping stage to remember the preceding fill. Only `PathFlags` does.
Phase 2 gave `FillMapping`, `TranspFillMapping` and `FillEffect` **slots of
their own** (`attr/tags.rs`), so tags 160–165, 180–182, 206 and 207 become
ordinary attribute nodes and the format's "modifies the preceding fill"
wording is a statement about the original's data structures, not about
ours.

### 7. Look-ahead replaces mutation

`DocumentBuilder` is append-only, and three records describe the node that
encloses them: `TAG_PATH_FLAGS` (the path's first child),
`TAG_SPREADINFORMATION` and `TAG_LAYERDETAILS` (children of the spread and
the layer). Rather than widen the builder with an "amend the node I just
made" method — which would be a hole in the one-construction-path rule —
the mapper reads those children *before* emitting the parent. The record
tree is already fully materialised, so it is free, and it is what the
original does under the name `ReadPostChildren`. The same trick merges
`TAG_GRIDRULERSETTINGS` and `TAG_GRIDRULERORIGIN`, which are two sibling
records describing one `Grid`, and reads `TAG_TEXT_STORY_WORD_WRAP_INFO`
into a story's `TextLayout`.

### 8. `ColourRegistry` cannot own the palette any more

`ColourRegistry` kept its own `ColourTable`, but the palette has to end up
in the *document's* table, and the only way to put it there is
`DocumentBuilder::define_colour`, which allocates the `ColourId`. Rather
than duplicate the parent-resolution logic, the registry gained
`define_external(record, def, diags, insert)`: it still resolves the parent
chain from record numbers and still diagnoses a parent the file never
defined, and the caller supplies the entry. In that mode
`ColourRegistry::table()` stays empty — `report.rs` still uses the owning
mode, so both are live.

### 9. Bitmap deduplication was collapsing every bitmap onto the first

`BitmapResource::content_hash` hashed the *decoded* payload, which Phase 3
deliberately leaves empty until Phase 10 decodes it. Every bitmap in a
file therefore hashed identically and `insert_bitmap` returned one id for
all of them. `content_hash` now also covers `original`, which is both the
fix and the more correct rule: two bitmaps with the same pixels but
different encodings are two resources, because the writer emits `original`
verbatim and collapsing them would throw one encoding away. Changed in
`xarast-doc`; `document-model.md` records it.

### 10. Model gaps the mapping found, and what it does about them

None of these is a blocker; all are recorded so that Phase 4 and Phase 7
do not rediscover them.

* **Three-point linear fills** (`TAG_LINEARFILL3POINT` 4121, 17 records;
  `TAG_LINEARTRANSPARENTFILL3POINT` 4123, 10). The model's `Linear` has
  `persp: Option<Perspective>`, which is *two* extra corners; the format
  gives one. Mapped to a plain `Linear`, losing the skew, with an `Info`
  diagnostic. A `FillGeometry::Linear` that takes one extra axis point is
  the real fix.
* **`TAG_FILL_REPEATING_EXTRA`** (206/207, 10 and 0 records). The model's
  `Tiling` has four values and none is the original's "extra" repeat;
  mapped to `Repeat`.
* **Predefined dash patterns** (`TAG_DASHSTYLE` with a reference in
  −1..−20 or −22, 15 records). The twenty patterns are numbers in
  `Kernel/cxfdash.h`; their geometry lives in the original's dash gallery
  and is not in the format. Rather than invent them, the attribute is not
  emitted and the line keeps what it inherited, with a diagnostic. −21
  ("solid") and a null reference map to the empty pattern, which is right.
* **`TAG_QUALITY`** (179, absent from the corpus) is a 0..110 slider, not
  an enum. The original's thresholds (`Kernel/quality.cpp:157-255`) — 30
  for real lines, 60 for graduated fills, 100 for antialiasing — are
  exactly the model's four levels, and that is the mapping.
* **`TAG_SPREAD_ANIMPROPS`** carries seven words — loop, global delay,
  dither, web palette, colours palette, colour count, flags — and
  `AnimProps` has a field for one of them. The other six describe how the
  spread is *exported* as an animated GIF rather than what it is, so they
  are dropped rather than crammed into `hidden`/`background`, which mean
  something else. A `GifExportProps` on the spread is the real fix, and
  nothing needs it before Phase 10.
* **`TextLayout::OnPath { reversed }`** is derived from the LEFT/RIGHT half
  of tags 2110–2117, reading `research/01 §4.12.2`'s "from which end, and
  in which direction it flows". Seven records in the corpus; confirm
  against a reference rendering in Phase 9.

### 11. Unhandled records survive as `NodeKind::Opaque`

1 594 of them across the corpus: shadows, bevels, contours, blends,
moulds and brushes. Anything whose `TagClass` is `Ignorable` is dropped
instead (printing, export hints, WizOp), because round-tripping a printer
setting into a drawing is not worth a node. Phase 13 replaces the opaque
nodes with `NodeKind::Live`.

### 12. A regular shape's paths are edge templates, not its outline

Found by XARA-T-0013 (2026-09-23). The two paths in
`TAG_REGULAR_SHAPE_PHASE_2` each describe **one edge**. An unedited edge
is a two-point `MoveTo, LineTo` at an arbitrary position, typically
`(-576pt, -576pt) → (-504pt, -576pt)`. The importer used to store the
primary one as `QuickShape.path`. Bounds, the walker and the renderer all
treat that field as document geometry, so every one of the 35k quick
shapes got a sliver of bounds far from where it sat and was culled. The
circular flag, the stellation offset and both curvatures were being
dropped as well.

The outline is now generated by `xarast_geom::regular_shape_outline`,
from the facts in `research/01 §4.7.1`. The generator builds it in the
shape's own untransformed space, where the record's rounding and edge
templates are exact, and only then applies the record's matrix. A
two-point template means a straight edge and is not stored; a curved one
is kept in `QuickShape.primary_edge`/`secondary_edge`. More than
`MAX_REGULAR_SIDES` (4096) sides yields no outline, which keeps a hostile
file from asking for 65 535-gons 35 000 times. After the fix, all 42
corpus files that have primitives paint (73 files counting the wxOil
templates). Before it, 38 had primitives and only 35 painted.
`TestBitmapFill` now draws its outline; the bitmap fill itself is the
renderer's job.

### 13. Import performance: where ProbeX16's 644 ms went

Measured by phase on the reference machine (2026-09-23, XARA-T-0031),
before the fixes: parse ~95 ms, mapping ~130 ms, `DocumentBuilder::finish`
~200 ms (four whole-tree repair walks ~70 ms, `validate()` ~129 ms), a
node census ~12 ms, a **second** `validate()` ~127 ms, and dropping the
`FileAnalysis` ~28 ms. Fixes: reuse the builder's report
(`finish_with_report`), record repair candidates at `node()` time, make
`validate()` O(n) and SipHash-free, skip `attach`'s walks for leaves, take
the decoded path instead of cloning it, and count kinds from the arena
when everything is reachable. Result: **329–343 ms** (budget 350), and
458–473 ms for the whole corpus. What is left, roughly: parse 95–130 ms,
record decode ~37 ms, builder `node()` ~60 ms, `validate()` ~47 ms,
dropping the analysis ~28 ms, and quick-shape outlines ~12 ms.

## Dead ends (do not retry)

- Storing a regular shape's edge path as its outline (finding 12).
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
- **Assuming the spread coordinate origin is `(0, 0)`.** It is the
  pasteboard margin, and it is non-zero in 58 of the 59 corpus files. The
  previous note recorded the opposite; finding 1 explains how the files
  settle it.
- **Adding an "amend the node I just made" method to `DocumentBuilder`** so
  that `TAG_PATH_FLAGS`, `TAG_SPREADINFORMATION` and `TAG_LAYERDETAILS` can
  patch their parent. The record tree is already materialised, so reading
  ahead costs nothing and leaves the one-construction-path rule intact.
- **Mapping `TAG_CURRENTATTRIBUTES` onto `DefaultAttrs`.** Those are the
  editor's current attributes. Doing it painted SimpleSphere's unfilled
  frame black over the whole design (XARA-T-0037).
- **Mapping `TAG_FILL_NONREPEATING` to `Tiling::None`, or the extra repeat
  (206/207) to `Tiling::Repeat`.** The model keeps the original's values
  0–4 (`None`, `Simple`, `Repeat`, `RepeatInverted`, `RepeatExtra`).
  Non-repeating reads as 1 (`Simple`), and only 4 makes a gradient tile.
- **Handling only attributes inside `TAG_CURRENTATTRIBUTES`.** Its subtree
  also defines colours and fonts, and its own default fill references them
  by record number; skipping them loses palette entries and turns later
  references black.

## Open questions this phase did not settle

1. ~~**`TAG_TEXT_TRACKING`'s unit.**~~ **Settled in phase 9** (2026-09-23)
   from the original's formatter, not by guessing: thousandths of an em,
   converted as `MulDiv(tracking, FontEmWidth, 1000)`
   (`Kernel/nodetext.cpp:1781-1792`). Manual kerns (`TAG_TEXT_KERN`, our
   `TextItem::Kern`) use the same unit (`:1763-1767`). The import keeps the
   raw values; `format_story` converts. See `docs/memory/text.md`.
2. **The inconsistent angle encodings.** `ANGLE` is `FIXED16` radians, but
   `TAG_SHADOWCONTROLLER` uses a bespoke integer encoding and `TAG_BEVEL`
   integer degrees. None of those three records has a decoder yet, so
   nothing depends on it; whoever writes them in Phase 13 must check case
   by case.
3. ~~**Whether `TAG_CURRENTATTRIBUTES` should feed `DefaultAttrs`.**~~
   **Settled: no** (XARA-T-0037). It was first settled "yes" because the
   block differs from the defaults in 53 files, but differing is exactly
   what current attributes do. The original's loader makes them current,
   not default. See finding 3.
4. ~~**The coordinate origin for a non-zero pasteboard.**~~ **Settled**,
   and it is the normal case, not the exotic one. See finding 1.
5. **Units defined by the file, as opposed to the thirteen predefined
   ones.** `builtin_unit_mp` in `import.rs` covers the negative references
   of `research/01 §5.2`, which is what the corpus's grids use (pixels, and
   getting that wrong would make every grid a third too coarse). A unit
   defined by `TAG_DEFINE_UNITS` (85/86) and referenced by record number is
   not resolved; no corpus file does it.
6. **Whether `TextLayout::OnPath::reversed` is the LEFT/RIGHT bit of tags
   2110–2117 or the START/END bit.** Seven records; settle it in Phase 9
   against a reference rendering, as with tracking.

## Open TODOs

- ~~Run the fuzz targets for real (criteria 17 and 18).~~ Done
  2026-09-23; they run nightly in `.github/workflows/fuzz.yml`.
- The model gaps of finding 10: a one-extra-axis `Linear` fill, an "extra"
  tiling, and the twenty predefined dash patterns (which need a table that
  is not in the format at all).
- Live objects: 1 594 records currently round-trip as `NodeKind::Opaque`
  and should become `NodeKind::Live` in Phase 13.
- ~~A release-profile import benchmark~~: done 2026-09-23 as
  `cargo bench -p xarast-xar --bench import`. ~~`ProbeX16.xar` 644 ms
  against the 350 ms budget~~: 329–343 ms after XARA-T-0031 (finding 13).
  The margin is thin. The next wins are `Record.data` as a range into a
  shared buffer, which removes 870k allocations and the ~28 ms drop, and
  the depth walk in `Tree::attach`, which the builder could answer from its
  scope stack.
- `perf` is not usable on the reference machine (`perf_event_paranoid = 2`,
  and there is no `perf` binary), and `samply` refuses to run. Phase
  timings were taken with temporary `Instant` instrumentation.
- The per-tag-family breakdown of import time.
- Legacy regular shapes (1000–1217, 1900): zero occurrences in the corpus,
  layout rules in `research/01 §4.7.1`. They currently fall through to
  `UnknownTag`/`Info`, so the object goes missing rather than coming out
  wrong. `regular_shape_outline` already covers their geometry.
- Regular-shape edge templates are fitted by a similarity. That is exact
  in the importer, which generates before applying the matrix.
  `QuickShape::outline()` works in document space and is exact only when
  the shape's transform is a similarity. The original's corner smoothing
  pass after rounding (`nodershp.cpp` after `:2790`) is not reproduced.
  It only matters for curved edge templates, and the corpus has none.
- Absolute path tags 100–103 as top-level nodes, and `TAG_PATHREF_*` (118,
  4013) deduplication: the codecs exist, the corpus never exercises them.
- `TAG_DEFINESOUND_WAV` (70), contoned bitmap nodes (199) and XPE bitmap
  properties (4117/4118).
