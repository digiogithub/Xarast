//! Snapping, grid and guides (XARA-US-0036), driven through intents as the
//! shell drives them: exact landings, the radius in device pixels at three
//! zooms, the mid-drag toggle, the feedback marker, and grid and guides
//! surviving a `.xarast` save and reopen.

use xarast_app::snap::{GuideOp, SnapKind};
use xarast_app::{
    DevicePoint, DocumentId, HandleShape, Intent, OverlayShape, PointerButton, PointerSample,
    Session, ToolId,
};
use xarast_doc::{
    Attach, Command, EditError, GridNode, NodeId, NodeKind, ShapeKind, ShapeNode, Tx,
};
use xarast_geom::{Mp, Point, Vector};

#[derive(Debug)]
struct Square(i32, i32, i32);

impl Command for Square {
    fn label(&self) -> &'static str {
        "Fixture"
    }
    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        let layer = tx.doc().active_layer(tx.doc().active_spread()).unwrap();
        let n = tx.create(NodeKind::Shape(Box::new(ShapeNode {
            shape: ShapeKind::Rect,
            origin: Point::raw(self.0, self.1),
            major: Vector::raw(self.2, 0),
            minor: Vector::raw(0, self.2),
        })))?;
        tx.attach(n, layer, Attach::LastChild)?;
        let fill = tx.create(NodeKind::Attr(Box::new(xarast_doc::AttrNode::new(
            xarast_doc::AttrValue::Fill(xarast_doc::fill::Paint::Flat {
                value: xarast_color::Colour::Direct(xarast_color::ColourValue::rgbt(
                    0.2, 0.4, 0.8, 0.0,
                )),
            }),
        ))))?;
        tx.attach(fill, n, Attach::LastChild)
    }
}

fn sample(at: DevicePoint) -> PointerSample {
    PointerSample {
        at,
        pressure: None,
        time_ms: 0,
    }
}

fn down(s: &mut Session, at: DevicePoint) {
    s.apply(Intent::PointerMove(sample(at))).unwrap();
    s.apply(Intent::PointerDown {
        button: PointerButton::Primary,
        sample: sample(at),
    })
    .unwrap();
}

fn to(s: &mut Session, at: DevicePoint) {
    s.apply(Intent::PointerMove(sample(at))).unwrap();
}

fn up(s: &mut Session, at: DevicePoint) {
    s.apply(Intent::PointerUp {
        button: PointerButton::Primary,
        sample: sample(at),
    })
    .unwrap();
}

fn grid(step: i32) -> GridNode {
    GridNode {
        spacing: Mp::new(step),
        subdivisions: 1,
        origin: Point::ORIGIN,
        ..GridNode::default()
    }
}

fn session_with_square() -> (Session, NodeId) {
    let mut s = Session::new_empty(DocumentId(1));
    // A side of 60 pt: every anchor of the box (edges and centre) sits
    // on the same 10 pt grid as its origin, whichever one snaps.
    s.dispatch(&Square(100_000, 100_000, 60_000)).unwrap();
    let n = xarast_app::edit::selectable_objects(&s.doc).next().unwrap();
    (s, n)
}

fn origin_of(s: &Session, n: NodeId) -> Point {
    match s.doc.tree.kind(n) {
        Some(NodeKind::Shape(sh)) => sh.origin,
        other => panic!("not a shape: {other:?}"),
    }
}

fn preview_offset(s: &Session) -> Option<(f64, f64)> {
    s.preview()
        .transform
        .as_ref()
        .map(|(_, m)| (m.e.to_f64(), m.f.to_f64()))
}

#[test]
fn a_grid_snapped_move_lands_exactly_on_the_grid() {
    let (mut s, n) = session_with_square();
    s.apply(Intent::Guides(GuideOp::SetGrid(grid(10_000))))
        .unwrap();
    s.apply(Intent::ToggleSnap(SnapKind::Grid)).unwrap();
    assert!(s.edit.snap.grid);
    let from = s.viewport.doc_to_device(Point::raw(125_000, 125_000));
    down(&mut s, from);
    // An awkward displacement: 23.4 pt right, 7.7 pt down.
    let at = s.viewport.doc_to_device(Point::raw(148_400, 117_300));
    to(&mut s, at);
    assert!(
        s.overlay().iter().any(|o| matches!(
            o,
            OverlayShape::Handle {
                shape: HandleShape::Snap,
                ..
            }
        )),
        "the snap marker is shown mid-drag"
    );
    up(&mut s, at);
    let o = origin_of(&s, n);
    assert_eq!(o.x.raw() % 10_000, 0, "x on a grid line: {o:?}");
    assert_eq!(o.y.raw() % 10_000, 0, "y on a grid line: {o:?}");
    assert!(
        !s.overlay().iter().any(|o| matches!(
            o,
            OverlayShape::Handle {
                shape: HandleShape::Snap,
                ..
            }
        )),
        "the marker goes with the drag"
    );
}

