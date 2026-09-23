//! The bridge from [`ShellEvent`] to [`xarast_app::Intent`].
//!
//! Two vocabularies meet here and neither owns the other:
//!
//! * the shell's is **physical** — `Shift`, `Ctrl`, `Alt`, the middle
//!   button, a wheel notch, a pixel position in the window;
//! * the application's is **semantic** — `constrain`, `adjust`,
//!   `alternative`, `snap`, "pan by this much", "zoom about that point",
//!   a position in the canvas's own device space.
//!
//! This module is the one table between them. It deliberately does not live
//! in [`crate::input::translate`], which phase 14 rewrites per platform: the
//! mapping here is written once against [`ShellEvent`] and names no `winit`
//! type, so the Win32 and AppKit shells get it for free. Nor does it live in
//! `xarast-app`, which must not know that a shell exists.
//!
//! # What becomes what
//!
//! | Shell event | Intent |
//! |---|---|
//! | `Resized` / `ScaleChanged` | `SetDpi(96 × scale)`; the canvas size comes from [`IntentAdapter::set_canvas`] |
//! | pointer moved | `PointerMove`, plus `Pan` while the middle button is held |
//! | button pressed / released over the canvas | `PointerDown` / `PointerUp` with the semantic button |
//! | wheel | `Pan`; with **constrain** held, `Zoom` about the pointer |
//! | pinch / two-finger pan | `Zoom` / `Pan` |
//! | modifiers changed | `ModifiersChanged` with the semantic triple |
//! | pointer left, focus lost | `PointerLeft` |
//!
//! Everything else — keys, drops, portal answers, close — is not an input
//! intent and is left to the caller.
//!
//! # Ordering
//!
//! Pointer motion is taken from [`ShellEvent::Pointer`], not from
//! [`ShellEvent::Stroke`]. The stroke samples are drained once per frame, so
//! they arrive after every press and release of that frame; a pan that
//! started and ended inside one frame would lose its motion. The pointer
//! events are in order. The stroke path carries pressure, which `winit 0.30`
//! never supplies (`docs/memory/ui.md`), so nothing is lost today; merging
//! pressure back in by timestamp is a phase 7 job for the freehand tool.

use std::time::Instant;

use xarast_app::{DevicePoint, DeviceSize, Intent, PointerSample};

use crate::input::event::{GestureEvent, PointerButton, PointerPhase, ScrollUnit, ShellEvent};
use crate::input::keyboard::Modifiers;
use crate::scale::{PhysicalPos, ScaleFactor};

/// Device pixels per inch at a scale factor of one.
///
/// The desktop convention on Linux, Windows and the web. The shell is the
/// single owner of the resulting DPI (`docs/memory/app-core.md` §4).
pub const BASE_DPI: f64 = 96.0;

/// One wheel notch's zoom factor, the same ratio the canvas widget uses.
pub const WHEEL_ZOOM_STEP: f64 = std::f64::consts::SQRT_2;

/// Device pixels one wheel notch pans by, and the pixel delta a smooth
/// wheel or trackpad has to accumulate to count as one notch of zoom.
pub const PIXELS_PER_NOTCH: f64 = 50.0;

/// Maps the physical modifiers onto the application's semantic ones.
///
/// The table is Xara's (`research/04 §4.1`): `Ctrl` constrains, `Shift`
/// adjusts, `Alt` is the alternative. `snap` is not a key the user holds
/// but a toggle, so it is passed in. This is the function phase 14 edits
/// for macOS, where constrain wants `Cmd`.
#[must_use]
pub const fn semantic_modifiers(m: Modifiers, snap: bool) -> xarast_app::Modifiers {
    xarast_app::Modifiers {
        constrain: m.constrain(),
        adjust: m.adjust(),
        alternative: m.alternative(),
        snap,
    }
}

