//! The shape editor and the pen (XARA-US-0034), driven through intents
//! exactly as the shell drives them.

use std::sync::Arc;

use xarast_app::shapes::rectangle;
use xarast_app::{
    AppCommand, DevicePoint, DocumentId, EditCommand, HandleShape, InfobarField, InfobarItem,
    InfobarValue, Intent, Modifiers, OverlayShape, PointerButton, PointerSample, Session,
    ToolAction, ToolId,
};
use xarast_doc::{Attach, AttrSlot, AttrValue, Command, EditError, NodeId, NodeKind, PathNode, Tx};
use xarast_geom::{EditPath, FillRule, NodeRef, Path, PathBuilder, Point, SegRef, Side, Vector};

#[derive(Debug)]
struct AddPath(Path);

impl Command for AddPath {
    fn label(&self) -> &'static str {
        "Fixture"
    }
    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        let spread = tx.doc().active_spread();
        let layer = tx.doc().active_layer(spread).expect("a layer");
        let n = tx.create(NodeKind::Path(Box::new(PathNode::new(self.0.clone()))))?;
        tx.attach(n, layer, Attach::LastChild)
    }
}

/// An open path: a line, a curve, a line. Nodes at (100,100), (200,100),
/// (300,200), (300,300) points.
fn zigzag() -> Path {
    let mut b = PathBuilder::new();
    b.move_to(Point::raw(100_000, 100_000))
        .line_to(Point::raw(200_000, 100_000))
        .cubic_to(
            Point::raw(250_000, 100_000),
            Point::raw(300_000, 150_000),
            Point::raw(300_000, 200_000),
        )
        .line_to(Point::raw(300_000, 300_000));
    b.build()
}

/// A session holding one path, selected, with the shape editor in force.
fn fixture() -> (Session, NodeId) {
    let mut s = Session::new_empty(DocumentId(1));
    s.dispatch(&AddPath(zigzag())).expect("fixture");
    s.bus.history_mut().clear(&mut s.doc);
    let n = xarast_app::edit::selectable_objects(&s.doc)
        .next()
        .expect("the path");
    s.apply(Intent::Select {
        nodes: vec![n],
        mode: xarast_app::SelectMode::Replace,
    })
    .unwrap();
    s.apply(Intent::ChooseTool(ToolId::ShapeEditor)).unwrap();
    (s, n)
}

fn path(s: &Session, n: NodeId) -> Arc<Path> {
    match s.doc.tree.kind(n) {
        Some(NodeKind::Path(p)) => Arc::clone(&p.data),
        other => panic!("not a path: {other:?}"),
    }
}

fn nodes(s: &Session, n: NodeId) -> EditPath {
    EditPath::from_path(&path(s, n))
}

fn selected_points(s: &Session, n: NodeId) -> Vec<u32> {
    let mut v: Vec<u32> = s
        .edit
        .control_points(n)
        .map(|c| c.iter().collect())
        .unwrap_or_default();
    v.sort_unstable();
    v
}

fn dev(s: &Session, p: Point) -> DevicePoint {
    s.viewport.doc_to_device(p)
}

fn sample(at: DevicePoint, t: u64) -> PointerSample {
    PointerSample {
        at,
        pressure: None,
        time_ms: t,
    }
}

fn press(s: &mut Session, p: Point, t: u64) {
    let at = dev(s, p);
    s.apply(Intent::PointerMove(sample(at, t))).unwrap();
    s.apply(Intent::PointerDown {
        button: PointerButton::Primary,
        sample: sample(at, t),
    })
    .unwrap();
}

fn move_to(s: &mut Session, p: Point) {
    let at = dev(s, p);
    s.apply(Intent::PointerMove(sample(at, 0))).unwrap();
}

fn release(s: &mut Session, p: Point, t: u64) {
    let at = dev(s, p);
    s.apply(Intent::PointerUp {
        button: PointerButton::Primary,
        sample: sample(at, t),
    })
    .unwrap();
}

fn click(s: &mut Session, p: Point, t: u64) {
    press(s, p, t);
    release(s, p, t);
}

fn drag(s: &mut Session, from: Point, to: Point, t: u64) {
    press(s, from, t);
    for i in 1..=10 {
        let k = f64::from(i) / 10.0;
        let (x0, y0) = from.to_f64();
        let (x1, y1) = to.to_f64();
        move_to(
            s,
            Point::from_f64_round(x0 + (x1 - x0) * k, y0 + (y1 - y0) * k),
        );
    }
    release(s, to, t + 10);
}

