//! Which way up a bitmap fill and a bitmap transparency are drawn, pinned
//! with pixel probes (XARA-T-0171).
//!
//! A bitmap fill's start point is the image's **bottom-left** corner, its
//! end point the bottom-right and its second end point the top-left
//! (`docs/memory/render.md`, "Bitmap fill orientation"). The CPU renderer
//! used to put the image's top row at the start point, so every bitmap
//! fill and bitmap transparency came out upside down while placed bitmaps
//! were right.
//!
//! The documents are synthetic `.xar` streams built with
//! [`xarast_xar::synth::XarBuilder`] around a hand-made 2 × 2 DIB; no
//! corpus byte enters the repository.

use xarast_app::viewport::drawing_rect;
use xarast_app::{DeviceSize, DocumentId, HeadlessFrame, HeadlessOptions, Session, headless};
use xarast_xar::synth::XarBuilder;
use xarast_xar::{ImportOptions, import};

const TAG_DEFINEBITMAP_BMP: u32 = 65;
const TAG_FLATFILL_BLACK: u32 = 191;
const TAG_BITMAPFILL: u32 = 157;
const TAG_BITMAPTRANSPARENTFILL: u32 = 171;
const TAG_FILL_NONREPEATING: u32 = 164;
const TAG_FILL_REPEATINGINVERTED: u32 = 165;
const TAG_TRANSPARENTFILL_NONREPEATING: u32 = 181;
const TAG_PATH_RELATIVE_FILLED: u32 = 114;

/// The record numbers of the two bitmap definitions: the file header is
/// record 1 and the definitions follow it directly.
const COLOUR_BITMAP: i32 = 2;
const LEVEL_BITMAP: i32 = 3;

/// The three squares, left to right, in millipoints: a bitmap fill, a
/// bitmap transparency and a mirrored bitmap fill.
const SIDE: i32 = 200_000;
const Y0: i32 = 100_000;
const X0: [i32; 3] = [100_000, 400_000, 700_000];

type Rgb = [u8; 3];
const RED: Rgb = [255, 0, 0];
const GREEN: Rgb = [0, 255, 0];
const BLUE: Rgb = [0, 0, 255];
const YELLOW: Rgb = [255, 255, 0];
const WHITE: Rgb = [255, 255, 255];
const BLACK: Rgb = [0, 0, 0];

/// A headerless 2 × 2, 24 bpp DIB, as tag 65 stores it. `top` and
/// `bottom` are left-to-right; a DIB stores the bottom row first.
fn dib(top: [Rgb; 2], bottom: [Rgb; 2]) -> Vec<u8> {
    let mut d = Vec::new();
    for v in [40u32, 2, 2] {
        d.extend_from_slice(&v.to_le_bytes());
    }
    d.extend_from_slice(&1u16.to_le_bytes());
    d.extend_from_slice(&24u16.to_le_bytes());
    for v in [0u32, 16, 2835, 2835, 0, 0] {
        d.extend_from_slice(&v.to_le_bytes());
    }
    for row in [bottom, top] {
        for [r, g, b] in row {
            d.extend_from_slice(&[b, g, r]);
        }
        d.extend_from_slice(&[0, 0]);
    }
    d
}

fn bitmap_definition(top: [Rgb; 2], bottom: [Rgb; 2]) -> Vec<u8> {
    let mut p = Vec::new();
    for u in "Default".encode_utf16() {
        p.extend_from_slice(&u.to_le_bytes());
    }
    p.extend_from_slice(&0u16.to_le_bytes());
    p.extend_from_slice(&dib(top, bottom));
    p
}

fn interleave(x: i32, y: i32) -> [u8; 8] {
    let (x, y) = (x.to_be_bytes(), y.to_be_bytes());
    [x[0], y[0], x[1], y[1], x[2], y[2], x[3], y[3]]
}

/// A closed square as a relative path: point 0 absolute, then inverted
/// deltas, the last point closing the figure.
fn square(x0: i32) -> Vec<u8> {
    let mut p = vec![0x06];
    p.extend_from_slice(&interleave(x0, Y0));
    for (verb, dx, dy) in [
        (0x02, -SIDE, 0),
        (0x02, 0, -SIDE),
        (0x02, SIDE, 0),
        (0x03, 0, SIDE),
    ] {
        p.push(verb);
        p.extend_from_slice(&interleave(dx, dy));
    }
    p
}

/// Start (bottom-left), end (bottom-right) and second end (top-left) of a
/// tile `size` across whose bottom-left corner is at `(x0, Y0)`.
fn tile_points(x0: i32, size: i32) -> Vec<u8> {
    let mut f = Vec::new();
    for v in [x0, Y0, x0 + size, Y0, x0, Y0 + size] {
        f.extend_from_slice(&v.to_le_bytes());
    }
    f
}

fn bitmap_fill(x0: i32, size: i32) -> Vec<u8> {
    let mut f = tile_points(x0, size);
    f.extend_from_slice(&COLOUR_BITMAP.to_le_bytes());
    f.extend_from_slice(&0f64.to_le_bytes());
    f.extend_from_slice(&0f64.to_le_bytes());
    f
}

fn bitmap_transparency(x0: i32) -> Vec<u8> {
    let mut f = tile_points(x0, SIDE);
    // Start and end levels, then the mix mode.
    f.extend_from_slice(&[0, 255, 1]);
    f.extend_from_slice(&LEVEL_BITMAP.to_le_bytes());
    f.extend_from_slice(&0f64.to_le_bytes());
    f.extend_from_slice(&0f64.to_le_bytes());
    f
}

