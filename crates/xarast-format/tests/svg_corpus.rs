//! The SVG profile writer over the whole `.xar` corpus.
//!
//! Every one of the 59 files is imported, written as `document.svg` and
//! as a package; the SVG must be well-formed with unique ids and nothing
//! active or external, and the package must reopen with its digests
//! intact. The corpus is located through `XARAST_XAR_CORPUS`; without it
//! the test skips with a notice (`XARAST_CORPUS_REQUIRED=1` makes that a
//! failure), exactly as `crates/xarast-xar/tests/corpus.rs` does. Nothing
//! from the corpus is ever written into the repository.

use std::collections::HashSet;
use std::io::Cursor;
use std::path::PathBuf;

use quick_xml::events::Event;
use xarast_format::svg::{SvgOptions, write_svg};
use xarast_format::{ResourceIndex, SaveOptions, WriteOptions, XarastReader, save_to};

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

/// Well-formed, unique ids, nothing active, nothing external.
fn check(name: &str, svg: &str) -> usize {
    let mut r = quick_xml::Reader::from_str(svg);
    r.config_mut().check_end_names = true;
    let mut ids = HashSet::new();
    let mut elements = 0usize;
    loop {
        match r.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) => {
                elements += 1;
                let tag = e.name();
                let tag = tag.as_ref();
                assert!(
                    !matches!(tag, b"script" | b"foreignObject" | b"animate" | b"set"),
                    "{name}: forbidden element"
                );
                for a in e.attributes() {
                    let a = a.unwrap_or_else(|err| panic!("{name}: bad attribute: {err}"));
                    let key = a.key.as_ref();
                    assert!(!key.starts_with(b"on"), "{name}: event attribute");
                    if key == b"id" || key == b"xarast:id" {
                        let v = String::from_utf8_lossy(&a.value).into_owned();
                        assert!(ids.insert(v.clone()), "{name}: duplicate id {v}");
                    }
                    if key == b"href" || key == b"xlink:href" {
                        let v = String::from_utf8_lossy(&a.value);
                        assert!(
                            v.starts_with('#') || v.starts_with("resources/"),
                            "{name}: external reference {v}"
                        );
                    }
                }
            }
            Ok(Event::DocType(_)) => panic!("{name}: DOCTYPE"),
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => panic!("{name}: not well-formed at {}: {e}", r.buffer_position()),
        }
    }
    elements
}

#[test]
fn every_corpus_file_writes_a_valid_svg_and_a_package_that_reopens() {
    let Some((root, files)) = corpus() else {
        return;
    };
    let mut total_elements = 0usize;
    for rel in &files {
        let bytes = std::fs::read(root.join(rel)).unwrap();
        let (doc, _) = xarast_xar::import(&bytes, &xarast_xar::ImportOptions::default())
            .unwrap_or_else(|e| panic!("{rel}: import: {e}"));
        let mut res = ResourceIndex::new();
        let out = write_svg(&doc, &mut res, &SvgOptions::default());
        total_elements += check(rel, &out.svg);
        assert_eq!(out.stats.foreign_dropped, 0, "{rel}");

        let mut buf = Cursor::new(Vec::new());
        let opts = SaveOptions {
            write: WriteOptions::deterministic(),
            ..SaveOptions::default()
        };
        save_to(&doc, &mut buf, &opts).unwrap_or_else(|e| panic!("{rel}: save: {e}"));
        let mut r = XarastReader::open(Cursor::new(buf.into_inner()))
            .unwrap_or_else(|e| panic!("{rel}: reopen: {e}"));
        assert!(r.diagnostics().is_empty(), "{rel}: {:?}", r.diagnostics());
        assert!(r.verify_all().is_empty(), "{rel}");
        assert_eq!(r.document_bytes().unwrap(), out.svg.as_bytes(), "{rel}");
    }
    assert!(total_elements > 500_000, "{total_elements}");
}
