//! The colour editor's pointer drags, headless through `egui_kittest`: a
//! drag of a number or a derivation slider previews and then commits once,
//! and `Esc` in the middle of it cancels it (XARA-T-0305).
//!
//! egui 0.33 ends a drag by itself when `Esc` is pressed and reports the
//! widget's `drag_stopped` in that very frame, so a classifier that maps
//! every `drag_stopped` to "commit" would commit a cancelled drag.

use std::cell::RefCell;

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use xarast_app::colour_editor::{
    ColourEditorOp, ColourEditorView, ColourTarget, Derivation, NamedColour, PaintSlot,
};
use xarast_color::{ColourDef, ColourModel, ColourTable, ColourValue};
use xarast_ui::model::{CommandSink, UiCommand, UiModel};
use xarast_ui::panel::{Panel, PanelCtx};
use xarast_ui::panels::ColourPanel;
use xarast_ui::theme::{ResolvedTheme, ThemeTokens};

/// A colour editor on the selection's fill (`tint == false`), or on the
/// named colour "Paper", a tint of "Brand red" (`tint == true`).
fn view(tint: bool) -> ColourEditorView {
    let mut t = ColourTable::new();
    let brand = t.insert(ColourDef::normal(ColourValue::rgb(0.8, 0.1, 0.1)).named("Brand red"));
    let paper = t.insert(ColourDef::normal(ColourValue::rgb(1.0, 1.0, 0.95)).named("Paper"));
    let named = |id, name: &str, value| NamedColour {
        id,
        name: name.to_owned(),
        value,
        can_parent: true,
    };
    let value = ColourValue::rgb(0.2, 0.4, 0.9);
    let mut v = ColourEditorView {
        target: ColourTarget::Selection(PaintSlot::Fill),
        title: "Fill of 1 object".to_owned(),
        model: ColourModel::Rgbt,
        model_editable: true,
        components: value.components(),
        editable: [true; 4],
        value,
        original: None,
        dragging: false,
        linked_entry: None,
        entry: None,
        named: vec![
            named(brand, "Brand red", ColourValue::rgb(0.8, 0.1, 0.1)),
            named(paper, "Paper", ColourValue::rgb(1.0, 1.0, 0.95)),
        ],
        objects: 1,
    };
    if tint {
        v.target = ColourTarget::Entry(paper);
        v.editable = [false; 4];
        v.model_editable = false;
        v.entry = Some((
            "Paper".to_owned(),
            Derivation::Tint {
                parent: brand,
                factor: 0.4,
            },
        ));
    }
    v
}

fn harness<'a>(m: &'a UiModel, commands: &'a RefCell<Vec<UiCommand>>) -> Harness<'a> {
    let tokens = ThemeTokens::of(ResolvedTheme::Dark);
    let mut panel = ColourPanel::new();
    let mut h = Harness::builder()
        .with_size(egui::vec2(360.0, 900.0))
        .build_ui(move |ui| {
            let mut sink = CommandSink::new();
            let mut ctx = PanelCtx {
                model: m,
                tokens: &tokens,
                out: &mut sink,
            };
            panel.ui(ui, &mut ctx);
            commands.borrow_mut().extend(sink.drain());
        });
    h.run();
    h
}

fn ops_of(commands: &RefCell<Vec<UiCommand>>) -> Vec<ColourEditorOp> {
    commands
        .borrow()
        .iter()
        .filter_map(|c| match c {
            UiCommand::ColourEditor(op) => Some(op.clone()),
            _ => None,
        })
        .collect()
}

/// Presses on the widget, drags it right in six steps and releases;
/// `Esc` goes in before the fourth step when asked.
fn drag(h: &mut Harness<'_>, role: Role, name: &str, escape_midway: bool) {
    let rect = h.get_by_role_and_label(role, name).rect();
    let start = egui::pos2(rect.center().x, rect.center().y);
    h.hover_at(start);
    h.run();
    h.drag_at(start);
    h.run();
    for step in 1..=6 {
        if escape_midway && step == 4 {
            h.key_press(egui::Key::Escape);
            h.run();
        }
        h.hover_at(start + egui::vec2(8.0 * step as f32, 0.0));
        h.run();
    }
    h.drop_at(start + egui::vec2(48.0, 0.0));
    h.run();
}