fn drawing() -> Vec<u8> {
    let mut spread = Vec::new();
    for v in [1_000_000i32, 400_000, 0, 0] {
        spread.extend_from_slice(&v.to_le_bytes());
    }
    spread.push(2);
    let mut layer = vec![0x01 | 0x04 | 0x08];
    for u in "Layer 1".encode_utf16() {
        layer.extend_from_slice(&u.to_le_bytes());
    }
    layer.extend_from_slice(&0u16.to_le_bytes());

    XarBuilder::new()
        .record(
            TAG_DEFINEBITMAP_BMP,
            &bitmap_definition([RED, GREEN], [BLUE, YELLOW]),
        )
        .record(
            TAG_DEFINEBITMAP_BMP,
            &bitmap_definition([WHITE, WHITE], [BLACK, BLACK]),
        )
        .record(40, &[])
        .record(41, &[])
        .down()
        .record(42, &[])
        .down()
        .record(45, &spread)
        .record(43, &[])
        .down()
        .record(48, &layer)
        // One tile over the whole square.
        .record(TAG_PATH_RELATIVE_FILLED, &square(X0[0]))
        .down()
        .record(TAG_BITMAPFILL, &bitmap_fill(X0[0], SIDE))
        .record(TAG_FILL_NONREPEATING, &[])
        .up()
        // Black, seen through a white-over-black transparency bitmap.
        .record(TAG_PATH_RELATIVE_FILLED, &square(X0[1]))
        .down()
        .record(TAG_FLATFILL_BLACK, &[])
        .record(TAG_BITMAPTRANSPARENTFILL, &bitmap_transparency(X0[1]))
        .record(TAG_TRANSPARENTFILL_NONREPEATING, &[])
        .up()
        // A quarter-size tile, mirrored across the square.
        .record(TAG_PATH_RELATIVE_FILLED, &square(X0[2]))
        .down()
        .record(TAG_BITMAPFILL, &bitmap_fill(X0[2], SIDE / 2))
        .record(TAG_FILL_REPEATINGINVERTED, &[])
        .up()
        .up()
        .up()
        .up()
        .end_of_file()
        .finish()
}

/// Renders the drawing over opaque white and returns the colour at each
/// document point.
fn probe(at: &[(i32, i32)]) -> Vec<Rgb> {
    let (doc, diags) = import(&drawing(), &ImportOptions::default()).expect("imports");
    assert!(
        drawing_rect(&doc).width().raw() > 2 * SIDE,
        "all three squares imported: {diags:?}"
    );
    let session = Session::adopt(DocumentId(1), doc, None);
    let out = headless::render(
        &session,
        &HeadlessOptions {
            size: DeviceSize::new(800, 200),
            frame: HeadlessFrame::FitDrawing,
            ..HeadlessOptions::default()
        },
    )
    .expect("renders");
    assert_eq!(
        (out.walk.images_pending, out.walk.images_failed),
        (0, 0),
        "every bitmap decoded"
    );
    let m = out.view.transform.to_affine();
    let w = out.surface.width() as usize;
    at.iter()
        .map(|&(x, y)| {
            let p = m * kurbo::Point::new(f64::from(x), f64::from(y));
            let i = (p.y as usize * w + p.x as usize) * 4;
            let px = &out.surface.data()[i..i + 4];
            assert_eq!(px[3], 255, "the background is opaque");
            [px[0], px[1], px[2]]
        })
        .collect()
}

fn near(got: [u8; 3], want: Rgb) -> bool {
    got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 40)
}

/// A point `(fx, fy)` of the way across square `i`, `fy` measured up
/// from its bottom edge.
fn at(i: usize, fx: f64, fy: f64) -> (i32, i32) {
    let s = f64::from(SIDE);
    (
        X0[i] + (s * fx).round() as i32,
        Y0 + (s * fy).round() as i32,
    )
}

fn check(got: Vec<Rgb>, want: &[(Rgb, &str)]) {
    for (c, (want, name)) in got.into_iter().zip(want) {
        assert!(near(c, *want), "{name}: {c:?}, want {want:?}");
    }
}

#[test]
fn a_bitmap_fill_is_drawn_upright() {
    let got = probe(&[
        at(0, 0.25, 0.75),
        at(0, 0.75, 0.75),
        at(0, 0.25, 0.25),
        at(0, 0.75, 0.25),
    ]);
    check(
        got,
        &[
            (RED, "top left"),
            (GREEN, "top right"),
            (BLUE, "bottom left"),
            (YELLOW, "bottom right"),
        ],
    );
}

#[test]
fn a_bitmap_transparency_is_applied_upright() {
    // White in the transparency bitmap is fully transparent, so the black
    // square shows only where the bitmap's bottom row lies.
    let got = probe(&[at(1, 0.5, 0.75), at(1, 0.5, 0.25)]);
    check(
        got,
        &[(WHITE, "top, see-through"), (BLACK, "bottom, opaque")],
    );
}

#[test]
fn a_mirrored_bitmap_fill_flips_about_the_tile_edges() {
    // The tile is the bottom-left quarter; its texel centres are at an
    // eighth and three eighths of the square.
    let (lo, hi) = (0.125, 0.375);
    let got = probe(&[
        // The tile itself, upright.
        at(2, lo, hi),
        at(2, lo, lo),
        // The tile above is flipped vertically: its top row meets the
        // tile's top row at the seam.
        at(2, lo, 1.0 - hi),
        at(2, lo, 1.0 - lo),
        // The tile to the right is flipped horizontally.
        at(2, 1.0 - hi, hi),
        at(2, 1.0 - lo, hi),
    ]);
    check(
        got,
        &[
            (RED, "tile, top left"),
            (BLUE, "tile, bottom left"),
            (RED, "above, next to the seam"),
            (BLUE, "above, far from the seam"),
            (GREEN, "right, next to the seam"),
            (RED, "right, far from the seam"),
        ],
    );
}
