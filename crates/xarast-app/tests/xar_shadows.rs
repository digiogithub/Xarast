//! Shadows read from `.xar`, drawn through the renderer's offscreen
//! pipeline (phase 13, XARA-T-0318), pinned with pixel probes.
//!
//! A `TAG_SHADOWCONTROLLER` (4050) holds its attributes, a `TAG_SHADOW`
//! (4051) whose own fill is the shadow's colour, and the object that casts
//! it. The shadow is the object's silhouette moved (a wall), squashed and
//! sheared (a floor) or grown (a glow), blurred by half its penumbra and
//! drawn beneath the object at its darkness. Every document is built here
//! with [`xarast_xar::synth::XarBuilder`]; no corpus byte enters the
//! repository.

use xarast_app::DocumentId;
use xarast_app::{DeviceSize, HeadlessFrame, HeadlessOptions, Session, headless};
use xarast_doc::{Document, NodeKind};
use xarast_geom::Rect;
use xarast_xar::synth::XarBuilder;
use xarast_xar::{ImportOptions, import};

const TAG_DEFINERGBCOLOUR: u32 = 50;
const TAG_PATH_RELATIVE_FILLED: u32 = 114;
const TAG_FLATFILL: u32 = 150;
const TAG_FLATTRANSPARENTFILL: u32 = 166;
const TAG_LINECOLOUR_NONE: u32 = 193;
const TAG_SHADOWCONTROLLER: u32 = 4050;
const TAG_SHADOW: u32 = 4051;

/// Record 2: black; record 3: pure red.
const C_BLACK: i32 = 2;
const C_RED: i32 = 3;

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

/// A `TAG_SHADOWCONTROLLER` payload.
fn controller(
    kind: u8,
    penumbra: i32,
    offset: (i32, i32),
    angle: i32,
    height: i32,
    width: i32,
) -> Vec<u8> {
    let mut v = vec![kind];
    for n in [penumbra, offset.0, offset.1, angle, height, 100, width] {
        v.extend_from_slice(&n.to_le_bytes());
    }
    v
}

/// A red shadow at `darkness`, cast by a black square.
fn shadowed(b: XarBuilder, c: Vec<u8>, darkness: f64, square: Vec<u8>) -> XarBuilder {
    let mut s = Vec::new();
    for f in [0.0f64, 0.0, darkness] {
        s.extend_from_slice(&f.to_le_bytes());
    }
    let level = (255.0 * (1.0 - darkness)).round() as u8;
    b.record(TAG_SHADOWCONTROLLER, &c)
        .down()
        .record(TAG_LINECOLOUR_NONE, &[])
        .record(TAG_SHADOW, &s)
        .down()
        .record(TAG_FLATTRANSPARENTFILL, &[level, 1])
        .record(TAG_FLATFILL, &C_RED.to_le_bytes())
        .up()
        .record(TAG_PATH_RELATIVE_FILLED, &square)
        .down()
        .record(TAG_FLATFILL, &C_BLACK.to_le_bytes())
        .up()
        .up()
}

/// Left to right, 200 000 mp squares from y = 100 000 to 300 000:
/// a wall shadow 30 000 right and 30 000 down with a 20 000 penumbra at
/// half darkness; a glow 10 000 wide with no penumbra, fully dark; a floor
/// shadow at half height sheared 45° to the right, no penumbra, fully dark.
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
        .record(TAG_DEFINERGBCOLOUR, &[255, 0, 0])
        .record(40, &[])
        .record(41, &[])
        .down()
        .record(42, &[])
        .down()
        .record(45, &spread)
        .record(43, &[])
        .down()
        .record(48, &layer);
    b = shadowed(
        b,
        controller(1, 20_000, (30_000, -30_000), 0, 50, 0),
        0.5,
        rect(50_000, 100_000, 200_000, 200_000),
    );
    b = shadowed(
        b,
        controller(3, 0, (0, 0), 0, 50, 10_000),
        1.0,
        rect(400_000, 100_000, 200_000, 200_000),
    );
    b = shadowed(
        b,
        controller(2, 0, (0, 0), 785_398, 50, 0),
        1.0,
        rect(700_000, 100_000, 200_000, 200_000),
    );
    b.up().up().up().end_of_file().finish()
}