fn mods(s: &mut Session, constrain: bool, adjust: bool) {
    s.apply(Intent::ModifiersChanged(Modifiers {
        constrain,
        adjust,
        ..Modifiers::default()
    }))
    .unwrap();
}

fn action(s: &mut Session, a: ToolAction) {
    s.apply(AppCommand::Action(a).intent(DevicePoint::new(0.0, 0.0)))
        .unwrap();
}

fn count(s: &Session, shape: HandleShape) -> usize {
    s.overlay()
        .iter()
        .filter(|o| matches!(o, OverlayShape::Handle { shape: h, .. } if *h == shape))
        .count()
}

/// Undo restores the document exactly, redo reapplies it exactly.
fn undo_redo_exact(s: &mut Session, before: [u8; 32]) {
    let after = s.doc.canonical_digest();
    s.apply(Intent::Undo).unwrap();
    assert_eq!(s.doc.canonical_digest(), before, "undo is exact");
    s.apply(Intent::Redo).unwrap();
    assert_eq!(s.doc.canonical_digest(), after, "redo is exact");
}

// ─────────────────────────────────────────────── selecting nodes

#[test]
fn clicking_nodes_selects_them_and_never_touches_the_geometry() {
    let (mut s, n) = fixture();
    let data = path(&s, n);
    assert_eq!(count(&s, HandleShape::Node), 4);
    click(&mut s, Point::raw(200_000, 100_000), 0);
    assert_eq!(selected_points(&s, n), vec![1]);
    assert_eq!(count(&s, HandleShape::NodeSelected), 1);
    // The one selected node shows its handle (its outgoing curve handle).
    assert_eq!(count(&s, HandleShape::Control), 1);
    // Adjust adds, Adjust again removes.
    mods(&mut s, false, true);
    click(&mut s, Point::raw(300_000, 300_000), 1000);
    assert_eq!(selected_points(&s, n), vec![1, 5]);
    assert_eq!(
        count(&s, HandleShape::Control),
        0,
        "two selected: no handles"
    );
    click(&mut s, Point::raw(200_000, 100_000), 2000);
    assert_eq!(selected_points(&s, n), vec![5]);
    // Adjust+Constrain toggles every node of the path.
    mods(&mut s, true, true);
    click(&mut s, Point::raw(100_000, 100_000), 3000);
    assert_eq!(selected_points(&s, n), vec![0, 1, 4]);
    mods(&mut s, false, false);
    // Selecting points is not an edit: the geometry is the same Arc.
    assert!(Arc::ptr_eq(&data, &path(&s, n)));
    assert_eq!(s.bus.history().len(), 0);
    // Esc deselects the points first, then the object.
    s.apply(Intent::Cancel).unwrap();
    assert!(selected_points(&s, n).is_empty());
    assert!(s.edit.is_selected(n));
    s.apply(Intent::Cancel).unwrap();
    assert!(!s.edit.is_selected(n));
}

#[test]
fn a_marquee_selects_the_nodes_inside_it_and_select_all_every_node() {
    let (mut s, n) = fixture();
    drag(
        &mut s,
        Point::raw(150_000, 50_000),
        Point::raw(350_000, 250_000),
        0,
    );
    assert_eq!(selected_points(&s, n), vec![1, 4]);
    assert_eq!(s.bus.history().len(), 0);
    s.apply(Intent::SelectAll).unwrap();
    assert_eq!(selected_points(&s, n), vec![0, 1, 4, 5]);
    // Select all selected points, not other objects: the object
    // selection is unchanged.
    assert_eq!(s.edit.selection_len(), 1);
}

// ─────────────────────────────────────────────── moving

