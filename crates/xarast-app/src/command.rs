//! The named operations a menu item or a shortcut runs, and their keys.
//!
//! This is the application's command table (`phase-05 §W4.7`): one list
//! that the menu bar draws, the shell binds keys from and the tests walk.
//! A command is not an [`Intent`] — it is the *name* of an operation, with
//! a label and its shortcuts — and [`AppCommand::intent`] is the one place
//! that turns it into what the application core consumes. That keeps the
//! rule "the UI and the shell raise the same intents" true by construction:
//! both go through this table.
//!
//! Keys are described here in a toolkit-neutral way ([`KeyChord`]); the
//! shell turns them into its own `Shortcut`s and owns the matching rule,
//! including "never while a text field has the keyboard".

use std::fmt;

use crate::edit::ToolId;
use crate::geometry::DevicePoint;
use crate::intent::Intent;
use crate::viewport::ZoomTarget;

/// The key of a [`KeyChord`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChordKey {
    /// A key that types a character, as the layout produces it.
    Char(char),
    /// The Home key.
    Home,
    /// The Delete key.
    Delete,
    /// The Escape key.
    Escape,
    /// A function key, `F1` to `F24`.
    Function(u8),
}

/// A key and the modifiers held with it.
///
/// `ctrl` is the platform's command modifier: Control on Linux and
/// Windows, and the one the shell maps to Command on macOS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyChord {
    /// The key.
    pub key: ChordKey,
    /// The command modifier.
    pub ctrl: bool,
    /// Shift. For punctuation (`+`), the layout decides whether Shift is
    /// needed to type it, so the shell accepts it either way; see
    /// [`KeyChord::shift_is_layout_dependent`].
    pub shift: bool,
}

impl KeyChord {
    /// A key on its own.
    #[must_use]
    pub const fn plain(key: ChordKey) -> KeyChord {
        KeyChord {
            key,
            ctrl: false,
            shift: false,
        }
    }

    /// The key with the command modifier.
    #[must_use]
    pub const fn ctrl(c: char) -> KeyChord {
        KeyChord {
            key: ChordKey::Char(c),
            ctrl: true,
            shift: false,
        }
    }

    /// A character key on its own.
    #[must_use]
    pub const fn char(c: char) -> KeyChord {
        KeyChord::plain(ChordKey::Char(c))
    }

    /// The key with the command modifier and Shift.
    #[must_use]
    pub const fn ctrl_shift(c: char) -> KeyChord {
        KeyChord {
            key: ChordKey::Char(c),
            ctrl: true,
            shift: true,
        }
    }

    /// A function key on its own.
    #[must_use]
    pub const fn f(n: u8) -> KeyChord {
        KeyChord::plain(ChordKey::Function(n))
    }

    /// A function key with Shift.
    #[must_use]
    pub const fn shift_f(n: u8) -> KeyChord {
        KeyChord {
            key: ChordKey::Function(n),
            ctrl: false,
            shift: true,
        }
    }

    /// Whether the character is one some layouts type with Shift and
    /// others without (`+` is Shift+`=` on a US keyboard and a key of its
    /// own on the numeric keypad). Letters and digits are not: Shift
    /// changes what they are.
    #[must_use]
    pub const fn shift_is_layout_dependent(&self) -> bool {
        matches!(self.key, ChordKey::Char(c) if !c.is_ascii_alphanumeric())
    }
}

impl fmt::Display for KeyChord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            f.write_str("Ctrl+")?;
        }
        if self.shift {
            f.write_str("Shift+")?;
        }
        match self.key {
            ChordKey::Char(c) => write!(f, "{}", c.to_ascii_uppercase()),
            ChordKey::Home => f.write_str("Home"),
            ChordKey::Delete => f.write_str("Del"),
            ChordKey::Escape => f.write_str("Esc"),
            ChordKey::Function(n) => write!(f, "F{n}"),
        }
    }
}

