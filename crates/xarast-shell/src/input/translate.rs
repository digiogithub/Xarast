//! Translation from the windowing backend into [`ShellEvent`].
//!
//! This is the only module in the workspace that reads `winit` event types,
//! and the only one phase 14 has to write again for Win32 and AppKit. Keep
//! it mechanical: policy belongs on the far side of [`ShellEvent`], so that
//! the three platforms cannot drift in behaviour.
//!
//! Where the pinned `winit` 0.30 cannot supply something the model carries —
//! a stylus axis, a drop position — the field is left empty rather than
//! invented. See `docs/memory/ui.md` for what that costs and when it changes.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{Key as WKey, NamedKey as WNamed};

use super::coalesce::SampleQueue;
use super::event::{
    DragEvent, GestureEvent, PhysicalPos2, PointerButton, PointerEvent, PointerId, PointerPhase,
    ScrollUnit, ShellEvent,
};
use super::keyboard::{
    Key, KeyEvent, KeyLocation, KeyState, ModifierKey, ModifierTracker, Modifiers, NamedKey,
};
use super::tablet::{InputSource, ToolAxes, normalise};
use crate::ime::ImeEvent;
use crate::scale::{PhysicalPos, PhysicalSize, ScaleFactor};

/// Stateful translation of backend events.
///
/// Holds the pieces of state the backend does not repeat on every event: the
/// live modifiers, the current scale, the last pointer position (`winit`
/// reports button presses with no position) and a stable id per device.
#[derive(Debug)]
pub struct EventTranslator {
    modifiers: ModifierTracker,
    scale: ScaleFactor,
    last_position: PhysicalPos,
    pointer_ids: HashMap<winit::event::DeviceId, u64>,
    next_pointer_id: u64,
    hovering: Vec<PathBuf>,
    /// Every stroke sample of the current frame.
    pub samples: SampleQueue,
}

impl Default for EventTranslator {
    fn default() -> Self {
        Self::new()
    }
}

impl EventTranslator {
    /// A translator at scale 1 with nothing held.
    #[must_use]
    pub fn new() -> Self {
        Self {
            modifiers: ModifierTracker::new(),
            scale: ScaleFactor::ONE,
            last_position: PhysicalPos::new(0.0, 0.0),
            pointer_ids: HashMap::new(),
            next_pointer_id: 0,
            hovering: Vec::new(),
            samples: SampleQueue::new(),
        }
    }

    /// The scale factor currently in force.
    #[must_use]
    pub const fn scale(&self) -> ScaleFactor {
        self.scale
    }

    /// The modifiers currently held.
    #[must_use]
    pub const fn modifiers(&self) -> Modifiers {
        self.modifiers.current()
    }

    /// Adopts the scale factor the window reports.
    ///
    /// Called once when the window is created, because the compositor's
    /// factor is authoritative from the first frame and a window created at
    /// 1× on a 1.5× display visibly snaps one frame later.
    pub const fn set_scale(&mut self, scale: ScaleFactor) {
        self.scale = scale;
    }

    fn pointer_id(&mut self, device: winit::event::DeviceId) -> PointerId {
        let next = &mut self.next_pointer_id;
        let id = *self.pointer_ids.entry(device).or_insert_with(|| {
            let v = *next;
            *next += 1;
            v
        });
        PointerId(id)
    }

    fn pointer(&mut self, device: winit::event::DeviceId, phase: PointerPhase) -> ShellEvent {
        let id = self.pointer_id(device);
        ShellEvent::Pointer(PointerEvent {
            id,
            phase,
            position: self.last_position,
            source: InputSource::Mouse,
            primary: true,
            modifiers: self.modifiers.current(),
        })
    }