/// Maps a physical button onto the application's semantic one.
///
/// A stylus tip is the primary button and a barrel button the secondary,
/// which is what every drawing program does. `Back`, `Forward` and unknown
/// buttons have no meaning on the canvas and map to nothing.
#[must_use]
pub const fn semantic_button(b: PointerButton) -> Option<xarast_app::PointerButton> {
    match b {
        PointerButton::Primary | PointerButton::ToolTip => Some(xarast_app::PointerButton::Primary),
        PointerButton::Secondary | PointerButton::ToolBarrel => {
            Some(xarast_app::PointerButton::Secondary)
        }
        PointerButton::Middle => Some(xarast_app::PointerButton::Middle),
        PointerButton::Back | PointerButton::Forward | PointerButton::Other(_) => None,
    }
}

/// Where the canvas is inside the window, in whole device pixels.
///
/// Intents are always canvas-relative (`xarast_app::PointerSample`), so the
/// adapter subtracts this origin and ignores input outside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CanvasRegion {
    /// Left edge, device pixels from the window's left.
    pub x: i32,
    /// Top edge, device pixels from the window's top.
    pub y: i32,
    /// Width in device pixels.
    pub width: u32,
    /// Height in device pixels.
    pub height: u32,
}

impl CanvasRegion {
    /// A region.
    #[must_use]
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> CanvasRegion {
        CanvasRegion {
            x,
            y,
            width,
            height,
        }
    }

    /// Whether a window position is inside the canvas.
    #[must_use]
    pub fn contains(&self, p: PhysicalPos) -> bool {
        let (x0, y0) = (f64::from(self.x), f64::from(self.y));
        p.x >= x0
            && p.y >= y0
            && p.x < x0 + f64::from(self.width)
            && p.y < y0 + f64::from(self.height)
    }

    /// A window position in the canvas's device space.
    #[must_use]
    pub fn to_canvas(&self, p: PhysicalPos) -> DevicePoint {
        DevicePoint::new(p.x - f64::from(self.x), p.y - f64::from(self.y))
    }

    /// The canvas size, which is what the viewport is sized to.
    #[must_use]
    pub const fn size(&self) -> DeviceSize {
        DeviceSize::new(self.width, self.height)
    }
}

/// Turns a stream of [`ShellEvent`]s into [`Intent`]s.
///
/// Stateful only where the platform is: which buttons went down over the
/// canvas, where the pointer was, whether a middle-button pan is under way,
/// and the live modifiers. Nothing here is latched: modifiers are forwarded
/// the moment they change, and the wheel reads them per event.
#[derive(Debug, Clone)]
pub struct IntentAdapter {
    canvas: CanvasRegion,
    scale: ScaleFactor,
    modifiers: Modifiers,
    snap: bool,
    /// Buttons pressed while the pointer was over the canvas. A press that
    /// started over a panel does not become a canvas drag when the pointer
    /// wanders in, and its release is not reported to the canvas either.
    held: Vec<xarast_app::PointerButton>,
    /// The last position a middle-button pan was at.
    pan_from: Option<PhysicalPos>,
    last: Option<PhysicalPos>,
    epoch: Instant,
}

impl Default for IntentAdapter {
    fn default() -> Self {
        IntentAdapter::new()
    }
}

impl IntentAdapter {
    /// An adapter with an empty canvas at scale one.
    #[must_use]
    pub fn new() -> IntentAdapter {
        IntentAdapter {
            canvas: CanvasRegion::default(),
            scale: ScaleFactor::new(1.0),
            modifiers: Modifiers::NONE,
            snap: false,
            held: Vec::new(),
            pan_from: None,
            last: None,
            epoch: Instant::now(),
        }
    }

    /// The canvas region in force.
    #[must_use]
    pub const fn canvas(&self) -> CanvasRegion {
        self.canvas
    }

    /// The live semantic modifiers.
    #[must_use]
    pub const fn modifiers(&self) -> xarast_app::Modifiers {
        semantic_modifiers(self.modifiers, self.snap)
    }

    /// Moves or resizes the canvas. Emits `Resize` when the size changed;
    /// a move alone only changes how later positions are offset.
    pub fn set_canvas(&mut self, region: CanvasRegion, out: &mut Vec<Intent>) {
        let resized = region.size() != self.canvas.size();
        self.canvas = region;
        if resized && !region.size().is_empty() {
            out.push(Intent::Resize(region.size()));
        }
    }