#[test]
fn dragging_nodes_is_one_labelled_step_and_undo_is_exact() {
    let (mut s, n) = fixture();
    let before = s.doc.canonical_digest();
    click(&mut s, Point::raw(200_000, 100_000), 0);
    mods(&mut s, false, true);
    click(&mut s, Point::raw(100_000, 100_000), 1000);
    mods(&mut s, false, false);
    // Mid-drag the document is untouched and the path is hidden.
    press(&mut s, Point::raw(200_000, 100_000), 3000);
    move_to(&mut s, Point::raw(210_000, 120_000));
    assert_eq!(s.doc.canonical_digest(), before);
    assert_eq!(s.preview().hidden, vec![n]);
    release(&mut s, Point::raw(210_000, 120_000), 3010);
    assert!(s.preview().is_empty());
    assert_eq!(s.bus.history().len(), 1);
    assert_eq!(s.undo_label(), Some("Move Points"));
    let e = nodes(&s, n);
    let at = |i| e.node(NodeRef::new(0, i)).unwrap().at;
    let near =
        |p: Point, x: i32, y: i32| (p.x.raw() - x).abs() <= 1_500 && (p.y.raw() - y).abs() <= 1_500;
    assert!(near(at(0), 110_000, 120_000), "{:?}", at(0));
    assert!(near(at(1), 210_000, 120_000), "{:?}", at(1));
    assert_eq!(at(2), Point::raw(300_000, 200_000), "unselected nodes stay");
    // The handle moved with its node.
    let out = e.node(NodeRef::new(0, 1)).unwrap().ctrl_out.unwrap().at;
    assert_eq!(out - at(1), Vector::raw(50_000, 0));
    // The selection survives the edit.
    assert_eq!(selected_points(&s, n), vec![0, 1]);
    undo_redo_exact(&mut s, before);
}

#[test]
fn constrain_moves_nodes_along_45_degrees() {
    let (mut s, n) = fixture();
    click(&mut s, Point::raw(300_000, 300_000), 0);
    mods(&mut s, true, false);
    drag(
        &mut s,
        Point::raw(300_000, 300_000),
        Point::raw(340_000, 305_000),
        1000,
    );
    let p = nodes(&s, n).node(NodeRef::new(0, 3)).unwrap().at;
    assert_eq!(p.y.raw(), 300_000, "horizontal: {p:?}");
    assert!(p.x.raw() > 330_000);
}

#[test]
fn escape_at_any_point_of_a_node_drag_leaves_nothing_behind() {
    for cut in 0..10 {
        let (mut s, _) = fixture();
        click(&mut s, Point::raw(200_000, 100_000), 0);
        let before = s.doc.canonical_digest();
        let h = s.bus.history().len();
        press(&mut s, Point::raw(200_000, 100_000), 1000);
        for i in 0..cut {
            move_to(&mut s, Point::raw(200_000 + i * 3_000, 100_000 + i * 2_000));
        }
        s.apply(Intent::Cancel).unwrap();
        release(&mut s, Point::raw(260_000, 160_000), 1100);
        assert_eq!(s.doc.canonical_digest(), before, "cut {cut}");
        assert_eq!(s.bus.history().len(), h, "cut {cut}");
        assert!(s.preview().is_empty());
    }
}

#[test]
fn a_smooth_node_keeps_its_handles_collinear_when_one_is_dragged() {
    let (mut s, n) = fixture();
    // Add a point in the middle of the curve: it is smooth.
    let e = nodes(&s, n);
    let seg = SegRef {
        subpath: 0,
        segment: 1,
    };
    let mid = e.segment(seg).unwrap().to_kurbo();
    let q = kurbo::ParamCurve::eval(&mid, 0.5);
    let on = Point::from_f64_round(q.x, q.y);
    click(&mut s, on, 0);
    assert_eq!(s.undo_label(), Some("Add Point"));
    let e = nodes(&s, n);
    assert_eq!(e.subpaths[0].nodes.len(), 5);
    let added = NodeRef::new(0, 2);
    assert!(e.node(added).unwrap().is_smooth());
    // The new node is selected and shows both its handles.
    assert_eq!(count(&s, HandleShape::Control), 2);
    let node = e.node(added).unwrap().clone();
    let out = node.ctrl_out.unwrap().at;
    let keep = (node.ctrl_in.unwrap().at - node.at).length();
    drag(&mut s, out, out + Vector::raw(0, 20_000), 1000);
    assert_eq!(s.undo_label(), Some("Move Points"));
    let after = nodes(&s, n).node(added).unwrap().clone();
    let (i, o) = (
        after.ctrl_in.unwrap().at - after.at,
        after.ctrl_out.unwrap().at - after.at,
    );
    let (ix, iy) = i.to_f64();
    let (ox, oy) = o.to_f64();
    let cross = (ix * oy - iy * ox).abs() / (i.length() * o.length());
    assert!(cross < 1e-3, "collinear: {i:?} {o:?}");
    assert!(
        (i.length() - keep).abs() < 2.0,
        "the other keeps its length"
    );
    // A cusp's handles move alone.
    action(&mut s, ToolAction::Cusp);
    assert_eq!(s.undo_label(), Some("Cusp Points"));
    let o2 = after.ctrl_out.unwrap().at;
    drag(&mut s, o2, o2 + Vector::raw(20_000, 0), 2000);
    let cusp = nodes(&s, n).node(added).unwrap().clone();
    assert_eq!(cusp.ctrl_in.unwrap().at, after.ctrl_in.unwrap().at);
    let _ = Side::In;
}

