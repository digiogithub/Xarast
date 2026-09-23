//! Rendering fidelity of attribute defaults, pinned with pixel probes.
//!
//! The documents are synthetic `.xar` streams built with
//! [`xarast_xar::synth::XarBuilder`]; no corpus byte enters the repository.
//! They reproduce the two faults behind XARA-T-0037, found in
//! `Designs/SimpleSphere.xar`:
//!
//! * a filled-and-stroked frame with **no fill attribute**, drawn over
//!   everything else, came out opaque black because the importer took the
//!   file's `TAG_CURRENTATTRIBUTES` block (the editor's current fill) for the
//!   document defaults. The factory default fill is "no colour";
//! * every gradient wrapped into hard bars, because a plain "repeating"
//!   mapping was taken at face value. The original clamps a graduated fill
//!   under every mapping except the "extra" repeat.

use xarast_app::viewport::drawing_rect;
use xarast_app::{DeviceSize, DocumentId, HeadlessFrame, HeadlessOptions, Session, headless};
use xarast_xar::synth::XarBuilder;
use xarast_xar::{ImportOptions, import};

const TAG_FLATFILL_BLACK: u32 = 191;
const TAG_FILL_REPEATING: u32 = 163;
const TAG_FILL_REPEATING_EXTRA: u32 = 206;
const TAG_LINEARFILL: u32 = 153;
const TAG_LINECOLOUR: u32 = 151;
const TAG_LINEWIDTH: u32 = 152;
const TAG_PATH_RELATIVE_FILLED_STROKED: u32 = 116;
const TAG_CURRENTATTRIBUTES: u32 = 4119;

const RED: i32 = -4;
const BLUE: i32 = -6;
const BLACK: i32 = -2;

/// The square every object in these documents occupies, in millipoints.
const LO: i32 = 100_000;
const HI: i32 = 300_000;

fn interleave(x: i32, y: i32) -> [u8; 8] {
    let (x, y) = (x.to_be_bytes(), y.to_be_bytes());
    [x[0], y[0], x[1], y[1], x[2], y[2], x[3], y[3]]
}

/// The closed square as a relative path: point 0 absolute, then inverted
/// deltas, the last point closing the figure.
fn square() -> Vec<u8> {
    let side = HI - LO;
    let mut p = vec![0x06];
    p.extend_from_slice(&interleave(LO, LO));
    for (verb, dx, dy) in [
        (0x02, -side, 0),
        (0x02, 0, -side),
        (0x02, side, 0),
        (0x03, 0, side),
    ] {
        p.push(verb);
        p.extend_from_slice(&interleave(dx, dy));
    }
    p
}

/// A red-to-blue linear fill across the middle fifth of the square.
fn linear_fill() -> Vec<u8> {
    let span = HI - LO;
    let mut f = Vec::new();
    for v in [LO + span * 2 / 5, LO, LO + span * 3 / 5, LO, RED, BLUE] {
        f.extend_from_slice(&v.to_le_bytes());
    }
    f.extend_from_slice(&0f64.to_le_bytes());
    f.extend_from_slice(&0f64.to_le_bytes());
    f
}

/// A gradient-filled square under a frame with no fill attribute, in a file
/// whose current attributes are a black fill and a repeating mapping.
fn drawing(mapping: u32) -> Vec<u8> {
    let mut spread = Vec::new();
    for v in [600_000i32, 450_000, 0, 0] {
        spread.extend_from_slice(&v.to_le_bytes());
    }
    spread.push(2);
    let mut layer = vec![0x01 | 0x04 | 0x08];
    for u in "Layer 1".encode_utf16() {
        layer.extend_from_slice(&u.to_le_bytes());
    }
    layer.extend_from_slice(&0u16.to_le_bytes());

    XarBuilder::new()
        .record(40, &[])
        .down()
        .record(TAG_CURRENTATTRIBUTES, &[1])
        .down()
        .record(TAG_FLATFILL_BLACK, &[])
        .record(TAG_FILL_REPEATING, &[])
        .up()
        .record(41, &[])
        .down()
        .record(42, &[])
        .down()
        .record(45, &spread)
        .record(43, &[])
        .down()
        .record(48, &layer)
        // The gradient-filled square.
        .record(TAG_PATH_RELATIVE_FILLED_STROKED, &square())
        .down()
        .record(TAG_LINEARFILL, &linear_fill())
        .record(mapping, &[])
        .up()
        // The frame: a stroke and no fill of its own.
        .record(TAG_PATH_RELATIVE_FILLED_STROKED, &square())
        .down()
        .record(TAG_LINEWIDTH, &500i32.to_le_bytes())
        .record(TAG_LINECOLOUR, &BLACK.to_le_bytes())
        .up()
        .up()
        .up()
        .up()
        .end_of_file()
        .finish()
}

/// Renders the drawing and samples it at fractions of the square's width,
/// half-way up.
fn probe(bytes: &[u8], at: &[f64]) -> Vec<[u8; 4]> {
    let (doc, _) = import(bytes, &ImportOptions::default()).expect("imports");
    let r = drawing_rect(&doc).to_kurbo();
    let session = Session::adopt(DocumentId(1), doc, None);
    let out = headless::render(
        &session,
        &HeadlessOptions {
            size: DeviceSize::new(200, 200),
            frame: HeadlessFrame::FitDrawing,
            ..HeadlessOptions::default()
        },
    )
    .expect("renders");
    let m = out.view.transform.to_affine();
    let w = out.surface.width() as usize;
    at.iter()
        .map(|f| {
            let p = m * kurbo::Point::new(r.x0 + r.width() * f, r.y0 + r.height() * 0.5);
            let i = (p.y as usize * w + p.x as usize) * 4;
            let px = &out.surface.data()[i..i + 4];
            [px[0], px[1], px[2], px[3]]
        })
        .collect()
}

#[test]
fn an_unfilled_frame_does_not_take_the_current_fill() {
    let [left, mid, right] = probe(&drawing(TAG_FILL_REPEATING), &[0.1, 0.5, 0.9])[..] else {
        unreachable!()
    };
    // The frame's interior is "no colour": the gradient shows through it.
    assert!(mid[0] > 64 && mid[2] > 64, "the middle is covered: {mid:?}");
    // And a plain "repeating" mapping clamps a graduated fill.
    assert!(left[0] > 200 && left[2] < 40, "left of the ramp: {left:?}");
    assert!(
        right[2] > 200 && right[0] < 40,
        "right of the ramp: {right:?}"
    );
}

#[test]
fn only_the_extra_repeat_makes_a_gradient_tile() {
    let [right] = probe(&drawing(TAG_FILL_REPEATING_EXTRA), &[0.7])[..] else {
        unreachable!()
    };
    // 0.7 is 1.5 ramps past the start: wrapped, it is the mid colour.
    // Clamped, it would be pure blue.
    assert!(
        right[0] > 64 && right[2] > 64,
        "the ramp did not wrap: {right:?}"
    );
}

#[test]
fn the_drawing_rect_includes_half_of_each_stroke() {
    // The frame's 500 mp stroke reaches 250 mp outside the square; framing
    // it with a zero extent cut thick strokes off at the image border.
    let (doc, _) = import(&drawing(TAG_FILL_REPEATING), &ImportOptions::default()).unwrap();
    let r = drawing_rect(&doc);
    let side = HI - LO + 500;
    assert_eq!((r.width().raw(), r.height().raw()), (side, side), "{r:?}");
}