    /// Sets the snapping toggle and publishes the new modifier state.
    pub fn set_snap(&mut self, snap: bool, out: &mut Vec<Intent>) {
        if snap != self.snap {
            self.snap = snap;
            out.push(Intent::ModifiersChanged(self.modifiers()));
        }
    }

    fn sample(&self, p: PhysicalPos) -> PointerSample {
        let ms = u64::try_from(self.epoch.elapsed().as_millis()).unwrap_or(u64::MAX);
        PointerSample {
            at: self.canvas.to_canvas(p),
            pressure: None,
            time_ms: ms,
        }
    }

    /// Translates one event, appending zero or more intents.
    pub fn translate(&mut self, event: &ShellEvent, out: &mut Vec<Intent>) {
        match event {
            ShellEvent::Resized { scale, .. } | ShellEvent::ScaleChanged(scale) => {
                if *scale != self.scale {
                    self.scale = *scale;
                    out.push(Intent::SetDpi(BASE_DPI * scale.get()));
                }
            }
            ShellEvent::ModifiersChanged(m) => {
                if *m != self.modifiers {
                    self.modifiers = *m;
                    out.push(Intent::ModifiersChanged(self.modifiers()));
                }
            }
            ShellEvent::Focused(false) => {
                // Whatever was held is no longer known to be held. The
                // translator clears the modifiers itself; the buttons are
                // ours to forget.
                self.release_all(out);
            }
            ShellEvent::Pointer(p) => self.pointer(p.phase, p.position, out),
            ShellEvent::Gesture(g) => self.gesture(*g, out),
            _ => {}
        }
    }

    /// Initial intents for a freshly created canvas: its size and the DPI.
    /// Call once, before the first frame, so the viewport never renders at
    /// a stale size or scale.
    pub fn prime(&mut self, region: CanvasRegion, scale: ScaleFactor, out: &mut Vec<Intent>) {
        self.canvas = CanvasRegion::default();
        self.set_canvas(region, out);
        self.scale = scale;
        out.push(Intent::SetDpi(BASE_DPI * scale.get()));
    }

    fn release_all(&mut self, out: &mut Vec<Intent>) {
        let at = self.last.unwrap_or(PhysicalPos::new(
            f64::from(self.canvas.x),
            f64::from(self.canvas.y),
        ));
        for button in std::mem::take(&mut self.held) {
            out.push(Intent::PointerUp {
                button,
                sample: self.sample(at),
            });
        }
        self.pan_from = None;
        out.push(Intent::PointerLeft);
    }

    fn pointer(&mut self, phase: PointerPhase, at: PhysicalPos, out: &mut Vec<Intent>) {
        let inside = self.canvas.contains(at);
        match phase {
            PointerPhase::Entered => {}
            PointerPhase::Moved => {
                if let Some(from) = self.pan_from {
                    let (dx, dy) = (at.x - from.x, at.y - from.y);
                    if dx != 0.0 || dy != 0.0 {
                        out.push(Intent::Pan { dx, dy });
                    }
                    self.pan_from = Some(at);
                }
                // A drag that started on the canvas keeps reporting when it
                // leaves it: the tool owns the pointer until release.
                if inside || !self.held.is_empty() {
                    out.push(Intent::PointerMove(self.sample(at)));
                } else if self.last.is_some_and(|l| self.canvas.contains(l)) {
                    out.push(Intent::PointerLeft);
                }
            }
            PointerPhase::Pressed(b) => {
                let Some(button) = semantic_button(b) else {
                    return;
                };
                if !inside || self.held.contains(&button) {
                    return;
                }
                self.held.push(button);
                if button == xarast_app::PointerButton::Middle {
                    self.pan_from = Some(at);
                }
                out.push(Intent::PointerDown {
                    button,
                    sample: self.sample(at),
                });
            }
            PointerPhase::Released(b) => {
                let Some(button) = semantic_button(b) else {
                    return;
                };
                let Some(i) = self.held.iter().position(|h| *h == button) else {
                    return;
                };
                self.held.remove(i);
                if button == xarast_app::PointerButton::Middle {
                    self.pan_from = None;
                }
                out.push(Intent::PointerUp {
                    button,
                    sample: self.sample(at),
                });
            }
            PointerPhase::Scroll { dx, dy, unit } => {
                if inside {
                    self.wheel(dx, dy, unit, at, out);
                }
            }
            PointerPhase::Left => {
                if self.held.is_empty() && self.last.is_some_and(|l| self.canvas.contains(l)) {
                    out.push(Intent::PointerLeft);
                }
            }
        }
        if !matches!(phase, PointerPhase::Left) {
            self.last = Some(at);
        }
    }

