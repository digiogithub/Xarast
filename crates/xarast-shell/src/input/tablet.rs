//! Tablet and stylus input, normalised.
//!
//! Two kinds of source feed the same stream. A *pointer* source is the window
//! system's own event queue, translated in [`crate::input::translate`]; a
//! *device* source is an out-of-band listener that polls its own queue — on
//! X11 or macOS, where the window system does not carry the axes, that is the
//! only way to get them. Both produce [`StrokeSample`], and nothing above the
//! shell can tell which one it came from except by reading
//! [`StrokeSample::source`].
//!
//! Sampling rate is a correctness matter, not an optimisation. A Wacom
//! reports at roughly 200 Hz and the compositor coalesces to the frame rate;
//! a tool that only sees the last sample of each frame draws visibly
//! different strokes at speed. Everything here is built so that *every*
//! sample survives to the application — see [`SampleQueue`].

use std::time::Instant;

/// What produced a sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputSource {
    /// A mouse, a trackpad, or anything else with no pressure.
    Mouse,
    /// A finger on a touchscreen.
    Touch,
    /// A stylus tip.
    Pen,
    /// The other end of the stylus.
    Eraser,
    /// A tablet tool we could not classify.
    Unknown,
}

impl InputSource {
    /// True for tools that can carry pressure and tilt.
    #[must_use]
    pub const fn is_tablet_tool(self) -> bool {
        matches!(self, Self::Pen | Self::Eraser)
    }
}

/// The raw axes a backend read off a device, before normalisation.
///
/// Deliberately not a `winit` type. This is the seam that makes the `winit`
/// version, and later the Win32 and AppKit backends, a detail: a backend
/// fills this in whatever units it has, and [`normalise`] is the single place
/// that decides what those units mean.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ToolAxes {
    /// Pressure, already divided by the device maximum, so nominally `0..=1`.
    pub force: Option<f64>,
    /// Barrel pressure, nominally `-1..=1`.
    pub tangential: Option<f32>,
    /// Rotation of the tool about its own axis, in degrees.
    pub twist: Option<f32>,
    /// Tilt away from vertical along x, in degrees.
    pub tilt_x: Option<f32>,
    /// Tilt away from vertical along y, in degrees.
    pub tilt_y: Option<f32>,
}

/// One normalised sample of a stroke.
///
/// Position is in physical device pixels, because that is the only space in
/// which sub-pixel sampling is meaningful before the view transform is known.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrokeSample {
    /// Horizontal position in device pixels.
    pub x: f64,
    /// Vertical position in device pixels.
    pub y: f64,
    /// Pressure in `0..=1`, or `None` when the device has none.
    pub pressure: Option<f32>,
    /// Tilt along x in degrees, `-90..=90`.
    pub tilt_x: Option<f32>,
    /// Tilt along y in degrees, `-90..=90`.
    pub tilt_y: Option<f32>,
    /// Twist in degrees, `0..360`.
    pub twist: Option<f32>,
    /// Barrel pressure in `-1..=1`.
    pub tangential: Option<f32>,
    /// When the sample was taken. Tools need this to reconstruct velocity
    /// across a frame boundary.
    pub timestamp: Instant,
    /// What produced it.
    pub source: InputSource,
}

impl StrokeSample {
    /// Pressure, or `1.0` for a device that has none.
    ///
    /// A mouse draws at full width: that is the behaviour users expect, and
    /// it keeps every tool from having to spell the fallback itself.
    #[must_use]
    pub fn pressure_or_full(&self) -> f32 {
        self.pressure.unwrap_or(1.0)
    }
}

fn clamp_f32(v: f32, lo: f32, hi: f32) -> Option<f32> {
    if v.is_finite() {
        Some(v.clamp(lo, hi))
    } else {
        None
    }
}

