//! The fill and transparency tools (XARA-US-0039), driven through intents
//! exactly as the shell drives them.

use xarast_app::fill_tool::{FillCommand, TRANSP_MODES};
use xarast_app::shapes::rectangle;
use xarast_app::{
    AppCommand, DevicePoint, DocumentId, EditCommand, HandleShape, InfobarField, InfobarItem,
    InfobarValue, Intent, Modifiers, OverlayShape, PointerButton, PointerSample, Session, ToolId,
};
use xarast_color::{Colour, ColourValue, TranspMode, Transparency};
use xarast_doc::fill::{FillGeometry, Paint, TranspPaint};
use xarast_doc::fill_edit::{FillChannel, FillValue, PaintSlot, fill_in_force};
use xarast_doc::{AttrValue, NodeId, SetFillGeometry};
use xarast_geom::Point;

const RED: ColourValue = ColourValue::Rgbt {
    r: 1.0,
    g: 0.0,
    b: 0.0,
    t: 0.0,
};

fn flat(c: ColourValue) -> AttrValue {
    AttrValue::Fill(FillGeometry::Flat {
        value: Colour::Direct(c),
    })
}

/// A session with rectangles centred at the given points (100 × 60 pt),
/// each filled flat red, all selected, the given tool in force.
fn fixture(centres: &[(i32, i32)], tool: ToolId) -> (Session, Vec<NodeId>) {
    let mut s = Session::new_empty(DocumentId(1));
    let spread = s.doc.active_spread();
    let layer = s.doc.active_layer(spread).unwrap();
    for &(x, y) in centres {
        s.apply_edit(EditCommand::CreateShape {
            layer,
            shape: Box::new(rectangle(Point::raw(x, y), 50_000.0, 30_000.0)),
            attrs: vec![flat(RED)],
        })
        .unwrap();
    }
    s.bus.history_mut().clear(&mut s.doc);
    let nodes: Vec<NodeId> = xarast_app::edit::selectable_objects(&s.doc).collect();
    s.apply(Intent::Select {
        nodes: nodes.clone(),
        mode: xarast_app::SelectMode::Replace,
    })
    .unwrap();
    s.apply(Intent::ChooseTool(tool)).unwrap();
    // Leave room around the objects at 100 %.
    (s, nodes)
}

fn colour_fill(s: &Session, n: NodeId) -> Paint {
    match fill_in_force(&s.doc, n, PaintSlot::Fill, FillChannel::Colour) {
        FillValue::Colour(g) => g,
        FillValue::Transparency(_) => unreachable!(),
    }
}