#[test]
fn the_numpad_toggle_snaps_from_that_point_on_and_releases() {
    let (mut s, n) = session_with_square();
    s.apply(Intent::Guides(GuideOp::SetGrid(grid(10_000))))
        .unwrap();
    let start = origin_of(&s, n);
    let from = s.viewport.doc_to_device(Point::raw(125_000, 125_000));
    down(&mut s, from);
    let at = DevicePoint::new(from.x + 29.0, from.y - 13.0);
    to(&mut s, at);
    let free = preview_offset(&s).unwrap();
    assert!(
        (free.0 % 10_000.0).abs() > 1.0,
        "free movement first: {free:?}"
    );
    // NumPad . mid-drag, no pointer motion: the preview snaps at once.
    s.apply(Intent::ToggleSnap(SnapKind::Grid)).unwrap();
    let snapped = preview_offset(&s).unwrap();
    assert_eq!(snapped.0 % 10_000.0, 0.0, "{snapped:?}");
    assert_eq!(snapped.1 % 10_000.0, 0.0, "{snapped:?}");
    // And off again: free once more.
    s.apply(Intent::ToggleSnap(SnapKind::Grid)).unwrap();
    assert_eq!(preview_offset(&s).unwrap(), free);
    s.apply(Intent::ToggleSnap(SnapKind::Grid)).unwrap();
    up(&mut s, at);
    let o = origin_of(&s, n);
    assert_eq!((o.x.raw() - start.x.raw()) % 10_000, 0);
    assert_eq!(s.undo_label(), Some("Move"), "still one undo step");
}

/// Draws a rectangle whose first corner is `dx` device pixels right of a
/// vertical guide at x = 200 pt, at the given zoom; returns the drawn
/// shape's left edge.
fn rectangle_near_guide(zoom: f64, dx: f64) -> Mp {
    let mut s = Session::new_empty(DocumentId(1));
    s.apply(Intent::Guides(GuideOp::Add {
        horizontal: false,
        position: Mp::new(200_000),
    }))
    .unwrap();
    s.apply(Intent::SetZoom { zoom, anchor: None }).unwrap();
    s.viewport
        .set_centre(xarast_app::DocPointF::new(200_000.0, 300_000.0));
    s.apply(Intent::ChooseTool(ToolId::Rectangle)).unwrap();
    let g = s.viewport.doc_to_device(Point::raw(200_000, 300_000));
    let from = DevicePoint::new(g.x + dx, g.y);
    let end = DevicePoint::new(g.x + dx + 60.0, g.y + 40.0);
    down(&mut s, from);
    to(&mut s, DevicePoint::new(g.x + dx + 30.0, g.y + 20.0));
    to(&mut s, end);
    up(&mut s, end);
    let n = xarast_app::edit::selectable_objects(&s.doc).last().unwrap();
    match s.doc.tree.kind(n) {
        Some(NodeKind::QuickShape(q)) => q.path.as_ref().unwrap().bounds().lo.x,
        other => panic!("not a quick shape: {other:?}"),
    }
}

#[test]
fn the_guide_snap_radius_is_in_device_pixels_at_every_zoom() {
    for zoom in [0.5, 1.0, 4.0] {
        assert_eq!(
            rectangle_near_guide(zoom, 5.0),
            Mp::new(200_000),
            "5 px away snaps at {zoom}"
        );
        assert_ne!(
            rectangle_near_guide(zoom, 12.0),
            Mp::new(200_000),
            "12 px away does not at {zoom}"
        );
    }
}

