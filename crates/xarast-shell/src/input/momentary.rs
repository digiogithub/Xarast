//! Momentary tool switching: hold a key, use another tool, let go and the
//! chosen tool is back (`research/04 §4.6`, the `ToolSwitch` note).
//!
//! | Held | Tool |
//! |---|---|
//! | `Space` | Selector |
//! | `Alt+S` | Selector |
//! | `Alt+Z` | Zoom |
//! | `Alt+X` | Push |
//!
//! The press raises [`Intent::MomentaryTool`] with the tool, the release of
//! the *same* key raises it with `None`, whatever the modifiers are by
//! then: letting go of `Alt` before `Z` must not strand the zoom tool.
//! Auto-repeat presses are swallowed. Losing focus releases the switch, as
//! the key-up will never arrive.

use xarast_app::{Intent, ToolId};

use super::keyboard::{Key, KeyEvent, KeyState, NamedKey};

/// Which physical key holds the switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HoldKey {
    Space,
    Letter(char),
}

impl HoldKey {
    fn of(key: &Key) -> Option<HoldKey> {
        match key {
            Key::Named(NamedKey::Space) => Some(HoldKey::Space),
            Key::Character(s) if s == " " => Some(HoldKey::Space),
            Key::Character(s) => {
                let mut chars = s.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => Some(HoldKey::Letter(c.to_ascii_lowercase())),
                    _ => None,
                }
            }
            Key::Named(_) => None,
        }
    }
}

/// The tool a key press switches to momentarily, if it is one of the
/// switches.
#[must_use]
pub fn momentary_tool(event: &KeyEvent) -> Option<ToolId> {
    let m = event.modifiers;
    if m.ctrl || m.logo {
        return None;
    }
    match HoldKey::of(&event.key)? {
        HoldKey::Space if !m.alt => Some(ToolId::Selector),
        HoldKey::Letter('s') if m.alt => Some(ToolId::Selector),
        HoldKey::Letter('z') if m.alt => Some(ToolId::Zoom),
        HoldKey::Letter('x') if m.alt => Some(ToolId::Pan),
        _ => None,
    }
}

/// Tracks the one momentary switch that may be held.
#[derive(Debug, Default)]
pub struct MomentarySwitch {
    held: Option<HoldKey>,
}

impl MomentarySwitch {
    /// Nothing held.
    #[must_use]
    pub fn new() -> MomentarySwitch {
        MomentarySwitch::default()
    }

    /// Whether a switch is held.
    #[must_use]
    pub const fn is_held(&self) -> bool {
        self.held.is_some()
    }

    /// Feeds a key event. Returns the intent it raises, if any; an event
    /// that returns `Some` — or a repeat of the held key — is consumed and
    /// must not also run a shortcut.
    pub fn key(&mut self, event: &KeyEvent) -> (Option<Intent>, bool) {
        let key = HoldKey::of(&event.key);
        match event.state {
            KeyState::Pressed => {
                if self.held.is_some() {
                    // A repeat of the held key is swallowed; anything else
                    // goes on to the shortcut table.
                    return (None, key.is_some() && key == self.held);
                }
                if event.repeat {
                    return (None, false);
                }
                match momentary_tool(event) {
                    Some(tool) => {
                        self.held = key;
                        (Some(Intent::MomentaryTool(Some(tool))), true)
                    }
                    None => (None, false),
                }
            }
            KeyState::Released => {
                if key.is_some() && key == self.held {
                    self.held = None;
                    (Some(Intent::MomentaryTool(None)), true)
                } else {
                    (None, false)
                }
            }
        }
    }

    /// The window lost the keyboard: whatever was held is released.
    pub fn release(&mut self) -> Option<Intent> {
        self.held.take().map(|_| Intent::MomentaryTool(None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::keyboard::{KeyLocation, Modifiers};

    fn ev(key: Key, modifiers: Modifiers, state: KeyState, repeat: bool) -> KeyEvent {
        KeyEvent {
            key,
            location: KeyLocation::Standard,
            state,
            repeat,
            text: None,
            modifiers,
        }
    }

    #[test]
    fn each_switch_names_its_tool() {
        let alt = Modifiers::NONE.with_alt();
        let press = |k, m| momentary_tool(&ev(k, m, KeyState::Pressed, false));
        assert_eq!(
            press(Key::Named(NamedKey::Space), Modifiers::NONE),
            Some(ToolId::Selector)
        );
        assert_eq!(press(Key::char('s'), alt), Some(ToolId::Selector));
        assert_eq!(press(Key::char('z'), alt), Some(ToolId::Zoom));
        assert_eq!(press(Key::char('X'), alt), Some(ToolId::Pan));
        assert_eq!(press(Key::char('z'), Modifiers::NONE), None);
        assert_eq!(press(Key::char('z'), alt.with_ctrl()), None);
    }

    #[test]
    fn the_release_of_the_same_key_restores_whatever_the_modifiers() {
        let mut m = MomentarySwitch::new();
        let alt = Modifiers::NONE.with_alt();
        let (i, consumed) = m.key(&ev(Key::char('z'), alt, KeyState::Pressed, false));
        assert_eq!(i, Some(Intent::MomentaryTool(Some(ToolId::Zoom))));
        assert!(consumed);
        // Auto-repeat and other keys do not re-trigger.
        assert_eq!(
            m.key(&ev(Key::char('z'), alt, KeyState::Pressed, true)),
            (None, true)
        );
        assert_eq!(
            m.key(&ev(
                Key::Named(NamedKey::Space),
                Modifiers::NONE,
                KeyState::Pressed,
                false
            )),
            (None, false)
        );
        // Alt let go first: Z's release still ends the switch.
        let (i, _) = m.key(&ev(
            Key::char('z'),
            Modifiers::NONE,
            KeyState::Released,
            false,
        ));
        assert_eq!(i, Some(Intent::MomentaryTool(None)));
        assert!(!m.is_held());
    }

    #[test]
    fn losing_focus_releases_the_switch() {
        let mut m = MomentarySwitch::new();
        m.key(&ev(
            Key::Named(NamedKey::Space),
            Modifiers::NONE,
            KeyState::Pressed,
            false,
        ));
        assert_eq!(m.release(), Some(Intent::MomentaryTool(None)));
        assert_eq!(m.release(), None);
    }
}
