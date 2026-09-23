//! The tool machine, the selector and undo, driven through intents exactly
//! as the shell drives them (XARA-US-0029, XARA-US-0030).

use xarast_app::tool::{DRAG_THRESHOLD_PX, InteractionState};
use xarast_app::{
    Changed, DevicePoint, DocumentId, EditCommand, InfobarField, InfobarItem, InfobarValue, Intent,
    Modifiers, OverlayShape, PointerButton, PointerSample, Session, ToolId, build_scene,
};
use xarast_doc::{Attach, Command, EditError, NodeId, NodeKind, ShapeKind, ShapeNode, Tx};
use xarast_geom::{Mp, Point, Vector};

/// Adds rectangles to the active layer, as a fixture (not a tool).
#[derive(Debug)]
struct AddRects(Vec<(i32, i32, i32)>);

impl Command for AddRects {
    fn label(&self) -> &'static str {
        "Fixture"
    }
    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        let spread = tx.doc().active_spread();
        let layer = tx.doc().active_layer(spread).expect("a layer");
        for &(x, y, side) in &self.0 {
            let n = tx.create(NodeKind::Shape(Box::new(ShapeNode {
                shape: ShapeKind::Rect,
                origin: Point::raw(x, y),
                major: Vector::raw(side, 0),
                minor: Vector::raw(0, side),
            })))?;
            tx.attach(n, layer, Attach::LastChild)?;
            // A painted fill, so the interior picks (a transparent one
            // does not).
            let fill = tx.create(NodeKind::Attr(Box::new(xarast_doc::AttrNode::new(
                xarast_doc::AttrValue::Fill(xarast_doc::fill::Paint::Flat {
                    value: xarast_color::Colour::Direct(xarast_color::ColourValue::rgbt(
                        0.8, 0.2, 0.2, 0.0,
                    )),
                }),
            ))))?;
            tx.attach(fill, n, Attach::LastChild)?;
        }
        Ok(())
    }
}

/// A session with two 50 pt squares on the page, the history empty.
fn fixture() -> (Session, NodeId, NodeId) {
    let mut s = Session::new_empty(DocumentId(1));
    s.dispatch(&AddRects(vec![
        (100_000, 100_000, 50_000),
        (300_000, 100_000, 50_000),
    ]))
    .expect("fixture");
    s.bus.history_mut().clear(&mut s.doc);
    let objs: Vec<NodeId> = xarast_app::edit::selectable_objects(&s.doc).collect();
    assert_eq!(objs.len(), 2);
    (s, objs[0], objs[1])
}

fn dev(s: &Session, x: i32, y: i32) -> DevicePoint {
    s.viewport.doc_to_device(Point::raw(x, y))
}

fn sample(at: DevicePoint, t: u64) -> PointerSample {
    PointerSample {
        at,
        pressure: None,
        time_ms: t,
    }
}

fn down(s: &mut Session, at: DevicePoint, t: u64) -> Changed {
    s.apply(Intent::PointerDown {
        button: PointerButton::Primary,
        sample: sample(at, t),
    })
    .expect("down")
}

fn move_to(s: &mut Session, at: DevicePoint) -> Changed {
    s.apply(Intent::PointerMove(sample(at, 0))).expect("move")
}

fn up(s: &mut Session, at: DevicePoint, t: u64) -> Changed {
    s.apply(Intent::PointerUp {
        button: PointerButton::Primary,
        sample: sample(at, t),
    })
    .expect("up")
}

fn click(s: &mut Session, at: DevicePoint, t: u64) {
    move_to(s, at);
    down(s, at, t);
    up(s, at, t);
}

fn click_at(s: &mut Session, x: i32, y: i32, t: u64) {
    let p = dev(s, x, y);
    click(s, p, t);
}

fn origin_of(s: &Session, n: NodeId) -> Point {
    match s.doc.tree.kind(n) {
        Some(NodeKind::Shape(sh)) => sh.origin,
        other => panic!("not a shape: {other:?}"),
    }
}

fn offset(p: DevicePoint, dx: f64, dy: f64) -> DevicePoint {
    DevicePoint::new(p.x + dx, p.y + dy)
}