    fn wheel(&self, dx: f64, dy: f64, unit: ScrollUnit, at: PhysicalPos, out: &mut Vec<Intent>) {
        if !dx.is_finite() || !dy.is_finite() {
            return;
        }
        let per_notch = match unit {
            ScrollUnit::Lines => PIXELS_PER_NOTCH,
            ScrollUnit::Pixels => 1.0,
        };
        let (px, py) = (dx * per_notch, dy * per_notch);
        if self.modifiers.constrain() {
            // Constrain + wheel zooms about the pointer. A positive `dy` is
            // the wheel turned away from the user, which zooms in.
            let notches = py / PIXELS_PER_NOTCH;
            if notches != 0.0 {
                out.push(Intent::Zoom {
                    factor: WHEEL_ZOOM_STEP.powf(notches),
                    anchor: self.canvas.to_canvas(at),
                });
            }
        } else if self.modifiers.adjust() && px == 0.0 {
            // Adjust + wheel scrolls sideways on a wheel with one axis.
            if py != 0.0 {
                out.push(Intent::Pan { dx: py, dy: 0.0 });
            }
        } else if px != 0.0 || py != 0.0 {
            out.push(Intent::Pan { dx: px, dy: py });
        }
    }

    fn gesture(&self, g: GestureEvent, out: &mut Vec<Intent>) {
        match g {
            GestureEvent::Pinch { delta, at } => {
                let factor = 1.0 + delta;
                if delta.is_finite() && factor > 0.0 && delta != 0.0 {
                    out.push(Intent::Zoom {
                        factor,
                        anchor: self.canvas.to_canvas(at),
                    });
                }
            }
            GestureEvent::Pan { dx, dy } => {
                if dx.is_finite() && dy.is_finite() && (dx != 0.0 || dy != 0.0) {
                    out.push(Intent::Pan { dx, dy });
                }
            }
            // Rotating the view is not a Phase 5 feature, and a double tap
            // has no binding yet.
            GestureEvent::Rotate { .. } | GestureEvent::DoubleTap => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::event::{PointerEvent, PointerId};
    use crate::input::tablet::InputSource;
    use crate::scale::PhysicalSize;
    use xarast_app::PointerButton as AppButton;

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

    fn adapter() -> IntentAdapter {
        let mut a = IntentAdapter::new();
        let mut out = Vec::new();
        a.prime(
            CanvasRegion::new(100, 50, 800, 600),
            ScaleFactor::new(1.0),
            &mut out,
        );
        a
    }

    fn run(a: &mut IntentAdapter, events: &[ShellEvent]) -> Vec<Intent> {
        let mut out = Vec::new();
        for e in events {
            a.translate(e, &mut out);
        }
        out
    }

    #[test]
    fn the_physical_modifiers_map_onto_xaras_roles() {
        let m = semantic_modifiers(Modifiers::NONE.with_ctrl(), false);
        assert!(m.constrain && !m.adjust && !m.alternative && !m.snap);
        let m = semantic_modifiers(Modifiers::NONE.with_shift(), true);
        assert!(!m.constrain && m.adjust && !m.alternative && m.snap);
        let m = semantic_modifiers(Modifiers::NONE.with_alt(), false);
        assert!(m.alternative && !m.constrain && !m.adjust);
        // Super has no role of its own.
        assert_eq!(
            semantic_modifiers(Modifiers::NONE.with_logo(), false),
            xarast_app::Modifiers::default()
        );
    }

    #[test]
    fn the_physical_buttons_map_onto_semantic_ones() {
        assert_eq!(
            semantic_button(PointerButton::Primary),
            Some(AppButton::Primary)
        );
        assert_eq!(
            semantic_button(PointerButton::ToolTip),
            Some(AppButton::Primary)
        );
        assert_eq!(
            semantic_button(PointerButton::ToolBarrel),
            Some(AppButton::Secondary)
        );
        assert_eq!(
            semantic_button(PointerButton::Middle),
            Some(AppButton::Middle)
        );
        assert_eq!(semantic_button(PointerButton::Back), None);
        assert_eq!(semantic_button(PointerButton::Other(9)), None);
    }

    #[test]
    fn priming_sizes_the_viewport_and_sets_the_dpi() {
        let mut a = IntentAdapter::new();
        let mut out = Vec::new();
        a.prime(
            CanvasRegion::new(0, 0, 1600, 1000),
            ScaleFactor::new(1.5),
            &mut out,
        );
        assert_eq!(
            out,
            vec![
                Intent::Resize(DeviceSize::new(1600, 1000)),
                Intent::SetDpi(144.0)
            ]
        );
    }

    #[test]
    fn a_scale_change_becomes_a_dpi_and_a_repeat_does_not() {
        let mut a = adapter();
        let ev = ShellEvent::Resized {
            physical: PhysicalSize::new(1000, 700),
            scale: ScaleFactor::new(1.25),
        };
        assert_eq!(
            run(&mut a, std::slice::from_ref(&ev)),
            vec![Intent::SetDpi(120.0)]
        );
        assert!(run(&mut a, &[ev]).is_empty());
    }

    #[test]
    fn moving_the_canvas_alone_does_not_resize_the_viewport() {
        let mut a = adapter();
        let mut out = Vec::new();
        a.set_canvas(CanvasRegion::new(0, 0, 800, 600), &mut out);
        assert!(out.is_empty());
        a.set_canvas(CanvasRegion::new(0, 0, 810, 600), &mut out);
        assert_eq!(out, vec![Intent::Resize(DeviceSize::new(810, 600))]);
    }

    #[test]
    fn positions_are_canvas_relative() {
        let mut a = adapter();
        let out = run(&mut a, &[ptr(PointerPhase::Moved, 150.0, 70.0)]);
        let [Intent::PointerMove(s)] = out.as_slice() else {
            panic!("{out:?}")
        };
        assert_eq!(s.at, DevicePoint::new(50.0, 20.0));
    }

    #[test]
    fn a_middle_drag_pans_by_the_pointer_delta_in_order() {
        let mut a = adapter();
        let out = run(
            &mut a,
            &[
                ptr(PointerPhase::Moved, 200.0, 200.0),
                ptr(PointerPhase::Pressed(PointerButton::Middle), 200.0, 200.0),
                ptr(PointerPhase::Moved, 210.0, 195.0),
                ptr(PointerPhase::Moved, 230.0, 190.0),
                ptr(PointerPhase::Released(PointerButton::Middle), 230.0, 190.0),
                ptr(PointerPhase::Moved, 240.0, 190.0),
            ],
        );
        let pans: Vec<_> = out
            .iter()
            .filter_map(|i| match i {
                Intent::Pan { dx, dy } => Some((*dx, *dy)),
                _ => None,
            })
            .collect();
        assert_eq!(pans, vec![(10.0, -5.0), (20.0, -5.0)]);
        assert!(matches!(
            out[1],
            Intent::PointerDown {
                button: AppButton::Middle,
                ..
            }
        ));
        assert!(out.iter().any(|i| matches!(
            i,
            Intent::PointerUp {
                button: AppButton::Middle,
                ..
            }
        )));
    }

    #[test]
    fn a_press_outside_the_canvas_is_not_a_canvas_press() {
        let mut a = adapter();
        let out = run(
            &mut a,
            &[
                ptr(PointerPhase::Pressed(PointerButton::Primary), 10.0, 10.0),
                ptr(PointerPhase::Moved, 200.0, 200.0),
                ptr(PointerPhase::Released(PointerButton::Primary), 200.0, 200.0),
            ],
        );
        assert!(
            out.iter()
                .all(|i| !matches!(i, Intent::PointerDown { .. } | Intent::PointerUp { .. })),
            "{out:?}"
        );
    }

    #[test]
    fn a_drag_that_leaves_the_canvas_keeps_reporting_until_release() {
        let mut a = adapter();
        let out = run(
            &mut a,
            &[
                ptr(PointerPhase::Pressed(PointerButton::Primary), 200.0, 200.0),
                ptr(PointerPhase::Moved, 5.0, 5.0),
                ptr(PointerPhase::Released(PointerButton::Primary), 5.0, 5.0),
            ],
        );
        assert!(matches!(out[1], Intent::PointerMove(_)), "{out:?}");
        assert!(matches!(out[2], Intent::PointerUp { .. }), "{out:?}");
    }

    #[test]
    fn the_wheel_pans_and_constrain_wheel_zooms_about_the_pointer() {
        let mut a = adapter();
        let scroll = |dy| {
            ptr(
                PointerPhase::Scroll {
                    dx: 0.0,
                    dy,
                    unit: ScrollUnit::Lines,
                },
                300.0,
                250.0,
            )
        };
        assert_eq!(
            run(&mut a, &[scroll(1.0)]),
            vec![Intent::Pan { dx: 0.0, dy: 50.0 }]
        );

        let out = run(
            &mut a,
            &[
                ShellEvent::ModifiersChanged(Modifiers::NONE.with_ctrl()),
                scroll(2.0),
            ],
        );
        assert!(matches!(out[0], Intent::ModifiersChanged(m) if m.constrain));
        let Intent::Zoom { factor, anchor } = out[1] else {
            panic!("{out:?}")
        };
        assert!((factor - 2.0).abs() < 1e-12, "two notches is ×2: {factor}");
        assert_eq!(anchor, DevicePoint::new(200.0, 200.0));

        let out = run(
            &mut a,
            &[
                ShellEvent::ModifiersChanged(Modifiers::NONE.with_shift()),
                scroll(-1.0),
            ],
        );
        assert_eq!(out[1], Intent::Pan { dx: -50.0, dy: 0.0 });
    }

    #[test]
    fn a_wheel_outside_the_canvas_does_nothing() {
        let mut a = adapter();
        let out = run(
            &mut a,
            &[ptr(
                PointerPhase::Scroll {
                    dx: 0.0,
                    dy: 3.0,
                    unit: ScrollUnit::Pixels,
                },
                10.0,
                10.0,
            )],
        );
        assert!(out.is_empty());
    }

    #[test]
    fn hostile_deltas_are_dropped() {
        let mut a = adapter();
        let out = run(
            &mut a,
            &[
                ptr(
                    PointerPhase::Scroll {
                        dx: f64::NAN,
                        dy: 1.0,
                        unit: ScrollUnit::Pixels,
                    },
                    300.0,
                    300.0,
                ),
                ShellEvent::Gesture(GestureEvent::Pinch {
                    delta: -1.0,
                    at: PhysicalPos::new(300.0, 300.0),
                }),
                ShellEvent::Gesture(GestureEvent::Pan {
                    dx: f64::INFINITY,
                    dy: 0.0,
                }),
            ],
        );
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn gestures_become_zoom_and_pan() {
        let mut a = adapter();
        let out = run(
            &mut a,
            &[
                ShellEvent::Gesture(GestureEvent::Pinch {
                    delta: 0.25,
                    at: PhysicalPos::new(500.0, 350.0),
                }),
                ShellEvent::Gesture(GestureEvent::Pan { dx: 4.0, dy: -3.0 }),
                ShellEvent::Gesture(GestureEvent::Rotate { delta: 0.3 }),
            ],
        );
        assert_eq!(
            out,
            vec![
                Intent::Zoom {
                    factor: 1.25,
                    anchor: DevicePoint::new(400.0, 300.0)
                },
                Intent::Pan { dx: 4.0, dy: -3.0 }
            ]
        );
    }

    #[test]
    fn modifiers_are_forwarded_live_and_only_on_change() {
        let mut a = adapter();
        let out = run(
            &mut a,
            &[
                ptr(PointerPhase::Pressed(PointerButton::Primary), 200.0, 200.0),
                ShellEvent::ModifiersChanged(Modifiers::NONE.with_ctrl().with_alt()),
                ShellEvent::ModifiersChanged(Modifiers::NONE.with_ctrl().with_alt()),
                ptr(PointerPhase::Moved, 220.0, 200.0),
            ],
        );
        let mods: Vec<_> = out
            .iter()
            .filter_map(|i| match i {
                Intent::ModifiersChanged(m) => Some(*m),
                _ => None,
            })
            .collect();
        assert_eq!(mods.len(), 1, "a repeated state is not a change");
        assert!(mods[0].constrain && mods[0].alternative && !mods[0].adjust);

        let mut out = Vec::new();
        a.set_snap(true, &mut out);
        a.set_snap(true, &mut out);
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0], Intent::ModifiersChanged(m) if m.snap && m.constrain));
    }

