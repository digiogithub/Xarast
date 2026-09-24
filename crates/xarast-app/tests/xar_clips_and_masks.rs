//! ClipViews and bitmap transparencies read from `.xar`, pinned with pixel
//! probes (XARA-T-0306, XARA-T-0307).
//!
//! * A `TAG_CLIPVIEWCONTROLLER` clips the objects after its `TAG_CLIPVIEW`
//!   marker to the objects before it (the keyholes), and draws the keyholes
//!   themselves as ordinary objects beneath the rest.
//! * A bitmap transparency keeps its two levels and its mode: black maps
//!   to the start level, white to the end, composited in the mode's family.
//!   It used to be drawn in Mix over the full 0..255 range.
//!
//! The corpus has no ClipView at all, so these synthetic files are the
//! reference. Every document is built here with
//! [`xarast_xar::synth::XarBuilder`]; no corpus byte enters the repository.

use xarast_app::{DeviceSize, DocumentId, HeadlessFrame, HeadlessOptions, Session, headless};
use xarast_doc::{ClipViewMode, Document, NodeKind};
use xarast_geom::Rect;
use xarast_xar::synth::XarBuilder;
use xarast_xar::{ImportOptions, import};

const TAG_ATOMICTAGS: u32 = 10;
const TAG_DEFINERGBCOLOUR: u32 = 50;
const TAG_DEFINEBITMAP_BMP: u32 = 65;
const TAG_PATH_RELATIVE_FILLED: u32 = 114;
const TAG_FLATFILL: u32 = 150;
const TAG_BITMAPTRANSPARENTFILL: u32 = 171;
const TAG_TRANSPARENTFILL_NONREPEATING: u32 = 181;
const TAG_FLATFILL_NONE: u32 = 190;
const TAG_LINECOLOUR_NONE: u32 = 193;
const TAG_CLIPVIEWCONTROLLER: u32 = 4084;
const TAG_CLIPVIEW: u32 = 4085;

type Rgb = [u8; 3];
const RED: Rgb = [255, 0, 0];
const GREEN: Rgb = [0, 255, 0];
const BLUE: Rgb = [0, 0, 255];
const YELLOW: Rgb = [255, 255, 0];
const WHITE: Rgb = [255, 255, 255];
const BLACK: Rgb = [0, 0, 0];

/// Record numbers: the header is record 1, the atomic tag list record 2,
/// and the definitions follow in this order.
const C_RED: i32 = 3;
const C_GREEN: i32 = 4;
const C_BLUE: i32 = 5;
const C_YELLOW: i32 = 6;
const C_BLACK: i32 = 7;
/// Every texel black.
const B_BLACK: i32 = 8;
/// White on top, black at the bottom.
const B_SPLIT: i32 = 9;

fn interleave(x: i32, y: i32) -> [u8; 8] {
    let (x, y) = (x.to_be_bytes(), y.to_be_bytes());
    [x[0], y[0], x[1], y[1], x[2], y[2], x[3], y[3]]
}

/// A closed `w` × `h` rectangle with its bottom-left corner at `(x, y)`,
/// as a relative path: point 0 absolute, then inverted deltas.
fn rect(x: i32, y: i32, w: i32, h: i32) -> Vec<u8> {
    let mut p = vec![0x06];
    p.extend_from_slice(&interleave(x + w, y + h));
    for (verb, dx, dy) in [(0x02, w, 0), (0x02, 0, h), (0x02, -w, 0), (0x03, 0, -h)] {
        p.push(verb);
        p.extend_from_slice(&interleave(dx, dy));
    }
    p
}

