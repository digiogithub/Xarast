# xarast-format

Memory note for the native **`.xarast`** format (`crates/xarast-format`).
Normative spec: `docs/research/06-xarast-format.md`. Plan:
`docs/phases/phase-06-xarast-format.md`. Epic XARA-EP-0007.

## Current state

Phase 6, round 1 (2026-09-23): the byte layer is done; the SVG profile is not.

| Workstream | State | Where |
|---|---|---|
| W1 container | F1.1–F1.8 done | `name.rs`, `sniff.rs`, `eocd.rs`, `reader.rs`, `writer.rs`, `limits.rs` |
| W2 manifest | F2.1–F2.5, F2.8 (diagnostics only) done; **F2.6/F2.7 `meta.xml` model open** | `manifest.rs`, `digest.rs`, `reader.rs::consistency` |
| W3 SVG write | not started | — |
| W4 SVG read + preservation | not started (the container half of F4.8 is done: unknown entries are raw-copied with their rows) | `writer.rs::carry_from` |
| W5 resources | F5.1–F5.4, F5.6 done; F5.8 contract + validation done (no provider implementation) | `resource.rs`, `policy.rs`, `thumbnail.rs` |
| W6 durability | F6.1 (`write_atomic`, `.bak` = F6.2) and F6.3 (`DocumentLock`) done; F6.4–F6.9 open | `durability/` |

Public entry points: `XarastReader::{open, open_with}`, `PackageWriter`,
`ResourceIndex`, `Manifest`, `sniff`/`sniff_bytes`, `write_atomic[_with]`,
`DocumentLock`. The document-level `save_atomic(path, doc, ctx, opts)` of the
spec does not exist yet: it is `write_atomic(path, |f| writer.finish(f))` once
W3 can produce `document.svg`.

### How the SVG layer plugs in (W3/W4)

- **Save:** serialise the model to `document.svg` bytes and `meta.xml` bytes →
  `PackageWriter::set_document` / `set_meta`; `ResourceIndex::begin_recount`,
  `count_path` for every `href` written, `mark_referenced_in` over the foreign
  baggage text, `gc`, then `add_resources`; `carry_from(&source_reader)` for
  preserved entries; `finish_with_source(file, Some(&mut source_reader))`
  inside `write_atomic`.
- **Open:** `XarastReader::open` (never parses the SVG) → show
  `diagnostics()` / `suggests_read_only()` → `document_bytes()` to the SVG
  reader → `ResourceIndex::from_package(&reader)` → keep the reader alive for
  raw copies on the next save.
- `ThumbnailProvider` takes `&xarast_doc::Document`; the implementation lives
  over the renderer (xarast-app), never in this crate.

## Decisions taken (and why)

- **`zip` 8.6** (current stable major; 9.0 is pre-release), `MIT`, **every
  default feature off**, only `deflate-flate2`. No AES/bzip2/lzma/xz/ppmd/
  deflate64 in the reader's attack surface; `zopfli` is too slow for an
  interactive save. `quick-xml` **0.41** (already in the graph, `MIT`),
  `blake3` 1.8 (`CC0-1.0 OR Apache-2.0`). No `fs4`, no `tempfile` at
  runtime, no date crate: `std::fs::File::try_lock` (stable 1.89) gives
  `flock`, the temporary name is built by hand, and the two date conversions
  are the days-to-civil algorithm in `time.rs`.
- **DEFLATE backend: `miniz_oxide`, the workspace's only one.** The phase
  plan said "pin to `zlib-rs`"; that was tried and reverted. flate2 selects
  one backend per *build*, so enabling `zlib-rs` through `zip` switched the
  `.xar` reader too, and `xarast-xar/tests/fuzz_seeds.rs` failed in
  `cargo test --workspace` (a committed seed is compressed bytes) while
  passing with `-p xarast-xar`. The same mechanism would make a
  deterministic `.xarast` differ between binaries. One backend everywhere,
  pinned by the lock file, guarded by
  `tests/container.rs::deterministic_bytes_are_pinned`. Cost: 273 ms vs
  92 ms per 20 MB save (XARA-T-0090 tracks switching everyone to `zlib-rs`).
- **Zstandard is not compiled in.** The `compact` profile is v1.0 scope;
  a zstd entry opens (listed, diagnosed `UnsupportedMethod`, raw-copyable on
  save) but cannot be decoded. `WriteOptions::profile = Compact` is refused.