/// A named operation of the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppCommand {
    /// File › Open…: ask the platform for a file to open.
    Open,
    /// File › Close: close the active document.
    Close,
    /// File › Quit.
    Quit,
    /// View › Zoom in.
    ZoomIn,
    /// View › Zoom out.
    ZoomOut,
    /// View › Fit page.
    FitPage,
    /// View › Fit drawing.
    FitDrawing,
    /// View › 100 %.
    Zoom100,
    /// View › Zoom to selection. The original's key for it is Redo here
    /// (`tools.md`), so it takes `3`, which is where other editors keep
    /// it.
    ZoomSelection,
    /// Edit › Undo.
    Undo,
    /// Edit › Redo.
    Redo,
    /// Edit › Delete: delete the selected objects.
    Delete,
    /// Edit › Select all.
    SelectAll,
    /// `Esc`: cancel the gesture in flight, or select nothing.
    Cancel,
    /// Choose a tool from the palette or by its key.
    Tool(ToolId),
}

/// How much one zoom-in or zoom-out step multiplies the zoom.
pub const ZOOM_STEP: f64 = std::f64::consts::SQRT_2;

impl AppCommand {
    /// Every command, in menu order, then the tools in palette order.
    /// Tools reserved for later phases are not here: they have no key yet.
    pub const ALL: [AppCommand; 22] = [
        AppCommand::Open,
        AppCommand::Close,
        AppCommand::Quit,
        AppCommand::Undo,
        AppCommand::Redo,
        AppCommand::Delete,
        AppCommand::SelectAll,
        AppCommand::Cancel,
        AppCommand::ZoomIn,
        AppCommand::ZoomOut,
        AppCommand::FitPage,
        AppCommand::FitDrawing,
        AppCommand::Zoom100,
        AppCommand::ZoomSelection,
        AppCommand::Tool(ToolId::Selector),
        AppCommand::Tool(ToolId::ShapeEditor),
        AppCommand::Tool(ToolId::Rectangle),
        AppCommand::Tool(ToolId::Ellipse),
        AppCommand::Tool(ToolId::Pen),
        AppCommand::Tool(ToolId::Freehand),
        AppCommand::Tool(ToolId::Zoom),
        AppCommand::Tool(ToolId::Pan),
    ];