#[test]
fn dragging_a_segment_reshapes_it_under_the_pointer() {
    let (mut s, n) = fixture();
    let before = s.doc.canonical_digest();
    // The first segment is a line from (100,100) to (200,100).
    drag(
        &mut s,
        Point::raw(150_000, 100_000),
        Point::raw(150_000, 130_000),
        0,
    );
    assert_eq!(s.undo_label(), Some("Reshape Curve"));
    let e = nodes(&s, n);
    let seg = SegRef {
        subpath: 0,
        segment: 0,
    };
    assert!(e.is_curve(seg));
    let k = e.segment(seg).unwrap().to_kurbo();
    let q = kurbo::ParamCurve::eval(&k, 0.5);
    assert!((q.y - 130_000.0).abs() < 2_500.0, "{q:?}");
    undo_redo_exact(&mut s, before);
}

// ─────────────────────────────────────────────── adding and deleting

#[test]
fn adding_then_deleting_a_point_on_a_line_restores_the_path() {
    let (mut s, n) = fixture();
    let before = s.doc.canonical_digest();
    let original = path(&s, n);
    click(&mut s, Point::raw(150_000, 100_000), 0);
    assert_eq!(s.undo_label(), Some("Add Point"));
    assert_eq!(nodes(&s, n).subpaths[0].nodes.len(), 5);
    assert_eq!(selected_points(&s, n), vec![1]);
    s.apply(Intent::DeleteSelection).unwrap();
    assert_eq!(s.undo_label(), Some("Delete Points"));
    assert_eq!(*path(&s, n), *original);
    assert!(
        s.edit.is_selected(n),
        "Delete took the point, not the object"
    );
    s.apply(Intent::Undo).unwrap();
    s.apply(Intent::Undo).unwrap();
    assert_eq!(s.doc.canonical_digest(), before);
}

#[test]
fn deleting_a_node_between_curves_keeps_the_outer_tangents() {
    let (mut s, n) = fixture();
    let e = nodes(&s, n);
    let seg = SegRef {
        subpath: 0,
        segment: 1,
    };
    let k = e.segment(seg).unwrap().to_kurbo();
    let q = kurbo::ParamCurve::eval(&k, 0.4);
    click(&mut s, Point::from_f64_round(q.x, q.y), 0);
    s.apply(Intent::DeleteSelection).unwrap();
    let back = nodes(&s, n);
    let (a, b) = (
        back.node(NodeRef::new(0, 1)).unwrap().ctrl_out.unwrap().at,
        back.node(NodeRef::new(0, 2)).unwrap().ctrl_in.unwrap().at,
    );
    assert!(a.distance_to(Point::raw(250_000, 100_000)) < 30.0, "{a:?}");
    assert!(b.distance_to(Point::raw(300_000, 150_000)) < 30.0, "{b:?}");
}

#[test]
fn deleting_every_node_deletes_the_object() {
    let (mut s, n) = fixture();
    let before = s.doc.canonical_digest();
    s.apply(Intent::SelectAll).unwrap();
    s.apply(Intent::DeleteSelection).unwrap();
    assert!(!s.doc.tree.is_reachable(n));
    assert_eq!(s.undo_label(), Some("Delete"));
    s.apply(Intent::Undo).unwrap();
    assert_eq!(s.doc.canonical_digest(), before);
    // With no point selected, Delete deletes the object.
    s.apply(Intent::Select {
        nodes: vec![n],
        mode: xarast_app::SelectMode::Replace,
    })
    .unwrap();
    s.apply(Intent::DeleteSelection).unwrap();
    assert!(!s.doc.tree.is_reachable(n));
}

// ─────────────────────────────────────────────── the path operations

