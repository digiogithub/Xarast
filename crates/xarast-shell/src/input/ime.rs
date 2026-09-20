//! The input-method seam.
//!
//! Text editing is phase 9's. What phase 5 owes it is a seam that is already
//! correct: the composition state machine, the request to enable the IME, and
//! the cursor rectangle the candidate window is positioned against. Getting
//! those wrong is only discovered by someone typing Japanese into a shipped
//! build, so they are modelled and tested now, with no text tool in sight.
//!
//! The rule the rest of the application depends on: while
//! [`ImeState::is_composing`] is true, key events are the input method's, not
//! the application's. A shortcut fired from a key that was really a
//! composition keystroke is the classic IME bug.

use crate::scale::{LogicalSize, PhysicalPos};

/// An input-method event from the platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImeEvent {
    /// The input method became active for this window.
    Enabled,
    /// The composition text changed.
    ///
    /// `cursor` is a byte range within `text`. It is validated on the way in:
    /// a platform that reports a range that is not on a character boundary
    /// must not be able to panic us.
    Preedit {
        /// The text being composed.
        text: String,
        /// Selection within the composition, as a byte range.
        cursor: Option<(usize, usize)>,
    },
    /// The composition was accepted; this is the text to insert.
    Commit(String),
    /// The input method became inactive.
    Disabled,
}

/// What changed when an [`ImeEvent`] was applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImeChange {
    /// Nothing observable changed.
    None,
    /// The composition text or its cursor changed; redraw the caret area.
    PreeditChanged,
    /// Text was committed and should be inserted.
    Committed,
    /// Composition started.
    Started,
    /// Composition ended without a commit.
    Cancelled,
}

/// Where the platform should put the candidate window.
///
/// In logical units relative to the window, because that is what every
/// platform wants and it keeps the fractional scale out of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImeCursorArea {
    /// Top-left of the caret, in logical units.
    pub origin: PhysicalPos,
    /// Size of the caret box, in logical units.
    pub size: LogicalSize,
}

/// The composition state of one window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImeState {
    enabled: bool,
    preedit: String,
    cursor: Option<(usize, usize)>,
    last_commit: Option<String>,
}

impl ImeState {
    /// An inactive state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// True while an input method is composing text.
    ///
    /// Key events must not run shortcuts while this holds.
    #[must_use]
    pub fn is_composing(&self) -> bool {
        self.enabled && !self.preedit.is_empty()
    }

    /// True while the input method is active for this window.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// The text currently being composed.
    #[must_use]
    pub fn preedit(&self) -> &str {
        &self.preedit
    }

    /// The selection within the composition, as a validated byte range.
    #[must_use]
    pub const fn cursor(&self) -> Option<(usize, usize)> {
        self.cursor
    }

    /// The most recent committed text, if it has not been taken yet.
    #[must_use]
    pub fn last_commit(&self) -> Option<&str> {
        self.last_commit.as_deref()
    }

    /// Takes the committed text, leaving none.
    pub fn take_commit(&mut self) -> Option<String> {
        self.last_commit.take()
    }

    /// Applies an event and says what changed.
    pub fn apply(&mut self, event: ImeEvent) -> ImeChange {
        match event {
            ImeEvent::Enabled => {
                if self.enabled {
                    return ImeChange::None;
                }
                self.enabled = true;
                self.preedit.clear();
                self.cursor = None;
                ImeChange::Started
            }
            ImeEvent::Preedit { text, cursor } => {
                let cursor = cursor.and_then(|r| clamp_range(&text, r));
                if self.preedit == text && self.cursor == cursor {
                    return ImeChange::None;
                }
                self.preedit = text;
                self.cursor = cursor;
                ImeChange::PreeditChanged
            }
            ImeEvent::Commit(text) => {
                self.preedit.clear();
                self.cursor = None;
                self.last_commit = Some(text);
                ImeChange::Committed
            }
            ImeEvent::Disabled => {
                let was = self.enabled || !self.preedit.is_empty();
                self.enabled = false;
                self.preedit.clear();
                self.cursor = None;
                if was {
                    ImeChange::Cancelled
                } else {
                    ImeChange::None
                }
            }
        }
    }
}

