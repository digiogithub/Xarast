//! Keys, modifiers and shortcut matching.
//!
//! The vocabulary is Xara's, because the behaviour is Xara's
//! (`docs/research/04-feature-inventory.md` §4.1): `Ctrl` is **Constrain**,
//! `Shift` is **Adjust**, `Alt` is **Alternative**. Naming them by role
//! rather than by key is what lets the rest of the application read as the
//! feature list does, and it is the single place a platform remaps them —
//! macOS in phase 14 will want `Cmd` where Linux wants `Ctrl`.
//!
//! The types here deliberately do not mention `winit`. Phase 14 replaces the
//! translation layer, not this one.

use std::fmt;

/// A key that has a name rather than a character.
///
/// Not exhaustive of every key in existence: it covers the keys a vector
/// editor binds. Anything else arrives as [`Key::Character`] or
/// [`NamedKey::Other`], which still carries enough to be bound by scan code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NamedKey {
    /// Escape: cancels the drag in progress.
    Escape,
    /// Return or Enter: "edit the selection".
    Enter,
    /// Tab.
    Tab,
    /// Space: the momentary selector-tool switch.
    Space,
    /// Backspace.
    Backspace,
    /// Delete.
    Delete,
    /// Insert.
    Insert,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// Arrow up: nudge.
    ArrowUp,
    /// Arrow down.
    ArrowDown,
    /// Arrow left.
    ArrowLeft,
    /// Arrow right.
    ArrowRight,
    /// A function key, `F1` through `F24`.
    Function(u8),
    /// A modifier pressed on its own. Reported so that a modifier change
    /// during a drag is visible even with no pointer motion.
    Modifier(ModifierKey),
    /// Anything else. Bindable by scan code, not by name.
    Other,
}

/// Which physical modifier a [`NamedKey::Modifier`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModifierKey {
    /// Shift.
    Shift,
    /// Control.
    Control,
    /// Alt or Option.
    Alt,
    /// Super, Windows or Command.
    Super,
}

/// A key, as the layout interprets it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    /// A key that produced text, given as the text it produced.
    ///
    /// Layout-dependent on purpose: Xara's `CheckUnicode` shortcuts are the
    /// ones that must follow the layout rather than the physical key.
    Character(String),
    /// A named key.
    Named(NamedKey),
}

impl Key {
    /// A single-character key.
    #[must_use]
    pub fn char(c: char) -> Self {
        Self::Character(c.to_string())
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Character(s) => f.write_str(s),
            Self::Named(n) => write!(f, "{n:?}"),
        }
    }
}

/// Where on the keyboard a key is.
///
/// Xara's `Extended` flag and its numeric-keypad snapping toggles both need
/// this: `Numpad+` during a drag is a different binding from `+`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum KeyLocation {
    /// The main block.
    #[default]
    Standard,
    /// The left-hand one of a pair.
    Left,
    /// The right-hand one of a pair.
    Right,
    /// The numeric keypad.
    Numpad,
}

/// Pressed or released.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyState {
    /// The key went down.
    Pressed,
    /// The key came up.
    Released,
}

/// The live state of the four modifiers.
///
/// Sampled continuously, never latched at the start of a drag: `Ctrl`,
/// `Shift` and `Alt` change what a drag is doing *while* it is happening
/// (`research/04 §4.9` item 4), so a latched copy is a bug, not an
/// optimisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    /// Shift is held.
    pub shift: bool,
    /// Control is held.
    pub ctrl: bool,
    /// Alt is held.
    pub alt: bool,
    /// Super, Windows or Command is held.
    pub logo: bool,
}

impl Modifiers {
    /// Nothing held.
    pub const NONE: Self = Self {
        shift: false,
        ctrl: false,
        alt: false,
        logo: false,
    };

    /// Xara's **Constrain**: squares and circles, angles in multiples,
    /// preserved aspect ratio, and the prefix of most commands.
    #[must_use]
    pub const fn constrain(self) -> bool {
        self.ctrl
    }

    /// Xara's **Adjust**: add to the selection, the variant of a command.
    #[must_use]
    pub const fn adjust(self) -> bool {
        self.shift
    }

