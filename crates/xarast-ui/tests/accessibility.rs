//! Accessibility and keyboard-only operability, asserted headlessly.
//!
//! These tests walk the AccessKit tree the interface publishes, which is
//! the same tree `accesskit_winit` hands to AT-SPI. They need no display,
//! no GPU and no screen reader: criterion 12 of
//! `docs/phases/phase-05-shell-and-ui.md` asks for an automated assertion
//! that the layer tree, the fields and the toggles are present and
//! labelled, and this is it. The manual `accerciser` walk stays on the
//! release checklist; it is not what guards a refactor.

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use xarast_app::colour_editor::{
    ColourChange, ColourEditorOp, ColourEditorView, ColourTarget, Derivation, NamedColour,
    PaintSlot,
};
use xarast_color::{ColourDef, ColourModel, ColourTable, ColourValue};
use xarast_ui::model::{
    CommandSink, DocumentView, LayerInfo, PaletteEntry, StatusInfo, UiCommand, UiModel,
};
use xarast_ui::panel::{Panel, PanelCtx};
use xarast_ui::panels::{ColourPanel, LayerPanel};
use xarast_ui::theme::{ResolvedTheme, ThemeTokens};

fn model() -> UiModel {
    let layers = vec![
        LayerInfo::new(1, "Background"),
        LayerInfo {
            locked: true,
            ..LayerInfo::new(2, "Guides")
        },
        LayerInfo {
            visible: false,
            ..LayerInfo::new(3, "Sky")
        },
    ];
    let active = layers.first().map(|l| l.key);
    UiModel {
        document: Some(DocumentView {
            title: "poster.xar".to_owned(),
            layers,
            active_layer: active,
            ..Default::default()
        }),
        palette: vec![
            PaletteEntry::none(),
            PaletteEntry::colour("Brand red", ColourValue::rgb(0.8, 0.1, 0.1)),
            PaletteEntry::colour("Paper", ColourValue::rgb(1.0, 1.0, 0.95)),
        ],
        fill: Some(ColourValue::rgb(0.2, 0.4, 0.9)),
        colour_editor: Some(editor_view()),
        status: StatusInfo {
            renderer: "CPU (deterministic)".to_owned(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// A colour editor on the selection's fill, which uses the named colour
/// "Brand red".
fn editor_view() -> ColourEditorView {
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
    ColourEditorView {
        target: ColourTarget::Selection(PaintSlot::Fill),
        title: "Fill of 1 object".to_owned(),
        model: ColourModel::Rgbt,
        model_editable: true,
        components: value.components(),
        editable: [true; 4],
        value,
        original: None,
        dragging: false,
        linked_entry: Some(named(brand, "Brand red", ColourValue::rgb(0.8, 0.1, 0.1))),
        entry: None,
        named: vec![
            named(brand, "Brand red", ColourValue::rgb(0.8, 0.1, 0.1)),
            named(paper, "Paper", ColourValue::rgb(1.0, 1.0, 0.95)),
        ],
        objects: 1,
    }
}

/// Every interactive node of the tree, as (role, label).
fn interactive_nodes(harness: &Harness<'_>) -> Vec<(String, String)> {
    harness
        .root()
        .children()
        .flat_map(collect)
        .filter(|(role, _)| {
            matches!(
                role.as_str(),
                "Button" | "CheckBox" | "Slider" | "TextInput" | "SpinButton" | "Switch"
            )
        })
        .collect()
}

fn collect(node: egui_kittest::Node<'_>) -> Vec<(String, String)> {
    let ak = node.accesskit_node();
    let mut out = vec![(
        format!("{:?}", ak.role()),
        ak.label().unwrap_or_default().to_owned(),
    )];
    for child in node.children() {
        out.extend(collect(child));
    }
    out
}

#[test]
fn the_layer_panel_is_a_labelled_list_not_a_run_of_anonymous_buttons() {
    let m = model();
    let tokens = ThemeTokens::of(ResolvedTheme::Dark);
    let mut panel = LayerPanel::new();
    let mut sink = CommandSink::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(320.0, 400.0))
        .build_ui(|ui| {
            let mut ctx = PanelCtx {
                model: &m,
                tokens: &tokens,
                out: &mut sink,
            };
            panel.ui(ui, &mut ctx);
        });
    harness.run();

    // Each layer is reachable by the name a screen reader reads out.
    harness.get_by_label("Background, visible, unlocked");
    harness.get_by_label("Guides, visible, locked");
    harness.get_by_label("Sky, hidden, unlocked");

    // And its visibility toggle says which layer it belongs to.
    harness.get_by_label("Hide layer Background");
    harness.get_by_label("Show layer Sky");

    let nodes = interactive_nodes(&harness);
    assert!(
        nodes.len() >= 10,
        "only {} interactive nodes: {nodes:?}",
        nodes.len()
    );
    let anonymous: Vec<_> = nodes.iter().filter(|(_, label)| label.is_empty()).collect();
    assert!(
        anonymous.is_empty(),
        "these controls have no accessible name: {anonymous:?}"
    );
}

#[test]
fn the_colour_panel_names_every_swatch_and_marks_document_colours() {
    let mut m = model();
    m.palette[1].named = true;
    let tokens = ThemeTokens::of(ResolvedTheme::Light);
    let mut panel = ColourPanel::new();
    let mut sink = CommandSink::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(360.0, 260.0))
        .build_ui(|ui| {
            let mut ctx = PanelCtx {
                model: &m,
                tokens: &tokens,
                out: &mut sink,
            };
            panel.ui(ui, &mut ctx);
        });
    harness.run();

    harness.get_by_label("No colour");
    harness.get_by_label("Brand red (document colour)");
    harness.get_by_label("Paper");
    // The editor: the 2D field and its slider are sliders with names, the
    // numeric entries spin buttons named after their component.
    harness.get_by_role_and_label(egui::accesskit::Role::Slider, "Red slider");
    harness.get_by_role_and_label(
        egui::accesskit::Role::Slider,
        "Green (across) and Blue (up)",
    );
    for name in ["Red", "Green", "Blue", "Transparency"] {
        harness.get_by_role_and_label(egui::accesskit::Role::SpinButton, name);
    }
    // The explicit "redefine" action names the colour it redefines.
    harness.get_by_label("Redefine \u{2018}Brand red\u{2019}");
    assert_all_named(&harness);
}

fn assert_all_named(harness: &Harness<'_>) {
    let anonymous: Vec<_> = interactive_nodes(harness)
        .into_iter()
        .filter(|(_, label)| label.is_empty())
        .collect();
    assert!(
        anonymous.is_empty(),
        "these controls have no accessible name: {anonymous:?}"
    );
}

#[test]
fn the_derivation_editor_names_every_control() {
    let mut m = model();
    let mut v = editor_view();
    let (brand, paper) = (v.named[0].id, v.named[1].id);
    v.target = ColourTarget::Entry(paper);
    v.linked_entry = None;
    v.editable = [false; 4];
    v.model_editable = false;
    v.entry = Some((
        "Paper".to_owned(),
        Derivation::Tint {
            parent: brand,
            factor: 0.4,
        },
    ));
    m.colour_editor = Some(v);
    let tokens = ThemeTokens::of(ResolvedTheme::Dark);
    let mut panel = ColourPanel::new();
    let mut sink = CommandSink::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(360.0, 700.0))
        .build_ui(|ui| {
            let mut ctx = PanelCtx {
                model: &m,
                tokens: &tokens,
                out: &mut sink,
            };
            panel.ui(ui, &mut ctx);
        });
    harness.run();
    harness.get_by_label("Colour name");
    harness.get_by_label("Colour type: Tint");
    harness.get_by_label("Parent colour: Brand red");
    harness.get_by_role_and_label(egui::accesskit::Role::Slider, "Tint");
    harness.get_by_label("Apply to fill");
    harness.get_by_label("Apply to line");
    assert_all_named(&harness);
}

#[test]
fn the_colour_editor_is_operable_from_the_keyboard_alone() {
    let m = model();
    let tokens = ThemeTokens::of(ResolvedTheme::Dark);
    let mut panel = ColourPanel::new();
    let commands = std::cell::RefCell::new(Vec::new());
    let mut focused = Vec::new();
    {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(360.0, 600.0))
            .build_ui(|ui| {
                let mut sink = CommandSink::new();
                let mut ctx = PanelCtx {
                    model: &m,
                    tokens: &tokens,
                    out: &mut sink,
                };
                panel.ui(ui, &mut ctx);
                commands.borrow_mut().extend(sink.drain());
            });
        harness.run();
        // Tab until the 2D field has the focus, then step it.
        for _ in 0..30 {
            harness.key_press(egui::Key::Tab);
            harness.run();
            let now = harness.root().children().find_map(find_focused);
            if let Some(n) = &now {
                focused.push(n.clone());
            }
            if now.as_deref() == Some("Green (across) and Blue (up)") {
                break;
            }
        }
        commands.borrow_mut().clear();
        harness.key_press(egui::Key::ArrowRight);
        harness.run();
        harness.key_press(egui::Key::ArrowUp);
        harness.run();
    }
    assert!(
        focused.iter().any(|f| f == "Green (across) and Blue (up)"),
        "the field is reachable with Tab: {focused:?}"
    );
    let seen = commands.borrow();
    let sets: Vec<[f32; 4]> = seen
        .iter()
        .filter_map(|c| match c {
            UiCommand::ColourEditor(ColourEditorOp::Set(ColourChange::Components(c))) => Some(*c),
            _ => None,
        })
        .collect();
    assert_eq!(sets.len(), 2, "one step per arrow press: {seen:?}");
    assert!(
        (sets[0][1] - 0.41).abs() < 1e-5,
        "right moves green: {sets:?}"
    );
    assert!((sets[1][2] - 0.91).abs() < 1e-5, "up moves blue: {sets:?}");
}