    #[test]
    fn losing_focus_releases_every_held_button_and_ends_the_pan() {
        let mut a = adapter();
        let out = run(
            &mut a,
            &[
                ptr(PointerPhase::Pressed(PointerButton::Middle), 200.0, 200.0),
                ptr(PointerPhase::Pressed(PointerButton::Primary), 200.0, 200.0),
                ShellEvent::Focused(false),
                ptr(PointerPhase::Moved, 250.0, 250.0),
            ],
        );
        let ups = out
            .iter()
            .filter(|i| matches!(i, Intent::PointerUp { .. }))
            .count();
        assert_eq!(ups, 2);
        assert!(out.contains(&Intent::PointerLeft));
        assert!(
            !out.iter().any(|i| matches!(i, Intent::Pan { .. })),
            "no pan after focus loss: {out:?}"
        );
    }

    #[test]
    fn leaving_the_canvas_is_reported_once() {
        let mut a = adapter();
        let out = run(
            &mut a,
            &[
                ptr(PointerPhase::Moved, 200.0, 200.0),
                ptr(PointerPhase::Moved, 5.0, 5.0),
                ptr(PointerPhase::Moved, 6.0, 5.0),
            ],
        );
        assert_eq!(
            out.iter().filter(|i| **i == Intent::PointerLeft).count(),
            1,
            "{out:?}"
        );
    }

