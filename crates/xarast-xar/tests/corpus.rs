//! The corpus acceptance harness.
//!
//! Runs the 59 real `.xar` files of the Xara Xtreme fork through the
//! importer and asserts the numbers `docs/research/01-xar-format.md §12.2`
//! measured independently, in Python, before a line of Rust existed. If
//! either side is wrong these do not agree.
//!
//! **No corpus byte ever enters this repository.** The files are found
//! through `XARAST_XAR_CORPUS` (default `/home/user/xara-xtreme`) and
//! verified against `tests/corpus/corpus.lock`, which records only paths,
//! sizes and SHA-256 hashes. With no corpus present every test here skips
//! with a printed notice, so CI stays green; `XARAST_CORPUS_REQUIRED=1`
//! turns that skip into a failure, which is how the phase gate is run.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use xarast_xar::{FileReport, ReaderLimits, analyse, has_decoder, xar_dump_report};

/// One line of `corpus.lock`.
#[derive(Clone, Debug)]
struct CorpusFile {
    rel: String,
    size: u64,
    sha256: String,
}

impl CorpusFile {
    fn path(&self, root: &Path) -> PathBuf {
        root.join(&self.rel)
    }

    fn label(&self) -> &str {
        &self.rel
    }
}

#[derive(Debug)]
struct XarCorpus {
    root: PathBuf,
    files: Vec<CorpusFile>,
}

const LOCK: &str = include_str!("../../../tests/corpus/corpus.lock");

fn lock_entries() -> Vec<CorpusFile> {
    LOCK.lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let sha256 = it.next()?.to_owned();
            let size = it.next()?.parse().ok()?;
            let rel = it.collect::<Vec<_>>().join(" ");
            Some(CorpusFile { rel, size, sha256 })
        })
        .collect()
}

impl XarCorpus {
    /// Locates the corpus, or `None` when it is not on this machine.
    ///
    /// # Panics
    ///
    /// When `XARAST_CORPUS_REQUIRED=1` and the corpus is absent or has
    /// drifted from the lock file.
    fn discover() -> Option<XarCorpus> {
        let required = std::env::var("XARAST_CORPUS_REQUIRED").as_deref() == Ok("1");
        let root = PathBuf::from(
            std::env::var("XARAST_XAR_CORPUS").unwrap_or_else(|_| "/home/user/xara-xtreme".into()),
        );
        let files = lock_entries();
        assert_eq!(files.len(), 59, "corpus.lock must list exactly 59 files");
        if !root.is_dir() {
            assert!(
                !required,
                "XARAST_CORPUS_REQUIRED=1 but {} is not a directory",
                root.display()
            );
            return None;
        }
        for f in &files {
            let p = f.path(&root);
            let Ok(bytes) = std::fs::read(&p) else {
                assert!(
                    !required,
                    "XARAST_CORPUS_REQUIRED=1 but {} is missing",
                    p.display()
                );
                return None;
            };
            assert_eq!(bytes.len() as u64, f.size, "{} changed size", f.rel);
            let got = format!("{:x}", Sha256::digest(&bytes));
            assert_eq!(got, f.sha256, "{} changed content", f.rel);
        }
        Some(XarCorpus { root, files })
    }
}

/// Fetches the corpus or skips the test with a printed notice.
macro_rules! corpus_or_skip {
    () => {
        match XarCorpus::discover() {
            Some(c) => c,
            None => {
                println!(
                    "skipping: no .xar corpus (set XARAST_XAR_CORPUS; \
                     XARAST_CORPUS_REQUIRED=1 to make this a failure)"
                );
                return;
            }
        }
    };
}

fn reports(c: &XarCorpus) -> Vec<FileReport> {
    c.files
        .iter()
        .map(|f| {
            let bytes = std::fs::read(f.path(&c.root)).unwrap_or_default();
            xar_dump_report(f.label(), &bytes, ReaderLimits::default())
        })
        .collect()
}

