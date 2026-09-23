//! Selection, the command bridge and the intent surface.

use xarast_app::{
    AppState, Changed, DeviceSize, DocumentId, EditState, Intent, SelectMode, Session, ToolId,
    ZoomTarget,
};
use xarast_doc::{Document, NodeKind};

fn layers(doc: &Document) -> Vec<xarast_doc::NodeId> {
    doc.tree
        .preorder(doc.tree.root())
        .filter(|id| matches!(doc.tree.kind(*id), Some(NodeKind::Layer(_))))
        .collect()
}

#[test]
fn selection_modes_do_what_they_say() {
    let doc = Document::new_empty();
    let ids = layers(&doc);
    let (a, b) = (ids[0], ids[1]);
    let mut e = EditState::for_document(&doc);

    assert!(e.select([a], SelectMode::Replace));
    assert_eq!(e.selection().collect::<Vec<_>>(), vec![a]);

    assert!(e.select([b], SelectMode::Add));
    assert_eq!(e.selection().collect::<Vec<_>>(), vec![a, b]);
    assert_eq!(e.key_object(), Some(b));

    assert!(e.select([a], SelectMode::Toggle));
    assert_eq!(e.selection().collect::<Vec<_>>(), vec![b]);

    assert!(e.select([b], SelectMode::Remove));
    assert!(e.is_selection_empty());

    // A no-op selection reports no change, so an idle frame stays idle.
    assert!(!e.select([b], SelectMode::Remove));
}

#[test]
fn control_points_only_exist_on_a_selected_node() {
    let doc = Document::new_empty();
    let ids = layers(&doc);
    let mut e = EditState::for_document(&doc);
    assert!(e.control_points_mut(ids[0]).is_none());
    e.select([ids[0]], SelectMode::Replace);
    let cp = e.control_points_mut(ids[0]).expect("selected");
    cp.insert(3);
    cp.insert(7);
    assert_eq!(cp.len(), 2);
    assert!(e.has_control_points());
    // Deselecting drops the overlay with it.
    e.select([ids[0]], SelectMode::Remove);
    assert!(!e.has_control_points());
    assert!(e.control_points(ids[0]).is_none());
}

#[test]
fn a_control_point_overlay_survives_a_shortened_path() {
    let doc = Document::new_empty();
    let ids = layers(&doc);
    let mut e = EditState::for_document(&doc);
    e.select([ids[0]], SelectMode::Replace);
    let cp = e.control_points_mut(ids[0]).expect("selected");
    for i in 0..10 {
        cp.insert(i);
    }
    cp.truncate_to(4);
    assert_eq!(cp.iter().collect::<Vec<_>>(), vec![0, 1, 2, 3]);
}

#[test]
fn the_edit_state_never_names_a_node_that_is_gone() {
    let mut s = Session::new_empty(DocumentId(1));
    let spread = s.doc.active_spread();
    s.dispatch(&xarast_app::commands::AddLayer {
        spread,
        name: "Scratch".into(),
    })
    .expect("add");
    let scratch = *layers(&s.doc).last().expect("the new layer");
    s.edit.select([scratch], SelectMode::Replace);
    s.edit
        .control_points_mut(scratch)
        .expect("selected")
        .insert(0);
    assert_eq!(s.edit.selection_len(), 1);

    s.dispatch(&xarast_app::commands::DeleteNode { node: scratch })
        .expect("delete");
    assert!(
        s.edit.is_selection_empty(),
        "a deleted node must leave the selection"
    );
    assert!(!s.edit.has_control_points());

    // Undo brings the node back; the selection deliberately does not
    // come back with it, because the selection is not part of the
    // document (architecture 3.5b).
    s.apply(Intent::Undo).expect("undo");
    assert!(s.doc.tree.is_reachable(scratch));
    assert!(s.edit.is_selection_empty());
}

#[test]
fn layer_visibility_is_a_command_and_undoes() {
    let mut s = Session::new_empty(DocumentId(1));
    let layer = *layers(&s.doc).last().expect("a layer");
    let before = s.doc.canonical_digest();

    let changed = s
        .apply(Intent::SetLayerVisible {
            layer,
            visible: false,
        })
        .expect("visible");
    assert!(changed.contains(Changed::DOCUMENT));
    assert!(s.is_modified());
    assert!(matches!(s.doc.tree.kind(layer), Some(NodeKind::Layer(l)) if !l.visible));

    s.apply(Intent::Undo).expect("undo");
    assert_eq!(
        s.doc.canonical_digest(),
        before,
        "undo must restore the document byte for byte"
    );
}

