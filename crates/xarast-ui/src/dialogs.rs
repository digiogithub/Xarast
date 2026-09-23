//! The application's questions, as modal dialogs inside the window.
//!
//! The application core decides what to ask ([`xarast_app::Prompt`]:
//! unsaved changes, a locked document, recovery) and waits for the answer;
//! this draws it over everything, with its choices as buttons in the order
//! given. Escape (or a click outside) gives the cancel answer and Enter the
//! default one. Nothing is kept here: the question stays on screen for as
//! long as the model carries it.

use xarast_app::{ChoiceRole, Prompt};

use crate::model::{CommandSink, UiCommand};
use crate::theme::ThemeTokens;

/// The id of the prompt's modal area.
pub const PROMPT_ID: &str = "xarast_prompt";

/// Draws `prompt` as a modal dialog and pushes the answer, if one is given
/// this frame.
pub fn prompt(ctx: &egui::Context, prompt: &Prompt, tokens: &ThemeTokens, out: &mut CommandSink) {
    let mut answer = None;
    let response = egui::Modal::new(egui::Id::new(PROMPT_ID)).show(ctx, |ui| {
        ui.set_max_width(420.0);
        ui.heading(&prompt.title);
        ui.add_space(6.0);
        ui.label(&prompt.message);
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            for choice in &prompt.choices {
                let text = match choice.role {
                    ChoiceRole::Default => egui::RichText::new(choice.label).strong(),
                    ChoiceRole::Destructive => {
                        egui::RichText::new(choice.label).color(tokens.error)
                    }
                    ChoiceRole::Cancel | ChoiceRole::Normal => egui::RichText::new(choice.label),
                };
                let b = ui.button(text);
                if choice.role == ChoiceRole::Default && !b.has_focus() {
                    // Keyboard users land on the suggested answer.
                    ui.memory_mut(|m| {
                        if m.focused().is_none() {
                            m.request_focus(b.id);
                        }
                    });
                }
                if b.clicked() {
                    answer = Some(choice.answer);
                }
            }
        });
    });
    if answer.is_none() && response.should_close() {
        answer = prompt.cancel_answer();
    }
    if answer.is_none()
        && ctx.input(|i| i.key_pressed(egui::Key::Enter))
        && ctx.memory(|m| m.focused().is_none())
    {
        answer = prompt.default_answer();
    }
    if let Some(a) = answer {
        out.push(UiCommand::AnswerPrompt(a));
    }
}