fn assert_previews_then_one_commit(ops: &[ColourEditorOp]) {
    let previews = ops
        .iter()
        .filter(|o| matches!(o, ColourEditorOp::Preview(_)))
        .count();
    assert!(previews >= 3, "{ops:?}");
    assert_eq!(ops.last(), Some(&ColourEditorOp::Commit), "{ops:?}");
    assert_eq!(
        ops.iter().filter(|o| **o == ColourEditorOp::Commit).count(),
        1,
        "{ops:?}"
    );
    assert!(
        !ops.iter().any(|o| matches!(o, ColourEditorOp::Set(_))),
        "a drag sets nothing: {ops:?}"
    );
}

fn assert_cancelled(ops: &[ColourEditorOp]) {
    let cancel = ops
        .iter()
        .position(|o| *o == ColourEditorOp::Cancel)
        .unwrap_or_else(|| panic!("no cancel: {ops:?}"));
    assert!(
        ops[cancel + 1..].is_empty(),
        "the rest of the drag is ignored: {ops:?}"
    );
    assert!(!ops.contains(&ColourEditorOp::Commit), "{ops:?}");
}

#[test]
fn a_tint_slider_drag_previews_and_commits_once() {
    let m = UiModel {
        colour_editor: Some(view(true)),
        ..UiModel::default()
    };
    let commands = RefCell::new(Vec::new());
    let mut h = harness(&m, &commands);
    drag(&mut h, Role::Slider, "Tint", false);
    assert_previews_then_one_commit(&ops_of(&commands));
}

#[test]
fn escape_during_a_tint_slider_drag_cancels_it() {
    let m = UiModel {
        colour_editor: Some(view(true)),
        ..UiModel::default()
    };
    let commands = RefCell::new(Vec::new());
    let mut h = harness(&m, &commands);
    drag(&mut h, Role::Slider, "Tint", true);
    assert_cancelled(&ops_of(&commands));
}

#[test]
fn escape_during_a_field_or_strip_drag_cancels_it() {
    // The 2D field and the strip track the press themselves
    // (`PressState`), not egui's drag state: pinned here all the same.
    for name in ["Red slider", "Green (across) and Blue (up)"] {
        let m = UiModel {
            colour_editor: Some(view(false)),
            ..UiModel::default()
        };
        let commands = RefCell::new(Vec::new());
        let mut h = harness(&m, &commands);
        drag(&mut h, Role::Slider, name, false);
        let ops = ops_of(&commands);
        assert_eq!(ops.last(), Some(&ColourEditorOp::Commit), "{name}: {ops:?}");

        let commands = RefCell::new(Vec::new());
        let mut h = harness(&m, &commands);
        drag(&mut h, Role::Slider, name, true);
        assert_cancelled(&ops_of(&commands));
    }
}

#[test]
fn a_number_drag_previews_and_commits_once() {
    let m = UiModel {
        colour_editor: Some(view(false)),
        ..UiModel::default()
    };
    let commands = RefCell::new(Vec::new());
    let mut h = harness(&m, &commands);
    drag(&mut h, Role::SpinButton, "Green", false);
    assert_previews_then_one_commit(&ops_of(&commands));
}

#[test]
fn escape_during_a_number_drag_cancels_it() {
    let m = UiModel {
        colour_editor: Some(view(false)),
        ..UiModel::default()
    };
    let commands = RefCell::new(Vec::new());
    let mut h = harness(&m, &commands);
    drag(&mut h, Role::SpinButton, "Green", true);
    assert_cancelled(&ops_of(&commands));
}
