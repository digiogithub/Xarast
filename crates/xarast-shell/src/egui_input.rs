//! The `egui` input shim: [`ShellEvent`] in, [`egui::RawInput`] out.
//!
//! Written against the shell's own event model and not against `winit`, so
//! the Win32 and AppKit shells of phase 14 inherit it unchanged — the same
//! reason [`crate::intents`] lives outside `input::translate`.
//!
//! # Who gets the canvas's input
//!
//! Every event reaches `egui`, the canvas region included: `egui` needs to
//! know where the pointer is to hover, to close a popup on an outside click
//! and to report the pointer's document position. What `egui` does **not**
//! do is navigate the document. The canvas widget runs in
//! [`xarast_ui::CanvasNavigation::External`] mode, which turns its own
//! wheel, pinch, middle-drag and keyboard navigation off, and pan and zoom
//! come from [`crate::intents::IntentAdapter`] alone. Two paths reading the
//! same wheel is how a notch zooms twice; `viewer`'s tests pin that it
//! does not. The reverse gate is in the viewer too: the adapter only sees
//! a pointer event when `egui` is not showing something over the canvas at
//! that point (a popup, a tooltip, a floating window), and view keys only
//! when no text field has the keyboard.
//!
//! Coordinates convert from device pixels to logical points with the one
//! scale factor the shell owns ([`crate::scale::ScaleFactor`]).

use std::time::Instant;

use crate::clipboard::Clipboard;
use crate::ime::ImeEvent;
use crate::input::event::{GestureEvent, PointerButton, PointerPhase, ScrollUnit, ShellEvent};
use crate::input::keyboard::{Key, KeyState, Modifiers, NamedKey};
use crate::scale::PhysicalPos;

/// Accumulates one frame of `egui` input from [`ShellEvent`]s.
#[derive(Debug)]
pub struct EguiInput {
    events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
    focused: bool,
    epoch: Instant,
}

impl Default for EguiInput {
    fn default() -> Self {
        EguiInput::new()
    }
}

/// `egui`'s view of the shell's modifiers. `command` is `Ctrl` on Linux;
/// phase 14 remaps it for macOS here and in `semantic_modifiers` together.
#[must_use]
pub const fn egui_modifiers(m: Modifiers) -> egui::Modifiers {
    egui::Modifiers {
        alt: m.alt,
        ctrl: m.ctrl,
        shift: m.shift,
        mac_cmd: false,
        command: m.ctrl,
    }
}

/// The `egui` key for a shell key, if `egui` has one.
#[must_use]
pub fn egui_key(key: &Key) -> Option<egui::Key> {
    match key {
        Key::Character(s) => egui::Key::from_name(s),
        Key::Named(n) => Some(match n {
            NamedKey::Escape => egui::Key::Escape,
            NamedKey::Enter => egui::Key::Enter,
            NamedKey::Tab => egui::Key::Tab,
            NamedKey::Space => egui::Key::Space,
            NamedKey::Backspace => egui::Key::Backspace,
            NamedKey::Delete => egui::Key::Delete,
            NamedKey::Insert => egui::Key::Insert,
            NamedKey::Home => egui::Key::Home,
            NamedKey::End => egui::Key::End,
            NamedKey::PageUp => egui::Key::PageUp,
            NamedKey::PageDown => egui::Key::PageDown,
            NamedKey::ArrowUp => egui::Key::ArrowUp,
            NamedKey::ArrowDown => egui::Key::ArrowDown,
            NamedKey::ArrowLeft => egui::Key::ArrowLeft,
            NamedKey::ArrowRight => egui::Key::ArrowRight,
            NamedKey::Function(n) => return egui::Key::from_name(&format!("F{n}")),
            _ => return None,
        }),
    }
}

