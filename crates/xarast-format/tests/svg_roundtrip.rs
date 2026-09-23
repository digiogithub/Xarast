//! `.xar → .xarast → reload → .xarast` over the whole corpus
//! (XARA-US-0028, `research/06 §13.5 a)` and `b)`).
//!
//! For each of the 59 files:
//!
//! 1. **Model**: the reloaded document has the normal form of the imported
//!    one (`svg::normal_form`, XARA-T-0105) — with the writer's passes 4–5
//!    on (the default) and off (XARA-T-0107).
//! 2. **Bytes**: re-saving the reloaded document through `save_opened`
//!    with deterministic options reaches a fixed point: the second re-save
//!    equals the first re-save for every file, and the first re-save equals
//!    the original save for all but the files whose baked data is resampled
//!    from 8-bit key stops (counted, and bounded here).
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
use xarast_format::{OpenOptions, SaveOptions, WriteOptions, open_reader, save_opened_to, save_to};

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

#[test]
fn every_corpus_file_round_trips_on_the_normal_form_and_reaches_a_byte_fixed_point() {
    let Some((root, files)) = corpus() else {
        return;
    };
    let mut identical_first = 0usize;
    let mut settled_second = Vec::new();
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
        assert!(
            nf == got,
            "{rel}: normal form differs at {}",
            first_difference(&nf, &got)
        );

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
        assert!(
            normal_form(&reread.document) == got,
            "{rel}: passes 4-5 change the model read back"
        );

        let mut second = Cursor::new(Vec::new());
        save_opened_to(
            &opened.document,
            &mut opened.package,
            &mut second,
            &opts(SvgOptions::default()),
        )
        .unwrap();
        let second = second.into_inner();
        if second == first {
            identical_first += 1;
            continue;
        }
        let mut again = open_reader(Cursor::new(second.clone()), &OpenOptions::default()).unwrap();
        let mut third = Cursor::new(Vec::new());
        save_opened_to(
            &again.document,
            &mut again.package,
            &mut third,
            &opts(SvgOptions::default()),
        )
        .unwrap();
        assert!(
            third.into_inner() == second,
            "{rel}: not a fixed point on the second re-save"
        );
        settled_second.push(rel.clone());
    }
    eprintln!(
        "{identical_first}/59 identical on the first re-save; settled on the second: {settled_second:?}"
    );
    // Known today: four files whose profiled or approximated fills are
    // re-sampled from 8-bit key stops (Fill Types simple, Spitfire, WATCH2,
    // Watch4), and one whose meta.xml counts an unreferenced bitmap (scope3
    // simple); see docs/memory/xarast-format.md. More is a regression.
    assert!(identical_first >= 54, "{identical_first}");
}
