//! The colour editor's model (XARA-US-0041), driven through intents exactly
//! as the interface drives it.

use xarast_app::colour_editor::{
    ColourChange, ColourEditorOp as Op, ColourTarget, Derivation, PaletteCommand,
};
use xarast_app::shapes::rectangle;
use xarast_app::{DocumentId, EditCommand, Intent, Session};
use xarast_color::{Colour, ColourDef, ColourId, ColourModel, ColourValue};
use xarast_doc::fill::FillGeometry;
use xarast_doc::fill_edit::{FillChannel, FillValue, PaintSlot, fill_in_force};
use xarast_doc::{AttrValue, NodeId};
use xarast_geom::Point;

const FILL: ColourTarget = ColourTarget::Selection(PaintSlot::Fill);

fn flat(c: Colour) -> AttrValue {
    AttrValue::Fill(FillGeometry::Flat { value: c })
}

/// A session with `n` rectangles filled with `fill`, all selected, and a
/// clean history.
fn fixture(n: usize, fill: Colour) -> (Session, Vec<NodeId>) {
    let mut s = Session::new_empty(DocumentId(1));
    let spread = s.doc.active_spread();
    let layer = s.doc.active_layer(spread).unwrap();
    for i in 0..n {
        let x = 100_000 + 120_000 * i32::try_from(i).unwrap();
        s.apply_edit(EditCommand::CreateShape {
            layer,
            shape: Box::new(rectangle(Point::raw(x, 400_000), 50_000.0, 30_000.0)),
            attrs: vec![flat(fill.clone())],
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
    (s, nodes)
}

/// Adds a named colour outside the history.
fn named(s: &mut Session, name: &str, v: ColourValue) -> ColourId {
    s.apply_edit(EditCommand::Palette(PaletteCommand::Create {
        def: ColourDef::normal(v).named(name),
    }))
    .unwrap();
    s.bus.history_mut().clear(&mut s.doc);
    s.doc.resources.colours.by_name(name).unwrap()
}

fn fill_colour(s: &Session, n: NodeId) -> Colour {
    match fill_in_force(&s.doc, n, PaintSlot::Fill, FillChannel::Colour) {
        FillValue::Colour(FillGeometry::Flat { value }) => value,
        other => panic!("not a flat fill: {other:?}"),
    }
}

fn resolved(s: &Session, n: NodeId) -> ColourValue {
    fill_colour(s, n).resolve(&s.doc.resources.colours)
}

fn op(s: &mut Session, o: Op) {
    s.apply(Intent::ColourEditor(o)).unwrap();
}

fn comps(c: [f32; 4]) -> ColourChange {
    ColourChange::Components(c)
}

#[test]
fn a_sixty_event_drag_is_one_undo_step_and_undoes_exactly() {
    let (mut s, nodes) = fixture(3, Colour::Direct(ColourValue::rgb(1.0, 0.0, 0.0)));
    let before = s.doc.canonical_digest();
    let view = s.colour_editor_view().unwrap();
    assert_eq!(view.target, FILL, "the fill of the selection by default");
    assert_eq!(view.title, "Fill of 3 objects");
    assert_eq!(view.components, [1.0, 0.0, 0.0, 0.0]);

    for i in 0..60 {
        let g = i as f32 / 59.0;
        op(&mut s, Op::Preview(comps([1.0, g, 0.0, 0.0])));
        // Live: the document follows every frame.
        assert_eq!(resolved(&s, nodes[2]), ColourValue::rgb(1.0, g, 0.0));
        assert!(s.colour_editor().dragging());
    }
    op(&mut s, Op::Commit);
    assert!(!s.colour_editor().dragging());
    assert_eq!(s.bus.history().len(), 1, "one step for the whole drag");
    assert_eq!(s.undo_label(), Some("Set Fill Colour"));
    for &n in &nodes {
        assert_eq!(resolved(&s, n), ColourValue::rgb(1.0, 1.0, 0.0));
    }

    s.apply(Intent::Undo).unwrap();
    assert_eq!(s.doc.canonical_digest(), before, "undo is exact");
    assert_eq!(
        s.colour_editor_view().unwrap().components,
        [1.0, 0.0, 0.0, 0.0],
        "the editor follows the undo"
    );

    // A second drag is a second step.
    s.apply(Intent::Redo).unwrap();
    op(&mut s, Op::Preview(comps([0.0, 0.0, 1.0, 0.0])));
    op(&mut s, Op::Commit);
    assert_eq!(s.bus.history().len(), 2);
}

#[test]
fn escape_mid_drag_leaves_document_and_history_untouched() {
    let (mut s, nodes) = fixture(2, Colour::Direct(ColourValue::rgb(0.0, 0.0, 1.0)));
    // Some history to protect.
    op(&mut s, Op::Set(comps([0.0, 0.5, 1.0, 0.0])));
    let digest = s.doc.canonical_digest();
    let labels = s.bus.history().labels();
    let serial = s.bus.history().state_serial();

    for i in 0..20 {
        op(&mut s, Op::Preview(comps([i as f32 / 20.0, 0.0, 0.0, 0.0])));
    }
    assert_ne!(s.doc.canonical_digest(), digest);
    let view = s.colour_editor_view().unwrap();
    assert_eq!(view.original, Some(ColourValue::rgb(0.0, 0.5, 1.0)));
    op(&mut s, Op::Cancel);

    assert_eq!(s.doc.canonical_digest(), digest);
    assert_eq!(s.bus.history().labels(), labels, "no step, no redo entry");
    assert_eq!(s.bus.history().state_serial(), serial);
    assert!(!s.bus.history().can_redo());
    assert_eq!(resolved(&s, nodes[0]), ColourValue::rgb(0.0, 0.5, 1.0));
    assert!(!s.colour_editor().dragging());
}

#[test]
fn escape_mid_drag_brings_back_the_redo_branch() {
    let (mut s, nodes) = fixture(2, Colour::Direct(ColourValue::rgb(0.0, 0.0, 1.0)));
    op(&mut s, Op::Set(comps([0.0, 0.5, 1.0, 0.0])));
    let edited = s.doc.canonical_digest();
    s.apply(Intent::Undo).unwrap();
    let digest = s.doc.canonical_digest();
    let labels = s.bus.history().labels();
    let serial = s.bus.history().state_serial();
    assert_eq!(s.redo_label(), Some("Set Fill Colour"));

    for i in 0..20 {
        op(&mut s, Op::Preview(comps([i as f32 / 20.0, 0.0, 0.0, 0.0])));
    }
    assert_ne!(s.doc.canonical_digest(), digest);
    op(&mut s, Op::Cancel);

    assert_eq!(s.doc.canonical_digest(), digest);
    assert_eq!(s.bus.history().labels(), labels, "undo and redo lists");
    assert_eq!(s.bus.history().state_serial(), serial);
    s.apply(Intent::Redo).unwrap();
    assert_eq!(s.doc.canonical_digest(), edited, "redo re-applies the edit");
    assert_eq!(resolved(&s, nodes[0]), ColourValue::rgb(0.0, 0.5, 1.0));

    // A committed drag drops the branch, as any new edit does.
    s.apply(Intent::Undo).unwrap();
    op(&mut s, Op::Preview(comps([1.0, 0.0, 0.0, 0.0])));
    op(&mut s, Op::Commit);
    assert!(!s.bus.history().can_redo());
    assert_eq!(s.bus.history().len(), 1);

    // A drag that left the document alone (nothing selected: it sets the
    // current attribute) keeps it.
    s.apply(Intent::Undo).unwrap();
    s.apply(Intent::Select {
        nodes: Vec::new(),
        mode: xarast_app::SelectMode::Replace,
    })
    .unwrap();
    assert_eq!(s.redo_label(), Some("Set Fill Colour"));
    op(&mut s, Op::Preview(comps([0.0, 1.0, 0.0, 0.0])));
    assert!(!s.bus.history().can_redo(), "set aside while dragging");
    op(&mut s, Op::Commit);
    assert_eq!(s.redo_label(), Some("Set Fill Colour"));
}

#[test]
fn a_typed_value_is_one_step_per_entry() {
    let (mut s, nodes) = fixture(1, Colour::Direct(ColourValue::BLACK));
    op(&mut s, Op::Set(comps([0.2, 0.0, 0.0, 0.0])));
    op(&mut s, Op::Set(comps([0.4, 0.0, 0.0, 0.0])));
    assert_eq!(s.bus.history().len(), 2);
    assert_eq!(resolved(&s, nodes[0]), ColourValue::rgb(0.4, 0.0, 0.0));
}

#[test]
fn a_hue_survives_a_saturation_dragged_to_zero() {
    let (mut s, _) = fixture(1, Colour::Direct(ColourValue::hsvt(0.6, 1.0, 1.0, 0.0)));
    op(&mut s, Op::SetModel(ColourModel::Hsvt));
    let v = s.colour_editor_view().unwrap();
    assert_eq!(v.model, ColourModel::Hsvt);
    assert!((v.components[0] - 0.6).abs() < 1e-6);
    op(&mut s, Op::Preview(comps([0.6, 0.0, 1.0, 0.0])));
    op(&mut s, Op::Preview(comps([0.6, 0.0, 0.5, 0.0])));
    assert!((s.colour_editor_view().unwrap().components[0] - 0.6).abs() < 1e-6);
    op(&mut s, Op::Preview(comps([0.6, 0.8, 0.5, 0.0])));
    op(&mut s, Op::Commit);
    assert!((s.colour_editor_view().unwrap().components[0] - 0.6).abs() < 1e-6);
}

#[test]
fn editing_a_linked_object_breaks_its_link_and_redefine_is_explicit() {
    let (mut s, nodes) = fixture(3, Colour::Direct(ColourValue::BLACK));
    let sky = named(&mut s, "Sky", ColourValue::rgb(0.2, 0.4, 1.0));
    // Put "Sky" on all three as a live reference: its own action.
    op(
        &mut s,
        Op::ApplyEntry {
            id: sky,
            slot: PaintSlot::Fill,
        },
    );
    assert_eq!(s.undo_label(), Some("Set Fill Colour"));
    for &n in &nodes {
        assert_eq!(
            fill_colour(&s, n),
            Colour::Indexed {
                id: sky,
                tint: None
            }
        );
    }
    let view = s.colour_editor_view().unwrap();
    assert_eq!(
        view.linked_entry.as_ref().map(|e| e.name.as_str()),
        Some("Sky")
    );

    // Editing only the first object: its link is broken, "Sky" is not
    // touched, the others still use it.
    s.apply(Intent::Select {
        nodes: vec![nodes[0]],
        mode: xarast_app::SelectMode::Replace,
    })
    .unwrap();
    op(&mut s, Op::Set(comps([1.0, 0.0, 0.0, 0.0])));
    assert_eq!(
        fill_colour(&s, nodes[0]),
        Colour::Direct(ColourValue::rgb(1.0, 0.0, 0.0))
    );
    assert_eq!(
        s.doc.resources.colours.resolve(sky),
        ColourValue::rgb(0.2, 0.4, 1.0)
    );
    assert_eq!(
        fill_colour(&s, nodes[1]),
        Colour::Indexed {
            id: sky,
            tint: None
        }
    );

    // Redefining "Sky" is the other action: every user repaints.
    op(&mut s, Op::SetTarget(ColourTarget::Entry(sky)));
    let view = s.colour_editor_view().unwrap();
    assert_eq!(view.title, "Colour \u{2018}Sky\u{2019}");
    for i in 0..30 {
        op(&mut s, Op::Preview(comps([0.0, i as f32 / 29.0, 0.0, 0.0])));
    }
    op(&mut s, Op::Commit);
    assert_eq!(s.undo_label(), Some("Edit Colour"));
    assert_eq!(resolved(&s, nodes[1]), ColourValue::rgb(0.0, 1.0, 0.0));
    assert_eq!(resolved(&s, nodes[2]), ColourValue::rgb(0.0, 1.0, 0.0));
    assert_eq!(resolved(&s, nodes[0]), ColourValue::rgb(1.0, 0.0, 0.0));
    let steps = s.bus.history().len();
    s.apply(Intent::Undo).unwrap();
    assert_eq!(s.bus.history().len(), steps - 1, "the drag was one step");
    assert_eq!(resolved(&s, nodes[1]), ColourValue::rgb(0.2, 0.4, 1.0));
}

#[test]
fn with_nothing_selected_the_editor_sets_the_current_attribute() {
    let (mut s, _) = fixture(1, Colour::Direct(ColourValue::BLACK));
    s.apply(Intent::SelectNone).unwrap();
    assert_eq!(
        s.colour_editor_view().unwrap().title,
        "Fill for new objects"
    );
    let before = s.edit.current.clone();
    op(&mut s, Op::Preview(comps([0.0, 1.0, 0.0, 0.0])));
    assert!(
        s.edit
            .current
            .values()
            .contains(&flat(Colour::Direct(ColourValue::rgb(0.0, 1.0, 0.0))))
    );
    op(&mut s, Op::Cancel);
    assert_eq!(s.edit.current, before);
    op(&mut s, Op::Set(comps([0.0, 0.0, 1.0, 0.0])));
    assert!(
        s.edit
            .current
            .values()
            .contains(&flat(Colour::Direct(ColourValue::rgb(0.0, 0.0, 1.0))))
    );
    assert!(
        s.bus.history().is_empty(),
        "current attributes are not undone"
    );
}

#[test]
fn the_line_colour_is_its_own_target() {
    let (mut s, nodes) = fixture(1, Colour::Direct(ColourValue::BLACK));
    op(
        &mut s,
        Op::SetTarget(ColourTarget::Selection(PaintSlot::Stroke)),
    );
    assert_eq!(s.colour_editor_view().unwrap().title, "Line of 1 object");
    op(&mut s, Op::Set(comps([1.0, 0.0, 1.0, 0.0])));
    let line = match fill_in_force(&s.doc, nodes[0], PaintSlot::Stroke, FillChannel::Colour) {
        FillValue::Colour(FillGeometry::Flat { value }) => value,
        other => panic!("{other:?}"),
    };
    assert_eq!(line, Colour::Direct(ColourValue::rgb(1.0, 0.0, 1.0)));
    assert_eq!(
        fill_colour(&s, nodes[0]),
        Colour::Direct(ColourValue::BLACK)
    );
}

#[test]
fn a_tint_amount_drag_is_one_step_and_follows_its_parent() {
    let (mut s, nodes) = fixture(1, Colour::Direct(ColourValue::BLACK));
    let base = named(&mut s, "Base", ColourValue::rgb(0.0, 0.0, 1.0));
    let tint = named(&mut s, "Pale", ColourValue::WHITE);
    op(
        &mut s,
        Op::ApplyEntry {
            id: tint,
            slot: PaintSlot::Fill,
        },
    );
    op(&mut s, Op::SetTarget(ColourTarget::Entry(tint)));
    let steps = s.bus.history().len();
    for i in 0..=10 {
        op(
            &mut s,
            Op::Preview(ColourChange::Derivation(Derivation::Tint {
                parent: base,
                factor: i as f32 / 20.0,
            })),
        );
    }
    op(&mut s, Op::Commit);
    assert_eq!(s.bus.history().len(), steps + 1);
    assert_eq!(s.undo_label(), Some("Link Colour"));
    let v = s.colour_editor_view().unwrap();
    assert_eq!(v.entry.as_ref().map(|e| e.1.label()), Some("Tint"));
    assert_eq!(v.editable, [false; 4], "a tint's components are derived");
    assert!(!v.model_editable);
    // Half blue: 1 - 0.5 (1 - c).
    assert_eq!(resolved(&s, nodes[0]), ColourValue::rgb(0.5, 0.5, 1.0));
    // Component edits of a tint are refused silently.
    op(&mut s, Op::Set(comps([0.0; 4])));
    assert_eq!(s.bus.history().len(), steps + 1);

    // The parent chooser never offers the entry itself or its descendants.
    op(&mut s, Op::SetTarget(ColourTarget::Entry(base)));
    let v = s.colour_editor_view().unwrap();
    let pale = v.named.iter().find(|n| n.id == tint).unwrap();
    assert!(!pale.can_parent, "a descendant cannot become the parent");
    // And the command refuses a cycle, leaving everything as it was.
    let digest = s.doc.canonical_digest();
    let err = s.apply(Intent::ColourEditor(Op::Set(ColourChange::Derivation(
        Derivation::Tint {
            parent: tint,
            factor: 0.5,
        },
    ))));
    assert!(err.is_err());
    assert_eq!(s.doc.canonical_digest(), digest);

    // Redefining the parent repaints the tint's user.
    op(&mut s, Op::Set(comps([1.0, 0.0, 0.0, 0.0])));
    assert_eq!(resolved(&s, nodes[0]), ColourValue::rgb(1.0, 0.5, 0.5));
}

#[test]
fn a_hsv_link_inheriting_the_hue_follows_its_parent_hue() {
    let (mut s, _) = fixture(1, Colour::Direct(ColourValue::BLACK));
    let parent = named(&mut s, "Parent", ColourValue::hsvt(0.25, 1.0, 1.0, 0.0));
    let child = named(&mut s, "Child", ColourValue::hsvt(0.75, 0.5, 0.5, 0.0));
    op(&mut s, Op::SetTarget(ColourTarget::Entry(child)));
    op(
        &mut s,
        Op::Set(ColourChange::Derivation(Derivation::Linked {
            parent,
            model: ColourModel::Hsvt,
            inherit: [true, false, false, false],
        })),
    );
    let v = s.colour_editor_view().unwrap();
    assert_eq!(v.editable, [false, true, true, true]);
    let c = v.components;
    assert!((c[0] - 0.25).abs() < 1e-6, "hue from the parent: {c:?}");
    assert!((c[1] - 0.5).abs() < 1e-6 && (c[2] - 0.5).abs() < 1e-6);
    // Editing the link's value keeps the hue inherited.
    op(&mut s, Op::Set(comps([0.9, 0.5, 0.8, 0.0])));
    let def = s.doc.resources.colours.get(child).unwrap();
    assert_eq!(def.components[0], None);
    // The parent's hue moves the child's.
    op(&mut s, Op::SetTarget(ColourTarget::Entry(parent)));
    op(&mut s, Op::Set(comps([0.5, 1.0, 1.0, 0.0])));
    let child_now = s
        .doc
        .resources
        .colours
        .resolve(child)
        .to_hsvt()
        .components();
    assert!((child_now[0] - 0.5).abs() < 1e-3, "{child_now:?}");
    assert!((child_now[2] - 0.8).abs() < 1e-3, "{child_now:?}");
}

#[test]
fn a_new_named_colour_holds_the_colour_shown_and_becomes_the_target() {
    let (mut s, nodes) = fixture(1, Colour::Direct(ColourValue::rgb(0.1, 0.2, 0.3)));
    let name = xarast_app::colour_editor::fresh_name(&s, "Colour");
    assert_eq!(name, "Colour 1");
    op(&mut s, Op::NewNamed(name.clone()));
    let id = s.doc.resources.colours.by_name(&name).unwrap();
    assert_eq!(s.colour_editor().target(), ColourTarget::Entry(id));
    assert_eq!(
        s.doc.resources.colours.resolve(id),
        ColourValue::rgb(0.1, 0.2, 0.3)
    );
    assert_eq!(s.undo_label(), Some("Create Colour"));
    // The object is not linked until the user applies the colour.
    assert!(matches!(fill_colour(&s, nodes[0]), Colour::Direct(_)));
    op(&mut s, Op::Rename("Ink".to_owned()));
    assert_eq!(s.doc.resources.colours.by_name("Ink"), Some(id));
    assert_eq!(s.undo_label(), Some("Rename Colour"));
}

#[test]
fn changing_an_entrys_model_converts_it_and_is_undoable() {
    let (mut s, _) = fixture(1, Colour::Direct(ColourValue::BLACK));
    let id = named(&mut s, "Ink", ColourValue::rgb(1.0, 0.0, 0.0));
    op(&mut s, Op::SetTarget(ColourTarget::Entry(id)));
    op(&mut s, Op::SetModel(ColourModel::Cmyk));
    let def = s.doc.resources.colours.get(id).unwrap();
    assert_eq!(def.model, ColourModel::Cmyk);
    assert_eq!(
        s.doc.resources.colours.resolve(id).to_rgba8(),
        ColourValue::rgb(1.0, 0.0, 0.0).to_rgba8()
    );
    assert_eq!(s.colour_editor_view().unwrap().model, ColourModel::Cmyk);
    s.apply(Intent::Undo).unwrap();
    assert_eq!(
        s.doc.resources.colours.get(id).unwrap().model,
        ColourModel::Rgbt
    );
}

#[test]
fn an_undo_mid_drag_keeps_the_drag_and_closes_it() {
    let (mut s, nodes) = fixture(1, Colour::Direct(ColourValue::BLACK));
    op(&mut s, Op::Preview(comps([0.5, 0.0, 0.0, 0.0])));
    s.apply(Intent::Undo).unwrap();
    assert!(!s.colour_editor().dragging());
    assert_eq!(resolved(&s, nodes[0]), ColourValue::BLACK);
    // A cancel afterwards has nothing to take back.
    op(&mut s, Op::Cancel);
    assert!(s.bus.history().can_redo());
}

#[test]
fn a_locked_layer_refuses_the_edit_and_nothing_changes() {
    let (mut s, _) = fixture(1, Colour::Direct(ColourValue::BLACK));
    let spread = s.doc.active_spread();
    let layer = s.doc.active_layer(spread).unwrap();
    s.apply(Intent::SetLayerLocked {
        layer,
        locked: true,
    })
    .unwrap();
    s.bus.history_mut().clear(&mut s.doc);
    let digest = s.doc.canonical_digest();
    assert!(
        s.apply(Intent::ColourEditor(Op::Set(comps([1.0, 1.0, 1.0, 0.0]))))
            .is_err()
    );
    assert_eq!(s.doc.canonical_digest(), digest);
    assert!(s.bus.history().is_empty());
}