    /// Xara's **Alternative**: select the object beneath, nudge by a pixel,
    /// the alternative behaviour of a tool.
    #[must_use]
    pub const fn alternative(self) -> bool {
        self.alt
    }

    /// True when no modifier at all is held.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        !self.shift && !self.ctrl && !self.alt && !self.logo
    }

    /// Builder: with Shift held.
    #[must_use]
    pub const fn with_shift(mut self) -> Self {
        self.shift = true;
        self
    }

    /// Builder: with Control held.
    #[must_use]
    pub const fn with_ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    /// Builder: with Alt held.
    #[must_use]
    pub const fn with_alt(mut self) -> Self {
        self.alt = true;
        self
    }

    /// Builder: with Super held.
    #[must_use]
    pub const fn with_logo(mut self) -> Self {
        self.logo = true;
        self
    }
}

impl fmt::Display for Modifiers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.logo {
            parts.push("Super");
        }
        if parts.is_empty() {
            f.write_str("none")
        } else {
            f.write_str(&parts.join("+"))
        }
    }
}

/// One key event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    /// The key.
    pub key: Key,
    /// Where it is on the keyboard.
    pub location: KeyLocation,
    /// Pressed or released.
    pub state: KeyState,
    /// True when the platform generated this from auto-repeat.
    pub repeat: bool,
    /// The text it produced, if any. `None` while an IME is composing: the
    /// composed text arrives through [`super::ime`] instead.
    pub text: Option<String>,
    /// The modifiers held at the moment of the event.
    pub modifiers: Modifiers,
}

/// Tracks the live modifier state and reports every change.
///
/// The point of a tracker rather than a bare field: a modifier pressed with
/// the pointer stationary produces no pointer event, so the change has to be
/// noticed and published on its own. A tool mid-drag depends on it.
#[derive(Debug, Clone, Copy, Default)]
pub struct ModifierTracker {
    current: Modifiers,
}

impl ModifierTracker {
    /// A tracker with nothing held.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            current: Modifiers::NONE,
        }
    }

    /// The modifiers held right now.
    #[must_use]
    pub const fn current(&self) -> Modifiers {
        self.current
    }

    /// Records a new state, returning it when it differs from the last.
    ///
    /// `None` means nothing changed and nothing should be published; the
    /// platform repeats the state on every key event, so the filter is what
    /// keeps the event stream from doubling.
    pub fn update(&mut self, next: Modifiers) -> Option<Modifiers> {
        if next == self.current {
            return None;
        }
        self.current = next;
        Some(next)
    }

    /// Clears the state, which is what focus loss means.
    ///
    /// Without this a window that loses focus with `Ctrl` held believes it is
    /// still held when focus returns, and the next drag constrains itself for
    /// no visible reason.
    pub fn clear(&mut self) -> Option<Modifiers> {
        self.update(Modifiers::NONE)
    }
}

/// A key combination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    /// The key.
    pub key: Key,
    /// The modifiers that must be held, exactly.
    pub modifiers: Modifiers,
    /// Where the key must be. `None` matches anywhere.
    pub location: Option<KeyLocation>,
    /// Xara's `WorksInDrag`: the shortcut stays live while a drag is in
    /// progress. Everything else is suppressed during a drag, so that a
    /// stray keystroke cannot run a command in the middle of one.
    pub works_in_drag: bool,
}

impl Shortcut {
    /// A shortcut on a key with the given modifiers.
    #[must_use]
    pub fn new(key: Key, modifiers: Modifiers) -> Self {
        Self {
            key,
            modifiers,
            location: None,
            works_in_drag: false,
        }
    }

    /// Restricts the shortcut to one keyboard location.
    #[must_use]
    pub fn at(mut self, location: KeyLocation) -> Self {
        self.location = Some(location);
        self
    }

    /// Marks the shortcut live during a drag.
    #[must_use]
    pub const fn works_in_drag(mut self) -> Self {
        self.works_in_drag = true;
        self
    }