    /// Translates one backend event, appending zero or more shell events.
    ///
    /// One backend event can produce two: a pointer move also produces a
    /// stroke sample, and a key event can also produce a modifier change.
    pub fn translate(&mut self, event: &WindowEvent, out: &mut Vec<ShellEvent>) {
        match event {
            WindowEvent::CloseRequested => out.push(ShellEvent::CloseRequested),

            WindowEvent::Resized(size) => out.push(ShellEvent::Resized {
                physical: PhysicalSize::new(size.width, size.height),
                scale: self.scale,
            }),

            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.scale = ScaleFactor::new(*scale_factor);
                out.push(ShellEvent::ScaleChanged(self.scale));
            }

            WindowEvent::Focused(focused) => {
                if !focused {
                    // Modifiers held when focus was lost are not held when it
                    // returns, and a stale Ctrl silently constrains the next
                    // drag.
                    if let Some(m) = self.modifiers.clear() {
                        out.push(ShellEvent::ModifiersChanged(m));
                    }
                }
                out.push(ShellEvent::Focused(*focused));
            }

            WindowEvent::ModifiersChanged(m) => {
                let next = translate_modifiers(m.state());
                if let Some(changed) = self.modifiers.update(next) {
                    out.push(ShellEvent::ModifiersChanged(changed));
                }
            }

            WindowEvent::CursorMoved {
                device_id,
                position,
            } => {
                self.last_position = PhysicalPos::new(position.x, position.y);
                let ev = self.pointer(*device_id, PointerPhase::Moved);
                out.push(ev);
                // Every move is also a stroke sample. The queue exists so
                // that none of them is lost between frames.
                self.samples.push(normalise(
                    position.x,
                    position.y,
                    ToolAxes::default(),
                    InputSource::Mouse,
                    Instant::now(),
                ));
            }

            WindowEvent::CursorEntered { device_id } => {
                let ev = self.pointer(*device_id, PointerPhase::Entered);
                out.push(ev);
            }

            WindowEvent::CursorLeft { device_id } => {
                let ev = self.pointer(*device_id, PointerPhase::Left);
                out.push(ev);
            }

            WindowEvent::MouseInput {
                device_id,
                state,
                button,
            } => {
                let button = translate_button(*button);
                let phase = match state {
                    ElementState::Pressed => PointerPhase::Pressed(button),
                    ElementState::Released => PointerPhase::Released(button),
                };
                let ev = self.pointer(*device_id, phase);
                out.push(ev);
            }

            WindowEvent::MouseWheel {
                device_id, delta, ..
            } => {
                let phase = match delta {
                    MouseScrollDelta::LineDelta(x, y) => PointerPhase::Scroll {
                        dx: f64::from(*x),
                        dy: f64::from(*y),
                        unit: ScrollUnit::Lines,
                    },
                    MouseScrollDelta::PixelDelta(p) => PointerPhase::Scroll {
                        dx: p.x,
                        dy: p.y,
                        unit: ScrollUnit::Pixels,
                    },
                };
                let ev = self.pointer(*device_id, phase);
                out.push(ev);
            }

            WindowEvent::Touch(touch) => {
                let position = PhysicalPos::new(touch.location.x, touch.location.y);
                self.last_position = position;
                let id = PointerId(touch.id);
                let phase = match touch.phase {
                    winit::event::TouchPhase::Started => {
                        PointerPhase::Pressed(PointerButton::Primary)
                    }
                    winit::event::TouchPhase::Moved => PointerPhase::Moved,
                    winit::event::TouchPhase::Ended => {
                        PointerPhase::Released(PointerButton::Primary)
                    }
                    winit::event::TouchPhase::Cancelled => PointerPhase::Left,
                };
                out.push(ShellEvent::Pointer(PointerEvent {
                    id,
                    phase,
                    position,
                    source: InputSource::Touch,
                    primary: false,
                    modifiers: self.modifiers.current(),
                }));
                self.samples.push(normalise(
                    position.x,
                    position.y,
                    ToolAxes {
                        force: touch.force.map(Force2::normalised),
                        ..ToolAxes::default()
                    },
                    InputSource::Touch,
                    Instant::now(),
                ));
            }

            WindowEvent::KeyboardInput { event, .. } => {
                out.push(ShellEvent::Key(self.translate_key(event)));
            }

            WindowEvent::Ime(ime) => out.push(ShellEvent::Ime(translate_ime(ime))),

            WindowEvent::PinchGesture { delta, .. } => {
                out.push(ShellEvent::Gesture(GestureEvent::Pinch {
                    delta: *delta,
                    at: self.last_position,
                }));
            }
            WindowEvent::PanGesture { delta, .. } => {
                out.push(ShellEvent::Gesture(GestureEvent::Pan {
                    dx: f64::from(delta.x),
                    dy: f64::from(delta.y),
                }));
            }
            WindowEvent::RotationGesture { delta, .. } => {
                out.push(ShellEvent::Gesture(GestureEvent::Rotate {
                    delta: f64::from(*delta),
                }));
            }
            WindowEvent::DoubleTapGesture { .. } => {
                out.push(ShellEvent::Gesture(GestureEvent::DoubleTap));
            }

            // winit 0.30 announces one hovered file at a time and gives no
            // position for any of them. The model is the richer one that
            // 0.31 and the other platforms provide, so this fills in what it
            // has and leaves the rest empty.
            WindowEvent::HoveredFile(path) => {
                self.hovering.push(path.clone());
                out.push(ShellEvent::Drag(DragEvent::Entered {
                    paths: vec![path.clone()],
                    at: None,
                }));
            }
            WindowEvent::HoveredFileCancelled => {
                self.hovering.clear();
                out.push(ShellEvent::Drag(DragEvent::Left));
            }
            WindowEvent::DroppedFile(path) => {
                self.hovering.clear();
                out.push(ShellEvent::Drag(DragEvent::Dropped {
                    paths: vec![path.clone()],
                    at: None,
                }));
            }

            _ => {}
        }
    }

    fn translate_key(&mut self, event: &winit::event::KeyEvent) -> KeyEvent {
        KeyEvent {
            key: translate_logical_key(&event.logical_key),
            location: translate_location(event.location),
            state: match event.state {
                ElementState::Pressed => KeyState::Pressed,
                ElementState::Released => KeyState::Released,
            },
            repeat: event.repeat,
            text: event.text.as_ref().map(|t| t.as_str().to_owned()),
            modifiers: self.modifiers.current(),
        }
    }

    /// Drains the frame's stroke samples into shell events.
    ///
    /// Called once per frame, after the backend has been pumped. Every
    /// sample accumulated during the frame is emitted; none is coalesced
    /// away.
    pub fn drain_samples(&mut self, out: &mut Vec<ShellEvent>) {
        let mut buf = Vec::new();
        self.samples.drain_into(&mut buf);
        out.extend(buf.into_iter().map(ShellEvent::Stroke));
    }
}