fn document() -> Document {
    let (doc, report) = import(&drawing(), &ImportOptions::default()).expect("imports");
    assert_eq!(report.errors(), 0, "{:?}", report.diagnostics);
    let controllers = doc
        .tree
        .preorder(doc.tree.root())
        .filter(|&n| {
            matches!(doc.tree.kind(n), Some(NodeKind::Live(l))
                if l.role == xarast_doc::LiveRole::Controller)
        })
        .count();
    assert_eq!(controllers, 3);
    doc
}

/// The page rendered at 1 px per 1 000 mp over white, and a probe that
/// reads the RGB at a document point.
struct Page {
    rgba: Vec<u8>,
    width: usize,
    m: kurbo::Affine,
    walk: xarast_app::WalkStats,
}

impl Page {
    fn at(&self, x: i32, y: i32) -> [u8; 3] {
        let p = self.m * kurbo::Point::new(f64::from(x) + 500.0, f64::from(y) + 500.0);
        let i = (p.y as usize * self.width + p.x as usize) * 4;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2]]
    }
}

fn page() -> Page {
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
    Page {
        rgba: out.surface.data().to_vec(),
        width: out.surface.width() as usize,
        m: out.view.transform.to_affine(),
        walk: out.walk,
    }
}

#[test]
fn every_shadow_is_drawn_and_nothing_is_pending() {
    let p = page();
    assert_eq!(p.walk.effects, 3);
    assert_eq!(p.walk.live_pending, 0);
    assert!(p.walk.is_complete(), "{:?}", p.walk);
}

#[test]
fn a_wall_shadow_is_moved_blurred_at_its_darkness_and_under_its_object() {
    let p = page();
    // The object itself: black, its shadow beneath it.
    assert_eq!(p.at(150_000, 200_000), [0, 0, 0]);
    // Right of the object, inside the moved square, a penumbra radius
    // (10 000) from every edge: red at half darkness over white.
    let [r, g, b] = p.at(265_000, 200_000);
    assert_eq!(r, 255);
    assert!((125..=130).contains(&g) && g == b, "{g} {b}");
    // Above the object's bottom right the shadow is not (it went down).
    assert_eq!(p.at(265_000, 295_000), [255, 255, 255]);
    // The penumbra: soft across the moved edge at x = 280 000.
    let mid = p.at(279_000, 200_000)[1];
    assert!((150..=235).contains(&mid), "{mid}");
    assert_eq!(p.at(292_000, 200_000), [255, 255, 255]);
    // Below the object, down by the offset.
    let [_, g, _] = p.at(150_000, 85_000);
    assert!((125..=130).contains(&g), "{g}");
    assert_eq!(p.at(150_000, 55_000), [255, 255, 255]);
}

#[test]
fn a_glow_surrounds_its_object_by_its_width() {
    let p = page();
    assert_eq!(p.at(500_000, 200_000), [0, 0, 0]);
    // Fully dark red, 5 000 outside every edge.
    assert_eq!(p.at(395_000, 200_000), [255, 0, 0]);
    assert_eq!(p.at(605_000, 200_000), [255, 0, 0]);
    assert_eq!(p.at(500_000, 95_000), [255, 0, 0]);
    assert_eq!(p.at(500_000, 305_000), [255, 0, 0]);
    // Nothing 15 000 out.
    assert_eq!(p.at(385_000, 200_000), [255, 255, 255]);
}

#[test]
fn a_floor_shadow_is_half_as_tall_and_leans_right() {
    let p = page();
    // At y = 150 000 the floor holds what stood at y = 200 000, pushed
    // right by tan 45° × 0.5 × 100 000 = 50 000: x from 750 000 to 950 000.
    assert_eq!(p.at(925_000, 150_000), [255, 0, 0]);
    // Above half the object's height there is no shadow.
    assert_eq!(p.at(925_000, 250_000), [255, 255, 255]);
    // Nothing left of the object: the shadow leans right.
    assert_eq!(p.at(690_000, 150_000), [255, 255, 255]);
}
