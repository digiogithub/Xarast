//! Feathers read from `.xar`, drawn through the renderer's offscreen
//! pipeline (phase 13, XARA-US-0068), pinned with pixel probes.
//!
//! A `TAG_FEATHER` (4086) on an object fades it from fully opaque one
//! feather-size inside its outline to clear at the outline; on a group it
//! feathers the group as one unit, never each member again. Every document
//! is built here with [`xarast_xar::synth::XarBuilder`]; no corpus byte
//! enters the repository.

use xarast_app::DocumentId;
use xarast_app::{DeviceSize, HeadlessFrame, HeadlessOptions, Session, headless};
use xarast_doc::{AttrValue, Document, NodeKind};
use xarast_geom::Rect;
use xarast_xar::synth::XarBuilder;
use xarast_xar::{ImportOptions, import};

const TAG_DEFINERGBCOLOUR: u32 = 50;
const TAG_GROUP: u32 = 104;
const TAG_PATH_RELATIVE_FILLED: u32 = 114;
const TAG_FLATFILL: u32 = 150;
const TAG_LINECOLOUR_NONE: u32 = 193;
const TAG_FEATHER: u32 = 4086;

const C_BLACK: i32 = 2;

fn interleave(x: i32, y: i32) -> [u8; 8] {
    let (x, y) = (x.to_be_bytes(), y.to_be_bytes());
    [x[0], y[0], x[1], y[1], x[2], y[2], x[3], y[3]]
}

/// A closed `w` × `h` rectangle with its bottom-left corner at `(x, y)`.
fn rect(x: i32, y: i32, w: i32, h: i32) -> Vec<u8> {
    let mut p = vec![0x06];
    p.extend_from_slice(&interleave(x + w, y + h));
    for (verb, dx, dy) in [(0x02, w, 0), (0x02, 0, h), (0x02, -w, 0), (0x03, 0, -h)] {
        p.push(verb);
        p.extend_from_slice(&interleave(dx, dy));
    }
    p
}

fn feather(size: i32) -> Vec<u8> {
    let mut f = size.to_le_bytes().to_vec();
    f.extend_from_slice(&0f64.to_le_bytes());
    f.extend_from_slice(&0f64.to_le_bytes());
    f
}

/// A black square, feathered when `size` is given.
fn square(b: XarBuilder, r: Vec<u8>, feathered: Option<i32>) -> XarBuilder {
    let b = b
        .record(TAG_PATH_RELATIVE_FILLED, &r)
        .down()
        .record(TAG_FLATFILL, &C_BLACK.to_le_bytes())
        .record(TAG_LINECOLOUR_NONE, &[]);
    let b = match feathered {
        Some(s) => b.record(TAG_FEATHER, &feather(s)),
        None => b,
    };
    b.up()
}

/// Left to right: a square feathered by 40 000 mp; a plain square; a group
/// of two abutting squares feathered by 40 000 mp as one.
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
    let mut b = XarBuilder::new()
        .record(TAG_DEFINERGBCOLOUR, &[0, 0, 0])
        .record(40, &[])
        .record(41, &[])
        .down()
        .record(42, &[])
        .down()
        .record(45, &spread)
        .record(43, &[])
        .down()
        .record(48, &layer);
    b = square(b, rect(50_000, 100_000, 200_000, 200_000), Some(40_000));
    b = square(b, rect(350_000, 100_000, 200_000, 200_000), None);
    // A group's attributes come before its members.
    b = b
        .record(TAG_GROUP, &[])
        .down()
        .record(TAG_FEATHER, &feather(40_000));
    b = square(b, rect(650_000, 100_000, 100_000, 200_000), None);
    b = square(b, rect(750_000, 100_000, 100_000, 200_000), None);
    b = b.up();
    b.up().up().up().end_of_file().finish()
}

fn document() -> Document {
    let (doc, report) = import(&drawing(), &ImportOptions::default()).expect("imports");
    assert_eq!(report.errors(), 0, "{:?}", report.diagnostics);
    let feathers = doc
        .tree
        .preorder(doc.tree.root())
        .filter(|&n| {
            matches!(doc.tree.kind(n), Some(NodeKind::Attr(a))
                if matches!(a.value, AttrValue::Feather { .. }))
        })
        .count();
    assert_eq!(feathers, 2);
    doc
}

/// Grey levels (R) along the row through the squares' middle, 1 px per
/// 1 000 mp, over opaque white.
fn row() -> (Vec<u8>, xarast_app::WalkStats) {
    let frame = Rect::new(
        xarast_geom::Point::raw(0, 0),
        xarast_geom::Point::raw(1_000_000, 400_000),
    );
    let session = Session::adopt(DocumentId(1), document(), None);
    let out = headless::render(
        &session,
        &HeadlessOptions {
            size: DeviceSize::new(1_000, 400),
            frame: HeadlessFrame::Fit(frame),
            ..HeadlessOptions::default()
        },
    )
    .expect("renders");
    let m = out.view.transform.to_affine();
    let w = out.surface.width() as usize;
    let grey = (0..1_000)
        .map(|x| {
            let p = m * kurbo::Point::new(f64::from(x) * 1000.0 + 500.0, 200_000.0);
            let i = (p.y as usize * w + p.x as usize) * 4;
            out.surface.data()[i]
        })
        .collect();
    (grey, out.walk)
}

#[test]
fn a_feathered_object_fades_to_clear_at_its_outline() {
    let (g, walk) = row();
    assert_eq!(
        walk.effects, 2,
        "one per feather attribute, the group's once"
    );
    // Outside: the page.
    assert_eq!(g[45], 255);
    // At the outline: nearly clear; a feather size in: fully black.
    assert!(g[50] > 200, "outline {}", g[50]);
    assert!(g[70] > 20 && g[70] < 235, "halfway {}", g[70]);
    assert_eq!(g[92], 0, "one feather size in");
    assert_eq!(g[150], 0);
    // Monotone from the edge inwards.
    for x in 50..90 {
        assert!(g[x] >= g[x + 1], "x = {x}: {} then {}", g[x], g[x + 1]);
    }
    // The feather does not grow the object.
    assert_eq!(g[249], g[50].max(g[249]), "symmetric edge");
    assert_eq!(g[252], 255);
}

#[test]
fn an_unfeathered_object_keeps_its_hard_edge() {
    let (g, _) = row();
    assert_eq!(g[349], 255);
    assert_eq!(g[351], 0);
    assert_eq!(g[548], 0);
    assert_eq!(g[551], 255);
}

#[test]
fn a_feathered_group_is_feathered_as_one_unit() {
    let (g, _) = row();
    // The seam between the two members is deep inside the unit: no fade.
    assert_eq!(g[749], 0);
    assert_eq!(g[750], 0);
    // Its outer edges fade.
    assert!(g[651] > 150, "left edge {}", g[651]);
    assert!(g[848] > 150, "right edge {}", g[848]);
}
