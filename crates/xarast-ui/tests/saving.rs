//! File › Save, Save As… and the application's questions (XARA-US-0084),
//! driven headlessly through the AccessKit tree a screen reader would read.

use std::cell::RefCell;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use xarast_app::{AppCommand, Prompt, PromptAnswer};
use xarast_ui::model::{DocumentView, LayerInfo, UiCommand, UiModel};
use xarast_ui::{Scale, Workspace};

const MENU_ITEM: egui::accesskit::Role = egui::accesskit::Role::MenuItem;

fn with_document() -> UiModel {
    UiModel {
        document: Some(DocumentView {
            title: "poster.xarast".to_owned(),
            layers: vec![LayerInfo::new(1, "Background")],
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
fn file_save_and_save_as_raise_their_commands_with_their_keys() {
    for (label, command, key) in [
        ("Save", AppCommand::Save, "Ctrl+S"),
        ("Save As…", AppCommand::SaveAs, "Ctrl+Shift+S"),
    ] {
        let model = with_document();
        let commands = RefCell::new(Vec::new());
        let mut h = harness(&model, &commands);
        h.get_by_label("File").click();
        h.run();
        let item = h.get_by_role_and_label(MENU_ITEM, label);
        assert_eq!(item.accesskit_node().data().keyboard_shortcut(), Some(key));
        item.click();
        h.run();
        drop(h);
        assert_eq!(
            commands.into_inner(),
            vec![UiCommand::App(command)],
            "{label}"
        );
    }
}

#[test]
fn a_prompt_is_a_modal_dialog_whose_buttons_answer_it() {
    for (label, answer) in [
        ("Save", PromptAnswer::Save),
        ("Discard", PromptAnswer::Discard),
        ("Cancel", PromptAnswer::Cancel),
    ] {
        let model = UiModel {
            prompt: Some(Prompt::unsaved("poster.xarast", "closing")),
            ..with_document()
        };
        let commands = RefCell::new(Vec::new());
        let mut h = harness(&model, &commands);
        h.get_by_label("Unsaved changes");
        h.get_by_label(label).click();
        h.run();
        drop(h);
        let got = commands.into_inner();
        assert!(
            got.contains(&UiCommand::AnswerPrompt(answer)),
            "{label}: {got:?}"
        );
    }
}

#[test]
fn escape_gives_the_cancel_answer() {
    let model = UiModel {
        prompt: Some(Prompt::locked("shared.xarast", "")),
        ..with_document()
    };
    let commands = RefCell::new(Vec::new());
    let mut h = harness(&model, &commands);
    h.get_by_label("Force (risky)");
    h.get_by_label("Open a copy");
    h.key_press(egui::Key::Escape);
    h.run();
    drop(h);
    let got = commands.into_inner();
    assert!(
        got.contains(&UiCommand::AnswerPrompt(PromptAnswer::Cancel)),
        "{got:?}"
    );
}
