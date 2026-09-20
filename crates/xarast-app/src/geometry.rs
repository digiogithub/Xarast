//! The two coordinate spaces the application lives between, and the
//! newtypes that keep them apart.
//!
//! * **Document space** is integer millipoints, exactly as `xarast-doc`
//!   and both file formats hold it (architecture §3.4). Its `y` axis
//!   points **up**: a page's `lo` corner is its bottom-left.
//! * **Device space** is `f64` pixels with `y` pointing **down**, which is
//!   what every surface, every window and `xarast_render::DeviceRect`
//!   expect.
//!
//! The flip between them belongs to the [`Viewport`](crate::Viewport) and
//! nowhere else. Neither `xarast-doc` nor `xarast-render` has an opinion
//! about which way up a document is, and giving a second component one is
//! how drawings end up mirrored.

use xarast_geom::{Mp, Point, Rect};

/// A rectangle in document space, in millipoints.
pub type DocRect = Rect;

/// A point in document space, in integer millipoints.
pub type DocPoint = Point;

/// A continuous point in document space, in millipoints.
///
/// Used where a device coordinate has been mapped back but not yet
/// quantised — a drag in progress, a gradient handle under the pointer.
/// Quantise with [`DocPointF::to_doc_point`] at the moment the value
/// enters a command, never before.
pub type DocPointF = kurbo::Point;

/// A point in device space: `f64` pixels, `y` down, origin at the
/// top-left of the viewport.
#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub struct DevicePoint {
    /// Pixels right of the viewport's left edge.
    pub x: f64,
    /// Pixels below the viewport's top edge.
    pub y: f64,
}

impl DevicePoint {
    /// A point.
    #[must_use]
    pub const fn new(x: f64, y: f64) -> DevicePoint {
        DevicePoint { x, y }
    }

    /// The origin.
    pub const ZERO: DevicePoint = DevicePoint { x: 0.0, y: 0.0 };

    /// The same point as a `kurbo` point, for handing to the renderer.
    #[must_use]
    pub const fn to_kurbo(self) -> kurbo::Point {
        kurbo::Point::new(self.x, self.y)
    }

    /// Whether both coordinates are finite.
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl From<kurbo::Point> for DevicePoint {
    fn from(p: kurbo::Point) -> DevicePoint {
        DevicePoint { x: p.x, y: p.y }
    }
}

impl From<DevicePoint> for kurbo::Point {
    fn from(p: DevicePoint) -> kurbo::Point {
        kurbo::Point::new(p.x, p.y)
    }
}

/// The size of a viewport in whole device pixels.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct DeviceSize {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl DeviceSize {
    /// A size.
    #[must_use]
    pub const fn new(width: u32, height: u32) -> DeviceSize {
        DeviceSize { width, height }
    }

    /// Whether either dimension is zero, which is what a minimised window
    /// reports and what every divisor in [`Viewport`](crate::Viewport) has
    /// to survive.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// The whole surface as a device rectangle.
    #[must_use]
    pub fn to_rect(self) -> xarast_render::DeviceRect {
        xarast_render::DeviceRect::from_size(self.width, self.height)
    }
}

/// Quantisation helpers for a continuous document point.
pub trait DocPointF64Ext {
    /// Rounds to the nearest millipoint, saturating into
    /// [`Mp::MIN`]`..=`[`Mp::MAX`] rather than wrapping.
    fn to_doc_point(self) -> DocPoint;
}

impl DocPointF64Ext for DocPointF {
    fn to_doc_point(self) -> DocPoint {
        DocPoint::new(mp_from_f64(self.x), mp_from_f64(self.y))
    }
}

/// Rounds a continuous millipoint value into an [`Mp`], saturating.
///
/// `Mp::from_f64_round` is documented to be the rounding rule; this adds
/// the saturation a viewport needs, because a wild zoom can map the corner
/// of a window far outside the representable document.
#[must_use]
pub fn mp_from_f64(v: f64) -> Mp {
    if v.is_nan() {
        return Mp::ZERO;
    }
    let lo = f64::from(Mp::MIN.raw());
    let hi = f64::from(Mp::MAX.raw());
    Mp::new(v.round().clamp(lo, hi) as i32)
}

/// The document rectangle enclosing four continuous corners, saturating
/// into the millipoint range.
#[must_use]
pub fn doc_rect_enclosing(corners: [DocPointF; 4]) -> DocRect {
    let mut r = DocRect::from_point(corners[0].to_doc_point());
    for c in &corners[1..] {
        r = r.union_point(c.to_doc_point());
    }
    r
}