/// Turns raw axes into a [`StrokeSample`].
///
/// Every axis is range-checked and every non-finite value becomes `None`.
/// Drivers do report NaN — an unfiltered NaN pressure propagates into the
/// stroke width and then into the geometry, where it is far harder to trace.
#[must_use]
pub fn normalise(
    x: f64,
    y: f64,
    axes: ToolAxes,
    source: InputSource,
    timestamp: Instant,
) -> StrokeSample {
    StrokeSample {
        x,
        y,
        #[allow(clippy::cast_possible_truncation)]
        pressure: axes
            .force
            .filter(|f| f.is_finite())
            .map(|f| f.clamp(0.0, 1.0) as f32),
        tilt_x: axes.tilt_x.and_then(|v| clamp_f32(v, -90.0, 90.0)),
        tilt_y: axes.tilt_y.and_then(|v| clamp_f32(v, -90.0, 90.0)),
        twist: axes.twist.and_then(|v| {
            if v.is_finite() {
                Some(v.rem_euclid(360.0))
            } else {
                None
            }
        }),
        tangential: axes.tangential.and_then(|v| clamp_f32(v, -1.0, 1.0)),
        timestamp,
        source,
    }
}

/// What a tablet source can actually report.
///
/// Printed at start-up and shown in the status bar. A user whose pressure
/// does not work needs to know whether the application never saw any, which
/// is a different problem from the brush ignoring it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabletCaps {
    /// Where the samples come from, for diagnostics.
    pub source_name: &'static str,
    /// Pressure is reported.
    pub pressure: bool,
    /// Tilt is reported.
    pub tilt: bool,
    /// Twist is reported.
    pub twist: bool,
    /// Barrel pressure is reported.
    pub tangential: bool,
    /// Pad buttons, rings and strips are reported.
    pub pad: bool,
}

impl TabletCaps {
    /// A source with no axes at all.
    #[must_use]
    pub const fn none(source_name: &'static str) -> Self {
        Self {
            source_name,
            pressure: false,
            tilt: false,
            twist: false,
            tangential: false,
            pad: false,
        }
    }
}

/// A source of stroke samples that has to be polled.
///
/// Implemented by backends that own a device queue of their own rather than
/// riding on the window system's events. The shell polls every registered
/// source once per frame, before the application's frame callback.
pub trait TabletSource: std::fmt::Debug + Send {
    /// Appends every sample seen since the last poll. Must not block.
    fn poll(&mut self, out: &mut Vec<StrokeSample>);

    /// What this source can report.
    fn capabilities(&self) -> TabletCaps;
}

/// The source used when nothing better is available.
///
/// It reports no axes and produces nothing on its own; the pointer stream is
/// what actually feeds the queue. Its job is to give the status bar an honest
/// answer — "mouse, no pressure" — rather than leaving the field blank.
#[derive(Debug, Default, Clone, Copy)]
pub struct MouseOnlySource;

impl TabletSource for MouseOnlySource {
    fn poll(&mut self, _out: &mut Vec<StrokeSample>) {}

    fn capabilities(&self) -> TabletCaps {
        TabletCaps::none("mouse (no tablet axes)")
    }
}

/// A source you push samples into, for tests and for replaying a recorded or
/// synthesised stroke.
///
/// This is what the virtual-tablet acceptance test drives: it lets the whole
/// pipeline — normalisation, coalescing, delivery — be exercised with no
/// device and no compositor.
#[derive(Debug, Default)]
pub struct ScriptedSource {
    pending: Vec<StrokeSample>,
    caps: Option<TabletCaps>,
}

impl ScriptedSource {
    /// An empty scripted source claiming the given capabilities.
    #[must_use]
    pub fn new(caps: TabletCaps) -> Self {
        Self {
            pending: Vec::new(),
            caps: Some(caps),
        }
    }

    /// Queues a sample to be returned by the next [`TabletSource::poll`].
    pub fn push(&mut self, sample: StrokeSample) {
        self.pending.push(sample);
    }
}

impl TabletSource for ScriptedSource {
    fn poll(&mut self, out: &mut Vec<StrokeSample>) {
        out.append(&mut self.pending);
    }