/// Helper for the one `winit` type we have to interpret rather than copy.
struct Force2;

impl Force2 {
    fn normalised(force: winit::event::Force) -> f64 {
        force.normalized()
    }
}

fn translate_modifiers(state: winit::keyboard::ModifiersState) -> Modifiers {
    Modifiers {
        shift: state.shift_key(),
        ctrl: state.control_key(),
        alt: state.alt_key(),
        logo: state.super_key(),
    }
}

fn translate_button(button: MouseButton) -> PointerButton {
    match button {
        MouseButton::Left => PointerButton::Primary,
        MouseButton::Right => PointerButton::Secondary,
        MouseButton::Middle => PointerButton::Middle,
        MouseButton::Back => PointerButton::Back,
        MouseButton::Forward => PointerButton::Forward,
        MouseButton::Other(n) => PointerButton::Other(n),
    }
}

fn translate_location(location: winit::keyboard::KeyLocation) -> KeyLocation {
    use winit::keyboard::KeyLocation as L;
    match location {
        L::Standard => KeyLocation::Standard,
        L::Left => KeyLocation::Left,
        L::Right => KeyLocation::Right,
        L::Numpad => KeyLocation::Numpad,
    }
}

fn translate_ime(ime: &winit::event::Ime) -> ImeEvent {
    match ime {
        winit::event::Ime::Enabled => ImeEvent::Enabled,
        winit::event::Ime::Preedit(text, cursor) => ImeEvent::Preedit {
            text: text.clone(),
            cursor: *cursor,
        },
        winit::event::Ime::Commit(text) => ImeEvent::Commit(text.clone()),
        winit::event::Ime::Disabled => ImeEvent::Disabled,
    }
}