    /// Whether this event fires this shortcut.
    ///
    /// Only key presses match: firing a command on release would double every
    /// shortcut. Auto-repeat does match, because held arrows nudge.
    #[must_use]
    pub fn matches(&self, event: &KeyEvent, in_drag: bool) -> bool {
        if event.state != KeyState::Pressed {
            return false;
        }
        if in_drag && !self.works_in_drag {
            return false;
        }
        if let Some(loc) = self.location
            && loc != event.location
        {
            return false;
        }
        self.key == event.key && self.modifiers == event.modifiers
    }
}

impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.modifiers.is_empty() {
            write!(f, "{}", self.key)
        } else {
            write!(f, "{}+{}", self.modifiers, self.key)
        }
    }
}

/// A set of shortcuts bound to commands.
///
/// Generic over the command type on purpose: the command vocabulary belongs
/// to `xarast-app`, and the shell must not invent one. The shell owns only
/// the matching rule.
#[derive(Debug, Clone)]
pub struct ShortcutMap<C> {
    entries: Vec<(Shortcut, C)>,
}

impl<C> Default for ShortcutMap<C> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<C: Clone> ShortcutMap<C> {
    /// An empty map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds a shortcut, replacing an identical existing binding.
    ///
    /// Returns the command that was displaced, so that a conflicting binding
    /// is a reportable event rather than a silent loss.
    pub fn bind(&mut self, shortcut: Shortcut, command: C) -> Option<C> {
        if let Some(slot) = self.entries.iter_mut().find(|(s, _)| *s == shortcut) {
            return Some(std::mem::replace(&mut slot.1, command));
        }
        self.entries.push((shortcut, command));
        None
    }

    /// The command this event runs, if any.
    ///
    /// Later bindings win over earlier ones with the same key but different
    /// location restrictions, because the more specific binding is added
    /// afterwards by convention; ties on an identical shortcut cannot occur,
    /// since [`bind`](Self::bind) replaces.
    #[must_use]
    pub fn resolve(&self, event: &KeyEvent, in_drag: bool) -> Option<C> {
        self.entries
            .iter()
            .rev()
            .find(|(s, _)| s.matches(event, in_drag))
            .map(|(_, c)| c.clone())
    }

    /// Number of bindings.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when nothing is bound.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every binding, in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&Shortcut, &C)> {
        self.entries.iter().map(|(s, c)| (s, c))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(key: Key, modifiers: Modifiers) -> KeyEvent {
        KeyEvent {
            key,
            location: KeyLocation::Standard,
            state: KeyState::Pressed,
            repeat: false,
            text: None,
            modifiers,
        }
    }

    #[test]
    fn the_xara_roles_map_onto_the_expected_keys() {
        let m = Modifiers::NONE.with_ctrl();
        assert!(m.constrain() && !m.adjust() && !m.alternative());
        let m = Modifiers::NONE.with_shift();
        assert!(m.adjust() && !m.constrain());
        let m = Modifiers::NONE.with_alt();
        assert!(m.alternative() && !m.constrain());
    }

    #[test]
    fn the_tracker_publishes_a_change_exactly_once() {
        let mut t = ModifierTracker::new();
        let ctrl = Modifiers::NONE.with_ctrl();
        assert_eq!(t.update(ctrl), Some(ctrl));
        assert_eq!(
            t.update(ctrl),
            None,
            "an unchanged state must not republish"
        );
        assert_eq!(t.current(), ctrl);
        assert_eq!(t.update(Modifiers::NONE), Some(Modifiers::NONE));
    }

    #[test]
    fn modifiers_change_during_a_drag_without_any_pointer_event() {
        // This is the whole reason the tracker exists: the sequence below
        // contains no pointer motion at all, and the tool still has to see
        // Constrain go on and off.
        let mut t = ModifierTracker::new();
        let seen: Vec<_> = [
            Modifiers::NONE.with_ctrl(),
            Modifiers::NONE.with_ctrl().with_shift(),
            Modifiers::NONE.with_shift(),
            Modifiers::NONE,
        ]
        .into_iter()
        .filter_map(|m| t.update(m))
        .collect();
        assert_eq!(seen.len(), 4);
        assert!(seen[0].constrain());
        assert!(seen[1].constrain() && seen[1].adjust());
        assert!(!seen[2].constrain() && seen[2].adjust());
        assert!(seen[3].is_empty());
    }