/// A headerless 2 × 2, 24 bpp DIB, as tag 65 stores it, bottom row first.
fn bitmap(top: Rgb, bottom: Rgb) -> Vec<u8> {
    let mut d = Vec::new();
    for u in "Default".encode_utf16() {
        d.extend_from_slice(&u.to_le_bytes());
    }
    d.extend_from_slice(&0u16.to_le_bytes());
    for v in [40u32, 2, 2] {
        d.extend_from_slice(&v.to_le_bytes());
    }
    d.extend_from_slice(&1u16.to_le_bytes());
    d.extend_from_slice(&24u16.to_le_bytes());
    for v in [0u32, 16, 2835, 2835, 0, 0] {
        d.extend_from_slice(&v.to_le_bytes());
    }
    for [r, g, b] in [bottom, top] {
        d.extend_from_slice(&[b, g, r, b, g, r]);
        // Rows are padded to four bytes.
        d.extend_from_slice(&[0, 0]);
    }
    d
}

/// A bitmap transparency whose tile covers the square at `(x, y)`.
fn bitmap_transparency(x: i32, y: i32, side: i32, levels: [u8; 2], mode: u8, bm: i32) -> Vec<u8> {
    let mut f = Vec::new();
    for v in [x, y, x + side, y, x, y + side] {
        f.extend_from_slice(&v.to_le_bytes());
    }
    f.extend_from_slice(&[levels[0], levels[1], mode]);
    f.extend_from_slice(&bm.to_le_bytes());
    f.extend_from_slice(&0f64.to_le_bytes());
    f.extend_from_slice(&0f64.to_le_bytes());
    f
}

/// A filled rectangle with no outline.
fn shape(b: XarBuilder, r: Vec<u8>, fill: Option<i32>) -> XarBuilder {
    let b = b.record(TAG_PATH_RELATIVE_FILLED, &r).down();
    let b = match fill {
        Some(c) => b.record(TAG_FLATFILL, &c.to_le_bytes()),
        None => b.record(TAG_FLATFILL_NONE, &[]),
    };
    b.record(TAG_LINECOLOUR_NONE, &[]).up()
}

