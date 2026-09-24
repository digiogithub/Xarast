//! The render half of the corpus round trip (XARA-US-0028): every `.xar`
//! file is imported and rendered, saved as `.xarast`, opened again through
//! the application's own open path (`Session::open_bytes`, the one
//! `File › Open` and the command line use) and rendered again, with the
//! CPU backend at the same size and frame. The two renders must be
//! **pixel-identical** for every file: geometry is exact (integer
//! millipoints both ways), palette colours come back bit for bit (their
//! components are written in the shortest round-trip form) and ramp,
//! three/four-colour, fractal, noise and contone key colours name their
//! palette colour (`xarast:stop-refs`, `xarast:colour-refs`,
//! `xarast:contone-refs`, XARA-T-0110) and bitmaps keep their encoded
//! bytes and, for a `.xar` JPEG8BPP bitmap, the palette its colours are
//! snapped to (`xarast:palette`, XARA-T-0154). Text stories carry every
//! run's attributes, kerns and breaks (`xarast:exact`, XARA-T-0172), so
//! text is laid out and drawn identically after the reload; the renders
//! use the pinned test fonts. The save uses the SVG text placer, as
//! `xarast-cli convert` does (text placed for browsers, on a path too:
//! T9.5.6), and saving the reopened document must give the same bytes.
//!
//! The corpus is found through `XARAST_XAR_CORPUS`; nothing from it is
//! written into the repository.

use std::path::{Path, PathBuf};

use xarast_app::viewport::drawing_or_page_rect;
use xarast_app::{
    DeviceSize, DocRect, DocumentId, HeadlessFrame, HeadlessOptions, Session, headless,
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

/// Renders `s` framing `frame`: the same rectangle for both documents
/// (the drawing's bounds include line widths the profile does not write
/// for unstroked objects, so each document's own fit could differ).
fn render(s: &Session, frame: DocRect) -> Vec<u8> {
    let opts = HeadlessOptions {
        size: DeviceSize::new(480, 360),
        frame: HeadlessFrame::Fit(frame),
        ..HeadlessOptions::default()
    };
    headless::render(s, &opts)
        .expect("render")
        .surface
        .data()
        .to_vec()
}

#[test]
fn every_corpus_file_renders_the_same_after_a_xarast_round_trip() {
    let Some((root, files)) = corpus() else {
        return;
    };
    let mut failures: Vec<String> = Vec::new();
    // Pinned fonts: a render never depends on the machine's.
    let _ = xarast_app::fonts::set_shared(xarast_app::fonts::FontService::from_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../xarast-text/tests/fonts"),
    ));
    for rel in &files {
        let bytes = std::fs::read(root.join(rel)).unwrap();
        let original = Session::open_bytes(DocumentId(1), Path::new(rel), &bytes)
            .unwrap_or_else(|e| panic!("{rel}: {e}"));
        let mut package = std::io::Cursor::new(Vec::new());
        // With the text placer, as `xarast-cli convert` saves: the base SVG
        // places text (on a path too, T9.5.6), which a reader ignores.
        let opts = xarast_format::SaveOptions {
            write: xarast_format::WriteOptions::deterministic(),
            svg: xarast_format::svg::SvgOptions {
                text: Some(xarast_app::svg_text::placer()),
                ..xarast_format::svg::SvgOptions::default()
            },
            ..xarast_format::SaveOptions::default()
        };
        xarast_format::save_to(&original.doc, &mut package, &opts)
            .unwrap_or_else(|e| panic!("{rel}: save: {e}"));
        let package = package.into_inner();
        let reopened = Session::open_bytes(DocumentId(2), Path::new("round-trip.xarast"), &package)
            .unwrap_or_else(|e| panic!("{rel}: open .xarast: {e}"));
        // Saving what was read gives the same bytes.
        let mut again = std::io::Cursor::new(Vec::new());
        xarast_format::save_to(&reopened.doc, &mut again, &opts)
            .unwrap_or_else(|e| panic!("{rel}: save again: {e}"));
        if again.into_inner() != package {
            failures.push(format!("{rel}: a re-save differs"));
        }

        let frame = drawing_or_page_rect(&original.doc);
        let (a, b) = (render(&original, frame), render(&reopened, frame));
        assert_eq!(a.len(), b.len(), "{rel}");
        let mut max = 0u8;
        let mut differing = 0usize;
        for (pa, pb) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0) {
            let d = pa
                .iter()
                .zip(pb)
                .map(|(x, y)| x.abs_diff(*y))
                .max()
                .unwrap_or(0);
            if d > 0 {
                differing += 1;
                max = max.max(d);
            }
        }
        if differing > 0 {
            let fraction = differing as f64 / (a.len() / 4) as f64;
            failures.push(format!(
                "{rel}: {differing} pixels differ ({:.4} %), by up to {max} levels",
                fraction * 100.0
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