/// The shell's cursor for the one `egui` asks for.
#[must_use]
pub const fn cursor_shape(icon: egui::CursorIcon) -> crate::CursorShape {
    use crate::CursorShape as S;
    use egui::CursorIcon as C;
    match icon {
        C::None => S::Hidden,
        C::Text | C::VerticalText => S::Text,
        C::PointingHand => S::Hand,
        C::Grab => S::Grab,
        C::Grabbing => S::Grabbing,
        C::Move | C::AllScroll => S::Move,
        C::Crosshair | C::Cell => S::Crosshair,
        C::NotAllowed | C::NoDrop => S::NotAllowed,
        C::ResizeHorizontal | C::ResizeColumn | C::ResizeEast | C::ResizeWest => {
            S::ResizeHorizontal
        }
        C::ResizeVertical | C::ResizeRow | C::ResizeNorth | C::ResizeSouth => S::ResizeVertical,
        C::ResizeNwSe | C::ResizeNorthWest | C::ResizeSouthEast => S::ResizeNwSe,
        C::ResizeNeSw | C::ResizeNorthEast | C::ResizeSouthWest => S::ResizeNeSw,
        C::Wait | C::Progress => S::Wait,
        C::Help => S::Help,
        C::ZoomIn => S::ZoomIn,
        C::ZoomOut => S::ZoomOut,
        _ => S::Default,
    }
}

const fn egui_button(b: PointerButton) -> Option<egui::PointerButton> {
    Some(match b {
        PointerButton::Primary | PointerButton::ToolTip => egui::PointerButton::Primary,
        PointerButton::Secondary | PointerButton::ToolBarrel => egui::PointerButton::Secondary,
        PointerButton::Middle => egui::PointerButton::Middle,
        PointerButton::Back => egui::PointerButton::Extra1,
        PointerButton::Forward => egui::PointerButton::Extra2,
        PointerButton::Other(_) => return None,
    })
}

impl EguiInput {
    /// An empty frame of input, focused.
    #[must_use]
    pub fn new() -> EguiInput {
        EguiInput {
            events: Vec::new(),
            modifiers: egui::Modifiers::NONE,
            focused: true,
            epoch: Instant::now(),
        }
    }

