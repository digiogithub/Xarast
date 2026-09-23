//! `.xar → .xarast → reload → .xarast` over the whole corpus
//! (XARA-US-0028, `research/06 §13.5 a)` and `b)`).
//!
//! For each of the 59 files:
//!
//! 1. **Model**: the reloaded document has the normal form of the imported
//!    one (`svg::normal_form`, XARA-T-0105) — with the writer's passes 4–5
//!    on (the default) and off (XARA-T-0107).
//! 2. **Bytes**: re-saving the reloaded document through `save_opened`
//!    with deterministic options is byte-identical to the original save
//!    (every entry, the container included): read then write is a fixed
//!    point from the first re-save.
//! 3. **No warnings**: nothing the writer produced reads back with a
//!    warning.
//!
//! The render comparison of the reloaded document against the import lives
//! in `crates/xarast-app/tests/xarast_roundtrip.rs` (the renderer is not a
//! dependency of this crate). Nothing from the corpus is written into the
//! repository.

use std::io::Cursor;
use std::path::PathBuf;

use xarast_doc::Severity;
use xarast_format::svg::{SvgOptions, normal_form};
use xarast_format::{
    OpenOptions, SaveOptions, WriteOptions, XarastReader, open_reader, save_opened_to, save_to,
};

const LOCK: &str = include_str!("../../../tests/corpus/corpus.lock");

fn corpus() -> Option<(PathBuf, Vec<String>)> {
    let required = std::env::var("XARAST_CORPUS_REQUIRED").as_deref() == Ok("1");
    let root = PathBuf::from(
        std::env::var("XARAST_XAR_CORPUS").unwrap_or_else(|_| "/home/user/xara-xtreme".into()),
    );
    let files: Vec<String> = LOCK
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            it.next()?;
            it.next()?;
            Some(it.collect::<Vec<_>>().join(" "))
        })
        .collect();
    assert_eq!(files.len(), 59);
    if !root.is_dir() {
        assert!(!required, "XARAST_CORPUS_REQUIRED=1 but no corpus");
        eprintln!("skipping: no .xar corpus (set XARAST_XAR_CORPUS)");
        return None;
    }
    Some((root, files))
}

fn opts(svg: SvgOptions) -> SaveOptions {
    SaveOptions {
        write: WriteOptions::deterministic(),
        svg,
        ..SaveOptions::default()
    }
}

fn first_difference(a: &str, b: &str) -> String {
    for (i, (x, y)) in a.lines().zip(b.lines()).enumerate() {
        if x != y {
            return format!("line {i}:\n  {x}\n  {y}");
        }
    }
    format!("{} vs {} lines", a.lines().count(), b.lines().count())
}

/// The first package entry whose bytes differ, and where.
fn first_entry_difference(a: &[u8], b: &[u8]) -> String {
    let mut ra = XarastReader::open(Cursor::new(a)).unwrap();
    let mut rb = XarastReader::open(Cursor::new(b)).unwrap();
    let names: Vec<String> = ra.entries().iter().map(|e| e.name.clone()).collect();
    let other: Vec<String> = rb.entries().iter().map(|e| e.name.clone()).collect();
    if names != other {
        return format!("entries {names:?} vs {other:?}");
    }
    // The manifest only repeats the other entries' digests: look at it last.
    let mut names = names;
    names.sort_by_key(|n| n == "META-INF/manifest.xml");
    for n in names {
        let (x, y) = (ra.entry(&n).unwrap(), rb.entry(&n).unwrap());
        if x != y {
            let (x, y) = (String::from_utf8_lossy(&x), String::from_utf8_lossy(&y));
            return format!("{n}: {}", first_difference(&x, &y));
        }
    }
    "the container differs".into()
}

#[test]
fn every_corpus_file_round_trips_on_the_normal_form_and_reaches_a_byte_fixed_point() {
    let Some((root, files)) = corpus() else {
        return;
    };
    let mut failures: Vec<String> = Vec::new();
    for rel in &files {
        let bytes = std::fs::read(root.join(rel)).unwrap();
        let (doc, _) = xarast_xar::import(&bytes, &xarast_xar::ImportOptions::default())
            .unwrap_or_else(|e| panic!("{rel}: import: {e}"));
        let nf = normal_form(&doc);

        let mut first = Cursor::new(Vec::new());
        save_to(&doc, &mut first, &opts(SvgOptions::default())).unwrap();
        let first = first.into_inner();
        let mut opened = open_reader(Cursor::new(first.clone()), &OpenOptions::default())
            .unwrap_or_else(|e| panic!("{rel}: open: {e}"));
        let warnings: Vec<_> = opened
            .diagnostics
            .iter()
            .filter(|d| d.severity >= Severity::Warning)
            .collect();
        assert!(warnings.is_empty(), "{rel}: {warnings:?}");
        assert!(opened.preservation.intact(), "{rel}");
        let got = normal_form(&opened.document);
        if nf != got {
            failures.push(format!(
                "{rel}: normal form differs at {}",
                first_difference(&nf, &got)
            ));
        }

        // Passes 4–5 off: the same model.
        let mut plain = Cursor::new(Vec::new());
        save_to(
            &doc,
            &mut plain,
            &opts(SvgOptions {
                hoist: false,
                classes: false,
                ..SvgOptions::default()
            }),
        )
        .unwrap();
        let reread = open_reader(Cursor::new(plain.into_inner()), &OpenOptions::default())
            .unwrap_or_else(|e| panic!("{rel}: open (passes off): {e}"));
        if normal_form(&reread.document) != got {
            failures.push(format!("{rel}: passes 4-5 change the model read back"));
        }

        let mut second = Cursor::new(Vec::new());
        save_opened_to(
            &opened.document,
            &mut opened.package,
            &mut second,
            &opts(SvgOptions::default()),
        )
        .unwrap();
        let second = second.into_inner();
        if second != first {
            failures.push(format!(
                "{rel}: the first re-save is not byte-identical: {}",
                first_entry_difference(&first, &second)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