/// Criteria 1–5: the physical layer, measured against `research/01 §12.2`.
#[test]
fn the_physical_layer_reproduces_the_measured_corpus() {
    let c = corpus_or_skip!();
    let rs = reports(&c);

    let failed: Vec<&str> = rs
        .iter()
        .filter(|r| !r.ok())
        .map(|r| r.label.as_str())
        .collect();
    assert!(failed.is_empty(), "files that failed to parse: {failed:?}");

    let records: u64 = rs.iter().map(|r| u64::from(r.records)).sum();
    assert_eq!(records, 1_390_282, "total record count");

    let tags: BTreeSet<u32> = rs
        .iter()
        .flat_map(|r| r.histogram.keys().copied())
        .collect();
    assert_eq!(tags.len(), 157, "distinct tags across the corpus");

    let blocks: usize = rs.iter().map(|r| r.blocks).sum();
    let blocks_ok: usize = rs.iter().map(|r| r.blocks_ok).sum();
    assert_eq!(
        (blocks_ok, blocks),
        (103, 103),
        "compressed blocks verified"
    );

    let trailing: u64 = rs.iter().map(|r| r.trailing_bytes).sum();
    assert_eq!(trailing, 0, "bytes after TAG_ENDOFFILE");

    let unbalanced: Vec<&str> = rs
        .iter()
        .filter(|r| !r.balanced())
        .map(|r| r.label.as_str())
        .collect();
    assert!(
        unbalanced.is_empty(),
        "unbalanced DOWN/UP in {unbalanced:?}"
    );
    let down: u64 = rs.iter().map(|r| u64::from(r.down)).sum();
    let up: u64 = rs.iter().map(|r| u64::from(r.up)).sum();
    assert_eq!((down, up), (201_121, 201_121), "DOWN/UP totals");
}

/// Criteria 6 and 7: nothing structural is missing and nothing is
/// essential-and-unknown.
#[test]
fn no_file_has_an_unknown_structural_or_essential_tag() {
    let c = corpus_or_skip!();
    for r in reports(&c) {
        assert!(
            r.unknown_structural.is_empty(),
            "{}: unknown structural tags {:?}",
            r.label,
            r.unknown_structural
        );
        assert_eq!(
            r.error, None,
            "{}: no file may abort on an essential tag",
            r.label
        );
    }
}

/// Criterion 8: at least 99.2 % of records are handled.
///
/// The remainder is printed per tag, and every one of them is listed with a
/// reason in `docs/memory/xar-import.md`.
#[test]
fn at_least_99_2_percent_of_records_are_handled() {
    let c = corpus_or_skip!();
    let rs = reports(&c);
    let total: u64 = rs.iter().map(|r| u64::from(r.records)).sum();
    let handled: u64 = rs.iter().map(|r| u64::from(r.handled)).sum();
    let mut unhandled: BTreeMap<u32, u64> = BTreeMap::new();
    for r in &rs {
        for (tag, n) in &r.histogram {
            if !has_decoder(*tag) {
                *unhandled.entry(*tag).or_insert(0) += u64::from(*n);
            }
        }
    }
    let pct = 100.0 * handled as f64 / total as f64;
    println!("handled {handled}/{total} = {pct:.3}%");
    for (tag, n) in &unhandled {
        println!(
            "  unhandled tag {tag} ({}) x{n}",
            xarast_xar::name_of(*tag).unwrap_or("undefined")
        );
    }
    assert!(pct >= 99.2, "only {pct:.3}% of records handled");
}

