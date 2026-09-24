//! The questions the application asks the user.
//!
//! A [`Prompt`] is what the core wants answered before it goes on: unsaved
//! changes before a close, a quit or an open; a document locked by another
//! session; autosaves to recover. The core never draws it. The interface
//! shows [`crate::AppState::prompt`] as a modal dialog *inside the window*
//! (no portal has a question dialog) and answers with
//! [`crate::Intent::AnswerPrompt`]; nothing else happens until it does.

/// One possible answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PromptAnswer {
    /// Save, then go on.
    Save,
    /// Go on without saving.
    Discard,
    /// Do nothing. Also what Escape and the window's close button answer.
    Cancel,
    /// Open a locked document without taking its lock; saving asks for a
    /// new name.
    OpenReadOnly,
    /// Open a locked document as an untitled copy.
    OpenCopy,
    /// Take the lock from its holder.
    Force,
    /// Open the autosaved snapshots.
    Recover,
    /// Delete the autosaved snapshots.
    DiscardRecovery,
    /// Keep the autosaved snapshots and ask again next time.
    Later,
    /// After a crash: switch this session to safe mode.
    SafeMode,
    /// After a crash: put the crash report's path on the clipboard.
    CopyReport,
    /// After a crash: carry on as normal.
    Continue,
}

/// How a choice is presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoiceRole {
    /// The suggested answer: Enter picks it.
    Default,
    /// Loses work or takes a risk.
    Destructive,
    /// Leaves everything as it was: Escape picks it.
    Cancel,
    /// Anything else.
    Normal,
}

/// A button of a prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptChoice {
    /// What it answers.
    pub answer: PromptAnswer,
    /// Its label.
    pub label: &'static str,
    /// How to present it.
    pub role: ChoiceRole,
}

/// A question for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// The dialog's title, which is also its accessible name.
    pub title: String,
    /// The question, in a sentence or two.
    pub message: String,
    /// The buttons, in the order they are shown.
    pub choices: Vec<PromptChoice>,
}

impl Prompt {
    /// The answer Escape gives: the cancel choice, or the last one.
    #[must_use]
    pub fn cancel_answer(&self) -> Option<PromptAnswer> {
        self.choices
            .iter()
            .find(|c| c.role == ChoiceRole::Cancel)
            .or_else(|| self.choices.last())
            .map(|c| c.answer)
    }

    /// The answer Enter gives, if one is marked as the default.
    #[must_use]
    pub fn default_answer(&self) -> Option<PromptAnswer> {
        self.choices
            .iter()
            .find(|c| c.role == ChoiceRole::Default)
            .map(|c| c.answer)
    }

    /// Whether `answer` is one of this prompt's choices.
    #[must_use]
    pub fn offers(&self, answer: PromptAnswer) -> bool {
        self.choices.iter().any(|c| c.answer == answer)
    }

    /// "Save changes to drawing.xarast before closing?"
    #[must_use]
    pub fn unsaved(name: &str, action: &str) -> Prompt {
        Prompt {
            title: "Unsaved changes".to_owned(),
            message: format!(
                "Save the changes to \u{201c}{name}\u{201d} before {action}? \
                 If you don't, they will be lost."
            ),
            choices: vec![
                PromptChoice {
                    answer: PromptAnswer::Save,
                    label: "Save",
                    role: ChoiceRole::Default,
                },
                PromptChoice {
                    answer: PromptAnswer::Discard,
                    label: "Discard",
                    role: ChoiceRole::Destructive,
                },
                PromptChoice {
                    answer: PromptAnswer::Cancel,
                    label: "Cancel",
                    role: ChoiceRole::Cancel,
                },
            ],
        }
    }