#[test]
fn make_line_make_curve_smooth_cusp_close_break_and_join() {
    let (mut s, n) = fixture();
    let before = s.doc.canonical_digest();
    // Select the two ends of the curve.
    click(&mut s, Point::raw(200_000, 100_000), 0);
    mods(&mut s, false, true);
    click(&mut s, Point::raw(300_000, 200_000), 1000);
    mods(&mut s, false, false);
    action(&mut s, ToolAction::MakeLine);
    assert_eq!(s.undo_label(), Some("Make Line"));
    let seg1 = SegRef {
        subpath: 0,
        segment: 1,
    };
    assert!(!nodes(&s, n).is_curve(seg1));
    assert_eq!(selected_points(&s, n).len(), 2, "still selected");
    action(&mut s, ToolAction::MakeCurve);
    assert_eq!(s.undo_label(), Some("Make Curve"));
    assert!(nodes(&s, n).is_curve(seg1));
    action(&mut s, ToolAction::Smooth);
    assert_eq!(s.undo_label(), Some("Smooth Points"));
    assert!(nodes(&s, n).node(NodeRef::new(0, 1)).unwrap().is_smooth());
    action(&mut s, ToolAction::Cusp);
    assert!(!nodes(&s, n).node(NodeRef::new(0, 1)).unwrap().is_smooth());
    // Close: the path gets a fourth segment and is filled.
    action(&mut s, ToolAction::ClosePath);
    assert_eq!(s.undo_label(), Some("Close Path"));
    assert!(nodes(&s, n).subpaths[0].closed);
    // Break at the two selected nodes: two open subpaths.
    action(&mut s, ToolAction::Break);
    assert_eq!(s.undo_label(), Some("Break Path"));
    let e = nodes(&s, n);
    assert!(e.subpaths.iter().all(|p| !p.closed));
    assert_eq!(e.subpaths.len(), 2);
    // Join two ends back: the end of the first piece and the start of
    // the second, which lie on each other.
    let pieces = xarast_app::node_edit::Nodes::of(&path(&s, n));
    let last = pieces.path.subpaths[0].nodes.len() - 1;
    let pick = pieces.indices(&[NodeRef::new(0, last), NodeRef::new(1, 0)]);
    s.edit.set_control_points(vec![(n, pick)]);
    action(&mut s, ToolAction::Join);
    assert_eq!(s.undo_label(), Some("Join Ends"));
    assert_eq!(nodes(&s, n).subpaths.len(), 1);
    // Every step undoes, all the way back.
    while s.undo_label().is_some() {
        s.apply(Intent::Undo).unwrap();
    }
    assert_eq!(s.doc.canonical_digest(), before);
}

#[test]
fn double_click_on_a_selected_node_toggles_smooth_and_cusp() {
    let (mut s, n) = fixture();
    let p = Point::raw(200_000, 100_000);
    click(&mut s, p, 0);
    click(&mut s, p, 100);
    assert_eq!(s.undo_label(), Some("Smooth Points"));
    assert!(nodes(&s, n).node(NodeRef::new(0, 1)).unwrap().is_smooth());
    click(&mut s, p, 2000);
    click(&mut s, p, 2100);
    assert_eq!(s.undo_label(), Some("Cusp Points"));
    assert!(!nodes(&s, n).node(NodeRef::new(0, 1)).unwrap().is_smooth());
}

#[test]
fn typing_x_and_y_moves_the_selected_node() {
    let (mut s, n) = fixture();
    click(&mut s, Point::raw(300_000, 300_000), 0);
    let bar = s.infobar();
    assert!(bar.items.iter().any(|i| matches!(
        i,
        InfobarItem::Measure { field: InfobarField::X, value: Some(v), editable: true } if v.raw() == 300_000
    )));
    s.apply(Intent::InfobarEdit {
        field: InfobarField::X,
        value: InfobarValue::Length(xarast_geom::Mp::new(320_000)),
    })
    .unwrap();
    assert_eq!(
        nodes(&s, n).node(NodeRef::new(0, 3)).unwrap().at,
        Point::raw(320_000, 300_000)
    );
    assert_eq!(s.undo_label(), Some("Move Points"));
}