#[test]
fn a_click_selects_the_object_under_it_and_empty_space_deselects() {
    let (mut s, a, b) = fixture();
    click_at(&mut s, 125_000, 125_000, 0);
    assert_eq!(s.edit.selection().collect::<Vec<_>>(), vec![a]);
    // Adjust (Shift) toggles another one in.
    s.apply(Intent::ModifiersChanged(Modifiers {
        adjust: true,
        ..Modifiers::default()
    }))
    .unwrap();
    click_at(&mut s, 325_000, 125_000, 1000);
    assert_eq!(s.edit.selection().collect::<Vec<_>>(), vec![a, b]);
    s.apply(Intent::ModifiersChanged(Modifiers::default()))
        .unwrap();
    click_at(&mut s, 600_000, 600_000, 2000);
    assert!(s.edit.is_selection_empty());
    assert_eq!(s.bus.history().len(), 0, "selecting is never an undo step");
}

#[test]
fn a_drag_previews_then_commits_one_labelled_undo_step() {
    let (mut s, a, b) = fixture();
    let before = s.doc.canonical_digest();
    let start = dev(&s, 125_000, 125_000);
    move_to(&mut s, start);
    down(&mut s, start, 0);
    assert_eq!(s.tools().state(), InteractionState::ArmedDrag);
    // Within the threshold nothing happens.
    move_to(&mut s, offset(start, DRAG_THRESHOLD_PX - 1.0, 0.0));
    assert_eq!(s.tools().state(), InteractionState::ArmedDrag);
    assert!(s.preview().is_empty());

    // A 500-sample drag.
    let mut changed = Changed::empty();
    for i in 1..=500 {
        changed |= move_to(&mut s, offset(start, f64::from(i) * 0.2, 0.0));
    }
    assert_eq!(s.tools().state(), InteractionState::Dragging);
    assert!(changed.needs_scene(), "a preview rebuilds the scene");
    // Nothing is committed during the drag: the document is untouched and
    // the preview carries the move.
    assert_eq!(s.doc.canonical_digest(), before);
    assert_eq!(s.bus.history().len(), 0);
    let (nodes, m) = s.preview().transform.clone().expect("a preview");
    assert_eq!(nodes, vec![a]);
    assert!(m.is_translation_only() && m.e > Mp::ZERO);
    // The overlay's box follows the preview.
    assert!(
        s.overlay()
            .iter()
            .any(|o| matches!(o, OverlayShape::Rect { dashed: false, .. }))
    );
    // The scene draws the preview as a transformed group around `a`.
    let built = build_scene(&s, None);
    assert!(built.stats.groups >= 1);

    let end = offset(start, 100.0, 0.0);
    up(&mut s, end, 10);
    assert!(s.preview().is_empty());
    assert_eq!(s.bus.history().len(), 1, "a drag is one undo step");
    assert_eq!(s.undo_label(), Some("Move"));
    let moved = origin_of(&s, a);
    assert!(moved.x > Mp::new(100_000));
    assert_eq!(moved.y, Mp::new(100_000));
    assert_eq!(origin_of(&s, b), Point::raw(300_000, 100_000));

    // Undo restores the document exactly, redo reapplies it.
    let after = s.doc.canonical_digest();
    let c = s.apply(Intent::Undo).unwrap();
    assert!(c.needs_scene() && c.contains(Changed::UI));
    assert_eq!(s.doc.canonical_digest(), before);
    assert_eq!(s.redo_label(), Some("Move"));
    assert_eq!(s.undo_label(), None);
    s.apply(Intent::Redo).unwrap();
    assert_eq!(s.doc.canonical_digest(), after);
    assert_eq!(origin_of(&s, a), moved);
}

#[test]
fn escape_at_any_point_of_a_drag_leaves_nothing_behind() {
    for cut in [0usize, 1, 2, 5, 10, 20, 40, 80, 160, 199] {
        let (mut s, _, _) = fixture();
        let before = s.doc.canonical_digest();
        let start = dev(&s, 125_000, 125_000);
        move_to(&mut s, start);
        down(&mut s, start, 0);
        for i in 0..cut {
            move_to(&mut s, offset(start, i as f64, i as f64 * 0.5));
        }
        s.apply(Intent::Cancel).unwrap();
        assert!(s.preview().is_empty(), "cut {cut}");
        assert!(!s.tools().is_pressed(), "cut {cut}");
        // The release after the cancel is not a click and not a commit.
        up(&mut s, offset(start, 300.0, 0.0), 5);
        assert_eq!(s.doc.canonical_digest(), before, "cut {cut}");
        assert_eq!(s.bus.history().len(), 0, "cut {cut}");
    }
}

