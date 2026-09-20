//! The precision layer: `f64` transform algebra, the rounding rule, and the
//! newtype that makes the tile-local `f32` conversion the only one there is.
//!
//! # The golden rule
//!
//! *Never put absolute millipoints into an `f32`* (`research/03 §3.7`). A
//! document 5 m wide is 3.6 × 10⁸ millipoints; `f32` resolves that to about
//! 43 mp ≈ 0.04 pt, which is visible once you zoom in. Every conversion to
//! `f32` in this crate happens **after** subtracting the tile origin, and
//! [`TileLocal`] exists so that the compiler can say where.
//!
//! # The rounding rule
//!
//! CDraw truncates on device conversion and the application compensates by
//! adding half a pixel (`grndrgn.cpp:5290-5300`). Xarast does **not** inherit
//! the compensation: it rounds **half away from zero**, with no added offset.
//! This is a deliberate, documented one-pixel difference from the original;
//! see [`round_device`].

use kurbo::Affine;
use xarast_geom::{Matrix, Point};

/// A point in continuous space, in `f64`. Document space uses millipoints,
/// device space uses pixels; which one a value is in is carried by the
/// transform that produced it, not by the type.
pub type Point64 = kurbo::Point;

/// A document-to-device transform, in `f64` throughout.
///
/// `f64` has 52 bits of mantissa, which covers the 31-bit millipoint range
/// with room for scale and rotation. It is what the original does too, with
/// the `double DX` in its edge records.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform2D(Affine);

impl Default for Transform2D {
    fn default() -> Transform2D {
        Transform2D::IDENTITY
    }
}

impl Transform2D {
    /// The identity transform.
    pub const IDENTITY: Transform2D = Transform2D(Affine::IDENTITY);

    /// Builds a transform from the six affine coefficients
    /// `[a, b, c, d, e, f]`, as `kurbo::Affine` orders them.
    #[must_use]
    pub const fn new(coeffs: [f64; 6]) -> Transform2D {
        Transform2D(Affine::new(coeffs))
    }

    /// A uniform scale about the origin.
    #[must_use]
    pub fn scale(s: f64) -> Transform2D {
        Transform2D(Affine::scale(s))
    }

    /// A non-uniform scale about the origin.
    #[must_use]
    pub fn scale_non_uniform(sx: f64, sy: f64) -> Transform2D {
        Transform2D(Affine::scale_non_uniform(sx, sy))
    }

    /// A translation.
    #[must_use]
    pub fn translate(dx: f64, dy: f64) -> Transform2D {
        Transform2D(Affine::translate((dx, dy)))
    }

    /// A rotation about the origin, in radians.
    #[must_use]
    pub fn rotate(radians: f64) -> Transform2D {
        Transform2D(Affine::rotate(radians))
    }

    /// Adopts a document-space [`Matrix`] from `xarast-geom`.
    ///
    /// The renderer does not define a second matrix type; this is the one
    /// bridge, and it is lossless because `Matrix` is already `f64`.
    #[must_use]
    pub fn from_document(m: Matrix) -> Transform2D {
        Transform2D(m.to_affine())
    }

    /// The `kurbo` affine underneath, for handing geometry to the rasteriser.
    #[must_use]
    pub const fn to_affine(self) -> Affine {
        self.0
    }

    /// `self` followed by `other`.
    #[must_use]
    pub fn then(self, other: Transform2D) -> Transform2D {
        Transform2D(other.0 * self.0)
    }

    /// The inverse, or `None` when the transform is singular.
    #[must_use]
    pub fn invert(self) -> Option<Transform2D> {
        let d = self.0.determinant();
        if d.abs() < 1e-12 || !d.is_finite() {
            None
        } else {
            Some(Transform2D(self.0.inverse()))
        }
    }

    /// Transforms a point.
    #[must_use]
    pub fn apply(self, p: Point64) -> Point64 {
        self.0 * p
    }

    /// Transforms a document-space [`Point`] (integer millipoints) into
    /// continuous device space.
    #[must_use]
    pub fn apply_document(self, p: Point) -> Point64 {
        let (x, y) = p.to_f64();
        self.0 * Point64::new(x, y)
    }

    /// The larger of the two singular values: how many device units one
    /// document unit can stretch to. Flatness tolerance is derived from it.
    #[must_use]
    pub fn max_scale(self) -> f64 {
        let c = self.0.as_coeffs();
        let a = (c[0] * c[0] + c[1] * c[1]).sqrt();
        let b = (c[2] * c[2] + c[3] * c[3]).sqrt();
        a.max(b)
    }

