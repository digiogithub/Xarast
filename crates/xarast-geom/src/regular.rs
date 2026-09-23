//! Outlines of regular shapes: polygons, stars and ellipses held as parameters.
//!
//! The behaviour is specified in `docs/research/01-xar-format.md` §4.7.1. In
//! short, every point is placed in the frame the two axes span, as
//! `r·(cos θ · major − sin θ · minor)` about the centre. Primary points are at
//! `π/n + k·2π/n` and stellation points half a step (plus the offset) further
//! on. Corners may be rounded with one cubic each, and edges may follow a
//! one-cubic template.
//!
//! All arithmetic is in `f64` and is quantised once, when each point is
//! emitted. Hostile parameters (NaN, infinities, negative lengths, absurd side
//! counts) never panic. They produce a degenerate path, or `None`.

use crate::path::{Path, PathBuilder, Verb};
use crate::point::{Point, Vector};

/// The largest side count an outline is generated for.
///
/// A `.xar` record can ask for 65 535 sides. Generating that for every shape
/// of a hostile file would be a memory attack, and no drawing needs it.
pub const MAX_REGULAR_SIDES: u32 = 4096;

/// How far along the line to the corner a rounding control point sits.
const CURVE_FACTOR: f64 = 0.552;

/// The parameters of a regular shape.
#[derive(Clone, Copy, Debug)]
pub struct RegularShapeSpec<'a> {
    /// Number of sides (primary points).
    pub sides: u32,
    /// An ellipse through the ends of both axes, rather than a polygon.
    pub circular: bool,
    /// Whether there is a stellation point between each pair of primary points.
    pub stellated: bool,
    /// Whether the corners at the primary points are rounded.
    pub primary_curved: bool,
    /// Whether the corners at the stellation points are rounded.
    pub stellation_curved: bool,
    /// The centre.
    pub centre: Point,
    /// The major axis, from the centre.
    pub major: Vector,
    /// The minor axis, from the centre.
    pub minor: Vector,
    /// The stellation points' distance from the centre, as a ratio of the
    /// primary points'.
    pub stellation_radius: f64,
    /// The stellation points' angular offset, in units of one side's angle.
    pub stellation_offset: f64,
    /// The primary corners' rounding, as a ratio of the major axis length.
    pub primary_curvature: f64,
    /// The stellation corners' rounding, as a ratio of the major axis length.
    pub stellation_curvature: f64,
    /// The template for edges that leave a primary point. `None`, or anything
    /// that is not a single cubic, means a straight edge.
    pub primary_edge: Option<&'a Path>,
    /// The template for edges that leave a stellation point.
    pub secondary_edge: Option<&'a Path>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct P(f64, f64);

impl P {
    fn of(p: Point) -> P {
        let (x, y) = p.to_f64();
        P(x, y)
    }
    fn vec(v: Vector) -> P {
        P(v.dx.to_f64(), v.dy.to_f64())
    }
    fn add(self, o: P) -> P {
        P(self.0 + o.0, self.1 + o.1)
    }
    fn sub(self, o: P) -> P {
        P(self.0 - o.0, self.1 - o.1)
    }
    fn scale(self, k: f64) -> P {
        P(self.0 * k, self.1 * k)
    }
    fn len(self) -> f64 {
        self.0.hypot(self.1)
    }
    /// The point `k` of the way from `self` to `to`.
    fn toward(self, to: P, k: f64) -> P {
        self.add(to.sub(self).scale(k))
    }
    fn pt(self) -> Point {
        Point::from_f64_round(self.0, self.1)
    }
}