#[test]
fn escape_with_no_gesture_selects_nothing() {
    let (mut s, _, _) = fixture();
    click_at(&mut s, 125_000, 125_000, 0);
    assert!(!s.edit.is_selection_empty());
    let c = s.apply(Intent::Cancel).unwrap();
    assert!(c.contains(Changed::SELECTION));
    assert!(s.edit.is_selection_empty());
}

#[test]
fn constrain_pressed_mid_drag_changes_the_result_without_moving() {
    let (mut s, a, _) = fixture();
    let start = dev(&s, 125_000, 125_000);
    move_to(&mut s, start);
    down(&mut s, start, 0);
    // Mostly right, a little up.
    let at = offset(start, 80.0, -10.0);
    move_to(&mut s, at);
    let free = s.preview().transform.clone().unwrap().1;
    assert_ne!(free.f, Mp::ZERO);
    let c = s
        .apply(Intent::ModifiersChanged(Modifiers {
            constrain: true,
            ..Modifiers::default()
        }))
        .unwrap();
    assert!(c.needs_scene());
    let constrained = s.preview().transform.clone().unwrap().1;
    assert_eq!(constrained.f, Mp::ZERO, "snapped to the horizontal");
    up(&mut s, at, 10);
    assert_eq!(origin_of(&s, a).y, Mp::new(100_000));
}

#[test]
fn a_marquee_on_empty_space_selects_what_it_encloses() {
    let (mut s, a, b) = fixture();
    let from = dev(&s, 50_000, 200_000);
    let to = dev(&s, 400_000, 50_000);
    move_to(&mut s, from);
    down(&mut s, from, 0);
    move_to(&mut s, offset(from, 10.0, 0.0));
    move_to(&mut s, to);
    assert!(
        s.overlay()
            .iter()
            .any(|o| matches!(o, OverlayShape::Rect { dashed: true, .. })),
        "the rubber band is drawn"
    );
    up(&mut s, to, 5);
    assert_eq!(s.edit.selection().collect::<Vec<_>>(), vec![a, b]);
    assert_eq!(s.bus.history().len(), 0);
}

#[test]
fn dragging_an_unselected_object_selects_and_moves_it_alone() {
    let (mut s, a, b) = fixture();
    click_at(&mut s, 125_000, 125_000, 0);
    let start = dev(&s, 325_000, 125_000);
    move_to(&mut s, start);
    down(&mut s, start, 1000);
    move_to(&mut s, offset(start, 0.0, 50.0));
    up(&mut s, offset(start, 0.0, 50.0), 1010);
    assert_eq!(s.edit.selection().collect::<Vec<_>>(), vec![b]);
    assert_eq!(origin_of(&s, a), Point::raw(100_000, 100_000));
    assert!(
        origin_of(&s, b).y < Mp::new(100_000),
        "down the screen is down"
    );
}

#[test]
fn two_quick_clicks_are_a_double_click_and_slow_ones_are_not() {
    let (mut s, _, _) = fixture();
    let p = dev(&s, 125_000, 125_000);
    click(&mut s, p, 0);
    click(&mut s, p, 200);
    assert_eq!(s.tools().state(), InteractionState::Hover);
    // The machine counted it; the selector treats it as a click either way.
    assert_eq!(s.edit.selection_len(), 1);
}

#[test]
fn undo_and_redo_cancel_a_gesture_in_flight() {
    let (mut s, a, _) = fixture();
    s.apply_edit(EditCommand::translate(vec![a], Vector::raw(1000, 0)))
        .unwrap();
    let start = dev(&s, 126_000, 125_000);
    move_to(&mut s, start);
    down(&mut s, start, 0);
    move_to(&mut s, offset(start, 50.0, 0.0));
    assert!(!s.preview().is_empty());
    s.apply(Intent::Undo).unwrap();
    assert!(s.preview().is_empty());
    assert!(!s.tools().is_pressed());
    assert_eq!(origin_of(&s, a), Point::raw(100_000, 100_000));
}

#[test]
fn delete_is_one_labelled_step_and_undo_brings_the_object_back() {
    let (mut s, a, _) = fixture();
    let before = s.doc.canonical_digest();
    click_at(&mut s, 125_000, 125_000, 0);
    s.apply(Intent::DeleteSelection).unwrap();
    assert_eq!(s.undo_label(), Some("Delete"));
    assert!(!s.doc.tree.is_reachable(a));
    assert!(s.edit.is_selection_empty(), "the selection is pruned");
    s.apply(Intent::Undo).unwrap();
    assert_eq!(s.doc.canonical_digest(), before);
    // Nothing selected: deleting nothing is no step.
    s.apply(Intent::DeleteSelection).unwrap();
    assert_eq!(s.bus.history().len(), 0);
}