    #[test]
    fn losing_focus_drops_every_held_modifier() {
        let mut t = ModifierTracker::new();
        t.update(Modifiers::NONE.with_ctrl().with_alt());
        assert_eq!(t.clear(), Some(Modifiers::NONE));
        assert!(t.current().is_empty());
        assert_eq!(t.clear(), None);
    }

    #[test]
    fn a_shortcut_needs_the_exact_modifier_set() {
        let s = Shortcut::new(Key::char('g'), Modifiers::NONE.with_ctrl());
        assert!(s.matches(&press(Key::char('g'), Modifiers::NONE.with_ctrl()), false));
        assert!(!s.matches(
            &press(Key::char('g'), Modifiers::NONE.with_ctrl().with_shift()),
            false
        ));
        assert!(!s.matches(&press(Key::char('g'), Modifiers::NONE), false));
    }

    #[test]
    fn releases_never_fire_a_shortcut() {
        let s = Shortcut::new(Key::char('g'), Modifiers::NONE);
        let mut ev = press(Key::char('g'), Modifiers::NONE);
        ev.state = KeyState::Released;
        assert!(!s.matches(&ev, false));
    }

    #[test]
    fn only_works_in_drag_shortcuts_survive_a_drag() {
        let plain = Shortcut::new(Key::char('g'), Modifiers::NONE);
        let snap = Shortcut::new(Key::Named(NamedKey::Other), Modifiers::NONE)
            .at(KeyLocation::Numpad)
            .works_in_drag();

        let g = press(Key::char('g'), Modifiers::NONE);
        assert!(plain.matches(&g, false));
        assert!(
            !plain.matches(&g, true),
            "a stray key must not run a command mid-drag"
        );

        let mut numpad = press(Key::Named(NamedKey::Other), Modifiers::NONE);
        numpad.location = KeyLocation::Numpad;
        assert!(snap.matches(&numpad, true), "snapping toggles mid-drag");
    }

    #[test]
    fn a_location_restricted_shortcut_ignores_the_main_block() {
        let s = Shortcut::new(Key::char('+'), Modifiers::NONE).at(KeyLocation::Numpad);
        assert!(!s.matches(&press(Key::char('+'), Modifiers::NONE), false));
        let mut ev = press(Key::char('+'), Modifiers::NONE);
        ev.location = KeyLocation::Numpad;
        assert!(s.matches(&ev, false));
    }

    #[test]
    fn rebinding_reports_what_it_displaced() {
        let mut map: ShortcutMap<&'static str> = ShortcutMap::new();
        let s = Shortcut::new(Key::char('z'), Modifiers::NONE.with_ctrl());
        assert_eq!(map.bind(s.clone(), "undo"), None);
        assert_eq!(map.bind(s, "redo"), Some("undo"));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn resolution_finds_the_bound_command_and_respects_the_drag_rule() {
        let mut map: ShortcutMap<&'static str> = ShortcutMap::new();
        map.bind(
            Shortcut::new(Key::char('z'), Modifiers::NONE.with_ctrl()),
            "undo",
        );
        map.bind(
            Shortcut::new(Key::Named(NamedKey::Escape), Modifiers::NONE).works_in_drag(),
            "cancel",
        );

        let undo = press(Key::char('z'), Modifiers::NONE.with_ctrl());
        assert_eq!(map.resolve(&undo, false), Some("undo"));
        assert_eq!(map.resolve(&undo, true), None);

        let esc = press(Key::Named(NamedKey::Escape), Modifiers::NONE);
        assert_eq!(map.resolve(&esc, true), Some("cancel"));
        assert_eq!(map.iter().count(), 2);
        assert!(!map.is_empty());
    }

    #[test]
    fn shortcuts_print_the_way_a_menu_shows_them() {
        let s = Shortcut::new(Key::char('z'), Modifiers::NONE.with_ctrl().with_shift());
        assert_eq!(s.to_string(), "Ctrl+Shift+z");
        assert_eq!(
            Shortcut::new(Key::Named(NamedKey::Escape), Modifiers::NONE).to_string(),
            "Escape"
        );
    }
}