/// Generates the closed outline, in the same space as the centre and axes.
///
/// `None` when there is nothing to draw: no sides, or more than
/// [`MAX_REGULAR_SIDES`].
#[must_use]
pub fn regular_shape_outline(spec: &RegularShapeSpec<'_>) -> Option<Path> {
    let c = P::of(spec.centre);
    let major = P::vec(spec.major);
    let minor = P::vec(spec.minor);
    let mut b = Path::builder();
    if spec.circular {
        ellipse(&mut b, c, major, minor);
        return Some(b.build());
    }
    let n = spec.sides;
    if n == 0 || n > MAX_REGULAR_SIDES {
        return None;
    }

    let rs = if spec.stellated {
        finite_or(spec.stellation_radius, 1.0)
    } else {
        1.0
    };
    let offset = finite_or(spec.stellation_offset, 0.0);
    let step = std::f64::consts::TAU / f64::from(n);
    let at = |theta: f64, r: f64| {
        c.add(major.scale(theta.cos() * r))
            .sub(minor.scale(theta.sin() * r))
    };

    // The corners in outline order, each with its rounding length and
    // whether it is rounded at all.
    let unit = (major.len() * rs.abs().max(1.0)).max(1.0);
    let lp = unit * non_negative(spec.primary_curvature);
    let ls = unit * non_negative(spec.stellation_curvature);
    let mut corners: Vec<Corner> = Vec::with_capacity(n as usize * 2);
    for k in 0..n {
        let theta = step / 2.0 + f64::from(k) * step;
        corners.push(Corner {
            at: at(theta, 1.0),
            cut: lp,
            rounded: spec.primary_curved,
            primary: true,
        });
        if spec.stellated {
            corners.push(Corner {
                at: at(theta + (0.5 + offset) * step, rs),
                cut: ls,
                rounded: spec.stellation_curved,
                primary: false,
            });
        }
    }

    // Where each corner's rounding starts (on the incoming edge) and ends (on
    // the outgoing edge). An unrounded corner starts and ends on itself.
    let m = corners.len();
    let mut cut_in = vec![P(0.0, 0.0); m];
    let mut cut_out = vec![P(0.0, 0.0); m];
    for i in 0..m {
        let a = corners[i];
        let bn = corners[(i + 1) % m];
        let d = bn.at.sub(a.at).len();
        let want = a.cut + bn.cut;
        let (ka, kb) = if d < 1.0 || want < 1.0 {
            (0.0, 0.0)
        } else if want > d {
            (a.cut / want, bn.cut / want)
        } else {
            (a.cut / d, bn.cut / d)
        };
        cut_out[i] = if a.rounded {
            a.at.toward(bn.at, ka)
        } else {
            a.at
        };
        cut_in[(i + 1) % m] = if bn.rounded {
            bn.at.toward(a.at, kb)
        } else {
            bn.at
        };
    }

    b.move_to(cut_out[0].pt());
    for step_i in 1..=m {
        let i = step_i % m;
        let prev = step_i - 1;
        let template = if corners[prev].primary {
            spec.primary_edge
        } else {
            spec.secondary_edge
        };
        edge(&mut b, cut_out[prev], cut_in[i], template);
        let corner = corners[i];
        if corner.rounded {
            b.cubic_to(
                cut_in[i].toward(corner.at, CURVE_FACTOR).pt(),
                cut_out[i].toward(corner.at, CURVE_FACTOR).pt(),
                cut_out[i].pt(),
            );
        }
    }
    b.close();
    Some(b.build())
}

#[derive(Clone, Copy, Debug)]
struct Corner {
    at: P,
    cut: f64,
    rounded: bool,
    primary: bool,
}

fn finite_or(v: f64, fallback: f64) -> f64 {
    if v.is_finite() { v } else { fallback }
}

fn non_negative(v: f64) -> f64 {
    if v.is_finite() && v > 0.0 { v } else { 0.0 }
}

/// Four cubics through `+major`, `+minor`, `−major`, `−minor`.
fn ellipse(b: &mut PathBuilder, c: P, major: P, minor: P) {
    let ends = [
        c.add(major),
        c.add(minor),
        c.sub(major),
        c.sub(minor),
        c.add(major),
    ];
    b.move_to(ends[0].pt());
    for w in ends.windows(2) {
        let (from, to) = (w[0], w[1]);
        // The parallelogram corner between two adjacent axis ends.
        let corner = from.add(to).sub(c);
        b.cubic_to(
            from.toward(corner, CURVE_FACTOR).pt(),
            to.toward(corner, CURVE_FACTOR).pt(),
            to.pt(),
        );
    }
    b.close();
}

