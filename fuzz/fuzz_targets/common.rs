//! Structured-input helpers shared by the geometry, render and document
//! targets. Included with `mod common;`; each target uses a subset.

#![allow(dead_code)]

use arbitrary::Arbitrary;
use xarast_geom::{Mp, Path, PathBuilder, Point, Rect};

/// The most path operations one generated path may have. Enough for
/// self-intersecting, multi-subpath shapes; small enough that a case stays
/// in the millisecond range.
pub const MAX_OPS: usize = 48;

/// A coordinate, with a selector for its scale.
///
/// Uniformly random `i32`s almost never coincide, and coincidence —
/// shared vertices, collinear edges, zero-length segments — is exactly
/// where boolean engines break. So most coordinates are snapped to a
/// coarse grid, some are merely bounded, and some span the whole extent.
#[derive(Arbitrary, Debug, Clone, Copy)]
pub struct Coord(pub u8, pub i32);

impl Coord {
    pub fn mp(self) -> Mp {
        let v = self.1;
        let raw = match self.0 % 4 {
            // A 16 x 16 grid of whole points: maximal coincidence.
            0 | 1 => v.rem_euclid(16) * 1_000,
            // A page-sized region.
            2 => v.rem_euclid(2_000_000) - 1_000_000,
            // Anywhere in the document extent, edges included.
            _ => v.clamp(Mp::EXTENT_MIN.raw(), Mp::EXTENT_MAX.raw()),
        };
        Mp(raw)
    }
}

#[derive(Arbitrary, Debug, Clone, Copy)]
pub struct Pt(pub Coord, pub Coord);

impl Pt {
    pub fn point(self) -> Point {
        Point::new(self.0.mp(), self.1.mp())
    }

    /// The point, clamped to `-limit..=limit` on both axes.
    pub fn point_within(self, limit: i32) -> Point {
        let c = |m: Mp| Mp(m.raw().clamp(-limit, limit));
        Point::new(c(self.0.mp()), c(self.1.mp()))
    }
}

/// One step of building a path through the public builder.
#[derive(Arbitrary, Debug, Clone)]
pub enum PathOp {
    Move(Pt),
    Line(Pt),
    Cubic(Pt, Pt, Pt),
    Quad(Pt, Pt),
    Close,
    Rect(Pt, Pt),
    Ellipse(Pt, Coord, Coord),
}

/// Builds a path from at most [`MAX_OPS`] steps, anywhere in the extent.
pub fn build_path(ops: &[PathOp]) -> Path {
    build_path_within(ops, Mp::EXTENT_MAX.raw())
}

/// Builds a path whose coordinates all lie in `-limit..=limit`.
pub fn build_path_within(ops: &[PathOp], limit: i32) -> Path {
    let mut b = PathBuilder::new();
    let q = |p: &Pt| p.point_within(limit);
    let r = |c: &Coord| Mp(c.mp().raw().clamp(-limit, limit));
    for op in ops.iter().take(MAX_OPS) {
        match op {
            PathOp::Move(p) => {
                b.move_to(q(p));
            }
            PathOp::Line(p) => {
                b.line_to(q(p));
            }
            PathOp::Cubic(c1, c2, p) => {
                b.cubic_to(q(c1), q(c2), q(p));
            }
            PathOp::Quad(c, p) => {
                b.quad_to(q(c), q(p));
            }
            PathOp::Close => {
                b.close();
            }
            PathOp::Rect(a, c) => {
                b.rect(Rect::new(q(a), q(c)));
            }
            PathOp::Ellipse(c, rx, ry) => {
                b.ellipse(q(c), r(rx), r(ry));
            }
        }
    }
    b.build()
}

/// A finite `f64`: a NaN or an infinity becomes zero. Used where the real
/// producer of the value (a document transform, a gradient handle) can
/// only ever hand over finite numbers.
pub fn finite(v: f64) -> f64 {
    if v.is_finite() { v } else { 0.0 }
}