fn transp_fill(s: &Session, n: NodeId) -> TranspPaint {
    match fill_in_force(&s.doc, n, PaintSlot::Fill, FillChannel::Transparency) {
        FillValue::Transparency(g) => g,
        FillValue::Colour(_) => unreachable!(),
    }
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

/// Presses at `from`, moves in ten steps to `to`, and stops before the
/// release.
fn drag_open(s: &mut Session, from: Point, to: Point, t: u64) {
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
}

fn drag(s: &mut Session, from: Point, to: Point, t: u64) {
    drag_open(s, from, to, t);
    release(s, to, t + 10);
}

/// A headless render at draft quality: a previewed gradient's ramp is
/// always a draft-length table (T8.5.3), so comparing the preview with the
/// committed result is exact only at draft quality.
fn pixels(s: &Session) -> Vec<u8> {
    let r = xarast_app::headless::render(
        s,
        &xarast_app::HeadlessOptions {
            size: xarast_app::DeviceSize::new(160, 120),
            frame: xarast_app::HeadlessFrame::Fit(xarast_geom::Rect::new(
                Point::raw(120_000, 140_000),
                Point::raw(280_000, 260_000),
            )),
            quality: xarast_render::RenderQuality::Draft,
            ..xarast_app::HeadlessOptions::default()
        },
    )
    .unwrap();
    r.surface.data().to_vec()
}

fn handles(s: &Session) -> Vec<(Point, HandleShape)> {
    s.overlay()
        .into_iter()
        .filter_map(|o| match o {
            OverlayShape::Handle { at, shape } => Some((at, shape)),
            _ => None,
        })
        .collect()
}

fn arrows(s: &Session) -> usize {
    s.overlay()
        .iter()
        .filter(|o| matches!(o, OverlayShape::Arrow { .. }))
        .count()
}

fn infobar_choice(s: &Session, field: InfobarField) -> Option<usize> {
    s.infobar().items.iter().find_map(|i| match i {
        InfobarItem::Choice {
            field: f, selected, ..
        } if *f == field => *selected,
        _ => None,
    })
}

#[test]
fn dragging_across_a_flat_object_makes_one_linear_fill_step() {
    let (mut s, nodes) = fixture(&[(200_000, 200_000)], ToolId::Fill);
    let n = nodes[0];
    assert!(handles(&s).is_empty(), "a flat fill has no handles");
    let (a, b) = (Point::raw(160_000, 200_000), Point::raw(240_000, 200_000));
    drag(&mut s, a, b, 0);
    assert_eq!(s.bus.history().len(), 1);
    assert_eq!(s.undo_label(), Some("Set Fill"));
    let FillGeometry::Linear {
        start,
        end,
        from,
        to,
        ..
    } = colour_fill(&s, n)
    else {
        panic!("not linear: {:?}", colour_fill(&s, n));
    };
    assert!(start.distance_to(a) < 2_000.0 && end.distance_to(b) < 2_000.0);
    assert_eq!(from, Colour::Direct(RED));
    assert_ne!(to, from, "the far end is the desaturated colour");
    // The overlay now shows the arm and its two blobs, the end selected.
    assert_eq!(arrows(&s), 1);
    let h = handles(&s);
    assert_eq!(h.len(), 2);
    assert!(h.iter().any(|(_, k)| *k == HandleShape::FillBlobSelected));
    s.undo();
    assert!(matches!(colour_fill(&s, n), FillGeometry::Flat { .. }));
}

#[test]
fn a_handle_drag_previews_without_touching_the_document_and_commits_once() {
    let (mut s, nodes) = fixture(&[(200_000, 200_000)], ToolId::Fill);
    let n = nodes[0];
    drag(
        &mut s,
        Point::raw(160_000, 200_000),
        Point::raw(240_000, 200_000),
        0,
    );
    let before = s.doc.canonical_digest();
    let before_px = pixels(&s);
    let len = s.bus.history().len();
    let end = Point::raw(240_000, 200_000);
    let new_end = Point::raw(240_000, 230_000);
    s.rebuild_scene(None).expect("the scene at rest builds");
    let at_rest = s.resolver().ramps.bytes();
    assert_eq!(at_rest, 2048 * 4, "at rest, one final-length ramp");
    drag_open(&mut s, end, new_end, 100);
    assert_eq!(
        s.doc.canonical_digest(),
        before,
        "nothing is emitted mid-drag"
    );
    assert_eq!(s.preview().attrs.len(), 1, "the walker draws the new fill");
    assert!(matches!(
        &s.preview().attrs[0],
        (m, AttrValue::Fill(FillGeometry::Linear { end, .. })) if *m == n && end.distance_to(new_end) < 2_000.0
    ));
    // The handles follow the pointer while dragging.
    assert!(
        handles(&s)
            .iter()
            .any(|(p, _)| p.distance_to(new_end) < 2_000.0)
    );
    s.rebuild_scene(None).expect("the previewed scene builds");
    // The session walks at final quality, but the dragged fill's new ramp
    // is a draft table: 256 entries, not 2048.
    assert_eq!(s.resolver().ramps.bytes(), at_rest + 256 * 4);
    let previewed = pixels(&s);
    assert!(previewed != before_px, "the preview repaints the fill");
    release(&mut s, new_end, 200);
    assert!(
        pixels(&s) == previewed,
        "what was previewed is what commits"
    );
    assert!(s.preview().is_empty());
    assert_eq!(s.bus.history().len(), len + 1, "one step");
    assert_eq!(s.undo_label(), Some("Move Fill Handle"));
    let FillGeometry::Linear { end, .. } = colour_fill(&s, n) else {
        panic!()
    };
    assert!(end.distance_to(new_end) < 2_000.0);
    s.undo();
    assert_eq!(s.doc.canonical_digest(), before, "undo is exact");
}

#[test]
fn escape_mid_drag_leaves_the_document_and_the_history_untouched() {
    let (mut s, _) = fixture(&[(200_000, 200_000)], ToolId::Fill);
    drag(
        &mut s,
        Point::raw(160_000, 200_000),
        Point::raw(240_000, 200_000),
        0,
    );
    let before = s.doc.canonical_digest();
    let len = s.bus.history().len();
    drag_open(
        &mut s,
        Point::raw(240_000, 200_000),
        Point::raw(260_000, 260_000),
        100,
    );
    assert!(!s.preview().is_empty());
    s.apply(Intent::Cancel).unwrap();
    assert!(s.preview().is_empty());
    assert_eq!(s.doc.canonical_digest(), before);
    assert_eq!(s.bus.history().len(), len);
    // A new gradient dragged out and cancelled leaves nothing either.
    drag_open(
        &mut s,
        Point::raw(180_000, 190_000),
        Point::raw(220_000, 210_000),
        300,
    );
    s.apply(Intent::Cancel).unwrap();
    assert_eq!(s.doc.canonical_digest(), before);
    assert_eq!(s.bus.history().len(), len);
}

#[test]
fn a_double_click_on_the_arm_inserts_a_stop_and_delete_removes_it() {
    let (mut s, nodes) = fixture(&[(200_000, 200_000)], ToolId::Fill);
    let n = nodes[0];
    drag(
        &mut s,
        Point::raw(150_000, 200_000),
        Point::raw(250_000, 200_000),
        0,
    );
    let on_arm = Point::raw(200_000, 200_000);
    click(&mut s, on_arm, 1_000);
    click(&mut s, on_arm, 1_100);
    assert_eq!(s.undo_label(), Some("Add Fill Stop"));
    let FillGeometry::Linear { ramp, .. } = colour_fill(&s, n) else {
        panic!()
    };
    assert_eq!(ramp.stops().len(), 1);
    assert!((ramp.stops()[0].pos - 0.5).abs() < 0.02);
    // The new stop is the selected handle; the infobar shows its position.
    assert!(
        handles(&s)
            .iter()
            .any(|(_, k)| *k == HandleShape::FillStopSelected)
    );
    assert!(s.infobar().items.iter().any(|i| matches!(
        i,
        InfobarItem::Real { field: InfobarField::StopPosition, value: Some(v), .. } if (v - 50.0).abs() < 2.0
    )));
    // Drag the stop along the arm: one "Move Fill Stop" step.
    drag(&mut s, on_arm, Point::raw(225_000, 205_000), 2_000);
    assert_eq!(s.undo_label(), Some("Move Fill Stop"));
    let FillGeometry::Linear { ramp, .. } = colour_fill(&s, n) else {
        panic!()
    };
    assert!((ramp.stops()[0].pos - 0.75).abs() < 0.02, "{ramp:?}");
    s.apply(AppCommand::Delete.intent(DevicePoint::new(0.0, 0.0)))
        .unwrap();
    assert_eq!(s.undo_label(), Some("Delete Fill Stop"));
    let FillGeometry::Linear { ramp, .. } = colour_fill(&s, n) else {
        panic!()
    };
    assert!(ramp.is_empty());
    assert!(
        s.doc.tree.contains(n),
        "Delete took the stop, not the object"
    );
}

#[test]
fn objects_sharing_a_fill_show_one_handle_set_and_move_together() {
    let (mut s, nodes) = fixture(&[(200_000, 200_000), (400_000, 200_000)], ToolId::Fill);
    // The same linear fill on both.
    let shared = FillGeometry::Linear {
        start: Point::raw(150_000, 300_000),
        end: Point::raw(450_000, 300_000),
        persp: None,
        from: Colour::Direct(RED),
        to: Colour::Direct(ColourValue::WHITE),
        ramp: xarast_doc::fill::Ramp::new(),
    };
    s.apply_edit(EditCommand::Fill {
        edits: nodes
            .iter()
            .map(|&node| {
                FillCommand::SetGeometry(SetFillGeometry {
                    node,
                    slot: PaintSlot::Fill,
                    value: FillValue::Colour(shared.clone()),
                })
            })
            .collect(),
    })
    .unwrap();
    assert_eq!(arrows(&s), 1, "one handle set for two objects");
    assert_eq!(handles(&s).len(), 2);
    let len = s.bus.history().len();
    drag(
        &mut s,
        Point::raw(450_000, 300_000),
        Point::raw(500_000, 350_000),
        0,
    );
    assert_eq!(s.bus.history().len(), len + 1, "one step for both");
    for &n in &nodes {
        let FillGeometry::Linear { end, .. } = colour_fill(&s, n) else {
            panic!()
        };
        assert!(end.distance_to(Point::raw(500_000, 350_000)) < 2_000.0);
    }
    // Change one: now two sets, two arms.
    s.apply(Intent::Select {
        nodes: vec![nodes[0]],
        mode: xarast_app::SelectMode::Replace,
    })
    .unwrap();
    drag(
        &mut s,
        Point::raw(500_000, 350_000),
        Point::raw(520_000, 350_000),
        100,
    );
    s.apply(Intent::Select {
        nodes: nodes.clone(),
        mode: xarast_app::SelectMode::Replace,
    })
    .unwrap();
    assert_eq!(arrows(&s), 2, "different fills, a handle set each");
}

#[test]
fn the_type_menu_mutates_the_fill_and_constrain_turns_in_15_degree_steps() {
    let (mut s, nodes) = fixture(&[(200_000, 200_000)], ToolId::Fill);
    let n = nodes[0];
    assert_eq!(infobar_choice(&s, InfobarField::FillType), Some(0), "flat");
    s.apply(Intent::InfobarEdit {
        field: InfobarField::FillType,
        value: InfobarValue::Choice(2),
    })
    .unwrap();
    assert_eq!(s.undo_label(), Some("Fill Type"));
    assert!(matches!(
        colour_fill(&s, n),
        FillGeometry::Radial {
            aspect_locked: true,
            ..
        }
    ));
    assert_eq!(infobar_choice(&s, InfobarField::FillType), Some(2));
    // Linear, then drag its end with Constrain: the arm snaps to 15°.
    s.apply(Intent::InfobarEdit {
        field: InfobarField::FillType,
        value: InfobarValue::Choice(1),
    })
    .unwrap();
    let FillGeometry::Linear { start, end, .. } = colour_fill(&s, n) else {
        panic!()
    };
    s.apply(Intent::ModifiersChanged(Modifiers {
        constrain: true,
        ..Modifiers::default()
    }))
    .unwrap();
    let (sx, sy) = start.to_f64();
    let target = Point::from_f64_round(sx + 80_000.0, sy + 23_000.0);
    drag(&mut s, end, target, 0);
    let FillGeometry::Linear { start, end, .. } = colour_fill(&s, n) else {
        panic!()
    };
    let (dx, dy) = ((end.x - start.x).to_f64(), (end.y - start.y).to_f64());
    let deg = dy.atan2(dx).to_degrees();
    assert!((deg / 15.0 - (deg / 15.0).round()).abs() < 0.01, "{deg}");
}

#[test]
fn the_profile_sliders_and_effect_write_their_attributes() {
    let (mut s, nodes) = fixture(&[(200_000, 200_000)], ToolId::Fill);
    let n = nodes[0];
    drag(
        &mut s,
        Point::raw(160_000, 200_000),
        Point::raw(240_000, 200_000),
        0,
    );
    s.apply(Intent::InfobarEdit {
        field: InfobarField::ProfileBias,
        value: InfobarValue::Real(0.4),
    })
    .unwrap();
    assert!((colour_fill(&s, n).profile().bias - 0.4).abs() < 1e-9);
    assert_eq!(s.undo_label(), Some("Fill Profile"));
    s.apply(Intent::InfobarEdit {
        field: InfobarField::FillEffect,
        value: InfobarValue::Choice(1),
    })
    .unwrap();
    assert_eq!(infobar_choice(&s, InfobarField::FillEffect), Some(1));
    s.apply(Intent::InfobarEdit {
        field: InfobarField::FillTiling,
        value: InfobarValue::Choice(1),
    })
    .unwrap();
    assert_eq!(infobar_choice(&s, InfobarField::FillTiling), Some(1));
    s.apply(Intent::InfobarEdit {
        field: InfobarField::RampMapping,
        value: InfobarValue::Choice(1),
    })
    .unwrap();
    assert_eq!(
        colour_fill(&s, n).mapping(),
        xarast_doc::fill::RampMapping::Sin
    );
}

#[test]
fn the_transparency_tool_is_the_same_machine_over_transparency() {
    let (mut s, nodes) = fixture(&[(200_000, 200_000)], ToolId::Transparency);
    let n = nodes[0];
    drag(
        &mut s,
        Point::raw(160_000, 200_000),
        Point::raw(240_000, 200_000),
        0,
    );
    assert_eq!(s.undo_label(), Some("Set Transparency"));
    let FillGeometry::Linear { from, to, .. } = transp_fill(&s, n) else {
        panic!("{:?}", transp_fill(&s, n))
    };
    assert_eq!(from.level, 0, "opaque at the start");
    assert_eq!(to.level, 255, "clear at the end");
    assert!(
        matches!(colour_fill(&s, n), FillGeometry::Flat { .. }),
        "the colour fill is untouched"
    );
    // The blend-mode selector.
    let stained = TRANSP_MODES
        .iter()
        .position(|m| *m == TranspMode::StainedGlass)
        .unwrap();
    s.apply(Intent::InfobarEdit {
        field: InfobarField::TranspMode,
        value: InfobarValue::Choice(stained),
    })
    .unwrap();
    assert_eq!(s.undo_label(), Some("Transparency Type"));
    let FillGeometry::Linear { from, to, .. } = transp_fill(&s, n) else {
        panic!()
    };
    assert_eq!(
        (from.mode, to.mode),
        (TranspMode::StainedGlass, TranspMode::StainedGlass)
    );
    assert_eq!(infobar_choice(&s, InfobarField::TranspMode), Some(stained));
    // The selected end handle's level, typed in per cent.
    s.apply(Intent::InfobarEdit {
        field: InfobarField::StopLevel,
        value: InfobarValue::Real(50.0),
    })
    .unwrap();
    let FillGeometry::Linear { to, .. } = transp_fill(&s, n) else {
        panic!()
    };
    assert_eq!(
        to,
        Transparency {
            level: 128,
            mode: TranspMode::StainedGlass
        }
    );
}

#[test]
fn shift_drags_out_a_circular_fill_and_a_click_elsewhere_selects() {
    let (mut s, nodes) = fixture(&[(200_000, 200_000), (400_000, 200_000)], ToolId::Fill);
    s.apply(Intent::ModifiersChanged(Modifiers {
        adjust: true,
        ..Modifiers::default()
    }))
    .unwrap();
    drag(
        &mut s,
        Point::raw(200_000, 200_000),
        Point::raw(230_000, 200_000),
        0,
    );
    s.apply(Intent::ModifiersChanged(Modifiers::default()))
        .unwrap();
    for &n in &nodes {
        assert!(matches!(
            colour_fill(&s, n),
            FillGeometry::Radial {
                aspect_locked: true,
                ..
            }
        ));
    }
    // A click on one object away from the handles selects it alone.
    click(&mut s, Point::raw(430_000, 215_000), 1_000);
    assert_eq!(s.edit.selection().collect::<Vec<_>>(), vec![nodes[1]]);
    // A click on nothing selects nothing.
    click(&mut s, Point::raw(700_000, 700_000), 2_000);
    assert_eq!(s.edit.selection().count(), 0);
}

#[test]
fn an_object_with_no_attribute_of_its_own_previews_as_it_commits() {
    // A leaf: no attribute children, everything inherited.
    let mut s = Session::new_empty(DocumentId(1));
    let spread = s.doc.active_spread();
    let layer = s.doc.active_layer(spread).unwrap();
    s.apply_edit(EditCommand::CreateShape {
        layer,
        shape: Box::new(rectangle(Point::raw(200_000, 200_000), 50_000.0, 30_000.0)),
        attrs: Vec::new(),
    })
    .unwrap();
    let n = xarast_app::edit::selectable_objects(&s.doc).next().unwrap();
    assert!(s.doc.tree.links(n).first_child.is_none(), "a leaf");
    s.apply(Intent::Select {
        nodes: vec![n],
        mode: xarast_app::SelectMode::Replace,
    })
    .unwrap();
    s.apply(Intent::ChooseTool(ToolId::Fill)).unwrap();
    let before = pixels(&s);
    drag_open(
        &mut s,
        Point::raw(160_000, 200_000),
        Point::raw(240_000, 200_000),
        0,
    );
    let previewed = pixels(&s);
    assert!(previewed != before, "{:?}", s.preview());
    release(&mut s, Point::raw(240_000, 200_000), 10);
    assert!(pixels(&s) == previewed);
}

// ───────────────────── XARA-T-0220: the follow-ups ─────────────────────

/// Gives every node its own copy of `g` as its fill of `slot`.
fn set_paint(s: &mut Session, nodes: &[NodeId], slot: PaintSlot, g: &Paint) {
    s.apply_edit(EditCommand::Fill {
        edits: nodes
            .iter()
            .map(|&node| {
                FillCommand::SetGeometry(SetFillGeometry {
                    node,
                    slot,
                    value: FillValue::Colour(g.clone()),
                })
            })
            .collect(),
    })
    .unwrap();
}

fn set_fill(s: &mut Session, nodes: &[NodeId], g: &Paint) {
    set_paint(s, nodes, PaintSlot::Fill, g);
}

fn ellipse_fill() -> Paint {
    FillGeometry::Radial {
        centre: Point::raw(200_000, 200_000),
        major: Point::raw(240_000, 200_000),
        minor: Point::raw(200_000, 220_000),
        aspect_locked: false,
        persp: None,
        from: Colour::Direct(RED),
        to: Colour::Direct(ColourValue::WHITE),
        ramp: xarast_doc::fill::Ramp::new(),
    }
}

fn hold(s: &mut Session, m: Modifiers) {
    s.apply(Intent::ModifiersChanged(m)).unwrap();
}

/// Drags with `m` held, checking that nothing is written before the
/// release, that the release is one step whose render is the preview's,
/// and that undo is exact. Returns the fill after the release (redone).
fn locked_drag(s: &mut Session, n: NodeId, from: Point, to: Point, m: Modifiers) -> Paint {
    let before = s.doc.canonical_digest();
    let len = s.bus.history().len();
    hold(s, m);
    drag_open(s, from, to, 10_000);
    assert_eq!(s.doc.canonical_digest(), before, "nothing before release");
    let previewed = pixels(s);
    release(s, to, 10_100);
    hold(s, Modifiers::default());
    assert!(pixels(s) == previewed, "what was previewed is what commits");
    assert_eq!(s.bus.history().len(), len + 1, "one step");
    assert_eq!(s.undo_label(), Some("Move Fill Handle"));
    let after = colour_fill(s, n);
    s.undo();
    assert_eq!(s.doc.canonical_digest(), before, "undo is exact");
    s.redo();
    after
}

#[test]
fn constrain_keeps_a_centre_on_an_axis_and_adjust_locks_the_aspect() {
    let (mut s, nodes) = fixture(&[(200_000, 200_000)], ToolId::Fill);
    let n = nodes[0];
    set_fill(&mut s, &nodes, &ellipse_fill());
    let constrain = Modifiers {
        constrain: true,
        ..Modifiers::default()
    };
    // The axis lock: a centre dragged mostly right stays on its row.
    let g = locked_drag(
        &mut s,
        n,
        Point::raw(200_000, 200_000),
        Point::raw(230_000, 207_000),
        constrain,
    );
    let FillGeometry::Radial {
        centre,
        major,
        minor,
        ..
    } = g
    else {
        panic!("{g:?}")
    };
    assert_eq!(centre.y, Point::raw(0, 200_000).y, "{centre:?}");
    assert!((centre.x.to_f64() - 230_000.0).abs() < 1_000.0);
    // The whole fill moved with it.
    assert_eq!(
        major - centre,
        Point::raw(240_000, 200_000) - Point::raw(200_000, 200_000)
    );
    assert_eq!(
        minor - centre,
        Point::raw(200_000, 220_000) - Point::raw(200_000, 200_000)
    );

    // The aspect lock: the major axis turned a quarter and stretched by
    // half; the minor axis turns with it on its side and stretches too.
    let (mut s, nodes) = fixture(&[(200_000, 200_000)], ToolId::Fill);
    let n = nodes[0];
    set_fill(&mut s, &nodes, &ellipse_fill());
    let adjust = Modifiers {
        adjust: true,
        ..Modifiers::default()
    };
    let g = locked_drag(
        &mut s,
        n,
        Point::raw(240_000, 200_000),
        Point::raw(200_000, 260_000),
        adjust,
    );
    let FillGeometry::Radial { major, minor, .. } = g else {
        panic!("{g:?}")
    };
    assert!(
        major.distance_to(Point::raw(200_000, 260_000)) < 1_000.0,
        "{major:?}"
    );
    assert!(
        minor.distance_to(Point::raw(170_000, 200_000)) < 1_000.0,
        "{minor:?}"
    );

    // Without Adjust the minor axis stays where it was.
    let (mut s, nodes) = fixture(&[(200_000, 200_000)], ToolId::Fill);
    let n = nodes[0];
    set_fill(&mut s, &nodes, &ellipse_fill());
    let g = locked_drag(
        &mut s,
        n,
        Point::raw(240_000, 200_000),
        Point::raw(200_000, 260_000),
        Modifiers::default(),
    );
    let FillGeometry::Radial { minor, .. } = g else {
        panic!("{g:?}")
    };
    assert_eq!(minor, Point::raw(200_000, 220_000));
}
