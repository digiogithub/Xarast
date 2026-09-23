//! The render half of the corpus round trip (XARA-US-0028): every `.xar`
//! file is imported and rendered, saved as `.xarast`, opened again through
//! the application's own open path (`Session::open_bytes`, the one
//! `File › Open` and the command line use) and rendered again, with the
//! CPU backend at the same size and frame. The two renders are compared
//! pixel by pixel.
//!
//! The tolerance is stated, not hidden: a channel may differ by at most
//! [`MAX_CHANNEL`] levels, and at most [`MAX_DIFFERING`] of a file's pixels
//! (0.1 %) may differ at all. Both come from what the format quantises:
//! colours are written as 8-bit sRGB (a CMYK or HSV palette colour
//! resolved in `f32` and the same colour read back from its 8-bit spelling
//! can blend one level apart at an anti-aliased edge), and ramp key stops
//! are 8-bit. Geometry is exact (integer millipoints both ways). Files
//! with a known writer gap are listed in [`KNOWN_RENDER_GAPS`].
//!
//! The corpus is found through `XARAST_XAR_CORPUS`; nothing from it is
//! written into the repository.

use std::path::{Path, PathBuf};

use xarast_app::viewport::drawing_or_page_rect;
use xarast_app::{
    DeviceSize, DocRect, DocumentId, HeadlessFrame, HeadlessOptions, Session, headless,
};

const LOCK: &str = include_str!("../../../tests/corpus/corpus.lock");

/// Largest per-channel difference allowed, in 8-bit levels.
const MAX_CHANNEL: u8 = 2;
/// Largest fraction of pixels allowed to differ at all.
const MAX_DIFFERING: f64 = 0.001;

/// Files that render differently for a known writer gap, not a reader
/// fault: scope3 simple loses objects kept under an unknown record
/// (XARA-T-0108) and a fractal fill's mapping, Watch4 a noise fill's
/// mapping, Spitfire and WATCH their bitmap fills' mapping (XARA-T-0109).
/// Since bitmaps decode (XARA-T-0129) four more show writer gaps that
/// undecoded bitmaps used to hide: leafgirl and Groucho2 lose their
/// JPEG8BPP palette (XARA-T-0154), Fill Types simple a duotone bitmap
/// fill and a mirrored mapping (XARA-T-0155, XARA-T-0109), and JagSS100
/// simple the image of its bitmap-transparency shadow (XARA-T-0111). They
/// must still render, and the test demands the list shrink when the writer
/// is fixed.
const KNOWN_RENDER_GAPS: &[&str] = &[
    "Designs/Fill Types simple.xar",
    "Designs/Groucho2.xar",
    "Designs/JagSS100 simple.xar",
    "Designs/Spitfire.xar",
    "Designs/WATCH.xar",
    "Designs/Watch4.xar",
    "Designs/leafgirl.xar",
    "Designs/scope3 simple.xar",
];

/// Files whose text the writer still writes approximately — one `<tspan>`
/// per line, no tracking or kerns — while the application now draws text
/// from the real layout (phase 9). They must still render; XARA-T-0172
/// makes the writer use the layout and empties this list.
const KNOWN_TEXT_GAPS: &[&str] = &[
    "testfiles/ProbeX16.xar",
    "testfiles/ScaleTest.xar",
    "testfiles/ScaleTest2.xar",
    "Designs/GardenPlan.xar",
    "Designs/TextCurve.xar",
    "TextDesigns/AngledText.xar",
    "TextDesigns/BaselineShift.xar",
    "TextDesigns/FontChangesInText.xar",
    "TextDesigns/Kerning.xar",
    "TextDesigns/LineSpacing.xar",
    "TextDesigns/ManualKern.xar",
    "TextDesigns/Paragraph.xar",
    "TextDesigns/Rotated.xar",
    "TextDesigns/SimpleText.xar",
    "TextDesigns/SuperSub.xar",
    "TextDesigns/TextJust.xar",
    "TextDesigns/Tracking.xar",
    "TextDesigns/embeddedFonts.xar",
    "TextDesigns/hebrew.xar",
];

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
    let mut exact = 0usize;
    let mut worst: Vec<(String, u8, f64)> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    let mut gaps: Vec<String> = Vec::new();
    let mut text_gaps: Vec<String> = Vec::new();
    // Pinned fonts: a render never depends on the machine's.
    let _ = xarast_app::fonts::set_shared(xarast_app::fonts::FontService::from_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../xarast-text/tests/fonts"),
    ));
    for rel in &files {
        let bytes = std::fs::read(root.join(rel)).unwrap();
        let original = Session::open_bytes(DocumentId(1), Path::new(rel), &bytes)
            .unwrap_or_else(|e| panic!("{rel}: {e}"));
        let mut package = std::io::Cursor::new(Vec::new());
        let opts = xarast_format::SaveOptions {
            write: xarast_format::WriteOptions::deterministic(),
            ..xarast_format::SaveOptions::default()
        };
        xarast_format::save_to(&original.doc, &mut package, &opts)
            .unwrap_or_else(|e| panic!("{rel}: save: {e}"));
        let reopened = Session::open_bytes(
            DocumentId(2),
            Path::new("round-trip.xarast"),
            &package.into_inner(),
        )
        .unwrap_or_else(|e| panic!("{rel}: open .xarast: {e}"));

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
        let fraction = differing as f64 / (a.len() / 4) as f64;
        if differing == 0 {
            exact += 1;
        } else {
            worst.push((rel.clone(), max, fraction));
        }
        if max > MAX_CHANNEL || fraction > MAX_DIFFERING {
            if KNOWN_RENDER_GAPS.contains(&rel.as_str()) {
                gaps.push(rel.clone());
                continue;
            }
            if KNOWN_TEXT_GAPS.contains(&rel.as_str()) {
                text_gaps.push(rel.clone());
                continue;
            }
            failures.push(format!(
                "{rel}: {differing} pixels differ ({:.4} %), by up to {max} levels",
                fraction * 100.0
            ));
        }
    }
    eprintln!("{exact}/59 pixel-identical; the others: {worst:?}");
    assert!(failures.is_empty(), "{failures:#?}");
    assert_eq!(gaps, KNOWN_RENDER_GAPS, "update KNOWN_RENDER_GAPS");
    assert_eq!(text_gaps, KNOWN_TEXT_GAPS, "update KNOWN_TEXT_GAPS");
}
