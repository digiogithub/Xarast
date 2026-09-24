//! The text tool's infobar (phase 9, T9.4.9) driven headlessly through the
//! AccessKit tree: the size field, the font chooser and the OpenType panel
//! raise the edits the text tool applies.

use std::cell::RefCell;
use std::sync::Arc;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use xarast_app::{FeatureOption, Infobar, InfobarField, InfobarItem, InfobarValue, ToolId};
use xarast_ui::model::{DocumentView, EditingView, LayerInfo, UiCommand, UiModel};
use xarast_ui::toolbar::{format_scalar, parse_scalar};
use xarast_ui::{Scale, Workspace};

fn model() -> UiModel {
    let families: Arc<[Arc<str>]> =
        Arc::from(vec![Arc::from("Alpha Sans"), Arc::from("Beta Serif")]);
    UiModel {
        document: Some(DocumentView {
            title: "text.xar".to_owned(),
            layers: vec![LayerInfo::new(1, "Background")],
            ..Default::default()
        }),
        editing: Some(EditingView {
            tool: ToolId::Text,
            infobar: Infobar {
                items: vec![
                    InfobarItem::FontFamily {
                        field: InfobarField::TextFont,
                        families,
                        selected: Some(Arc::from("Missing Font")),
                    },
                    InfobarItem::Scalar {
                        field: InfobarField::TextSize,
                        value: Some(20.0),
                        suffix: "pt",
                        min: 0.5,
                        max: 5_000.0,
                    },
                    InfobarItem::Features {
                        options: vec![
                            FeatureOption {
                                tag: *b"liga",
                                label: "Standard ligatures",
                                on: Some(true),
                            },
                            FeatureOption {
                                tag: *b"smcp",
                                label: "Small capitals",
                                on: Some(false),
                            },
                        ],
                    },
                ],
            },
            ..Default::default()
        }),
        ..Default::default()
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
fn scalars_format_and_parse_with_their_suffix() {
    assert_eq!(format_scalar(20.0, "pt"), "20pt");
    assert_eq!(format_scalar(12.5, "%"), "12.5%");
    assert_eq!(format_scalar(-0.001, ""), "0");
    assert_eq!(parse_scalar(" 14 pt", "pt"), Some(14.0));
    assert_eq!(parse_scalar("150%", "%"), Some(150.0));
    assert_eq!(parse_scalar("-40", ""), Some(-40.0));
    assert_eq!(parse_scalar("abc", "pt"), None);
    assert_eq!(parse_scalar("inf", ""), None);
}

#[test]
fn typing_a_size_raises_it_in_points_clamped() {
    let m = model();
    let commands = RefCell::new(Vec::new());
    let mut h = harness(&m, &commands);
    let size = h.get_by_label_contains("Font size in points");
    assert!(
        size.accesskit_node()
            .data()
            .label()
            .unwrap()
            .contains("20pt")
    );
    size.click();
    h.run();
    h.key_combination_modifiers(egui::Modifiers::COMMAND, &[egui::Key::A]);
    h.run();
    h.get_by_label_contains("Font size in points")
        .type_text("9000");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    drop(h);
    assert_eq!(
        commands.into_inner(),
        vec![UiCommand::InfobarEdit {
            field: InfobarField::TextSize,
            value: InfobarValue::Real(5_000.0),
        }]
    );
}

#[test]
fn the_font_chooser_shows_a_missing_family_and_raises_the_chosen_index() {
    let m = model();
    let commands = RefCell::new(Vec::new());
    let mut h = harness(&m, &commands);
    h.get_by_label_contains("Font family of the text: Missing Font")
        .click();
    h.run();
    h.get_by_label("Beta Serif").click();
    h.run();
    drop(h);
    assert_eq!(
        commands.into_inner(),
        vec![UiCommand::InfobarEdit {
            field: InfobarField::TextFont,
            value: InfobarValue::Choice(1),
        }]
    );
}

#[test]
fn the_opentype_panel_switches_a_feature_by_its_tag() {
    let m = model();
    let commands = RefCell::new(Vec::new());
    let mut h = harness(&m, &commands);
    h.get_by_label("OpenType features").click();
    h.run();
    h.get_by_label_contains("Small capitals (smcp): off")
        .click();
    h.run();
    drop(h);
    assert_eq!(
        commands.into_inner(),
        vec![UiCommand::InfobarEdit {
            field: InfobarField::TextFeature(*b"smcp"),
            value: InfobarValue::Toggle(true),
        }]
    );
}