#[test]
fn the_active_layer_stays_unique_and_undoes_with_the_rest() {
    let mut s = Session::new_empty(DocumentId(1));
    let spread = s.doc.active_spread();
    s.dispatch(&xarast_app::commands::AddLayer {
        spread,
        name: "Second".into(),
    })
    .expect("add");
    let all = layers(&s.doc);
    let second = *all.last().expect("the new layer");
    let before = s.doc.canonical_digest();

    s.apply(Intent::SetActiveLayer(second)).expect("activate");
    let active: Vec<_> = all
        .iter()
        .copied()
        .filter(|id| matches!(s.doc.tree.kind(*id), Some(NodeKind::Layer(l)) if l.active))
        .collect();
    assert_eq!(active, vec![second], "exactly one active layer per spread");
    assert_eq!(s.edit.active_layer(), Some(second));

    s.apply(Intent::Undo).expect("undo");
    assert_eq!(s.doc.canonical_digest(), before);
}

#[test]
fn a_guide_layer_may_not_become_the_active_one() {
    let mut s = Session::new_empty(DocumentId(1));
    let guides = layers(&s.doc)
        .into_iter()
        .find(|id| matches!(s.doc.tree.kind(*id), Some(NodeKind::Layer(l)) if l.guide))
        .expect("the guide layer");
    assert!(s.apply(Intent::SetActiveLayer(guides)).is_err());
}

#[test]
fn an_invisible_layer_is_not_walked() {
    let mut s = Session::new_empty(DocumentId(1));
    let base = s.rebuild_scene(None).expect("scene");
    let layer = *layers(&s.doc).last().expect("a layer");
    s.apply(Intent::SetLayerVisible {
        layer,
        visible: false,
    })
    .expect("hide");
    let hidden = s.rebuild_scene(None).expect("scene");
    assert!(hidden.primitives() <= base.primitives());
}

#[test]
fn intents_report_only_what_they_changed() {
    let mut s = Session::new_empty(DocumentId(1));
    assert!(s.apply(Intent::SelectNone).expect("none").is_none());
    assert!(
        s.apply(Intent::Resize(DeviceSize::new(640, 480)))
            .expect("resize")
            .contains(Changed::VIEW)
    );
    // The same size again changes nothing.
    assert!(
        s.apply(Intent::Resize(DeviceSize::new(640, 480)))
            .expect("resize")
            .is_none()
    );
    assert!(
        s.apply(Intent::ChooseTool(ToolId::Pan))
            .expect("tool")
            .contains(Changed::UI)
    );
    assert!(
        s.apply(Intent::ZoomTo(ZoomTarget::Page))
            .expect("zoom")
            .needs_redraw()
    );
}

#[test]
fn a_xarast_package_opens_like_a_xar_file() {
    // The open path File › Open and the command line share.
    let doc = Document::new_empty();
    let mut bytes = std::io::Cursor::new(Vec::new());
    let opts = xarast_format::SaveOptions {
        write: xarast_format::WriteOptions::deterministic(),
        ..xarast_format::SaveOptions::default()
    };
    xarast_format::save_to(&doc, &mut bytes, &opts).expect("save");
    let s = Session::open_bytes(
        DocumentId(7),
        std::path::Path::new("drawing.XARAST"),
        &bytes.into_inner(),
    )
    .expect("opens");
    assert_eq!(layers(&s.doc).len(), layers(&doc).len());
    assert_eq!(
        xarast_format::svg::normal_form(&s.doc),
        xarast_format::svg::normal_form(&doc)
    );
    let err = Session::open_bytes(
        DocumentId(8),
        std::path::Path::new("broken.xarast"),
        b"not a package",
    )
    .expect_err("garbage is refused");
    assert!(matches!(err, xarast_app::SessionError::Xarast { .. }));
}

#[test]
fn saving_writes_xarast_and_never_xar() {
    let dir = std::env::temp_dir().join(format!("xarast-session-save-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    let mut s = Session::new_empty(DocumentId(1));
    let target = dir.join("drawing.xarast");
    let summary = s.save_as(&target).expect("a .xarast saves");
    assert!(summary.bytes > 0 && summary.thumbnail);
    assert_eq!(s.path.as_deref(), Some(target.as_path()));
    assert!(!s.is_modified());
    let err = s
        .save_as(&dir.join("nope.xar"))
        .expect_err("writing .xar is a non-goal");
    assert!(format!("{err}").contains("non-goal"));
    assert!(matches!(err, xarast_app::SessionError::Unsupported { .. }));
    let err = s
        .save_as(&dir.join("missing-dir").join("x.xarast"))
        .expect_err("an unwritable target fails");
    assert!(matches!(err, xarast_app::SessionError::Save { .. }));
    assert_eq!(
        s.path.as_deref(),
        Some(target.as_path()),
        "a failure keeps the path"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_application_tracks_tabs_and_the_active_one() {
    let mut app = AppState::new();
    assert!(app.active().is_none());
    let a = app.new_document();
    let b = app.new_document();
    assert_eq!(app.active, Some(b));
    assert_eq!(app.docs.len(), 2);
    assert!(app.close(b));
    assert_eq!(app.active, Some(a));
    assert!(app.close(a));
    assert!(app.docs.is_empty());
    assert!(app.active().is_none());
    // With nothing open an intent is a no-op, not a panic.
    assert!(app.apply(Intent::SelectAll).expect("no-op").is_none());
}
