//! The one owner of the scale factor.
//!
//! Three components want a scale: the compositor reports a fractional factor,
//! `egui` wants `pixels_per_point`, and the canvas wants physical device
//! pixels. When each computes its own, rulers land half a pixel off the page
//! corner and hairlines go grey. So the shell computes exactly one
//! [`ScaleFactor`] per frame and every consumer is handed that value; nothing
//! below the shell reads the window.

/// A window size in physical device pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhysicalSize {
    /// Width in device pixels.
    pub width: u32,
    /// Height in device pixels.
    pub height: u32,
}

impl PhysicalSize {
    /// A size, with both axes forced to at least one pixel.
    ///
    /// A zero-sized surface is not configurable, and a minimised window
    /// reports one, so every size that reaches `wgpu` goes through here.
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self {
            width: if width == 0 { 1 } else { width },
            height: if height == 0 { 1 } else { height },
        }
    }

    /// True when the compositor reported an unusable (zero-area) surface.
    #[must_use]
    pub const fn is_degenerate(width: u32, height: u32) -> bool {
        width == 0 || height == 0
    }
}

/// A size in logical (scale-independent) units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogicalSize {
    /// Width in logical units.
    pub width: f64,
    /// Height in logical units.
    pub height: f64,
}

impl LogicalSize {
    /// A logical size.
    #[must_use]
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

/// A position in physical device pixels, kept in `f64` because a fractional
/// scale makes sub-pixel pointer positions meaningful.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysicalPos {
    /// Horizontal position in device pixels.
    pub x: f64,
    /// Vertical position in device pixels.
    pub y: f64,
}

impl PhysicalPos {
    /// A physical position.
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// The compositor's scale factor, fractional on Wayland.
///
/// Construction is validating: a non-finite or non-positive factor is a
/// compositor bug we have to survive, so it collapses to `1.0` rather than
/// poisoning every later division.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct ScaleFactor(f64);

impl ScaleFactor {
    /// The identity scale.
    pub const ONE: Self = Self(1.0);

    /// Smallest factor we accept. Below this the window is unusable and the
    /// value is almost certainly a driver artefact.
    pub const MIN: f64 = 0.25;
    /// Largest factor we accept. 8× covers every shipping display.
    pub const MAX: f64 = 8.0;

    /// Clamps and sanitises a factor reported by the platform.
    #[must_use]
    pub fn new(factor: f64) -> Self {
        if !factor.is_finite() || factor <= 0.0 {
            return Self::ONE;
        }
        Self(factor.clamp(Self::MIN, Self::MAX))
    }

    /// The factor as a number.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }

    /// What `egui` calls `pixels_per_point`.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn pixels_per_point(self) -> f32 {
        self.0 as f32
    }

    /// Converts a logical size into the surface size to configure.
    ///
    /// Rounds rather than truncates: at 1.5× a 801-unit window truncates to
    /// 1201 and rounds to 1202, and the truncated form leaves a one-pixel
    /// unpainted strip on the right edge.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn to_physical(self, size: LogicalSize) -> PhysicalSize {
        let w = (size.width * self.0)
            .round()
            .clamp(1.0, f64::from(u32::MAX));
        let h = (size.height * self.0)
            .round()
            .clamp(1.0, f64::from(u32::MAX));
        PhysicalSize::new(w as u32, h as u32)
    }

    /// Converts a physical size back into logical units.
    #[must_use]
    pub fn to_logical(self, size: PhysicalSize) -> LogicalSize {
        LogicalSize::new(
            f64::from(size.width) / self.0,
            f64::from(size.height) / self.0,
        )
    }

    /// Rounds a device-pixel coordinate onto the pixel grid.
    ///
    /// Apply this *after* the `f64` view transform, never before: rounding in
    /// document space makes the error scale with the zoom.
    #[must_use]
    pub fn round_to_device(self, v: f64) -> f64 {
        v.round()
    }

    /// Snaps a coordinate to the centre of a device pixel.
    ///
    /// A one-pixel hairline drawn on an integer coordinate straddles two
    /// pixels and is rendered as two half-covered grey ones. Rulers, grid
    /// lines and page edges all go through here.
    #[must_use]
    pub fn snap_hairline(self, v: f64) -> f64 {
        v.floor() + 0.5
    }
}

impl Default for ScaleFactor {
    fn default() -> Self {
        Self::ONE
    }
}

impl std::fmt::Display for ScaleFactor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.4}×", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonsense_factors_collapse_to_one() {
        for bad in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
            assert_eq!(ScaleFactor::new(bad), ScaleFactor::ONE, "{bad}");
        }
    }

    #[test]
    fn factors_are_clamped_to_a_sane_range() {
        assert!((ScaleFactor::new(100.0).get() - ScaleFactor::MAX).abs() < 1e-12);
        assert!((ScaleFactor::new(0.01).get() - ScaleFactor::MIN).abs() < 1e-12);
    }

    #[test]
    fn the_four_interesting_scales_round_trip() {
        for factor in [1.0, 1.25, 1.5, 2.0] {
            let s = ScaleFactor::new(factor);
            let logical = LogicalSize::new(1280.0, 800.0);
            let physical = s.to_physical(logical);
            let back = s.to_logical(physical);
            assert!((back.width - logical.width).abs() < 1e-9, "{factor}");
            assert!((back.height - logical.height).abs() < 1e-9, "{factor}");
        }
    }

    #[test]
    fn fractional_scaling_rounds_instead_of_truncating() {
        let s = ScaleFactor::new(1.5);
        // 801 × 1.5 = 1201.5. Truncation leaves an unpainted column.
        assert_eq!(s.to_physical(LogicalSize::new(801.0, 801.0)).width, 1202);
    }

    #[test]
    fn a_surface_is_never_zero_sized() {
        let s = ScaleFactor::new(1.0);
        assert_eq!(s.to_physical(LogicalSize::new(0.0, 0.0)).width, 1);
        assert!(PhysicalSize::is_degenerate(0, 10));
        assert!(!PhysicalSize::is_degenerate(10, 10));
    }

    #[test]
    fn hairlines_land_on_pixel_centres() {
        let s = ScaleFactor::new(1.25);
        for v in [0.0, 0.2, 10.9, -3.4] {
            let snapped = s.snap_hairline(v);
            assert!((snapped.fract().abs() - 0.5).abs() < 1e-12, "{v}");
        }
    }

    #[test]
    fn pixels_per_point_matches_the_factor() {
        assert!((ScaleFactor::new(1.25).pixels_per_point() - 1.25).abs() < 1e-6);
    }
}