/// One edge from `from` to `to`, following the template when it is a single
/// cubic and drawing a straight line otherwise.
fn edge(b: &mut PathBuilder, from: P, to: P, template: Option<&Path>) {
    let Some(t) = template.filter(|t| t.verbs() == [Verb::MoveTo, Verb::CubicTo]) else {
        b.line_to(to.pt());
        return;
    };
    let pts = t.points();
    let (e0, e1, e2, e3) = (P::of(pts[0]), P::of(pts[1]), P::of(pts[2]), P::of(pts[3]));
    let span = e3.sub(e0);
    let target = to.sub(from);
    let span_sq = span.0 * span.0 + span.1 * span.1;
    if target.len() < 1.0 {
        b.cubic_to(from.pt(), to.pt(), to.pt());
        return;
    }
    if span_sq < 1.0 {
        b.line_to(to.pt());
        return;
    }
    // The similarity sending `span` to `target`, as complex division.
    let zr = (target.0 * span.0 + target.1 * span.1) / span_sq;
    let zi = (target.1 * span.0 - target.0 * span.1) / span_sq;
    let map = |p: P| {
        let v = p.sub(e0);
        from.add(P(zr * v.0 - zi * v.1, zr * v.1 + zi * v.0))
    };
    b.cubic_to(map(e1).pt(), map(e2).pt(), to.pt());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mp;

    fn spec(sides: u32) -> RegularShapeSpec<'static> {
        RegularShapeSpec {
            sides,
            circular: false,
            stellated: false,
            primary_curved: false,
            stellation_curved: false,
            centre: Point::new(Mp::new(100_000), Mp::new(200_000)),
            major: Vector::new(Mp::new(0), Mp::new(50_000)),
            minor: Vector::new(Mp::new(50_000), Mp::new(0)),
            stellation_radius: 0.5,
            stellation_offset: 0.0,
            primary_curvature: 0.0,
            stellation_curvature: 0.0,
            primary_edge: None,
            secondary_edge: None,
        }
    }

    #[test]
    fn a_square_is_centred_and_has_area() {
        let p = regular_shape_outline(&spec(4)).unwrap();
        let r = p.bounds();
        assert!(
            r.width().raw() > 60_000 && r.height().raw() > 60_000,
            "{r:?}"
        );
        let mid = r.centre();
        assert!((mid.x.raw() - 100_000).abs() <= 1 && (mid.y.raw() - 200_000).abs() <= 1);
        // Four corners plus the move: every point is on the circumscribed
        // circle of radius 50 000.
        for pt in p.points() {
            let d = pt.distance_to(Point::new(Mp::new(100_000), Mp::new(200_000)));
            assert!((d - 50_000.0).abs() < 2.0, "{d}");
        }
    }

    #[test]
    fn a_star_alternates_radii() {
        let mut s = spec(5);
        s.stellated = true;
        let p = regular_shape_outline(&s).unwrap();
        let c = Point::new(Mp::new(100_000), Mp::new(200_000));
        let d: Vec<f64> = p.points().iter().map(|q| q.distance_to(c)).collect();
        assert_eq!(d.len(), 11, "move, then ten edges");
        for (i, v) in d[1..].iter().enumerate() {
            let want = if i % 2 == 0 { 25_000.0 } else { 50_000.0 };
            assert!((v - want).abs() < 2.0, "point {i}: {v}");
        }
    }

    #[test]
    fn an_ellipse_spans_both_axes() {
        let mut s = spec(0);
        s.circular = true;
        s.minor = Vector::new(Mp::new(20_000), Mp::new(0));
        let r = regular_shape_outline(&s).unwrap().tight_bounds();
        assert!((r.width().raw() - 40_000).abs() <= 2, "{r:?}");
        assert!((r.height().raw() - 100_000).abs() <= 2, "{r:?}");
    }

    #[test]
    fn rounded_corners_stay_inside_the_polygon() {
        let mut s = spec(4);
        s.primary_curved = true;
        s.primary_curvature = 0.2;
        let sharp = regular_shape_outline(&spec(4)).unwrap().bounds();
        let round = regular_shape_outline(&s).unwrap().bounds();
        assert!(sharp.contains_rect(round), "{sharp:?} vs {round:?}");
        assert!(round.width().raw() > 0);
    }

    #[test]
    fn hostile_parameters_never_panic() {
        for sides in [
            0,
            1,
            2,
            3,
            MAX_REGULAR_SIDES,
            MAX_REGULAR_SIDES + 1,
            u32::MAX,
        ] {
            let mut s = spec(sides);
            s.stellated = true;
            s.primary_curved = true;
            s.stellation_curved = true;
            s.stellation_radius = f64::NAN;
            s.stellation_offset = f64::INFINITY;
            s.primary_curvature = -3.0;
            s.stellation_curvature = f64::MAX;
            s.major = Vector::new(Mp::MAX, Mp::MIN);
            let _ = regular_shape_outline(&s);
        }
    }
}