    fn capabilities(&self) -> TabletCaps {
        self.caps.unwrap_or(TabletCaps::none("scripted"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axes() -> ToolAxes {
        ToolAxes {
            force: Some(0.5),
            tangential: Some(0.25),
            twist: Some(90.0),
            tilt_x: Some(-30.0),
            tilt_y: Some(30.0),
        }
    }

    #[test]
    fn normalisation_passes_sane_axes_through() {
        let s = normalise(1.0, 2.0, axes(), InputSource::Pen, Instant::now());
        assert!((s.pressure.unwrap() - 0.5).abs() < 1e-6);
        assert!((s.twist.unwrap() - 90.0).abs() < 1e-6);
        assert_eq!(s.source, InputSource::Pen);
    }

    #[test]
    fn out_of_range_axes_are_clamped_not_trusted() {
        let raw = ToolAxes {
            force: Some(1.7),
            tangential: Some(-9.0),
            twist: Some(730.0),
            tilt_x: Some(-400.0),
            tilt_y: Some(400.0),
        };
        let s = normalise(0.0, 0.0, raw, InputSource::Pen, Instant::now());
        assert!((s.pressure.unwrap() - 1.0).abs() < 1e-6);
        assert!((s.tangential.unwrap() + 1.0).abs() < 1e-6);
        assert!((s.twist.unwrap() - 10.0).abs() < 1e-4, "{:?}", s.twist);
        assert!((s.tilt_x.unwrap() + 90.0).abs() < 1e-6);
        assert!((s.tilt_y.unwrap() - 90.0).abs() < 1e-6);
    }

    #[test]
    fn a_negative_twist_wraps_into_the_positive_range() {
        let raw = ToolAxes {
            twist: Some(-90.0),
            ..ToolAxes::default()
        };
        let s = normalise(0.0, 0.0, raw, InputSource::Pen, Instant::now());
        assert!((s.twist.unwrap() - 270.0).abs() < 1e-4);
    }

    #[test]
    fn non_finite_axes_become_absent_rather_than_poisoning_the_stroke() {
        let raw = ToolAxes {
            force: Some(f64::NAN),
            tangential: Some(f32::INFINITY),
            twist: Some(f32::NAN),
            tilt_x: Some(f32::NEG_INFINITY),
            tilt_y: Some(f32::NAN),
        };
        let s = normalise(0.0, 0.0, raw, InputSource::Pen, Instant::now());
        assert_eq!(s.pressure, None);
        assert_eq!(s.tangential, None);
        assert_eq!(s.twist, None);
        assert_eq!(s.tilt_x, None);
        assert_eq!(s.tilt_y, None);
    }

    #[test]
    fn a_device_without_pressure_draws_at_full_width() {
        let s = normalise(
            0.0,
            0.0,
            ToolAxes::default(),
            InputSource::Mouse,
            Instant::now(),
        );
        assert_eq!(s.pressure, None);
        assert!((s.pressure_or_full() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_mouse_source_is_honest_about_having_no_axes() {
        let caps = MouseOnlySource.capabilities();
        assert!(!caps.pressure && !caps.tilt && !caps.twist && !caps.pad);
        assert!(caps.source_name.contains("no tablet axes"));
    }

    #[test]
    fn a_scripted_source_hands_over_every_queued_sample_once() {
        let mut src = ScriptedSource::new(TabletCaps {
            source_name: "scripted",
            pressure: true,
            ..TabletCaps::none("scripted")
        });
        for i in 0..64 {
            src.push(normalise(
                f64::from(i),
                0.0,
                ToolAxes {
                    force: Some(f64::from(i) / 64.0),
                    ..ToolAxes::default()
                },
                InputSource::Pen,
                Instant::now(),
            ));
        }
        let mut out = Vec::new();
        src.poll(&mut out);
        assert_eq!(out.len(), 64);
        src.poll(&mut out);
        assert_eq!(
            out.len(),
            64,
            "a polled sample must not be handed out twice"
        );
        assert!(src.capabilities().pressure);
    }

    #[test]
    fn only_a_pen_or_eraser_counts_as_a_tablet_tool() {
        assert!(InputSource::Pen.is_tablet_tool());
        assert!(InputSource::Eraser.is_tablet_tool());
        assert!(!InputSource::Mouse.is_tablet_tool());
        assert!(!InputSource::Touch.is_tablet_tool());
    }
}
