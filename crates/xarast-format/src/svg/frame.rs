//! Document space → SVG user space (`research/06 §5.5`).
//!
//! The model is Y-up millipoints in document coordinates. A spread's SVG
//! space is Y-down points with its origin at the **top-left corner of the
//! spread's pages**. The conversion is a translation and a Y flip, done
//! once, here, in integers: no rounding, and exactly invertible by a reader
//! that knows the spread's origin (`xarast:origin`).

use xarast_geom::{Matrix, Point, Vector};

/// The mapping for one spread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    /// Document x of the SVG origin.
    pub ox: i64,
    /// Document y of the SVG origin (the top of the pages).
    pub oy: i64,
}

impl Frame {
    /// A point, in SVG millipoints.
    #[inline]
    #[must_use]
    pub fn pt(self, p: Point) -> (i64, i64) {
        (
            i64::from(p.x.raw()) - self.ox,
            self.oy - i64::from(p.y.raw()),
        )
    }

    /// A displacement, in SVG millipoints.
    #[inline]
    #[must_use]
    pub fn vec(self, v: Vector) -> (i64, i64) {
        (i64::from(v.dx.raw()), -i64::from(v.dy.raw()))
    }

    /// The SVG `matrix(a b c d e f)` of a model transform `m`, conjugated by
    /// the flip: what places content authored in a Y-down local space
    /// where `m` would place it in Y-up document space.
    ///
    /// With `F` the flip of this frame and `S` = `scale(1, -1)`, the result
    /// is `F · m · S`: `a` and `d` keep their sign, the shears change it, and
    /// the translation is the frame's image of `m`'s.
    #[must_use]
    pub fn local_matrix(self, m: &Matrix) -> [f64; 6] {
        let (e, f) = self.pt(Point::new(m.e, m.f));
        [m.a, -m.b, -m.c, m.d, e as f64, f as f64]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flip_is_exact_and_puts_the_top_of_the_pages_at_zero() {
        let f = Frame {
            ox: 36_000,
            oy: 877_890,
        };
        assert_eq!(f.pt(Point::raw(36_000, 877_890)), (0, 0));
        assert_eq!(f.pt(Point::raw(631_276, 36_000)), (595_276, 841_890));
        assert_eq!(f.vec(Vector::raw(10, 20)), (10, -20));
    }
}