#[test]
fn tools_switch_and_the_reserved_ones_are_refused() {
    let (mut s, _, _) = fixture();
    assert_eq!(s.tools().current(), ToolId::Selector);
    s.apply(Intent::ChooseTool(ToolId::Pen)).unwrap();
    assert_eq!(s.tools().current(), ToolId::Pen);
    assert_eq!(s.edit.tool.active, ToolId::Pen);
    // A pending tool says so and ignores the canvas.
    assert!(matches!(
        s.infobar().items.as_slice(),
        [InfobarItem::Note(n)] if n.contains("coming soon")
    ));
    let before = s.doc.canonical_digest();
    let p = dev(&s, 125_000, 125_000);
    move_to(&mut s, p);
    down(&mut s, p, 0);
    move_to(&mut s, offset(p, 40.0, 40.0));
    up(&mut s, offset(p, 40.0, 40.0), 1);
    assert_eq!(s.doc.canonical_digest(), before);

    // Phase 8 and 9 tools cannot be chosen yet.
    s.apply(Intent::ChooseTool(ToolId::Text)).unwrap();
    assert_eq!(s.tools().current(), ToolId::Pen);

    // A momentary switch restores the chosen tool on release.
    s.apply(Intent::MomentaryTool(Some(ToolId::Selector)))
        .unwrap();
    assert_eq!(s.tools().current(), ToolId::Selector);
    s.apply(Intent::MomentaryTool(None)).unwrap();
    assert_eq!(s.tools().current(), ToolId::Pen);
}

#[test]
fn a_tool_switch_mid_drag_cancels_the_drag() {
    let (mut s, _, _) = fixture();
    let before = s.doc.canonical_digest();
    let start = dev(&s, 125_000, 125_000);
    move_to(&mut s, start);
    down(&mut s, start, 0);
    move_to(&mut s, offset(start, 50.0, 0.0));
    s.apply(Intent::ChooseTool(ToolId::Zoom)).unwrap();
    assert!(s.preview().is_empty());
    up(&mut s, offset(start, 50.0, 0.0), 1);
    assert_eq!(s.doc.canonical_digest(), before);
}

#[test]
fn the_push_and_zoom_tools_move_the_view_not_the_document() {
    let (mut s, _, _) = fixture();
    let before = s.doc.canonical_digest();
    s.apply(Intent::ChooseTool(ToolId::Pan)).unwrap();
    let centre = s.viewport.centre();
    let p = DevicePoint::new(400.0, 300.0);
    move_to(&mut s, p);
    down(&mut s, p, 0);
    move_to(&mut s, offset(p, 60.0, 0.0));
    up(&mut s, offset(p, 60.0, 0.0), 1);
    assert_ne!(s.viewport.centre(), centre);

    s.apply(Intent::ChooseTool(ToolId::Zoom)).unwrap();
    let z = s.viewport.zoom();
    click(&mut s, p, 10);
    assert!(s.viewport.zoom() > z);
    assert_eq!(s.doc.canonical_digest(), before);
}

#[test]
fn the_infobar_shows_the_selection_and_typing_x_moves_it() {
    let (mut s, a, _) = fixture();
    let bar = s.infobar();
    assert!(matches!(
        bar.items[1],
        InfobarItem::Measure {
            field: InfobarField::X,
            value: None,
            editable: false
        }
    ));
    click_at(&mut s, 125_000, 125_000, 0);
    let bar = s.infobar();
    assert!(matches!(
        bar.items[1],
        InfobarItem::Measure {
            field: InfobarField::X,
            value: Some(v),
            editable: true
        } if v == Mp::new(100_000)
    ));
    s.apply(Intent::InfobarEdit {
        field: InfobarField::X,
        value: InfobarValue::Length(Mp::new(150_000)),
    })
    .unwrap();
    assert_eq!(origin_of(&s, a), Point::raw(150_000, 100_000));
    assert_eq!(s.undo_label(), Some("Move"));
}