#[test]
fn guides_and_grid_are_undoable_and_survive_a_xarast_round_trip() {
    let mut s = Session::new_empty(DocumentId(1));
    let digest = s.doc.canonical_digest();
    s.apply(Intent::Guides(GuideOp::Add {
        horizontal: true,
        position: Mp::new(123_000),
    }))
    .unwrap();
    s.apply(Intent::Guides(GuideOp::Add {
        horizontal: false,
        position: Mp::new(-45_000),
    }))
    .unwrap();
    let g = GridNode {
        spacing: Mp::new(36_000),
        subdivisions: 4,
        origin: Point::raw(1_000, 2_000),
        visible: true,
        ..GridNode::default()
    };
    s.apply(Intent::Guides(GuideOp::SetGrid(g.clone())))
        .unwrap();
    let first = xarast_app::snap::guidelines(&s.doc)[0].node;
    s.apply(Intent::Guides(GuideOp::Move {
        guide: first,
        position: Mp::new(124_000),
    }))
    .unwrap();
    assert_eq!(s.undo_label(), Some("Move Guide"));
    let guides = |d: &xarast_doc::Document| -> Vec<(bool, Mp)> {
        xarast_app::snap::guidelines(d)
            .iter()
            .map(|g| (g.horizontal, g.position))
            .collect()
    };
    assert_eq!(
        guides(&s.doc),
        vec![(true, Mp::new(124_000)), (false, Mp::new(-45_000))]
    );
    let mut bytes = std::io::Cursor::new(Vec::new());
    xarast_format::save::save_to(
        &s.doc,
        &mut bytes,
        &xarast_format::save::SaveOptions::default(),
    )
    .unwrap();
    let back = Session::open_bytes(
        DocumentId(2),
        std::path::Path::new("guides.xarast"),
        bytes.get_ref(),
    )
    .unwrap();
    assert_eq!(guides(&back.doc), guides(&s.doc), "guides reopen");
    let gb = xarast_app::snap::grid_of(&back.doc);
    assert_eq!(
        (gb.spacing, gb.subdivisions, gb.origin, gb.visible),
        (g.spacing, g.subdivisions, g.origin, g.visible),
        "the grid reopens"
    );
    while s.undo().is_some() {}
    assert_eq!(s.doc.canonical_digest(), digest);
}

#[test]
fn object_snap_takes_a_corner_first_and_an_outline_otherwise() {
    let mut s = Session::new_empty(DocumentId(1));
    s.dispatch(&Square(0, 0, 60_000)).unwrap();
    s.dispatch(&Square(200_000, 0, 60_000)).unwrap();
    let objs: Vec<NodeId> = xarast_app::edit::selectable_objects(&s.doc).collect();
    let (_, b) = (objs[0], objs[1]);
    s.apply(Intent::Guides(GuideOp::DeleteAll)).unwrap();
    s.apply(Intent::ToggleSnap(SnapKind::Object)).unwrap();
    // Drag B left so its left edge is 2 pt right of A's right edge, 10 pt
    // up: no corner of A is in reach, A's right edge is.
    let from = s.viewport.doc_to_device(Point::raw(230_000, 30_000));
    let at = s.viewport.doc_to_device(Point::raw(92_000, 40_000));
    down(&mut s, from);
    to(&mut s, at);
    up(&mut s, at);
    let o = origin_of(&s, b);
    assert_eq!(o.x, Mp::new(60_000), "on A's outline: {o:?}");
    // Now near A's top-right corner: the corner wins over the edge.
    let from = s.viewport.doc_to_device(Point::raw(90_000, 40_000));
    let at = s.viewport.doc_to_device(Point::raw(91_500, 81_000));
    down(&mut s, from);
    to(&mut s, at);
    up(&mut s, at);
    let o = origin_of(&s, b);
    assert_eq!(o, Point::raw(60_000, 60_000), "B's corner on A's corner");
}

#[test]
fn show_grid_and_guides_toggle_and_hidden_guides_do_not_snap() {
    let mut s = Session::new_empty(DocumentId(1));
    let before = xarast_app::snap::grid_of(&s.doc).visible;
    s.apply(Intent::ToggleGrid).unwrap();
    assert_ne!(xarast_app::snap::grid_of(&s.doc).visible, before);
    assert!(xarast_app::snap::guides_visible(&s.doc));
    s.apply(Intent::ToggleGuides).unwrap();
    assert!(!xarast_app::snap::guides_visible(&s.doc));
    assert!(
        xarast_app::snap::GuideSource::of(&s.doc).guides.is_empty(),
        "hidden guides do not snap"
    );
}