    /// The menu label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            AppCommand::Open => "Open…",
            AppCommand::Close => "Close",
            AppCommand::Quit => "Quit",
            AppCommand::ZoomIn => "Zoom in",
            AppCommand::ZoomOut => "Zoom out",
            AppCommand::FitPage => "Fit page",
            AppCommand::FitDrawing => "Fit drawing",
            AppCommand::Zoom100 => "100 %",
            AppCommand::ZoomSelection => "Zoom to selection",
            AppCommand::Undo => "Undo",
            AppCommand::Redo => "Redo",
            AppCommand::Delete => "Delete",
            AppCommand::SelectAll => "Select all",
            AppCommand::Cancel => "Select none",
            AppCommand::Tool(t) => t.label(),
        }
    }

    /// Whether the key works in the middle of a drag (`WorksInDrag` in
    /// the original): only `Esc`, which cancels it.
    #[must_use]
    pub const fn works_in_drag(self) -> bool {
        matches!(self, AppCommand::Cancel)
    }

    /// Every key that runs the command; the first is the one a menu shows.
    #[must_use]
    pub const fn shortcuts(self) -> &'static [KeyChord] {
        const OPEN: &[KeyChord] = &[KeyChord::ctrl('o')];
        const CLOSE: &[KeyChord] = &[KeyChord::ctrl('w')];
        const QUIT: &[KeyChord] = &[KeyChord::ctrl('q')];
        const ZOOM_IN: &[KeyChord] = &[
            KeyChord::char('+'),
            KeyChord::char('='),
            KeyChord::ctrl('+'),
            KeyChord::ctrl('='),
        ];
        const ZOOM_OUT: &[KeyChord] = &[KeyChord::char('-'), KeyChord::ctrl('-')];
        const FIT_PAGE: &[KeyChord] = &[KeyChord::char('0'), KeyChord::plain(ChordKey::Home)];
        const FIT_DRAWING: &[KeyChord] = &[KeyChord::char('d')];
        const ZOOM_100: &[KeyChord] = &[KeyChord::char('1')];
        const ZOOM_SELECTION: &[KeyChord] = &[KeyChord::char('3')];
        const UNDO: &[KeyChord] = &[KeyChord::ctrl('z')];
        // Ctrl+Shift+Z first because every other Linux program shows it;
        // Ctrl+Y is the original's (`research/04 §4.1`).
        const REDO: &[KeyChord] = &[KeyChord::ctrl_shift('z'), KeyChord::ctrl('y')];
        const DELETE: &[KeyChord] = &[KeyChord::plain(ChordKey::Delete)];
        const SELECT_ALL: &[KeyChord] = &[KeyChord::ctrl('a')];
        const CANCEL: &[KeyChord] = &[KeyChord::plain(ChordKey::Escape)];
        // The tool keys of `research/04 §4.6`.
        const SELECTOR: &[KeyChord] = &[KeyChord::f(2)];
        const FREEHAND: &[KeyChord] = &[KeyChord::f(3)];
        const SHAPE: &[KeyChord] = &[KeyChord::f(4)];
        const RECTANGLE: &[KeyChord] = &[KeyChord::shift_f(3)];
        const ELLIPSE: &[KeyChord] = &[KeyChord::shift_f(4)];
        const PEN: &[KeyChord] = &[KeyChord::shift_f(5)];
        const ZOOM_TOOL: &[KeyChord] = &[KeyChord::shift_f(7)];
        const PUSH: &[KeyChord] = &[KeyChord::shift_f(8)];
        const NONE: &[KeyChord] = &[];
        match self {
            AppCommand::Open => OPEN,
            AppCommand::Close => CLOSE,
            AppCommand::Quit => QUIT,
            AppCommand::ZoomIn => ZOOM_IN,
            AppCommand::ZoomOut => ZOOM_OUT,
            AppCommand::FitPage => FIT_PAGE,
            AppCommand::FitDrawing => FIT_DRAWING,
            AppCommand::Zoom100 => ZOOM_100,
            AppCommand::ZoomSelection => ZOOM_SELECTION,
            AppCommand::Undo => UNDO,
            AppCommand::Redo => REDO,
            AppCommand::Delete => DELETE,
            AppCommand::SelectAll => SELECT_ALL,
            AppCommand::Cancel => CANCEL,
            AppCommand::Tool(t) => match t {
                ToolId::Selector => SELECTOR,
                ToolId::Freehand => FREEHAND,
                ToolId::ShapeEditor => SHAPE,
                ToolId::Rectangle => RECTANGLE,
                ToolId::Ellipse => ELLIPSE,
                ToolId::Pen => PEN,
                ToolId::Zoom => ZOOM_TOOL,
                ToolId::Pan => PUSH,
                ToolId::Fill | ToolId::Transparency | ToolId::Text => NONE,
            },
        }
    }

    /// The shortcut a menu shows next to the label.
    #[must_use]
    pub fn primary_shortcut(self) -> Option<KeyChord> {
        self.shortcuts().first().copied()
    }

    /// Whether the command needs an open document to mean anything; a
    /// menu greys such an item out when nothing is open.
    #[must_use]
    pub const fn needs_document(self) -> bool {
        !matches!(self, AppCommand::Open | AppCommand::Quit)
    }

    /// The intent the command raises. A keyboard or menu zoom has no
    /// pointer to zoom about, so it keeps `centre` (the canvas centre, in
    /// device pixels) fixed.
    #[must_use]
    pub fn intent(self, centre: DevicePoint) -> Intent {
        match self {
            AppCommand::Open => Intent::ShowOpenDialog,
            AppCommand::Close => Intent::CloseDocument,
            AppCommand::Quit => Intent::Quit,
            AppCommand::ZoomIn => Intent::Zoom {
                factor: ZOOM_STEP,
                anchor: centre,
            },
            AppCommand::ZoomOut => Intent::Zoom {
                factor: 1.0 / ZOOM_STEP,
                anchor: centre,
            },
            AppCommand::FitPage => Intent::ZoomTo(ZoomTarget::Page),
            AppCommand::FitDrawing => Intent::ZoomTo(ZoomTarget::Drawing),
            AppCommand::Zoom100 => Intent::ZoomTo(ZoomTarget::Percent100),
            AppCommand::ZoomSelection => Intent::ZoomTo(ZoomTarget::Selection),
            AppCommand::Undo => Intent::Undo,
            AppCommand::Redo => Intent::Redo,
            AppCommand::Delete => Intent::DeleteSelection,
            AppCommand::SelectAll => Intent::SelectAll,
            AppCommand::Cancel => Intent::Cancel,
            AppCommand::Tool(t) => Intent::ChooseTool(t),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_two_commands_share_a_key() {
        let mut seen = std::collections::HashMap::new();
        for c in AppCommand::ALL {
            for k in c.shortcuts() {
                if let Some(other) = seen.insert(*k, c) {
                    panic!("{k} is bound to both {other:?} and {c:?}");
                }
            }
        }
    }

    #[test]
    fn the_edit_and_tool_keys_are_the_documented_ones() {
        let key = |c: AppCommand| c.primary_shortcut().unwrap().to_string();
        assert_eq!(key(AppCommand::Undo), "Ctrl+Z");
        assert_eq!(key(AppCommand::Redo), "Ctrl+Shift+Z");
        assert_eq!(AppCommand::Redo.shortcuts()[1].to_string(), "Ctrl+Y");
        assert_eq!(key(AppCommand::Delete), "Del");
        assert_eq!(key(AppCommand::Cancel), "Esc");
        assert!(AppCommand::Cancel.works_in_drag());
        assert!(!AppCommand::Undo.works_in_drag());
        for (tool, k) in [
            (ToolId::Selector, "F2"),
            (ToolId::Freehand, "F3"),
            (ToolId::ShapeEditor, "F4"),
            (ToolId::Rectangle, "Shift+F3"),
            (ToolId::Ellipse, "Shift+F4"),
            (ToolId::Pen, "Shift+F5"),
            (ToolId::Zoom, "Shift+F7"),
            (ToolId::Pan, "Shift+F8"),
        ] {
            assert_eq!(key(AppCommand::Tool(tool)), k, "{tool:?}");
            assert_eq!(
                AppCommand::Tool(tool).intent(DevicePoint::new(0.0, 0.0)),
                Intent::ChooseTool(tool)
            );
        }
        assert!(AppCommand::Tool(ToolId::Text).shortcuts().is_empty());
    }

    #[test]
    fn every_command_has_a_label_and_a_menu_shortcut() {
        for c in AppCommand::ALL {
            assert!(!c.label().is_empty());
            assert!(c.primary_shortcut().is_some(), "{c:?}");
        }
        assert_eq!(
            AppCommand::Open.primary_shortcut().unwrap().to_string(),
            "Ctrl+O"
        );
        assert_eq!(
            AppCommand::FitDrawing
                .primary_shortcut()
                .unwrap()
                .to_string(),
            "D"
        );
        assert_eq!(KeyChord::plain(ChordKey::Home).to_string(), "Home");
    }

    #[test]
    fn view_commands_raise_view_intents_about_the_centre() {
        let c = DevicePoint::new(400.0, 300.0);
        assert_eq!(
            AppCommand::ZoomIn.intent(c),
            Intent::Zoom {
                factor: ZOOM_STEP,
                anchor: c
            }
        );
        assert_eq!(AppCommand::Open.intent(c), Intent::ShowOpenDialog);
        assert_eq!(
            AppCommand::FitPage.intent(c),
            Intent::ZoomTo(ZoomTarget::Page)
        );
        assert!(!AppCommand::Open.needs_document());
        assert!(AppCommand::ZoomIn.needs_document());
    }

    #[test]
    fn only_punctuation_is_shift_tolerant() {
        assert!(KeyChord::char('+').shift_is_layout_dependent());
        assert!(!KeyChord::char('d').shift_is_layout_dependent());
        assert!(!KeyChord::char('1').shift_is_layout_dependent());
        assert!(!KeyChord::plain(ChordKey::Home).shift_is_layout_dependent());
    }
}