    #[test]
    fn keys_drops_and_portal_answers_are_not_input_intents() {
        let mut a = adapter();
        let out = run(
            &mut a,
            &[
                ShellEvent::CloseRequested,
                ShellEvent::Focused(true),
                ShellEvent::Drag(crate::input::event::DragEvent::Left),
            ],
        );
        assert!(out.is_empty());
    }

    #[test]
    fn scripted_sequences_drive_a_real_session() {
        // End to end over the app's own contract: the intents produced here
        // must move a real viewport the way the user expects.
        let mut session = xarast_app::Session::new_empty(xarast_app::DocumentId(1));
        let mut a = IntentAdapter::new();
        let mut out = Vec::new();
        a.prime(
            CanvasRegion::new(0, 0, 800, 600),
            ScaleFactor::new(1.0),
            &mut out,
        );
        out.extend(run(
            &mut a,
            &[
                ShellEvent::ModifiersChanged(Modifiers::NONE.with_ctrl()),
                ptr(
                    PointerPhase::Scroll {
                        dx: 0.0,
                        dy: 2.0,
                        unit: ScrollUnit::Lines,
                    },
                    400.0,
                    300.0,
                ),
            ],
        ));
        let before = session.viewport.zoom();
        let mut changed = xarast_app::Changed::empty();
        for i in out {
            changed |= session.apply(i).unwrap();
        }
        assert!(changed.needs_redraw());
        let after = session.viewport.zoom();
        assert!(after > before, "{before} -> {after}");
        assert!(session.edit.modifiers.constrain);
    }
}
