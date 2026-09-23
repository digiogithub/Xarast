//! The platform-neutral event the rest of the application consumes.
//!
//! Nothing above `xarast-shell` names `winit`. This is the contract that
//! makes that true, and it is sized so that phase 14 replaces only the
//! translation into it: the Win32 and AppKit shells produce the same
//! [`ShellEvent`] stream and every tool, panel and command keeps working.

use std::path::PathBuf;

use super::keyboard::{KeyEvent, Modifiers};
use super::tablet::{InputSource, StrokeSample};
use crate::ime::ImeEvent;
use crate::scale::{PhysicalPos, PhysicalSize, ScaleFactor};

/// A pointing device, distinguished so that two styluses, or a finger and a
/// pen, do not get merged into one stroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PointerId(pub u64);

/// A pointer button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerButton {
    /// Primary, usually left.
    Primary,
    /// Secondary, usually right.
    Secondary,
    /// Middle, usually the wheel.
    Middle,
    /// Back.
    Back,
    /// Forward.
    Forward,
    /// The stylus tip touching the surface.
    ToolTip,
    /// A barrel button on a stylus.
    ToolBarrel,
    /// Anything else, by index.
    Other(u16),
}

/// The unit a scroll delta is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollUnit {
    /// Notches of a stepped wheel. Multiply by a line height to use.
    Lines,
    /// Physical device pixels, from a trackpad or a smooth wheel.
    Pixels,
}

/// What a pointer did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointerPhase {
    /// Entered the window.
    Entered,
    /// Moved.
    Moved,
    /// A button went down.
    Pressed(PointerButton),
    /// A button came up.
    Released(PointerButton),
    /// Scrolled.
    Scroll {
        /// Horizontal delta.
        dx: f64,
        /// Vertical delta.
        dy: f64,
        /// What the deltas are measured in.
        unit: ScrollUnit,
    },
    /// Left the window.
    Left,
}

/// One pointer event, in physical device pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerEvent {
    /// Which device.
    pub id: PointerId,
    /// What it did.
    pub phase: PointerPhase,
    /// Where it was. Device pixels, `f64` because a fractional scale makes
    /// sub-pixel positions real.
    pub position: PhysicalPos,
    /// What kind of device it is.
    pub source: InputSource,
    /// True for the device that drives the cursor. A secondary touch point
    /// is not primary.
    pub primary: bool,
    /// The modifiers held at the time.
    pub modifiers: Modifiers,
}

/// A trackpad gesture.
///
/// Delivered on Wayland only; X11 has no protocol for them. The viewport
/// treats them as pan and zoom, so their absence degrades to scroll wheel
/// rather than to nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GestureEvent {
    /// Pinch to zoom. `delta` is a relative scale change per event.
    Pinch {
        /// Relative scale change.
        delta: f64,
        /// Where the gesture is centred.
        at: PhysicalPos,
    },
    /// Two-finger pan.
    Pan {
        /// Horizontal movement in device pixels.
        dx: f64,
        /// Vertical movement in device pixels.
        dy: f64,
    },
    /// Two-finger rotation, in radians.
    Rotate {
        /// Rotation delta in radians.
        delta: f64,
    },
    /// A double tap on the trackpad.
    DoubleTap,
}

/// A drag-and-drop event.
///
/// On Wayland the shell's own `wl_data_device` fills every field: the
/// position in device pixels and all the files of a drop in one event. On
/// X11 the pinned `winit` 0.30 reports one event per file with no position,
/// so `at` stays `None` there and a drop is treated as a drop on the canvas
/// centre; see `docs/memory/ui.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DragEvent {
    /// A drag entered the window.
    Entered {
        /// The files being dragged, as far as the platform will say.
        paths: Vec<PathBuf>,
        /// Where, if the platform says.
        at: Option<PhysicalPos2>,
    },
    /// The drag moved within the window.
    Moved {
        /// Where.
        at: PhysicalPos2,
    },
    /// The drag was dropped.
    Dropped {
        /// The files dropped.
        paths: Vec<PathBuf>,
        /// Where, if the platform says.
        at: Option<PhysicalPos2>,
    },
    /// The drag left the window or was cancelled.
    Left,
}

/// An integer device-pixel position.
///
/// Drag events are compared for equality in tests and stored in structures
/// that want `Eq`, which a `f64` position cannot give. Drop targets are
/// whole-pixel decisions anyway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhysicalPos2 {
    /// Horizontal position in device pixels.
    pub x: i32,
    /// Vertical position in device pixels.
    pub y: i32,
}

impl PhysicalPos2 {
    /// A position.
    #[must_use]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// The colour scheme the desktop asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorScheme {
    /// The desktop has no preference.
    #[default]
    NoPreference,
    /// Prefer dark.
    Dark,
    /// Prefer light.
    Light,
}

/// Everything the platform tells the application.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ShellEvent {
    /// The surface was resized. Carries the scale so that size and scale can
    /// never be observed out of step — the classic fractional-scaling bug.
    Resized {
        /// The new surface size in device pixels.
        physical: PhysicalSize,
        /// The scale in force for that size.
        scale: ScaleFactor,
    },
    /// The scale factor changed. A resize always follows.
    ScaleChanged(ScaleFactor),
    /// The window gained or lost keyboard focus.
    Focused(bool),
    /// The desktop colour scheme changed.
    ColorSchemeChanged(ColorScheme),
    /// A pointer did something.
    Pointer(PointerEvent),
    /// One stroke sample. Emitted for every sample of the frame, not just
    /// the last.
    Stroke(StrokeSample),
    /// A key went down or up.
    Key(KeyEvent),
    /// The modifiers changed, with or without an accompanying key event.
    ModifiersChanged(Modifiers),
    /// An input-method event.
    Ime(ImeEvent),
    /// A trackpad gesture.
    Gesture(GestureEvent),
    /// A drag-and-drop event.
    Drag(DragEvent),
    /// An answer from a portal request.
    Portal(crate::portal::PortalEvent),
    /// The GPU raised errors during the last frame. The shell has already
    /// logged them and is recovering; this is for the status bar.
    GpuError(crate::gpu_errors::GpuErrorReport),
    /// An assistive technology (a screen reader, say) started listening.
    /// From now on the application should publish its accessibility tree
    /// with [`crate::ShellCtx::update_accessibility`], the first time in
    /// full and no later than the next frame.
    AccessibilityActivated,
    /// The assistive technology stopped listening; publishing may stop.
    AccessibilityDeactivated,
    /// An assistive technology asked for an action: focus this node, press
    /// that button, set this value.
    AccessibilityAction(egui::accesskit::ActionRequest),
    /// The user asked to close the window.
    CloseRequested,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resize_carries_its_own_scale() {
        // Size and scale arrive together so that no consumer can pair a new
        // size with a stale factor.
        let ev = ShellEvent::Resized {
            physical: PhysicalSize::new(1600, 1000),
            scale: ScaleFactor::new(1.25),
        };
        let ShellEvent::Resized { physical, scale } = ev else {
            unreachable!()
        };
        assert_eq!(physical.width, 1600);
        assert!((scale.get() - 1.25).abs() < 1e-12);
    }

    #[test]
    fn drag_positions_are_optional_because_winit_030_has_none() {
        let ev = DragEvent::Dropped {
            paths: vec![PathBuf::from("/tmp/a.xar")],
            at: None,
        };
        match ev {
            DragEvent::Dropped { paths, at } => {
                assert_eq!(paths.len(), 1);
                assert!(at.is_none());
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn the_default_colour_scheme_is_no_preference() {
        assert_eq!(ColorScheme::default(), ColorScheme::NoPreference);
    }
}
