//! Fractional scaling, in one place.
//!
//! The classic high-DPI bug is three components each computing their own
//! scale: the window reports a fractional factor, egui wants
//! `pixels_per_point`, and the canvas wants physical device pixels. The rule
//! for the whole application is that the **shell** computes one scale per
//! frame and hands it down; this type is how the user interface receives it.
//! Off-by-half-a-pixel rulers are the symptom that the rule was broken.
//!
//! Hairlines are snapped to the *centre* of a device pixel (a half-integer
//! device coordinate), because a one-device-pixel line centred on an integer
//! coordinate spreads its coverage over two pixels and renders as a two-pixel
//! grey smear. Filled rectangles are snapped to integer device pixels
//! instead, where the opposite is true.

use std::fmt;

/// The one scale factor of a frame: logical points to physical device pixels.
///
/// `1.0` is a classic display, `2.0` a doubled one, and Wayland's
/// `wp_fractional_scale_v1` adds `1.25`, `1.5` and anything else in 1/120
/// steps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale {
    ppp: f64,
}

impl Default for Scale {
    fn default() -> Self {
        Scale::new(1.0)
    }
}

impl fmt::Display for Scale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.4}×", self.ppp)
    }
}

impl Scale {
    /// Builds a scale, clamping to a sane range.
    ///
    /// A compositor that reports a non-finite or absurd factor must not be
    /// able to make the user interface disappear, so the value is clamped to
    /// `0.5..=8.0` rather than trusted.
    pub fn new(pixels_per_point: f64) -> Scale {
        let ppp = if pixels_per_point.is_finite() {
            pixels_per_point.clamp(0.5, 8.0)
        } else {
            1.0
        };
        Scale { ppp }
    }

    /// The factor, as egui's `pixels_per_point`.
    pub fn pixels_per_point(self) -> f64 {
        self.ppp
    }

    /// The factor as the `f32` egui itself wants.
    pub fn ppp_f32(self) -> f32 {
        self.ppp as f32
    }

    /// Converts a length in logical points to physical device pixels.
    pub fn to_device(self, points: f64) -> f64 {
        points * self.ppp
    }

    /// Converts a length in physical device pixels to logical points.
    pub fn to_points(self, device: f64) -> f64 {
        device / self.ppp
    }

    /// Snaps a logical coordinate so that it lands on a device-pixel
    /// boundary, for rectangles and filled areas.
    pub fn snap_edge(self, points: f64) -> f64 {
        (points * self.ppp).round() / self.ppp
    }

    /// Snaps a logical coordinate to the centre of a device pixel, for
    /// one-pixel hairlines: rulers, grid lines, guides and the page edge.
    ///
    /// The result always sits on a half-integer *device* coordinate, at every
    /// scale factor, which is what keeps a hairline one crisp pixel wide
    /// instead of a two-pixel smear.
    pub fn snap_hairline(self, points: f64) -> f64 {
        ((points * self.ppp).floor() + 0.5) / self.ppp
    }

    /// The logical width that renders as exactly one device pixel.
    pub fn hairline_width(self) -> f64 {
        1.0 / self.ppp
    }

    /// Rounds a logical rectangle outwards to whole device pixels.
    ///
    /// The canvas region is reported to the shell in device pixels, and
    /// rounding outwards guarantees the rendered document covers every pixel
    /// egui left transparent — rounding inwards would leave a seam.
    pub fn device_rect(self, rect: egui::Rect) -> DeviceRect {
        let x0 = (rect.min.x as f64 * self.ppp).floor();
        let y0 = (rect.min.y as f64 * self.ppp).floor();
        let x1 = (rect.max.x as f64 * self.ppp).ceil();
        let y1 = (rect.max.y as f64 * self.ppp).ceil();
        DeviceRect {
            x: x0 as i32,
            y: y0 as i32,
            width: (x1 - x0).max(0.0) as u32,
            height: (y1 - y0).max(0.0) as u32,
        }
    }
}

/// A rectangle in whole physical device pixels.
///
/// This is what the canvas hands the shell: the scissor rectangle the
/// document is rendered into. `xarast_render::DeviceRect` is the same idea
/// one crate down; the conversion is a field copy and lives at the shell
/// boundary so that the user interface does not depend on the renderer's
/// surface types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeviceRect {
    /// Left edge, in device pixels from the surface origin.
    pub x: i32,
    /// Top edge, in device pixels from the surface origin.
    pub y: i32,
    /// Width in device pixels.
    pub width: u32,
    /// Height in device pixels.
    pub height: u32,
}

impl DeviceRect {
    /// True when the rectangle has no area, in which case the canvas pass is
    /// skipped entirely rather than submitted empty.
    pub fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCALES: [f64; 5] = [1.0, 1.25, 1.5, 2.0, 2.75];

    #[test]
    fn hairlines_land_on_half_device_pixels_at_every_scale() {
        for s in SCALES {
            let scale = Scale::new(s);
            for i in 0..200 {
                let raw = i as f64 * 0.37;
                let snapped = scale.snap_hairline(raw);
                let device = snapped * s;
                let frac = device - device.floor();
                assert!(
                    (frac - 0.5).abs() < 1e-9,
                    "scale {s}: {raw} snapped to {snapped} = {device} device px",
                );
            }
        }
    }

    #[test]
    fn hairline_snap_never_moves_more_than_one_device_pixel() {
        for s in SCALES {
            let scale = Scale::new(s);
            for i in 0..200 {
                let raw = i as f64 * 0.37;
                let moved = (scale.snap_hairline(raw) - raw).abs() * s;
                assert!(moved <= 1.0 + 1e-9, "scale {s}: moved {moved} device px");
            }
        }
    }

    #[test]
    fn edges_land_on_whole_device_pixels() {
        for s in SCALES {
            let scale = Scale::new(s);
            for i in 0..100 {
                let device = scale.snap_edge(i as f64 * 0.41) * s;
                assert!((device - device.round()).abs() < 1e-9, "scale {s}");
            }
        }
    }

    #[test]
    fn point_and_device_conversions_round_trip() {
        for s in SCALES {
            let scale = Scale::new(s);
            let v = 123.456;
            assert!((scale.to_points(scale.to_device(v)) - v).abs() < 1e-12);
        }
    }

    #[test]
    fn device_rect_covers_the_logical_rect_at_fractional_scale() {
        let scale = Scale::new(1.5);
        let r = egui::Rect::from_min_max(egui::pos2(10.3, 20.7), egui::pos2(100.4, 200.1));
        let d = scale.device_rect(r);
        assert!(d.x as f64 <= r.min.x as f64 * 1.5);
        assert!((d.x + d.width as i32) as f64 >= r.max.x as f64 * 1.5);
        assert!(d.y as f64 <= r.min.y as f64 * 1.5);
        assert!((d.y + d.height as i32) as f64 >= r.max.y as f64 * 1.5);
        assert!(!d.is_empty());
    }

    #[test]
    fn absurd_scale_factors_are_clamped_not_trusted() {
        assert_eq!(Scale::new(f64::NAN).pixels_per_point(), 1.0);
        assert_eq!(Scale::new(0.0).pixels_per_point(), 0.5);
        assert_eq!(Scale::new(1000.0).pixels_per_point(), 8.0);
    }

    #[test]
    fn one_device_pixel_is_one_device_pixel() {
        for s in SCALES {
            let scale = Scale::new(s);
            assert!((scale.to_device(scale.hairline_width()) - 1.0).abs() < 1e-12);
        }
    }
}