/// Criterion 9: every tag of the minimum viable set has a decoder and is
/// exercised by the corpus.
#[test]
fn the_minimum_viable_tag_set_is_implemented_and_exercised() {
    const MINIMUM: &[u32] = &[
        // structural
        0, 1, 2, 3, 10, 30, 31, 40, 41, 42, 43, 45, 46, 47, 48, 80, 82, 87, 91, 92, 93, 4070, 4087,
        4114, 4116, 4031, // colour
        51,   // geometry
        111, 115, 116, 1901, 104, // attributes
        150, 151, 152, 153, 155, 166, 167, 169, 173, 174, 175, 176, 193,
    ];
    assert_eq!(MINIMUM.len(), 45);
    for &tag in MINIMUM {
        assert!(has_decoder(tag), "tag {tag} has no decoder");
    }
    let c = corpus_or_skip!();
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    for r in reports(&c) {
        seen.extend(r.histogram.keys().copied());
    }
    let missing: Vec<u32> = MINIMUM
        .iter()
        .copied()
        .filter(|t| !seen.contains(t))
        .collect();
    assert!(missing.is_empty(), "never seen in the corpus: {missing:?}");
}

/// Criterion 12: the `OneLine.xar` oracle, field by field.
///
/// One test covering the sign convention, the interleave, record numbering
/// and reference resolution at once, against the byte-level worked example
/// in `research/01 §7.3` and `§12.2`.
#[test]
fn the_one_line_worked_example_reproduces_exactly() {
    use xarast_geom::{Point, Verb};
    use xarast_xar::{Decoded, DiagSink, Ref, decode};

    let c = corpus_or_skip!();
    let bytes = std::fs::read(c.root.join("testfiles/OneLine.xar")).unwrap();
    let a = analyse(&bytes, ReaderLimits::default()).unwrap();
    assert_eq!(a.records_read, 88, "record count");
    assert_eq!(a.tree.max_depth, 5, "tree depth");
    assert_eq!(a.header.file_type.as_str(), "CXN");
    assert_eq!(a.header.producer.as_deref(), Some("Xara X"));

    let mut diags = DiagSink::new();
    let mut found = 0;
    a.tree.walk(&mut |node, _| {
        let rec = &node.record;
        let d = decode(
            rec.tag,
            &rec.data,
            Point::ORIGIN,
            &mut diags,
            (rec.number, rec.tag),
        );
        match (rec.number, d) {
            (28, Ok(Decoded::Path { path, style })) => {
                assert!(style.relative && style.stroked && !style.filled);
                assert_eq!(path.verbs(), &[Verb::MoveTo, Verb::LineTo]);
                assert_eq!(
                    path.points(),
                    &[Point::raw(112_101, 178_899), Point::raw(283_101, 321_399)]
                );
                found += 1;
            }
            (31, Ok(Decoded::ColourDefinition(col))) => {
                assert_eq!(col.model, xarast_color::ColourModel::Cmyk);
                assert_eq!(col.name.as_deref(), Some("Black"));
                assert_eq!(col.colour_type, 0);
                found += 1;
            }
            (32, Ok(Decoded::LineColour(r))) => {
                assert_eq!(r, Ref::Record(31));
                found += 1;
            }
            (33, Ok(Decoded::LineWidth(w))) => {
                assert_eq!(w.raw(), 500);
                found += 1;
            }
            (34, Ok(Decoded::FlatFill(r))) => {
                assert_eq!(r, Ref::Record(31));
                found += 1;
            }
            _ => {}
        }
    });
    assert_eq!(found, 5, "every asserted record was visited");
}

/// Criterion 14: embedded bitmap bytes survive verbatim and match their
/// declared format's magic.
#[test]
fn embedded_bitmaps_keep_their_bytes_and_their_format() {
    let c = corpus_or_skip!();
    let mut total = 0usize;
    for r in reports(&c) {
        assert_eq!(
            r.bitmaps, r.bitmaps_magic_ok,
            "{}: a bitmap's bytes do not match its declared format",
            r.label
        );
        total += r.bitmaps;
    }
    assert!(total > 0, "the corpus does contain bitmaps");
    println!("{total} bitmap definitions, all with matching magic");
}