#[test]
fn the_winding_rule_is_an_own_attribute_of_the_path() {
    let (mut s, n) = fixture();
    let before = s.doc.canonical_digest();
    s.apply(Intent::InfobarEdit {
        field: InfobarField::EvenOdd,
        value: InfobarValue::Toggle(true),
    })
    .unwrap();
    assert_eq!(s.undo_label(), Some("Winding Rule"));
    let rule = xarast_doc::attr::resolve_uncached(&s.doc.tree, n, &s.doc.defaults)
        .get(AttrSlot::WindingRule)
        .clone();
    assert_eq!(rule, AttrValue::WindingRule(FillRule::EvenOdd));
    assert!(s.infobar().items.iter().any(|i| matches!(
        i,
        InfobarItem::Toggle {
            field: InfobarField::EvenOdd,
            on: true
        }
    )));
    undo_redo_exact(&mut s, before);
}

// ─────────────────────────────────────────────── parametric shapes

#[test]
fn a_rectangle_is_never_converted_silently_and_converts_on_request() {
    let mut s = Session::new_empty(DocumentId(1));
    let layer = s.edit.active_layer().unwrap();
    s.apply_edit(EditCommand::CreateShape {
        layer,
        shape: Box::new(rectangle(Point::raw(200_000, 200_000), 50_000.0, 30_000.0)),
        attrs: Vec::new(),
    })
    .unwrap();
    let r = s.doc.tree.children(layer).next_back().unwrap();
    s.apply(Intent::Select {
        nodes: vec![r],
        mode: xarast_app::SelectMode::Replace,
    })
    .unwrap();
    s.apply(Intent::ChooseTool(ToolId::ShapeEditor)).unwrap();
    let before = s.doc.canonical_digest();
    // The shape editor offers the conversion and draws no nodes.
    assert!(s.infobar().items.iter().any(|i| matches!(
        i,
        InfobarItem::Command {
            command: AppCommand::ConvertToShapes,
            enabled: true
        }
    )));
    assert_eq!(count(&s, HandleShape::Node), 0);
    // Clicking, dragging and deleting in the shape editor leave it a
    // quick shape.
    drag(
        &mut s,
        Point::raw(150_000, 170_000),
        Point::raw(160_000, 180_000),
        0,
    );
    click(&mut s, Point::raw(250_000, 200_000), 1000);
    assert!(matches!(s.doc.tree.kind(r), Some(NodeKind::QuickShape(_))));
    assert_eq!(s.doc.canonical_digest(), before);
    // Convert to editable shapes: same node, now a path with the outline.
    s.apply(Intent::Select {
        nodes: vec![r],
        mode: xarast_app::SelectMode::Replace,
    })
    .unwrap();
    s.apply(AppCommand::ConvertToShapes.intent(DevicePoint::new(0.0, 0.0)))
        .unwrap();
    assert_eq!(s.undo_label(), Some("Convert to Editable Shapes"));
    let Some(NodeKind::Path(p)) = s.doc.tree.kind(r) else {
        panic!("converted");
    };
    assert_eq!(EditPath::from_path(&p.data).subpaths[0].nodes.len(), 4);
    assert_eq!(count(&s, HandleShape::Node), 4);
    undo_redo_exact(&mut s, before);
}

// ─────────────────────────────────────────────── the pen

fn pen() -> Session {
    let mut s = Session::new_empty(DocumentId(1));
    s.apply(Intent::ChooseTool(ToolId::Pen)).unwrap();
    s
}

fn only_path(s: &Session) -> (NodeId, EditPath) {
    let n = xarast_app::edit::selectable_objects(&s.doc)
        .next()
        .expect("a path");
    (n, nodes(s, n))
}

#[test]
fn the_pen_draws_lines_point_by_point_one_step_each() {
    let mut s = pen();
    let before = s.doc.canonical_digest();
    click(&mut s, Point::raw(100_000, 100_000), 0);
    assert_eq!(s.bus.history().len(), 0, "a start point is not an edit");
    click(&mut s, Point::raw(200_000, 100_000), 1000);
    assert_eq!(s.undo_label(), Some("Create Path"));
    click(&mut s, Point::raw(200_000, 200_000), 2000);
    assert_eq!(s.undo_label(), Some("Add Segment"));
    let (n, e) = only_path(&s);
    assert_eq!(e.subpaths[0].nodes.len(), 3);
    assert!(!e.is_curve(SegRef {
        subpath: 0,
        segment: 0
    }));
    // The new end is the selected point the next click continues from.
    assert_eq!(selected_points(&s, n), vec![2]);
    // Enter finishes: the next click starts a new path.
    action(&mut s, ToolAction::Finish);
    assert!(selected_points(&s, n).is_empty());
    click(&mut s, Point::raw(400_000, 100_000), 5000);
    click(&mut s, Point::raw(400_000, 200_000), 6000);
    assert_eq!(xarast_app::edit::selectable_objects(&s.doc).count(), 2);
    assert_eq!(s.bus.history().len(), 3);
    while s.undo_label().is_some() {
        s.apply(Intent::Undo).unwrap();
    }
    assert_eq!(s.doc.canonical_digest(), before);
}