- **The writer buffers, then writes in one pass.** The manifest is entry #2
  and describes every other entry, so sizes and digests must be known before
  anything after `mimetype` is written. Payloads are `Arc<[u8]>` (no copy of
  what the document already holds) or raw copies from the source package.
- **Every data entry gets a digest**, not only the mandatory
  document/meta/resources: it is what lets a reader flag corruption. Only
  `mimetype` and the manifest itself carry none.
- **EFS (UTF-8) flag:** set for every non-ASCII name (the `zip` crate does
  it); pure-ASCII names are written without it, exactly as the spec's own
  verified example (§12.2, flags `00 00`). The reader rejects a non-ASCII
  name without the flag, detected as "raw bytes ≠ the name `zip` decoded"
  (without EFS it decodes CP437).
- **`..` is a path segment, not a substring.** `a..b` is a legal name,
  `a/../b` is not. Also rejected: empty and `.` segments (a name with two
  spellings defeats duplicate detection) and a drive-letter prefix (`C:`).
- **The zip-bomb ratio has a floor** (`Limits::ratio_floor`, 1 MiB): entries
  up to 1 MiB uncompressed are exempt, because 64 KiB of whitespace
  legitimately deflates 650:1 and a bomb under 1 MiB is harmless.
- **The manifest cap is 16 MiB** (`Limits::max_manifest_size`). A
  66,000-entry manifest is ~17 MB, so packages that size need the caller to
  raise it (the ZIP64 test does).
- **End-record pre-check (`eocd.rs`).** `zip` sizes its first allocation from
  the declared entry count (bounded only by the file size) and silently
  collapses duplicate names into one map slot. So the reader parses the end
  record itself first: entry count against `Limits::max_entries`,
  `count × 46 ≤ central directory size`, single disk, no trailing garbage, no
  end-record signature inside the archive comment (a backwards-searching
  reader would try that forged record first). Afterwards
  `zip.len() != declared count` ⇒ duplicate names ⇒ refused.
- **Diagnostics vs errors.** Hostile structure (bad names, duplicates, bombs,
  encryption, no manifest, wrong major) is an error. Manifest/ZIP divergence,
  newer `min-reader`, unknown required capability, digest-less mandatory rows,
  out-of-order entries are `Diagnostic`s; `suggests_read_only()` implements
  §3.4 rule 9. Digests are verified lazily, per read (`entry`,
  `entry_stream`), and in full by `verify_all()`.
- **Manifest namespaces are resolved by hand**, not with `NsReader`: foreign
  fragments must be re-emitted namespace-complete and `NsReader` does not
  expose the in-scope bindings. Foreign fragments are the **verbatim input
  bytes** with the inherited declarations *they use* inserted in the start
  tag. Inserting every in-scope binding made the second save differ from the
  first; inserting only used prefixes reaches the fixed point on the first
  re-save (fuzz-asserted). Unknown `mf:` attributes (a newer minor version)
  are kept as foreign attributes in the `mf` namespace; unknown roles as
  `Role::Other(String)`; unknown `mf:method` values are dropped (informative,
  recomputed on write).
- **Resource refcounts start from `mf:refcount` (or 1) on open**, so a save
  before the SVG layer recounts can never collect a resource. The SVG layer
  must recount (`begin_recount` + `count_path`) before `gc`.
- **`carry_from` carries** `history/`, `extensions/`, other `META-INF/*`,
  unknown top-level entries, and `resources/` entries that do not follow the
  hash naming — as raw copies with their rows, warts included (a digest-less
  row stays digest-less).
- **Atomic save** follows symlinks (replaces the target, keeps the link),
  copies the target's permissions to the temporary, and treats a directory
  that refuses `fsync` as non-fatal (after the rename it is too late to
  roll back anyway).
- **Lock staleness** needs all of: same host, same boot id, pid not alive
  (`/proc/<pid>`, Linux only — elsewhere every pid is "alive", the safe
  direction), and the `flock` free. The loser of a `force` never deletes the
  winner's lock file: release compares inode/device, not text (two locks of
  one process in the same second have identical text).

## Invariants that must not be broken

- `mimetype` first, STORED, no extra field, content at offset 38
  (`tests/container.rs::magic_bytes_at_fixed_offsets`).
