//! The application's save and `xarast-cli convert` write the same package
//! (XARA-T-0259): both hand the `.xarast` writer the SVG text placer, so a
//! file saved from File › Save shows placed text in a browser exactly as a
//! converted one does — on a path too (per-character `rotate`, T9.5.6).
//!
//! Runs over the 59-file corpus through `XARAST_XAR_CORPUS` and skips with
//! a notice when it is not there. Nothing from the corpus is written into
//! the repository.

use std::path::{Path, PathBuf};

use xarast_app::save::SaveKind;
use xarast_app::{DocumentId, Session};

const LOCK: &str = include_str!("../../../tests/corpus/corpus.lock");

fn corpus() -> Option<(PathBuf, Vec<String>)> {
    let root = PathBuf::from(
        std::env::var("XARAST_XAR_CORPUS").unwrap_or_else(|_| "/home/user/xara-xtreme".into()),
    );
    if !root.is_dir() {
        assert!(
            std::env::var("XARAST_CORPUS_REQUIRED").as_deref() != Ok("1"),
            "XARAST_CORPUS_REQUIRED=1 but no corpus"
        );
        println!("skipping: no .xar corpus (set XARAST_XAR_CORPUS)");
        return None;
    }
    let files = LOCK
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            it.next()?;
            it.next()?;
            Some(it.collect::<Vec<_>>().join(" "))
        })
        .collect();
    Some((root, files))
}

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("xarast-cli-app-save-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn document_svg(package: &Path) -> String {
    let mut r = xarast_format::XarastReader::open(std::fs::File::open(package).unwrap()).unwrap();
    String::from_utf8(r.document_bytes().unwrap()).unwrap()
}

/// An app save (the snapshot job File › Save, Save As and autosave run)
/// writes the bytes `xarast-cli convert --deterministic` writes, for every
/// corpus file. The one difference File › Save adds is `thumbnail.png`
/// (convert writes none): with it, `document.svg` is still the same.
#[test]
fn an_app_save_writes_the_bytes_convert_writes_over_the_corpus() {
    let Some((root, files)) = corpus() else {
        return;
    };
    // Pinned fonts: placed text never depends on the machine's.
    let _ = xarast_app::fonts::set_shared(xarast_app::fonts::FontService::from_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../xarast-text/tests/fonts"),
    ));
    let dir = scratch("corpus");
    let args = xarast_cli::convert::ConvertArgs {
        inputs: Vec::new(),
        output: None,
        out_dir: None,
        pretty: false,
        deterministic: true,
        quiet: true,
    };
    let mut rotated = 0;
    for (i, rel) in files.iter().enumerate() {
        let input = root.join(rel);
        let converted = dir.join(format!("{i}-convert.xarast"));
        xarast_cli::convert::convert_one(&input, &converted, &args)
            .unwrap_or_else(|e| panic!("{rel}: convert: {e}"));

        let session = Session::open(DocumentId(1), &input).unwrap_or_else(|e| panic!("{rel}: {e}"));
        let saved = dir.join(format!("{i}-app.xarast"));
        session
            .save_job(SaveKind::Document, &saved)
            .unwrap()
            .deterministic()
            .without_thumbnail()
            .run()
            .result
            .unwrap_or_else(|e| panic!("{rel}: app save: {e}"));
        assert!(
            std::fs::read(&saved).unwrap() == std::fs::read(&converted).unwrap(),
            "{rel}: the app save differs from xarast-cli convert"
        );
        let svg = document_svg(&saved);
        if svg.contains(" rotate=\"") {
            rotated += 1;
        }
        if rel.ends_with("TextCurve.xar") {
            assert!(
                svg.contains(" rotate=\""),
                "{rel}: text on a path is not placed per character"
            );
            // As File › Save writes it, thumbnail and all: only the
            // thumbnail is added.
            let full = dir.join(format!("{i}-thumb.xarast"));
            session
                .save_job(SaveKind::Document, &full)
                .unwrap()
                .deterministic()
                .run()
                .result
                .unwrap_or_else(|e| panic!("{rel}: app save: {e}"));
            let mut r =
                xarast_format::XarastReader::open(std::fs::File::open(&full).unwrap()).unwrap();
            assert!(r.thumbnail().unwrap().is_some(), "{rel}: no thumbnail");
            assert_eq!(document_svg(&full), svg, "{rel}");
            // Autosave: fast compression, no thumbnail, the same SVG.
            let auto = dir.join(format!("{i}-auto.xarast"));
            session
                .save_job(SaveKind::Autosave, &auto)
                .unwrap()
                .deterministic()
                .run()
                .result
                .unwrap_or_else(|e| panic!("{rel}: autosave: {e}"));
            assert_eq!(document_svg(&auto), svg, "{rel}: autosave");
        }
        for p in [&converted, &saved] {
            let _ = std::fs::remove_file(p);
        }
    }
    println!(
        "{rotated} of {} saves turn characters along a path",
        files.len()
    );
    assert!(rotated >= 1);
    let _ = std::fs::remove_dir_all(&dir);
}