    /// Whether anything has been queued since the last [`EguiInput::take`].
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Translates one shell event. `ppp` is the scale in force (device
    /// pixels per logical point); the clipboard is read only for a paste.
    pub fn push(&mut self, event: &ShellEvent, ppp: f32, clipboard: Option<&mut dyn Clipboard>) {
        let ppp = if ppp.is_finite() && ppp > 0.0 {
            ppp
        } else {
            1.0
        };
        let at = |p: PhysicalPos| egui::pos2(p.x as f32 / ppp, p.y as f32 / ppp);
        match event {
            ShellEvent::ModifiersChanged(m) => self.modifiers = egui_modifiers(*m),
            ShellEvent::Focused(f) => {
                self.focused = *f;
                if !f {
                    self.modifiers = egui::Modifiers::NONE;
                }
                self.events.push(egui::Event::WindowFocused(*f));
            }
            ShellEvent::Pointer(p) => {
                let modifiers = egui_modifiers(p.modifiers);
                self.modifiers = modifiers;
                match p.phase {
                    PointerPhase::Entered | PointerPhase::Moved => {
                        self.events.push(egui::Event::PointerMoved(at(p.position)));
                    }
                    PointerPhase::Pressed(b) | PointerPhase::Released(b) => {
                        if let Some(button) = egui_button(b) {
                            self.events.push(egui::Event::PointerButton {
                                pos: at(p.position),
                                button,
                                pressed: matches!(p.phase, PointerPhase::Pressed(_)),
                                modifiers,
                            });
                        }
                    }
                    PointerPhase::Scroll { dx, dy, unit } => {
                        if dx.is_finite() && dy.is_finite() {
                            let (unit, delta) = match unit {
                                ScrollUnit::Lines => {
                                    (egui::MouseWheelUnit::Line, egui::vec2(dx as f32, dy as f32))
                                }
                                // Device pixels to points.
                                ScrollUnit::Pixels => (
                                    egui::MouseWheelUnit::Point,
                                    egui::vec2(dx as f32 / ppp, dy as f32 / ppp),
                                ),
                            };
                            self.events.push(egui::Event::MouseWheel {
                                unit,
                                delta,
                                modifiers,
                            });
                        }
                    }
                    PointerPhase::Left => self.events.push(egui::Event::PointerGone),
                }
            }
            ShellEvent::Key(k) => self.key(k, clipboard),
            ShellEvent::Ime(ime) => self.events.push(egui::Event::Ime(match ime {
                ImeEvent::Enabled => egui::ImeEvent::Enabled,
                ImeEvent::Preedit { text, .. } => egui::ImeEvent::Preedit(text.clone()),
                ImeEvent::Commit(text) => egui::ImeEvent::Commit(text.clone()),
                ImeEvent::Disabled => egui::ImeEvent::Disabled,
            })),
            ShellEvent::Gesture(GestureEvent::Pinch { delta, .. }) => {
                let factor = 1.0 + *delta;
                if factor.is_finite() && factor > 0.0 {
                    self.events.push(egui::Event::Zoom(factor as f32));
                }
            }
            ShellEvent::Gesture(GestureEvent::Pan { dx, dy })
                if dx.is_finite() && dy.is_finite() =>
            {
                self.events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(*dx as f32 / ppp, *dy as f32 / ppp),
                    modifiers: self.modifiers,
                });
            }
            _ => {}
        }
    }

    fn key(&mut self, k: &crate::input::keyboard::KeyEvent, clipboard: Option<&mut dyn Clipboard>) {
        let modifiers = egui_modifiers(k.modifiers);
        self.modifiers = modifiers;
        let pressed = k.state == KeyState::Pressed;
        // The clipboard shortcuts become egui's own clipboard events, which
        // is what a text field listens for.
        if pressed && modifiers.command && !modifiers.alt {
            match &k.key {
                Key::Character(c) if c.eq_ignore_ascii_case("c") => {
                    self.events.push(egui::Event::Copy);
                    return;
                }
                Key::Character(c) if c.eq_ignore_ascii_case("x") => {
                    self.events.push(egui::Event::Cut);
                    return;
                }
                Key::Character(c) if c.eq_ignore_ascii_case("v") => {
                    if let Some(text) = clipboard.and_then(|c| c.text().ok()) {
                        self.events.push(egui::Event::Paste(text));
                    }
                    return;
                }
                _ => {}
            }
        }
        if let Some(key) = egui_key(&k.key) {
            self.events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: k.repeat,
                modifiers,
            });
        }
        // Typed text, unless a shortcut modifier is held (AltGr arrives as
        // Ctrl+Alt on some layouts and does produce text).
        if pressed
            && (!modifiers.ctrl || modifiers.alt)
            && let Some(text) = &k.text
            && !text.is_empty()
            && !text.chars().any(char::is_control)
        {
            self.events.push(egui::Event::Text(text.clone()));
        }
    }

    /// Hands over the frame's input and starts the next. `screen` is the
    /// window in logical points.
    pub fn take(&mut self, screen: egui::Rect) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(screen),
            time: Some(self.epoch.elapsed().as_secs_f64()),
            modifiers: self.modifiers,
            focused: self.focused,
            events: std::mem::take(&mut self.events),
            ..egui::RawInput::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::ClipboardError;
    use crate::input::event::{PointerEvent, PointerId};
    use crate::input::keyboard::{KeyEvent, KeyLocation};
    use crate::input::tablet::InputSource;

    fn ptr(phase: PointerPhase, x: f64, y: f64) -> ShellEvent {
        ShellEvent::Pointer(PointerEvent {
            id: PointerId(0),
            phase,
            position: PhysicalPos::new(x, y),
            source: InputSource::Mouse,
            primary: true,
            modifiers: Modifiers::NONE,
        })
    }

    fn key(key: Key, text: Option<&str>, modifiers: Modifiers) -> ShellEvent {
        ShellEvent::Key(KeyEvent {
            key,
            location: KeyLocation::Standard,
            state: KeyState::Pressed,
            repeat: false,
            text: text.map(str::to_owned),
            modifiers,
        })
    }

    #[test]
    fn pointer_positions_are_converted_to_points() {
        let mut input = EguiInput::new();
        input.push(&ptr(PointerPhase::Moved, 300.0, 150.0), 1.5, None);
        input.push(
            &ptr(PointerPhase::Pressed(PointerButton::Primary), 300.0, 150.0),
            1.5,
            None,
        );
        let raw = input.take(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        ));
        assert_eq!(
            raw.events[0],
            egui::Event::PointerMoved(egui::pos2(200.0, 100.0))
        );
        assert!(matches!(
            raw.events[1],
            egui::Event::PointerButton {
                pressed: true,
                button: egui::PointerButton::Primary,
                ..
            }
        ));
        assert!(input.is_empty(), "take starts a new frame");
    }

    #[test]
    fn a_wheel_notch_is_one_line_and_pixels_become_points() {
        let mut input = EguiInput::new();
        let wheel = |dx, dy, unit| PointerPhase::Scroll { dx, dy, unit };
        input.push(
            &ptr(wheel(0.0, 1.0, ScrollUnit::Lines), 0.0, 0.0),
            2.0,
            None,
        );
        input.push(
            &ptr(wheel(0.0, 40.0, ScrollUnit::Pixels), 0.0, 0.0),
            2.0,
            None,
        );
        let raw = input.take(egui::Rect::ZERO);
        assert!(matches!(
            raw.events[0],
            egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta, .. } if delta.y == 1.0
        ));
        assert!(matches!(
            raw.events[1],
            egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta, .. } if delta.y == 20.0
        ));
    }

    #[test]
    fn typing_produces_a_key_and_its_text_but_a_shortcut_no_text() {
        let mut input = EguiInput::new();
        input.push(&key(Key::char('a'), Some("a"), Modifiers::NONE), 1.0, None);
        input.push(
            &key(Key::char('s'), Some("s"), Modifiers::NONE.with_ctrl()),
            1.0,
            None,
        );
        let raw = input.take(egui::Rect::ZERO);
        let texts: Vec<_> = raw
            .events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["a"]);
        assert_eq!(
            raw.events
                .iter()
                .filter(|e| matches!(e, egui::Event::Key { .. }))
                .count(),
            2
        );
    }

    #[derive(Debug)]
    struct FixedClipboard(&'static str);
    impl Clipboard for FixedClipboard {
        fn text(&mut self) -> Result<String, ClipboardError> {
            Ok(self.0.to_owned())
        }
        fn set_text(&mut self, _: &str) -> Result<(), ClipboardError> {
            Ok(())
        }
        fn image(&mut self) -> Result<crate::clipboard::ClipboardImage, ClipboardError> {
            Err(ClipboardError::Empty("test"))
        }
        fn set_image(
            &mut self,
            _: &crate::clipboard::ClipboardImage,
        ) -> Result<(), ClipboardError> {
            Ok(())
        }
        fn persists_after_focus_loss(&self) -> bool {
            true
        }
    }

    #[test]
    fn clipboard_shortcuts_become_clipboard_events() {
        let mut input = EguiInput::new();
        let mut clip = FixedClipboard("Sky");
        let ctrl = Modifiers::NONE.with_ctrl();
        input.push(&key(Key::char('c'), Some("c"), ctrl), 1.0, None);
        input.push(&key(Key::char('v'), Some("v"), ctrl), 1.0, Some(&mut clip));
        let raw = input.take(egui::Rect::ZERO);
        assert_eq!(
            raw.events,
            [egui::Event::Copy, egui::Event::Paste("Sky".to_owned())]
        );
    }

    #[test]
    fn ime_and_focus_pass_through() {
        let mut input = EguiInput::new();
        input.push(
            &ShellEvent::Ime(ImeEvent::Commit("漢".to_owned())),
            1.0,
            None,
        );
        input.push(&ShellEvent::Focused(false), 1.0, None);
        let raw = input.take(egui::Rect::ZERO);
        assert_eq!(
            raw.events[0],
            egui::Event::Ime(egui::ImeEvent::Commit("漢".to_owned()))
        );
        assert!(!raw.focused);
    }

    #[test]
    fn named_and_function_keys_map() {
        assert_eq!(
            egui_key(&Key::Named(NamedKey::Enter)),
            Some(egui::Key::Enter)
        );
        assert_eq!(
            egui_key(&Key::Named(NamedKey::Function(2))),
            Some(egui::Key::F2)
        );
        assert_eq!(egui_key(&Key::char('1')), Some(egui::Key::Num1));
        assert_eq!(egui_key(&Key::Named(NamedKey::Other)), None);
    }
}