- One manifest row per ZIP entry (directory entries excepted), plus exactly
  one `/` row whose media type is the `mimetype` content.
- Names are validated and **rejected**, never sanitised.
- The reader never allocates from a declared size beyond 1 MiB up front;
  reads are capped at `declared + 1` and a mismatch is `ZipBomb`.
- `open` never reads `document.svg`
  (`tests/container.rs::open_does_not_parse_the_document`).
- Deterministic save: DOS 1980-01-01, `0o644`, system `Unix`, canonical
  order and manifest, one DEFLATE backend (the golden digest test). An
  unchanged re-save through `from_package` + `carry_from` + raw copies is
  **byte-identical** (`resave_from_the_package_is_a_fixed_point`).
- Preserved entries are raw-copied (same compressed bytes, same method,
  same CRC) — never recompressed.
- No `unsafe` (`#![forbid(unsafe_code)]`); parser modules deny indexing,
  unwrap/expect, panic and unchecked arithmetic.

### Fuzzing

Two targets, `fuzz_xarast_open` (reader + every read path + a full re-save
of whatever opens, which must reopen) and `fuzz_xarast_manifest` (parser +
the write–parse fixed point). Seeds are generated by
`cargo run -p xarast-format --example fuzz_seeds -- <dir>`, never committed
(`crates/xarast-xar/tests/fuzz_seeds.rs` rejects any file under
`fuzz/corpus/` it does not generate itself); CI generates them before the
run. Round 1, 2026-09-23, `Limits::FUZZ`, 1 MiB max input:

| Target | Runs | Executions | Coverage | Findings |
|---|---|---|---|---|
| `fuzz_xarast_open` | 3 × 10 min (last on the final build) | 5.17 M + 2.46 M + 2.16 M | 5 292 edges | none |
| `fuzz_xarast_manifest` | 4 × 10 min (the first three stopped at a finding) | final clean run 6.69 M | 2 970 edges | 4, all fixed (below) |

### Fuzz findings (round 1)

All four were in the manifest parser and all were "a save changes what
the file means", not crashes; each input is now a unit test in
`manifest.rs`.

1. `quick-xml` does not validate names: `<mf:file-entry0mf:full-path="x"`
   is read as one element name containing `="`. Every element and
   attribute name is now checked as a QName (and `:x` is refused).
2. `quick-xml` skips a leading BOM **without counting it** in
   `buffer_position`, so fragment offsets were 3 bytes short. The BOM is
   stripped before the reader sees the input; a second BOM is refused.
3. Fragments nested in the `/` row were re-emitted at manifest level, where
   a nested `mf:file-entry` became a real row; the same for capabilities in
   `mf:requires`. **Rule: a foreign fragment always goes back inside the
   element it was read in.**

## Dead ends (do not retry)

- JSON manifest (§3.5). `data:` URIs above 4 KiB. Recompressing already
  compressed resources. `roxmltree` as the main parser. Zstd method id 20.
- `NsReader` for the manifest (cannot enumerate in-scope bindings).
- Completing foreign fragments with *every* in-scope binding (not a fixed
  point on re-save).
- Selecting a DEFLATE backend through `zip`'s `deflate-flate2-zlib-rs`
  feature (changes every crate's backend; see Decisions).
- Comparing lock-file text to decide ownership on release.

## Open TODOs

- W3/W4: the SVG profile, `meta.xml` model (F2.6) and its `<metadata>` mirror
  (F2.7), preservation context, `save_atomic(doc)`.
- **Doc model has no per-node foreign-baggage container** (risk K1 of the
  plan): filed as a task; it must land before W4.
- F6.4 lock UX + `SIGINT`/`SIGTERM` cleanup (app), F6.5 autosave, F6.6
  journal, F6.7 recovery scan, F6.8 `xarast repair`, F6.9 partial XML
  recovery.
- F5.5 master/derived (`set_derivation` and the manifest fields exist; the
  `--no-derived` regeneration path does not), F5.7 geometry dedup, F5.9
  previews from a provider, F5.10 `data:` policy — all need W3.
- `compact` profile (zstd), `history/` (v1.0).
- Raw copies take the host system from the running OS (`zip` does not let
  `raw_copy_file_touch` set it): byte-identical re-saves hold per platform,
  not across Linux ↔ Windows.