#[test]
fn the_layer_panel_is_operable_from_the_keyboard_alone() {
    let m = model();
    let tokens = ThemeTokens::of(ResolvedTheme::Dark);
    let mut panel = LayerPanel::new();
    let commands = std::cell::RefCell::new(Vec::new());
    {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(320.0, 400.0))
            .build_ui(|ui| {
                let mut sink = CommandSink::new();
                let mut ctx = PanelCtx {
                    model: &m,
                    tokens: &tokens,
                    out: &mut sink,
                };
                panel.ui(ui, &mut ctx);
                commands.borrow_mut().extend(sink.drain());
            });
        harness.run();
        commands.borrow_mut().clear();

        // No pointer is ever used in this test.
        harness.key_press(egui::Key::Space);
        harness.run();
    }
    let seen = commands.borrow();
    assert!(
        seen.iter()
            .any(|c| matches!(c, UiCommand::SetLayerVisible { .. })),
        "space must toggle the active layer's visibility: {seen:?}"
    );
}

#[test]
fn tab_moves_the_focus_through_the_panel() {
    let m = model();
    let tokens = ThemeTokens::of(ResolvedTheme::Dark);
    let mut panel = LayerPanel::new();
    let mut sink = CommandSink::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(320.0, 400.0))
        .build_ui(|ui| {
            let mut ctx = PanelCtx {
                model: &m,
                tokens: &tokens,
                out: &mut sink,
            };
            panel.ui(ui, &mut ctx);
        });
    harness.run();

    let mut focused = Vec::new();
    for _ in 0..6 {
        harness.key_press(egui::Key::Tab);
        harness.run();
        if let Some(node) = harness.root().children().find_map(find_focused) {
            focused.push(node);
        }
    }
    assert!(
        focused.len() >= 3,
        "tab reached only {} widgets: {focused:?}",
        focused.len()
    );
    assert!(
        focused.iter().all(|label: &String| !label.is_empty()),
        "a focusable widget with no name: {focused:?}"
    );
}

fn find_focused(node: egui_kittest::Node<'_>) -> Option<String> {
    let ak = node.accesskit_node();
    if ak.is_focused() {
        return Some(ak.label().unwrap_or_default().to_owned());
    }
    node.children().find_map(find_focused)
}