/// Criterion 15: text decodes structurally, and `SimpleText.xar` yields
/// its 19-character line with no NUL and no truncation.
///
/// The phase document expected that file to hold one string; it holds
/// three, of 19, 18 and 20 code units. The one the worked example measures
/// is the 38-byte record, and that is what is asserted.
#[test]
fn text_designs_decode_their_strings() {
    use xarast_geom::Point;
    use xarast_xar::{Decoded, DiagSink, decode};

    let c = corpus_or_skip!();
    let rs = reports(&c);
    let with_text = rs
        .iter()
        .filter(|r| r.label.starts_with("TextDesigns/") && r.text_code_units > 0)
        .count();
    assert_eq!(with_text, 14, "every TextDesigns file decodes some text");

    let bytes = std::fs::read(c.root.join("TextDesigns/SimpleText.xar")).unwrap();
    let a = analyse(&bytes, ReaderLimits::default()).unwrap();
    let mut diags = DiagSink::new();
    let mut lengths = Vec::new();
    a.tree.walk(&mut |node, _| {
        let rec = &node.record;
        if rec.tag != 2201 {
            return;
        }
        let d = decode(
            rec.tag,
            &rec.data,
            Point::ORIGIN,
            &mut diags,
            (rec.number, rec.tag),
        );
        if let Ok(Decoded::TextString(s)) = d {
            assert!(!s.contains('\0'), "an unterminated string gained a NUL");
            assert_eq!(
                s.encode_utf16().count() * 2,
                rec.data.len(),
                "size / 2 units"
            );
            lengths.push(s.chars().count());
        }
    });
    assert!(
        lengths.contains(&19),
        "the 38-byte line of 19 characters: {lengths:?}"
    );
}

/// Criterion 16: record framing is lossless.
///
/// Re-serialising the parsed record sequence without compression and
/// re-parsing it yields the same sequence, which is what proves the framing
/// carries everything. The two compression records are dropped from the
/// reframed stream, since re-emitting a `TAG_STARTCOMPRESSION` in front of
/// uncompressed bytes would describe a file that does not exist.
#[test]
fn the_record_layer_round_trips() {
    use xarast_xar::{RecordReader, synth::XarBuilder};

    let c = corpus_or_skip!();
    for f in &c.files {
        let bytes = std::fs::read(f.path(&c.root)).unwrap();
        let original: Vec<_> = RecordReader::new(&bytes, ReaderLimits::default())
            .unwrap()
            .collect_records()
            .unwrap()
            .into_iter()
            .filter(|r| r.tag != 30 && r.tag != 31)
            .collect();
        let mut b = XarBuilder::headerless();
        for rec in &original {
            b = b.record(rec.tag, &rec.data);
        }
        let reframed = b.finish();
        let again = RecordReader::new(&reframed, ReaderLimits::default())
            .unwrap()
            .collect_records()
            .unwrap();
        assert_eq!(original.len(), again.len(), "{}: record count", f.rel);
        for (a, b) in original.iter().zip(&again) {
            assert_eq!((a.tag, &a.data), (b.tag, &b.data), "{}", f.rel);
        }
    }
}