    /// Whether this is a translation with no linear part, which lets pan
    /// reuse rasterised pixels.
    #[must_use]
    pub fn is_translation_only(self) -> bool {
        let c = self.0.as_coeffs();
        (c[0] - 1.0).abs() < 1e-12
            && c[1].abs() < 1e-12
            && c[2].abs() < 1e-12
            && (c[3] - 1.0).abs() < 1e-12
    }
}

/// Rounds a device coordinate to a whole pixel, **half away from zero**.
///
/// This is Xarast's rounding rule, stated once here and used everywhere. It
/// differs from CDraw, which truncates and lets the application add half a
/// pixel; the golden-image methodology has to know that, or the comparison
/// chases a phantom one-pixel offset.
#[must_use]
pub fn round_device(v: f64) -> i32 {
    if !v.is_finite() {
        return 0;
    }
    let r = if v >= 0.0 {
        (v + 0.5).floor()
    } else {
        (v - 0.5).ceil()
    };
    r.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

/// A coordinate relative to a tile origin, in `f32`.
///
/// The only way to build one is [`Tile::localise`], which subtracts the tile
/// origin first. That is the whole point: within a tile the magnitude is at
/// most a few thousand, so `f32` gives sub-micropixel precision, while an
/// absolute document coordinate in `f32` would not.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct TileLocal(f32);

impl TileLocal {
    /// The underlying `f32`, for handing to the rasteriser.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

/// A rasterisation tile: an origin in device pixels and a square size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tile {
    /// Device x of the tile's left edge.
    pub x: i32,
    /// Device y of the tile's top edge.
    pub y: i32,
    /// Edge length in pixels.
    pub size: u32,
}

impl Tile {
    /// Builds a tile.
    #[must_use]
    pub const fn new(x: i32, y: i32, size: u32) -> Tile {
        Tile { x, y, size }
    }

    /// Converts an absolute device coordinate pair into tile-local `f32`.
    ///
    /// This is the crate's only `f64 → f32` conversion of a coordinate.
    #[must_use]
    pub fn localise(self, p: Point64) -> (TileLocal, TileLocal) {
        (
            TileLocal((p.x - f64::from(self.x)) as f32),
            TileLocal((p.y - f64::from(self.y)) as f32),
        )
    }

    /// The transform that takes absolute device space into this tile's local
    /// space, to be applied before any `f32` conversion happens downstream.
    #[must_use]
    pub fn to_local_transform(self) -> Transform2D {
        Transform2D::translate(-f64::from(self.x), -f64::from(self.y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_half_away_from_zero() {
        assert_eq!(round_device(0.5), 1);
        assert_eq!(round_device(-0.5), -1);
        assert_eq!(round_device(1.5), 2);
        assert_eq!(round_device(-1.5), -2);
        assert_eq!(round_device(0.49), 0);
        assert_eq!(round_device(-0.49), 0);
        assert_eq!(round_device(f64::NAN), 0);
    }

    #[test]
    fn tile_local_keeps_precision_on_a_five_metre_document() {
        // 5 m is 3.6e8 millipoints. At 4000 % zoom (40 device pixels per
        // point) the device coordinate is about 1.9e7, where an `f32` ulp
        // is two whole pixels.
        let xf = Transform2D::scale(40.0 / 750.0);
        let doc = Point64::new(360_000_021.0, 0.0);
        let dev = xf.apply(doc);

        let tile = Tile::new(round_device(dev.x) - 7, 0, 256);
        let (lx, _) = tile.localise(dev);
        let exact = dev.x - f64::from(tile.x);
        let tile_error = (f64::from(lx.get()) - exact).abs();
        assert!(tile_error < 1e-4, "tile-local error {tile_error} px");

        // The same coordinate converted absolutely loses most of a pixel,
        // which is exactly the rule's reason for existing.
        let naive_error = (f64::from(dev.x as f32) - dev.x).abs();
        assert!(
            naive_error > 0.25,
            "the absolute f32 conversion was supposed to be bad, error {naive_error} px"
        );
        assert!(tile_error * 1000.0 < naive_error);
    }

    #[test]
    fn transforms_compose_in_application_order() {
        let a = Transform2D::translate(10.0, 0.0);
        let b = Transform2D::scale(2.0);
        let p = a.then(b).apply(Point64::new(1.0, 1.0));
        assert_eq!(p, Point64::new(22.0, 2.0));
    }
}