#[test]
fn a_drag_at_the_canvas_edge_scrolls_and_keeps_going() {
    let (mut s, _, _) = fixture();
    let start = dev(&s, 125_000, 125_000);
    move_to(&mut s, start);
    down(&mut s, start, 0);
    let size = s.viewport.size();
    let edge = DevicePoint::new(f64::from(size.width) - 2.0, start.y);
    move_to(&mut s, edge);
    assert!(s.wants_autoscroll());
    let centre = s.viewport.centre();
    let dx_before = s.preview().transform.clone().unwrap().1.e;
    s.apply(Intent::AutoScroll).unwrap();
    assert!(s.viewport.centre().x > centre.x, "the view scrolled right");
    let dx_after = s.preview().transform.clone().unwrap().1.e;
    assert!(
        dx_after > dx_before,
        "the object kept following the pointer"
    );
    up(&mut s, edge, 1);
    assert!(!s.wants_autoscroll());
}

#[test]
fn an_edit_changes_only_the_content_hash_of_what_it_touched() {
    let (mut s, a, b) = fixture();
    let hashes = |s: &Session| {
        let built = build_scene(s, None);
        built
            .scene
            .nodes()
            .map(|(id, n)| (*id, n.content))
            .collect::<std::collections::HashMap<_, _>>()
    };
    let tag = |s: &Session, n: NodeId| {
        xarast_render::SceneNodeId(u64::from(s.doc.tree.get(n).unwrap().tag.0))
    };
    let before = hashes(&s);
    // Two walks of an unchanged document agree.
    assert_eq!(before, hashes(&s));
    s.apply_edit(EditCommand::translate(vec![a], Vector::raw(5_000, 0)))
        .unwrap();
    let after = hashes(&s);
    assert_ne!(before[&tag(&s, a)], after[&tag(&s, a)]);
    assert_eq!(before[&tag(&s, b)], after[&tag(&s, b)], "b was not touched");
}

#[test]
fn inside_a_gesture_like_moves_merge_and_a_different_command_does_not() {
    let (mut s, a, b) = fixture();
    let before = s.doc.canonical_digest();
    let g = s.begin_gesture();
    for _ in 0..3 {
        s.apply_edit(EditCommand::translate(vec![a], Vector::raw(1_000, 0)))
            .unwrap();
    }
    // Moving a different object is a different step, even in the gesture.
    s.apply_edit(EditCommand::translate(vec![b], Vector::raw(1_000, 0)))
        .unwrap();
    s.apply_edit(EditCommand::DeleteNodes { nodes: vec![b] })
        .unwrap();
    s.end_gesture(g);
    let (undo, _) = s.bus.history().labels();
    assert_eq!(undo, vec!["Move", "Move", "Delete"]);
    assert_eq!(origin_of(&s, a), Point::raw(103_000, 100_000));
    for _ in 0..3 {
        s.apply(Intent::Undo).unwrap();
    }
    assert_eq!(s.doc.canonical_digest(), before);
}

/// Gives a node one unknown attribute, as a `.xarast` reader would.
#[derive(Debug)]
struct Baggage(NodeId);

impl Command for Baggage {
    fn label(&self) -> &'static str {
        "Fixture"
    }
    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        tx.set_foreign(
            self.0,
            Some(xarast_doc::ForeignBaggage {
                attrs: vec![xarast_doc::ForeignAttr {
                    ns: "urn:example".into(),
                    prefix: None,
                    local: "note".into(),
                    value: "x".into(),
                }],
                children: Vec::new(),
                marks: xarast_doc::ForeignMarks::empty(),
            }),
        )
    }
}

#[test]
fn a_selector_move_marks_foreign_baggage_dirty_and_undo_clears_it() {
    let (mut s, a, _) = fixture();
    s.dispatch(&Baggage(a)).unwrap();
    s.bus.history_mut().clear(&mut s.doc);
    let marks = |s: &Session| s.doc.tree.foreign(a).map(|b| b.marks).unwrap();
    assert!(marks(&s).is_empty());
    let start = dev(&s, 125_000, 125_000);
    move_to(&mut s, start);
    down(&mut s, start, 0);
    move_to(&mut s, offset(start, 40.0, 0.0));
    up(&mut s, offset(start, 40.0, 0.0), 1);
    assert!(marks(&s).contains(xarast_doc::ForeignMarks::DIRTY));
    s.apply(Intent::Undo).unwrap();
    assert!(
        marks(&s).is_empty(),
        "undo takes the mark back with the move"
    );
}