#[test]
fn a_pen_drag_makes_a_smooth_point_with_mirrored_handles() {
    let mut s = pen();
    click(&mut s, Point::raw(100_000, 100_000), 0);
    drag(
        &mut s,
        Point::raw(200_000, 100_000),
        Point::raw(230_000, 130_000),
        1000,
    );
    let (_, e) = only_path(&s);
    let p = e.node(NodeRef::new(0, 1)).unwrap().clone();
    assert!(p.is_smooth());
    assert_eq!(p.ctrl_in.unwrap().at, Point::raw(170_000, 70_000));
    // The next click leaves along the dragged handle.
    click(&mut s, Point::raw(300_000, 100_000), 2000);
    let (_, e) = only_path(&s);
    assert_eq!(
        e.node(NodeRef::new(0, 1)).unwrap().ctrl_out.unwrap().at,
        Point::raw(230_000, 130_000)
    );
}

#[test]
fn clicking_the_first_point_closes_and_fills_the_path() {
    let mut s = pen();
    for (i, (x, y)) in [(100_000, 100_000), (200_000, 100_000), (150_000, 200_000)]
        .into_iter()
        .enumerate()
    {
        click(&mut s, Point::raw(x, y), 1000 * i as u64);
    }
    click(&mut s, Point::raw(100_000, 100_000), 5000);
    assert_eq!(s.undo_label(), Some("Close Path"));
    let (n, e) = only_path(&s);
    assert!(e.subpaths[0].closed);
    assert_eq!(e.subpaths[0].nodes.len(), 3);
    assert!(matches!(s.doc.tree.kind(n), Some(NodeKind::Path(p)) if p.filled));
    assert!(selected_points(&s, n).is_empty(), "closing finishes");
}

#[test]
fn escape_drops_the_start_point_and_a_drag_in_flight() {
    let mut s = pen();
    let before = s.doc.canonical_digest();
    click(&mut s, Point::raw(100_000, 100_000), 0);
    s.apply(Intent::Cancel).unwrap();
    // The start was dropped: this click is a new start, not a segment.
    click(&mut s, Point::raw(200_000, 100_000), 1000);
    assert_eq!(s.doc.canonical_digest(), before);
    press(&mut s, Point::raw(300_000, 100_000), 2000);
    move_to(&mut s, Point::raw(330_000, 150_000));
    s.apply(Intent::Cancel).unwrap();
    release(&mut s, Point::raw(330_000, 150_000), 2100);
    assert_eq!(s.doc.canonical_digest(), before);
    assert_eq!(s.bus.history().len(), 0);
}

#[test]
fn the_pen_continues_a_selected_open_end() {
    let (mut s, n) = fixture();
    s.apply(Intent::ChooseTool(ToolId::Pen)).unwrap();
    // Pick up the last end, then extend it.
    click(&mut s, Point::raw(300_000, 300_000), 0);
    assert_eq!(selected_points(&s, n), vec![5]);
    click(&mut s, Point::raw(400_000, 300_000), 1000);
    assert_eq!(s.undo_label(), Some("Add Segment"));
    assert_eq!(nodes(&s, n).subpaths[0].nodes.len(), 5);
    // And from the first end: the new node goes before it.
    s.apply(Intent::Cancel).unwrap();
    click(&mut s, Point::raw(100_000, 100_000), 5000);
    click(&mut s, Point::raw(50_000, 100_000), 6000);
    let e = nodes(&s, n);
    assert_eq!(e.subpaths[0].nodes.len(), 6);
    assert_eq!(e.subpaths[0].nodes[0].at, Point::raw(50_000, 100_000));
}

#[test]
fn a_selector_double_click_on_a_path_opens_the_shape_editor() {
    let (mut s, _) = fixture();
    s.apply(Intent::ChooseTool(ToolId::Selector)).unwrap();
    let p = Point::raw(150_000, 100_000);
    click(&mut s, p, 0);
    click(&mut s, p, 100);
    assert_eq!(s.tools().current(), ToolId::ShapeEditor);
}