/// The layer's contents, left to right.
///
/// 1. A ClipView: a green keyhole 200 000 mp square at (200 000, 100 000),
///    and a red square that covers its left half and runs off to the left.
/// 2. A ClipView with two unfilled keyholes 100 000 mp across, at
///    x = 500 000 and 700 000, over one red bar across both.
/// 3. A yellow square over a blue one, the yellow made Stained Glass by an
///    all-black bitmap transparency with levels 0..255.
/// 4. A black square seen through a white-over-black bitmap transparency
///    with levels 115..255, in Mix.
/// 5. As 1, with no `TAG_CLIPVIEW` marker.
fn drawing() -> Vec<u8> {
    let mut spread = Vec::new();
    for v in [1_800_000i32, 400_000, 0, 0] {
        spread.extend_from_slice(&v.to_le_bytes());
    }
    spread.push(2);
    let mut layer = vec![0x01 | 0x04 | 0x08];
    for u in "Layer 1".encode_utf16() {
        layer.extend_from_slice(&u.to_le_bytes());
    }
    layer.extend_from_slice(&0u16.to_le_bytes());
    let mut atomic = Vec::new();
    for t in [TAG_CLIPVIEWCONTROLLER, TAG_CLIPVIEW] {
        atomic.extend_from_slice(&t.to_le_bytes());
    }

    let mut b = XarBuilder::new()
        // Real files declare both ClipView tags atomic; now that they are
        // understood the declaration must not strip them.
        .record(TAG_ATOMICTAGS, &atomic)
        .record(TAG_DEFINERGBCOLOUR, &RED)
        .record(TAG_DEFINERGBCOLOUR, &GREEN)
        .record(TAG_DEFINERGBCOLOUR, &BLUE)
        .record(TAG_DEFINERGBCOLOUR, &YELLOW)
        .record(TAG_DEFINERGBCOLOUR, &BLACK)
        .record(TAG_DEFINEBITMAP_BMP, &bitmap(BLACK, BLACK))
        .record(TAG_DEFINEBITMAP_BMP, &bitmap(WHITE, BLACK))
        .record(40, &[])
        .record(41, &[])
        .down()
        .record(42, &[])
        .down()
        .record(45, &spread)
        .record(43, &[])
        .down()
        .record(48, &layer);

    // 1.
    b = b.record(TAG_CLIPVIEWCONTROLLER, &[]).down();
    b = shape(b, rect(200_000, 100_000, 200_000, 200_000), Some(C_GREEN));
    b = b.record(TAG_CLIPVIEW, &[]);
    b = shape(b, rect(50_000, 50_000, 250_000, 300_000), Some(C_RED));
    b = b.up();

    // 2.
    b = b.record(TAG_CLIPVIEWCONTROLLER, &[]).down();
    b = shape(b, rect(500_000, 100_000, 100_000, 200_000), None);
    b = shape(b, rect(700_000, 100_000, 100_000, 200_000), None);
    b = b.record(TAG_CLIPVIEW, &[]);
    b = shape(b, rect(450_000, 150_000, 400_000, 100_000), Some(C_RED));
    b = b.up();

    // 3.
    b = shape(b, rect(900_000, 100_000, 200_000, 200_000), Some(C_BLUE));
    b = b
        .record(
            TAG_PATH_RELATIVE_FILLED,
            &rect(900_000, 100_000, 200_000, 200_000),
        )
        .down()
        .record(TAG_FLATFILL, &C_YELLOW.to_le_bytes())
        .record(TAG_LINECOLOUR_NONE, &[])
        .record(
            TAG_BITMAPTRANSPARENTFILL,
            &bitmap_transparency(900_000, 100_000, 200_000, [0, 255], 2, B_BLACK),
        )
        .record(TAG_TRANSPARENTFILL_NONREPEATING, &[])
        .up();

    // 4.
    b = b
        .record(
            TAG_PATH_RELATIVE_FILLED,
            &rect(1_200_000, 100_000, 200_000, 200_000),
        )
        .down()
        .record(TAG_FLATFILL, &C_BLACK.to_le_bytes())
        .record(TAG_LINECOLOUR_NONE, &[])
        .record(
            TAG_BITMAPTRANSPARENTFILL,
            &bitmap_transparency(1_200_000, 100_000, 200_000, [115, 255], 1, B_SPLIT),
        )
        .record(TAG_TRANSPARENTFILL_NONREPEATING, &[])
        .up();

    // 5.
    b = b.record(TAG_CLIPVIEWCONTROLLER, &[]).down();
    b = shape(b, rect(1_550_000, 100_000, 200_000, 200_000), Some(C_GREEN));
    b = shape(b, rect(1_450_000, 50_000, 150_000, 300_000), Some(C_RED));
    b = b.up();

    b.up().up().up().end_of_file().finish()
}

fn document() -> Document {
    let (doc, report) = import(&drawing(), &ImportOptions::default()).expect("imports");
    assert_eq!(report.errors(), 0, "{:?}", report.diagnostics);
    assert_eq!(report.records_stripped, 0, "nothing stripped as atomic");
    assert_eq!(
        report.records_mapped + report.records_skipped + report.records_stripped,
        report.records_read
    );
    // The one controller without a marker says so.
    let degraded: Vec<u64> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == xarast_xar::DiagCode::ClipViewDegraded)
        .map(|d| d.detail)
        .collect();
    assert_eq!(degraded, [0]);
    doc
}

