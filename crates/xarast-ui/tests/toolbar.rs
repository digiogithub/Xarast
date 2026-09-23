//! The tool palette, the Edit menu and the infobar, driven headlessly
//! through the AccessKit tree a screen reader would read.

use std::cell::RefCell;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use xarast_app::{AppCommand, Infobar, InfobarField, InfobarItem, ToolId};
use xarast_geom::Mp;
use xarast_ui::model::{DocumentView, EditingView, LayerInfo, UiCommand, UiModel};
use xarast_ui::units::Unit;
use xarast_ui::{Scale, Workspace};

const MENU_ITEM: egui::accesskit::Role = egui::accesskit::Role::MenuItem;
const BUTTON: egui::accesskit::Role = egui::accesskit::Role::Button;

fn model(editing: EditingView) -> UiModel {
    UiModel {
        document: Some(DocumentView {
            title: "poster.xar".to_owned(),
            layers: vec![LayerInfo::new(1, "Background")],
            unit: Unit::Millimetre,
            ..Default::default()
        }),
        editing: Some(editing),
        ..Default::default()
    }
}

fn selector_bar(x: Option<Mp>) -> Infobar {
    Infobar {
        items: vec![
            InfobarItem::Measure {
                field: InfobarField::X,
                value: x,
                editable: x.is_some(),
            },
            InfobarItem::Measure {
                field: InfobarField::W,
                value: x,
                editable: false,
            },
        ],
    }
}

fn harness<'a>(model: &'a UiModel, commands: &'a RefCell<Vec<UiCommand>>) -> Harness<'a> {
    let mut workspace = Workspace::new();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build(move |ctx| {
            let out = workspace.ui(ctx, model, Scale::new(1.0), &[]);
            commands.borrow_mut().extend(out.commands);
        });
    h.run();
    h
}

#[test]
fn the_palette_lists_every_tool_on_the_left_with_the_active_one_pressed() {
    let m = model(EditingView {
        tool: ToolId::Selector,
        ..Default::default()
    });
    let commands = RefCell::new(Vec::new());
    let h = harness(&m, &commands);
    let mut last_y = 0.0;
    for tool in ToolId::ALL {
        // The colour panel has a "Fill" button of its own; the palette's
        // is the one in the left strip.
        let node = h
            .query_all_by_role_and_label(BUTTON, tool.label())
            .find(|n| n.rect().max.x < 60.0)
            .unwrap_or_else(|| panic!("{tool:?} is not in the left strip"));
        let r = node.rect();
        assert!(r.min.y > last_y, "{tool:?} is out of order");
        last_y = r.min.y;
        let ak = node.accesskit_node();
        let data = ak.data();
        let pressed = data.toggled() == Some(egui::accesskit::Toggled::True);
        assert_eq!(pressed, tool == ToolId::Selector, "{tool:?}");
        assert_eq!(data.is_disabled(), !tool.is_available(), "{tool:?}");
        if let Some(k) = AppCommand::Tool(tool).primary_shortcut() {
            assert_eq!(data.keyboard_shortcut(), Some(k.to_string().as_str()));
        }
    }
    drop(h);
    assert!(commands.into_inner().is_empty());
}

#[test]
fn clicking_a_tool_raises_its_command_and_a_later_phase_tool_does_nothing() {
    let m = model(EditingView::default());
    let commands = RefCell::new(Vec::new());
    let mut h = harness(&m, &commands);
    h.get_by_role_and_label(BUTTON, "Rectangle").click();
    h.run();
    h.get_by_role_and_label(BUTTON, "Text").click();
    h.run();
    // The chosen tool again is not a new command.
    h.get_by_role_and_label(BUTTON, "Selector").click();
    h.run();
    drop(h);
    assert_eq!(
        commands.into_inner(),
        vec![UiCommand::App(AppCommand::Tool(ToolId::Rectangle))]
    );
}

#[test]
fn the_edit_menu_names_what_undo_and_redo_would_do() {
    let m = model(EditingView {
        undo: Some("Move".to_owned()),
        redo: None,
        selected: 1,
        ..Default::default()
    });
    let commands = RefCell::new(Vec::new());
    let mut h = harness(&m, &commands);
    h.get_by_label("Edit").click();
    h.run();
    let undo = h.get_by_role_and_label(MENU_ITEM, "Undo Move");
    assert_eq!(
        undo.accesskit_node().data().keyboard_shortcut(),
        Some("Ctrl+Z")
    );
    let redo = h.get_by_role_and_label(MENU_ITEM, "Redo");
    assert!(redo.accesskit_node().data().is_disabled());
    assert_eq!(
        redo.accesskit_node().data().keyboard_shortcut(),
        Some("Ctrl+Shift+Z")
    );
    h.get_by_role_and_label(MENU_ITEM, "Delete");
    h.get_by_role_and_label(MENU_ITEM, "Undo Move").click();
    h.run();
    drop(h);
    assert_eq!(
        commands.into_inner(),
        vec![UiCommand::App(AppCommand::Undo)]
    );
}

#[test]
fn the_infobar_shows_the_tool_and_parses_what_is_typed() {
    let m = model(EditingView {
        tool: ToolId::Selector,
        infobar: selector_bar(Some(Mp::from_mm(20.0))),
        selected: 1,
        ..Default::default()
    });
    let commands = RefCell::new(Vec::new());
    let mut h = harness(&m, &commands);
    // Under the menu bar, above the canvas.
    let name = h.get_by_role_and_label(egui::accesskit::Role::Label, "Selector");
    assert!(name.rect().min.y > 20.0 && name.rect().max.y < 70.0);
    let x = h.get_by_label_contains("Horizontal position");
    assert!(x.accesskit_node().data().label().unwrap().contains("20"));
    x.click();
    h.run();
    h.key_combination_modifiers(egui::Modifiers::COMMAND, &[egui::Key::A]);
    h.run();
    h.get_by_label_contains("Horizontal position")
        .type_text("1in");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    drop(h);
    assert_eq!(
        commands.into_inner(),
        vec![UiCommand::InfobarEdit {
            field: InfobarField::X,
            value: Mp::from_pt(72.0),
        }]
    );
}

#[test]
fn a_pending_tool_says_it_is_coming_soon() {
    let m = model(EditingView {
        tool: ToolId::Rectangle,
        infobar: Infobar {
            items: vec![InfobarItem::Note(
                "The Rectangle tool is coming soon.".to_owned(),
            )],
        },
        ..Default::default()
    });
    let commands = RefCell::new(Vec::new());
    let h = harness(&m, &commands);
    h.get_by_label("The Rectangle tool is coming soon.");
}