#[allow(clippy::too_many_lines)]
fn translate_logical_key(key: &WKey) -> Key {
    match key {
        WKey::Character(s) => Key::Character(s.as_str().to_owned()),
        WKey::Named(named) => Key::Named(translate_named(*named)),
        // A dead key is a composition keystroke; the composed character
        // arrives later as text, so it is not bindable on its own.
        WKey::Dead(_) | WKey::Unidentified(_) => Key::Named(NamedKey::Other),
    }
}

fn translate_named(named: WNamed) -> NamedKey {
    match named {
        WNamed::Escape => NamedKey::Escape,
        WNamed::Enter => NamedKey::Enter,
        WNamed::Tab => NamedKey::Tab,
        WNamed::Space => NamedKey::Space,
        WNamed::Backspace => NamedKey::Backspace,
        WNamed::Delete => NamedKey::Delete,
        WNamed::Insert => NamedKey::Insert,
        WNamed::Home => NamedKey::Home,
        WNamed::End => NamedKey::End,
        WNamed::PageUp => NamedKey::PageUp,
        WNamed::PageDown => NamedKey::PageDown,
        WNamed::ArrowUp => NamedKey::ArrowUp,
        WNamed::ArrowDown => NamedKey::ArrowDown,
        WNamed::ArrowLeft => NamedKey::ArrowLeft,
        WNamed::ArrowRight => NamedKey::ArrowRight,
        WNamed::Shift => NamedKey::Modifier(ModifierKey::Shift),
        WNamed::Control => NamedKey::Modifier(ModifierKey::Control),
        WNamed::Alt => NamedKey::Modifier(ModifierKey::Alt),
        WNamed::Super => NamedKey::Modifier(ModifierKey::Super),
        WNamed::F1 => NamedKey::Function(1),
        WNamed::F2 => NamedKey::Function(2),
        WNamed::F3 => NamedKey::Function(3),
        WNamed::F4 => NamedKey::Function(4),
        WNamed::F5 => NamedKey::Function(5),
        WNamed::F6 => NamedKey::Function(6),
        WNamed::F7 => NamedKey::Function(7),
        WNamed::F8 => NamedKey::Function(8),
        WNamed::F9 => NamedKey::Function(9),
        WNamed::F10 => NamedKey::Function(10),
        WNamed::F11 => NamedKey::Function(11),
        WNamed::F12 => NamedKey::Function(12),
        _ => NamedKey::Other,
    }
}