/// Renders the drawing, 1 px per 1 000 millipoints, over opaque white.
fn probe(at: &[(i32, i32)]) -> Vec<Rgb> {
    let frame = Rect::new(
        xarast_geom::Point::raw(0, 0),
        xarast_geom::Point::raw(1_800_000, 400_000),
    );
    let session = Session::adopt(DocumentId(1), document(), None);
    let out = headless::render(
        &session,
        &HeadlessOptions {
            size: DeviceSize::new(1_800, 400),
            frame: HeadlessFrame::Fit(frame),
            ..HeadlessOptions::default()
        },
    )
    .expect("renders");
    assert_eq!(out.walk.clips_unsupported, 0, "every clip has geometry");
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

fn check(got: Vec<Rgb>, want: &[(Rgb, &str)], tolerance: u8) {
    for (c, (want, name)) in got.into_iter().zip(want) {
        assert!(
            c.iter().zip(want).all(|(g, w)| g.abs_diff(*w) <= tolerance),
            "{name}: {c:?}, want {want:?}"
        );
    }
}

#[test]
fn a_clip_view_imports_with_its_union_of_keyholes_first() {
    let doc = document();
    let clips: Vec<_> = doc
        .tree
        .preorder(doc.tree.root())
        .filter(|&id| matches!(doc.tree.kind(id), Some(NodeKind::ClipView(_))))
        .collect();
    assert_eq!(clips.len(), 2, "two controllers have a marker");
    for (cv, keyholes) in clips.iter().zip([1, 2]) {
        let Some(NodeKind::ClipView(node)) = doc.tree.kind(*cv) else {
            unreachable!()
        };
        assert_eq!(node.mode, ClipViewMode::Inside);
        // The first child is the clipping path; the second, the red square.
        let kids: Vec<_> = doc.tree.children(*cv).collect();
        let Some(NodeKind::Path(clip)) = kids.first().and_then(|&k| doc.tree.kind(k)) else {
            panic!("the first child is the clip path");
        };
        assert_eq!(clip.data.subpaths().count(), keyholes);
        assert!(matches!(
            kids.get(1).and_then(|&k| doc.tree.kind(k)),
            Some(NodeKind::Path(_))
        ));
        // The ClipView sits in a group after the keyholes, which are kept.
        let group = doc
            .tree
            .links(*cv)
            .parent
            .expect("in the controller's group");
        assert!(matches!(doc.tree.kind(group), Some(NodeKind::Group(_))));
        let inks = doc
            .tree
            .children(group)
            .filter(|&k| matches!(doc.tree.kind(k), Some(NodeKind::Path(_))))
            .count();
        assert_eq!(inks, keyholes);
    }
}

#[test]
fn a_clip_view_clips_to_its_keyhole_and_paints_the_keyhole() {
    let got = probe(&[(250_000, 200_000), (350_000, 200_000), (100_000, 200_000)]);
    check(
        got,
        &[
            (RED, "clipped object inside the keyhole"),
            (GREEN, "the keyhole, drawn beneath"),
            (WHITE, "clipped object outside the keyhole"),
        ],
        8,
    );
}

#[test]
fn two_keyholes_clip_to_their_union() {
    let got = probe(&[(550_000, 200_000), (650_000, 200_000), (750_000, 200_000)]);
    check(
        got,
        &[
            (RED, "first keyhole"),
            (WHITE, "between the keyholes"),
            (RED, "second keyhole"),
        ],
        8,
    );
}

#[test]
fn a_controller_without_a_marker_is_drawn_as_a_group() {
    let got = probe(&[(1_500_000, 200_000), (1_700_000, 200_000)]);
    check(
        got,
        &[(RED, "unclipped red"), (GREEN, "the would-be keyhole")],
        8,
    );
}

#[test]
fn a_bitmap_transparency_composites_in_its_mode() {
    // Level 0 everywhere: Stained Glass multiplies yellow into the blue
    // beneath, which is black; Mix would show yellow.
    let got = probe(&[(1_000_000, 200_000)]);
    check(got, &[(BLACK, "stained glass")], 8);
}

#[test]
fn a_bitmap_transparency_maps_black_to_its_start_level() {
    // Black texels take the start level, 115: black over white at
    // 115/255 transparency is grey 115. White texels take 255: clear.
    // The 2 × 2 tile is magnified a hundredfold and filtered, so the
    // probes sit near its bottom and top edges, away from the blend
    // between its rows.
    let got = probe(&[(1_300_000, 104_000), (1_300_000, 296_000)]);
    check(
        got,
        &[([115, 115, 115], "black texels"), (WHITE, "white texels")],
        6,
    );
}