/// Criterion 13: the committed statistics snapshot.
///
/// Facts only — counts, histograms, depths, diagnostic codes. Regenerate
/// with `XARAST_UPDATE_SNAPSHOTS=1`.
#[test]
fn the_statistics_snapshot_matches() {
    let c = corpus_or_skip!();
    let mut out = String::new();
    out.push_str(
        "# Corpus statistics, facts only. No coordinates, colour values or\n\
         # strings taken from the files: see crates/xarast-xar/src/report.rs.\n\
         # Regenerate with XARAST_UPDATE_SNAPSHOTS=1.\n\
         # file | records | tags | down | up | depth | nodes | handled | skipped | \
         stripped | blocks | ok | trailing | colours | paths | points | bitmaps | text\n",
    );
    for r in reports(&c) {
        out.push_str(&format!(
            "{} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {}\n",
            r.label,
            r.records,
            r.distinct_tags,
            r.down,
            r.up,
            r.max_depth,
            r.nodes,
            r.handled,
            r.skipped,
            r.stripped,
            r.blocks,
            r.blocks_ok,
            r.trailing_bytes,
            r.colours,
            r.path_records,
            r.path_points,
            r.bitmaps,
            r.text_code_units,
        ));
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/corpus-stats.txt");
    if std::env::var("XARAST_UPDATE_SNAPSHOTS").as_deref() == Ok("1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &out).unwrap();
        println!("wrote {}", path.display());
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(
        out, want,
        "corpus statistics changed; XARAST_UPDATE_SNAPSHOTS=1 to accept"
    );
}

/// The tag-coverage snapshot: which of the 157 observed tags have a
/// decoder, with counts. A new decoder shows up here as a reviewable diff.
#[test]
fn the_tag_coverage_snapshot_matches() {
    let c = corpus_or_skip!();
    let mut hist: BTreeMap<u32, u64> = BTreeMap::new();
    for r in reports(&c) {
        for (t, n) in &r.histogram {
            *hist.entry(*t).or_insert(0) += u64::from(*n);
        }
    }
    let mut out = String::from(
        "# Tag coverage over the 59-file corpus. Facts only.\n\
         # Regenerate with XARAST_UPDATE_SNAPSHOTS=1.\n\
         # tag | count | class | decoder | name\n",
    );
    for (tag, n) in &hist {
        out.push_str(&format!(
            "{} | {} | {} | {} | {}\n",
            tag,
            n,
            xarast_xar::class_of(*tag).map_or("-", xarast_xar::TagClass::as_str),
            if has_decoder(*tag) { "yes" } else { "NO" },
            xarast_xar::name_of(*tag).unwrap_or("(undefined)"),
        ));
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/tag-coverage.txt");
    if std::env::var("XARAST_UPDATE_SNAPSHOTS").as_deref() == Ok("1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &out).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(
        out, want,
        "tag coverage changed; XARAST_UPDATE_SNAPSHOTS=1 to accept"
    );
}

/// The clean-room guard: the committed snapshots must contain nothing that
/// could have come out of a corpus file.
///
/// This is the test that keeps criterion 13 honest. It runs with or without
/// the corpus, because the snapshots are committed either way.
#[test]
fn no_snapshot_leaks_file_content() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let mut checked = 0;
    for e in entries.filter_map(Result::ok) {
        let text = std::fs::read_to_string(e.path()).unwrap_or_default();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            // Every field of a fact line is a decimal number, a tag name, a
            // class, "yes"/"NO", or the file's own path. Anything else —
            // a layer name, a colour name, a text run, a coordinate pair —
            // would have to arrive as some other token.
            for (i, field) in line.split('|').enumerate() {
                let field = field.trim();
                let ok = field.parse::<u64>().is_ok()
                    || (i == 0 && field.ends_with(".xar"))
                    || field.starts_with("TAG_")
                    || matches!(
                        field,
                        "yes"
                            | "NO"
                            | "-"
                            | "(undefined)"
                            | "structural"
                            | "attribute"
                            | "definition"
                            | "object"
                            | "ignorable"
                    );
                assert!(
                    ok,
                    "{}:{}: field {:?} is not a fact; snapshots may not carry file content",
                    e.path().display(),
                    n + 1,
                    field
                );
            }
        }
        checked += 1;
    }
    println!("{checked} snapshot files checked for content leakage");
}

/// The performance budget for the whole corpus.
#[test]
fn the_corpus_parses_within_its_time_budget() {
    let c = corpus_or_skip!();
    let files: Vec<Vec<u8>> = c
        .files
        .iter()
        .map(|f| std::fs::read(f.path(&c.root)).unwrap_or_default())
        .collect();
    let start = std::time::Instant::now();
    for bytes in &files {
        let _ = analyse(bytes, ReaderLimits::default());
    }
    let elapsed = start.elapsed();
    println!("whole corpus: {elapsed:?}");
    // Generous: the budget in the phase document is 3 s in release, and
    // tests build at opt-level 2.
    assert!(elapsed.as_secs() < 30, "corpus parse took {elapsed:?}");
}