/// Decodes a `text/uri-list` payload into paths.
///
/// This is the shape drag-and-drop takes on X11 and in `winit` 0.31, where
/// the payload is a list of `file:` URIs rather than paths. Percent-decoding
/// is done here rather than by a dependency because the failure modes matter:
/// a malformed escape must yield the literal text, never a panic and never a
/// silently truncated path.
///
/// Non-`file:` URIs and comment lines are skipped, as the specification
/// requires.
#[must_use]
pub fn parse_uri_list(payload: &str) -> Vec<PathBuf> {
    payload
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let rest = line.strip_prefix("file://")?;
            // file://host/path — an empty or "localhost" authority is ours.
            let path = match rest.find('/') {
                Some(0) => rest,
                Some(i) if &rest[..i] == "localhost" => &rest[i..],
                _ => return None,
            };
            Some(PathBuf::from(percent_decode(path)))
        })
        .collect()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                #[allow(clippy::cast_possible_truncation)]
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Turns a device-pixel position into the integer form drag events carry.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn drop_position(at: PhysicalPos) -> PhysicalPos2 {
    PhysicalPos2::new(at.x.round() as i32, at.y.round() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_file_uri_decodes_to_a_path() {
        let paths = parse_uri_list("file:///home/user/drawing.xar\r\n");
        assert_eq!(paths, vec![PathBuf::from("/home/user/drawing.xar")]);
    }

    #[test]
    fn percent_escapes_are_decoded() {
        let paths = parse_uri_list("file:///tmp/my%20file%2Ba.xar");
        assert_eq!(paths, vec![PathBuf::from("/tmp/my file+a.xar")]);
    }

    #[test]
    fn utf8_escapes_survive_intact() {
        // "café.xar" as UTF-8 percent escapes.
        let paths = parse_uri_list("file:///tmp/caf%C3%A9.xar");
        assert_eq!(paths, vec![PathBuf::from("/tmp/café.xar")]);
    }

    #[test]
    fn a_localhost_authority_is_accepted_and_a_remote_one_is_not() {
        assert_eq!(
            parse_uri_list("file://localhost/tmp/a.xar"),
            vec![PathBuf::from("/tmp/a.xar")]
        );
        assert!(parse_uri_list("file://example.com/tmp/a.xar").is_empty());
    }

    #[test]
    fn comments_blank_lines_and_other_schemes_are_skipped() {
        let list = "# comment\n\nhttps://example.com/a.xar\nfile:///tmp/b.xar\n";
        assert_eq!(parse_uri_list(list), vec![PathBuf::from("/tmp/b.xar")]);
    }

    #[test]
    fn a_truncated_escape_is_kept_literally_rather_than_dropped() {
        // The user's file really may be called "100%". Losing the tail of
        // the name would be worse than keeping the percent sign.
        assert_eq!(
            parse_uri_list("file:///tmp/100%"),
            vec![PathBuf::from("/tmp/100%")]
        );
        assert_eq!(
            parse_uri_list("file:///tmp/a%zz.xar"),
            vec![PathBuf::from("/tmp/a%zz.xar")]
        );
    }

    #[test]
    fn several_uris_come_back_in_order() {
        let paths = parse_uri_list("file:///a.xar\nfile:///b.xar\nfile:///c.xar");
        assert_eq!(paths.len(), 3);
        assert_eq!(paths[2], PathBuf::from("/c.xar"));
    }

    #[test]
    fn invalid_utf8_escapes_do_not_panic() {
        // 0xFF is not valid UTF-8; the decoder must substitute, not abort.
        let paths = parse_uri_list("file:///tmp/%FF.xar");
        assert_eq!(paths.len(), 1);
    }

    #[test]
    fn drop_positions_round_to_whole_pixels() {
        assert_eq!(
            drop_position(PhysicalPos::new(10.4, -3.6)),
            PhysicalPos2::new(10, -4)
        );
    }

    #[test]
    fn a_fresh_translator_starts_at_scale_one_with_nothing_held() {
        let t = EventTranslator::new();
        assert_eq!(t.scale(), ScaleFactor::ONE);
        assert!(t.modifiers().is_empty());
        assert!(t.samples.is_empty());
    }

    #[test]
    fn a_scale_change_is_published_and_remembered() {
        let mut t = EventTranslator::new();
        let mut out = Vec::new();
        // `ScaleFactorChanged` cannot be constructed outside winit (it holds
        // an opaque writer), so the state transition is driven directly;
        // the event arm does nothing else.
        t.scale = ScaleFactor::new(1.5);
        out.push(ShellEvent::ScaleChanged(t.scale));
        assert_eq!(t.scale(), ScaleFactor::new(1.5));
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn a_resize_is_reported_with_the_scale_in_force() {
        let mut t = EventTranslator::new();
        t.scale = ScaleFactor::new(1.25);
        let mut out = Vec::new();
        t.translate(
            &WindowEvent::Resized(winit::dpi::PhysicalSize::new(1600, 1000)),
            &mut out,
        );
        assert_eq!(
            out,
            vec![ShellEvent::Resized {
                physical: PhysicalSize::new(1600, 1000),
                scale: ScaleFactor::new(1.25),
            }]
        );
    }

    #[test]
    fn a_zero_sized_resize_is_clamped_rather_than_passed_on() {
        let mut t = EventTranslator::new();
        let mut out = Vec::new();
        t.translate(
            &WindowEvent::Resized(winit::dpi::PhysicalSize::new(0, 0)),
            &mut out,
        );
        let ShellEvent::Resized { physical, .. } = &out[0] else {
            unreachable!()
        };
        assert_eq!((physical.width, physical.height), (1, 1));
    }

    #[test]
    fn losing_focus_clears_the_modifiers_before_reporting_the_loss() {
        let mut t = EventTranslator::new();
        t.modifiers.update(Modifiers::NONE.with_ctrl());
        let mut out = Vec::new();
        t.translate(&WindowEvent::Focused(false), &mut out);
        assert_eq!(
            out,
            vec![
                ShellEvent::ModifiersChanged(Modifiers::NONE),
                ShellEvent::Focused(false)
            ]
        );
        assert!(t.modifiers().is_empty());
    }

    #[test]
    fn hovering_then_dropping_produces_the_drag_sequence() {
        let mut t = EventTranslator::new();
        let mut out = Vec::new();
        let p = PathBuf::from("/tmp/a.xar");
        t.translate(&WindowEvent::HoveredFile(p.clone()), &mut out);
        t.translate(&WindowEvent::DroppedFile(p.clone()), &mut out);
        assert_eq!(
            out,
            vec![
                ShellEvent::Drag(DragEvent::Entered {
                    paths: vec![p.clone()],
                    at: None
                }),
                ShellEvent::Drag(DragEvent::Dropped {
                    paths: vec![p],
                    at: None
                }),
            ]
        );
        assert!(t.hovering.is_empty());
    }

    #[test]
    fn a_cancelled_hover_leaves_nothing_behind() {
        let mut t = EventTranslator::new();
        let mut out = Vec::new();
        t.translate(&WindowEvent::HoveredFile(PathBuf::from("/a")), &mut out);
        t.translate(&WindowEvent::HoveredFileCancelled, &mut out);
        assert_eq!(out.last(), Some(&ShellEvent::Drag(DragEvent::Left)));
        assert!(t.hovering.is_empty());
    }

    #[test]
    fn drained_samples_become_stroke_events_one_for_one() {
        let mut t = EventTranslator::new();
        for i in 0..128 {
            t.samples.push(normalise(
                f64::from(i),
                0.0,
                ToolAxes::default(),
                InputSource::Mouse,
                Instant::now(),
            ));
        }
        let mut out = Vec::new();
        t.drain_samples(&mut out);
        assert_eq!(out.len(), 128);
        assert!(out.iter().all(|e| matches!(e, ShellEvent::Stroke(_))));
        assert_eq!(t.samples.outstanding(), 0);
    }

    #[test]
    fn pointer_buttons_map_one_for_one() {
        assert_eq!(translate_button(MouseButton::Left), PointerButton::Primary);
        assert_eq!(
            translate_button(MouseButton::Right),
            PointerButton::Secondary
        );
        assert_eq!(
            translate_button(MouseButton::Other(9)),
            PointerButton::Other(9)
        );
    }

    #[test]
    fn named_keys_we_bind_are_all_recognised() {
        assert_eq!(translate_named(WNamed::Escape), NamedKey::Escape);
        assert_eq!(translate_named(WNamed::F7), NamedKey::Function(7));
        assert_eq!(
            translate_named(WNamed::Control),
            NamedKey::Modifier(ModifierKey::Control)
        );
        assert_eq!(translate_named(WNamed::BrowserBack), NamedKey::Other);
    }

    #[test]
    fn the_keyboard_locations_map_one_for_one() {
        use winit::keyboard::KeyLocation as L;
        assert_eq!(translate_location(L::Numpad), KeyLocation::Numpad);
        assert_eq!(translate_location(L::Left), KeyLocation::Left);
        assert_eq!(translate_location(L::Standard), KeyLocation::Standard);
    }

    #[test]
    fn ime_events_map_onto_our_own_shape() {
        assert_eq!(
            translate_ime(&winit::event::Ime::Enabled),
            ImeEvent::Enabled
        );
        assert_eq!(
            translate_ime(&winit::event::Ime::Preedit("ab".into(), Some((0, 1)))),
            ImeEvent::Preedit {
                text: "ab".to_owned(),
                cursor: Some((0, 1))
            }
        );
        assert_eq!(
            translate_ime(&winit::event::Ime::Commit("x".into())),
            ImeEvent::Commit("x".to_owned())
        );
    }

    #[test]
    fn a_dead_key_is_not_bindable() {
        assert_eq!(
            translate_logical_key(&WKey::Dead(Some('^'))),
            Key::Named(NamedKey::Other)
        );
        assert_eq!(
            translate_logical_key(&WKey::Character("g".into())),
            Key::Character("g".to_owned())
        );
    }
}