    /// "drawing.xarast is open in another session."
    #[must_use]
    pub fn locked(name: &str, holder: &str) -> Prompt {
        Prompt {
            title: "Document in use".to_owned(),
            message: format!(
                "\u{201c}{name}\u{201d} is open in another session{holder}. \
                 Two sessions saving the same file overwrite each other's work."
            ),
            choices: vec![
                PromptChoice {
                    answer: PromptAnswer::OpenReadOnly,
                    label: "Open read-only",
                    role: ChoiceRole::Default,
                },
                PromptChoice {
                    answer: PromptAnswer::OpenCopy,
                    label: "Open a copy",
                    role: ChoiceRole::Normal,
                },
                PromptChoice {
                    answer: PromptAnswer::Force,
                    label: "Force (risky)",
                    role: ChoiceRole::Destructive,
                },
                PromptChoice {
                    answer: PromptAnswer::Cancel,
                    label: "Cancel",
                    role: ChoiceRole::Cancel,
                },
            ],
        }
    }

    /// "Recover unsaved work?"
    #[must_use]
    pub fn recovery(names: &[String]) -> Prompt {
        let shown: Vec<&str> = names.iter().take(5).map(String::as_str).collect();
        let more = names.len().saturating_sub(shown.len());
        let mut list = shown
            .iter()
            .map(|n| format!("\u{201c}{n}\u{201d}"))
            .collect::<Vec<_>>()
            .join(", ");
        if more > 0 {
            list.push_str(&format!(" and {more} more"));
        }
        let what = if names.len() == 1 {
            "A document was"
        } else {
            "Some documents were"
        };
        Prompt {
            title: "Recover unsaved work".to_owned(),
            message: format!(
                "{what} not saved when Xarast last closed: {list}. \
                 Recover the autosaved copies?"
            ),
            choices: vec![
                PromptChoice {
                    answer: PromptAnswer::Recover,
                    label: "Recover",
                    role: ChoiceRole::Default,
                },
                PromptChoice {
                    answer: PromptAnswer::DiscardRecovery,
                    label: "Discard",
                    role: ChoiceRole::Destructive,
                },
                PromptChoice {
                    answer: PromptAnswer::Later,
                    label: "Later",
                    role: ChoiceRole::Cancel,
                },
            ],
        }
    }

    /// "Xarast closed unexpectedly" (XARA-US-0064 F7): where the crash
    /// report is, what safe mode is, and whether this session is in it.
    #[must_use]
    pub fn crashed(notice: &crate::crash::CrashNotice) -> Prompt {
        use crate::crash::SafeMode;
        let mut message = "Xarast closed unexpectedly last time.".to_owned();
        match &notice.report {
            Some(path) => message.push_str(&format!(
                " A crash report was saved to \u{201c}{}\u{201d}. It holds no \
                 document content and is never sent anywhere; attach it to a \
                 bug report if you want to.",
                path.display()
            )),
            None => message.push_str(" No crash report was written."),
        }
        let mut choices = Vec::new();
        match notice.safe_mode {
            SafeMode::Forced => message.push_str(&format!(
                " It has closed unexpectedly {} times in the last few minutes, so \
                 this session runs in safe mode: the canvas is drawn without GPU \
                 tiles, on a software graphics adapter where there is one, with \
                 default settings.",
                notice.recent_crashes
            )),
            SafeMode::Requested => {
                message.push_str(" This session runs in safe mode.");
            }
            SafeMode::Offered | SafeMode::Off => {
                message.push_str(
                    " Safe mode draws the canvas without GPU tiles, on a software \
                     graphics adapter where there is one, with default settings.",
                );
                choices.push(PromptChoice {
                    answer: PromptAnswer::SafeMode,
                    label: "Use safe mode",
                    role: ChoiceRole::Normal,
                });
            }
        }
        if notice.report.is_some() {
            choices.push(PromptChoice {
                answer: PromptAnswer::CopyReport,
                label: "Copy report path",
                role: ChoiceRole::Normal,
            });
        }
        choices.push(PromptChoice {
            answer: PromptAnswer::Continue,
            label: "Continue",
            role: ChoiceRole::Default,
        });
        Prompt {
            title: "Xarast closed unexpectedly".to_owned(),
            message,
            choices,
        }
    }
}