/// Clamps a byte range onto character boundaries within `text`.
///
/// A range past the end, inverted, or splitting a multi-byte character is
/// repaired rather than trusted. Slicing a `String` on a bad boundary panics,
/// and a panic in the event loop takes the document with it.
fn clamp_range(text: &str, (start, end): (usize, usize)) -> Option<(usize, usize)> {
    let floor = |i: usize| {
        let mut i = i.min(text.len());
        while i > 0 && !text.is_char_boundary(i) {
            i -= 1;
        }
        i
    };
    let (a, b) = (floor(start), floor(end));
    Some(if a <= b { (a, b) } else { (b, a) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_state_is_neither_enabled_nor_composing() {
        let s = ImeState::new();
        assert!(!s.is_enabled() && !s.is_composing());
        assert_eq!(s.preedit(), "");
    }

    #[test]
    fn a_full_composition_runs_start_preedit_commit() {
        let mut s = ImeState::new();
        assert_eq!(s.apply(ImeEvent::Enabled), ImeChange::Started);
        assert!(!s.is_composing(), "enabling alone is not composing");

        assert_eq!(
            s.apply(ImeEvent::Preedit {
                text: "にほ".to_owned(),
                cursor: Some((0, 6)),
            }),
            ImeChange::PreeditChanged
        );
        assert!(s.is_composing());
        assert_eq!(s.preedit(), "にほ");

        assert_eq!(
            s.apply(ImeEvent::Commit("日本".to_owned())),
            ImeChange::Committed
        );
        assert!(!s.is_composing(), "a commit ends the composition");
        assert_eq!(s.last_commit(), Some("日本"));
        assert_eq!(s.take_commit().as_deref(), Some("日本"));
        assert_eq!(s.last_commit(), None);
    }

    #[test]
    fn an_unchanged_preedit_reports_nothing() {
        let mut s = ImeState::new();
        s.apply(ImeEvent::Enabled);
        let ev = || ImeEvent::Preedit {
            text: "ab".to_owned(),
            cursor: Some((1, 1)),
        };
        assert_eq!(s.apply(ev()), ImeChange::PreeditChanged);
        assert_eq!(s.apply(ev()), ImeChange::None);
    }

    #[test]
    fn disabling_cancels_an_unfinished_composition() {
        let mut s = ImeState::new();
        s.apply(ImeEvent::Enabled);
        s.apply(ImeEvent::Preedit {
            text: "abc".to_owned(),
            cursor: None,
        });
        assert_eq!(s.apply(ImeEvent::Disabled), ImeChange::Cancelled);
        assert!(!s.is_enabled() && !s.is_composing());
        assert_eq!(s.apply(ImeEvent::Disabled), ImeChange::None);
    }

    #[test]
    fn enabling_twice_is_idempotent() {
        let mut s = ImeState::new();
        assert_eq!(s.apply(ImeEvent::Enabled), ImeChange::Started);
        assert_eq!(s.apply(ImeEvent::Enabled), ImeChange::None);
    }

    #[test]
    fn a_cursor_range_inside_a_multibyte_character_is_repaired() {
        let mut s = ImeState::new();
        s.apply(ImeEvent::Enabled);
        // "に" is three bytes; byte 1 and byte 2 are not boundaries.
        s.apply(ImeEvent::Preedit {
            text: "にほ".to_owned(),
            cursor: Some((1, 5)),
        });
        let (a, b) = s.cursor().unwrap();
        assert!(s.preedit().is_char_boundary(a));
        assert!(s.preedit().is_char_boundary(b));
        // Slicing must not panic; that is the point of the repair.
        let _ = &s.preedit()[a..b];
    }

    #[test]
    fn a_cursor_range_past_the_end_is_clamped() {
        let mut s = ImeState::new();
        s.apply(ImeEvent::Enabled);
        s.apply(ImeEvent::Preedit {
            text: "ab".to_owned(),
            cursor: Some((99, 400)),
        });
        assert_eq!(s.cursor(), Some((2, 2)));
    }

    #[test]
    fn an_inverted_cursor_range_is_put_back_in_order() {
        let mut s = ImeState::new();
        s.apply(ImeEvent::Enabled);
        s.apply(ImeEvent::Preedit {
            text: "abcd".to_owned(),
            cursor: Some((3, 1)),
        });
        assert_eq!(s.cursor(), Some((1, 3)));
    }
}
