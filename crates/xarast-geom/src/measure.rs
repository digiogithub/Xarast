//! Arc length, nearest point and hit testing.

use crate::{FillRule, Mp, Path, Point, Tolerance, Vector};
use kurbo::{ParamCurve, ParamCurveArclen, ParamCurveDeriv, ParamCurveNearest, Shape};

/// Total arc length in millipoints, to the requested accuracy.
///
/// Feeds text-on-a-path and dash placement. A caller that will ask many
/// questions of the same path should build a cumulative table once and keep
/// it: this crate is stateless by design, so the cache belongs to the caller.
#[must_use]
pub fn arclen(path: &Path, accuracy: f64) -> f64 {
    let acc = if accuracy.is_finite() && accuracy > 0.0 { accuracy } else { 1e-6 };
    path.segments().map(|s| s.to_kurbo().arclen(acc)).sum()
}

/// The point at a given arc-length distance along the path, with the unit
/// tangent there.
///
/// Returns `None` for an empty path or a distance outside `0..=arclen`. The
/// tangent is returned as a millipoint-scale [`Vector`], so a caller wanting
/// an angle should use its components rather than its length.
#[must_use]
pub fn point_at_arclen(path: &Path, distance: f64, accuracy: f64) -> Option<(Point, Vector)> {
    if !distance.is_finite() || distance < 0.0 {
        return None;
    }
    let acc = if accuracy.is_finite() && accuracy > 0.0 { accuracy } else { 1e-6 };
    let mut remaining = distance;
    let mut last: Option<(Point, Vector)> = None;
    for seg in path.segments() {
        let k = seg.to_kurbo();
        let len = k.arclen(acc);
        if len <= 0.0 {
            continue;
        }
        if remaining <= len {
            let t = k.inv_arclen(remaining, acc);
            return Some((Point::from_kurbo(k.eval(t)), tangent_at(&k, t)));
        }
        remaining -= len;
        let t = 1.0;
        last = Some((Point::from_kurbo(k.eval(t)), tangent_at(&k, t)));
    }
    // Land exactly on the end when rounding leaves a sliver of `remaining`.
    if remaining <= acc { last } else { None }
}

/// The unit tangent of a `kurbo` segment at parameter `t`, scaled to
/// millipoints.
fn tangent_at(seg: &kurbo::PathSeg, t: f64) -> Vector {
    let d = match seg {
        kurbo::PathSeg::Line(l) => l.deriv().eval(t).to_vec2(),
        kurbo::PathSeg::Quad(q) => q.deriv().eval(t).to_vec2(),
        kurbo::PathSeg::Cubic(c) => c.deriv().eval(t).to_vec2(),
    };
    let n = d.hypot();
    if n == 0.0 {
        return Vector::ZERO;
    }
    // A unit vector rounded to integer millipoints would be (1, 0) or (0, 0),
    // so the tangent is scaled to one point: enough resolution to recover the
    // angle to about a thousandth of a degree.
    Vector::from_kurbo(d / n * Mp::PER_PT as f64)
}

/// Where on the path a query point is closest to it.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Nearest {
    /// The closest point on the path.
    pub point: Point,
    /// Distance from the query point, in millipoints.
    pub distance: f64,
    /// Which subpath the closest point is on.
    pub subpath: usize,
    /// Which segment within that subpath.
    pub segment: usize,
    /// The segment parameter, in `0.0..=1.0`.
    pub t: f64,
}

/// The closest point on the path to `p`, or `None` for a path with no
/// segments.
#[must_use]
pub fn nearest_point(path: &Path, p: Point, accuracy: f64) -> Option<Nearest> {
    let acc = if accuracy.is_finite() && accuracy > 0.0 { accuracy } else { 1e-3 };
    let target = p.to_kurbo();
    let mut best: Option<Nearest> = None;
    for (sp, si, seg) in path.indexed_segments() {
        let k = seg.to_kurbo();
        let n = k.nearest(target, acc);
        let d = n.distance_sq;
        if best.as_ref().is_none_or(|b| d < b.distance * b.distance) {
            best = Some(Nearest {
                point: Point::from_kurbo(k.eval(n.t)),
                distance: d.sqrt(),
                subpath: sp,
                segment: si,
                t: n.t,
            });
        }
    }
    best
}

/// Whether `p` lies inside the path's fill under `rule`.
///
/// Exact: the winding number is computed against the curves themselves, not
/// against a flattened approximation.
#[must_use]
pub fn hit_fill(path: &Path, p: Point, rule: FillRule) -> bool {
    if path.is_empty() {
        return false;
    }
    rule.covers(path.to_bez_path().winding(p.to_kurbo()))
}

/// Whether `p` lies within `half_width` of the path's outline.
///
/// # The approximation
///
/// This measures distance to the centreline rather than building the stroke
/// outline, because hit-testing runs on every pointer move and stroke
/// expansion does not. Near caps and joins the two differ, by at most the
/// half-width itself — a butt cap is tested as though it were round. That
/// error is below the pick tolerance a user can perceive, and it errs towards
/// making objects easier to hit, which is the direction a user prefers. It is
/// documented rather than hidden because a caller that needs the exact
/// outline can build one with [`stroke_to_path`](crate::stroke_to_path).
#[must_use]
pub fn hit_stroke(path: &Path, p: Point, half_width: Mp, tol: Tolerance) -> bool {
    if path.is_empty() {
        return false;
    }
    let limit = half_width.to_f64().abs();
    // A quick rejection on the control hull first: it is conservative, so a
    // miss here is a genuine miss, and it costs one rectangle test against
    // the per-segment work below.
    if !path.bounds().inflated(half_width.abs() + Mp::new(1)).contains(p) {
        return false;
    }
    let acc = (tol.get() * 0.5).max(1e-3);
    nearest_point(path, p, acc).is_some_and(|n| n.distance <= limit)
}

/// A build-once, query-many acceleration structure for hit testing.
///
/// Holds the path flattened at [`Tolerance::EXPORT`] — 1 mp, or 0.35 um — as
/// edges, plus a uniform grid of row buckets over the bounding box. Ray
/// casting then touches only the edges in the query point's row.
///
/// A BVH would be more elegant and `parry2d` has one, but `parry2d` is
/// physics-shaped and its shape model does not fit Béziers, so it was
/// rejected in the technology survey. A uniform grid is enough while the
/// `hit_fill` budget holds; if a 100 000-segment path misses it, the fallback
/// is a static BVH built here rather than a new dependency.
///
/// The 1 mp flattening means results can differ from the exact
/// [`hit_fill`] within a millipoint of the outline. That is three orders of
/// magnitude below any pick tolerance, and it buys a bounded, predictable
/// query cost.
#[derive(Clone, Debug)]
pub struct HitIndex {
    /// Flattened edges as `(from, to)` pairs.
    edges: Vec<(Point, Point)>,
    /// For each row, the indices of the edges whose y-range meets it.
    rows: Vec<Vec<u32>>,
    /// The bounds the rows span.
    bounds: crate::Rect,
    /// Height of one row, in millipoints; never zero.
    row_height: f64,
}

impl HitIndex {
    /// The number of grid rows, chosen so that a typical path has a handful
    /// of edges per row without the row vector dominating memory.
    fn row_count(edges: usize) -> usize {
        edges.isqrt().clamp(1, 1024)
    }

    /// Builds the index. Cost is linear in the flattened edge count.
    #[must_use]
    pub fn build(path: &Path) -> HitIndex {
        let polys = crate::flatten(path, Tolerance::EXPORT);
        let mut edges: Vec<(Point, Point)> = Vec::new();
        for poly in &polys {
            let n = poly.points.len();
            if n < 2 {
                continue;
            }
            for i in 0..n - 1 {
                edges.push((poly.points[i], poly.points[i + 1]));
            }
            // Every subpath closes implicitly for fill purposes, exactly as
            // the winding rules see it.
            if poly.points[n - 1] != poly.points[0] {
                edges.push((poly.points[n - 1], poly.points[0]));
            }
        }
        let bounds = path.bounds();
        let nrows = HitIndex::row_count(edges.len());
        let h = (bounds.height().to_f64() / nrows as f64).max(1.0);
        let mut rows = vec![Vec::new(); nrows];
        if !bounds.is_empty() {
            let y0 = bounds.lo.y.to_f64();
            for (i, (a, b)) in edges.iter().enumerate() {
                let (lo, hi) = {
                    let (ay, by) = (a.y.to_f64(), b.y.to_f64());
                    if ay <= by { (ay, by) } else { (by, ay) }
                };
                let r0 = (((lo - y0) / h).floor() as isize).clamp(0, nrows as isize - 1) as usize;
                let r1 = (((hi - y0) / h).floor() as isize).clamp(0, nrows as isize - 1) as usize;
                for row in rows.iter_mut().take(r1 + 1).skip(r0) {
                    row.push(i as u32);
                }
            }
        }
        HitIndex { edges, rows, bounds, row_height: h }
    }

    /// The row a y coordinate falls in, or `None` when outside the bounds.
    fn row_of(&self, y: f64) -> Option<usize> {
        if self.rows.is_empty() || self.bounds.is_empty() {
            return None;
        }
        let r = ((y - self.bounds.lo.y.to_f64()) / self.row_height).floor();
        if r < 0.0 || r >= self.rows.len() as f64 {
            return None;
        }
        Some(r as usize)
    }

    /// Whether `p` is inside the fill, by ray casting over the row's edges.
    ///
    /// `path` is taken again rather than stored so that the index is cheap to
    /// keep alongside a copy-on-write path without duplicating its geometry.
    #[must_use]
    pub fn hit_fill(&self, path: &Path, p: Point, rule: FillRule) -> bool {
        if !self.bounds.contains(p) {
            return false;
        }
        let Some(row) = self.row_of(p.y.to_f64()) else {
            return crate::hit_fill(path, p, rule);
        };
        let (px, py) = p.to_f64();
        let mut winding = 0i32;
        for &i in &self.rows[row] {
            let (a, b) = self.edges[i as usize];
            let (ax, ay) = a.to_f64();
            let (bx, by) = b.to_f64();
            // Half-open in y so that a vertex exactly on the ray is counted
            // once, not twice or not at all.
            if (ay <= py) == (by <= py) {
                continue;
            }
            let t = (py - ay) / (by - ay);
            if ax + t * (bx - ax) > px {
                winding += if by > ay { 1 } else { -1 };
            }
        }
        rule.covers(winding)
    }

    /// Whether `p` is within `half_width` of the outline, using the row's
    /// edges plus its neighbours so that a near miss above or below the row
    /// boundary is still found.
    #[must_use]
    pub fn hit_stroke(&self, path: &Path, p: Point, half_width: Mp, tol: Tolerance) -> bool {
        let _ = tol;
        let limit = half_width.to_f64().abs();
        if !self.bounds.inflated(half_width.abs() + Mp::new(1)).contains(p) {
            return false;
        }
        let Some(row) = self.row_of(p.y.to_f64()) else {
            return crate::hit_stroke(path, p, half_width, tol);
        };
        // The band of rows the disc of radius `limit` can reach.
        let span = ((limit / self.row_height).ceil() as usize).min(self.rows.len());
        let lo = row.saturating_sub(span);
        let hi = (row + span).min(self.rows.len() - 1);
        let limit_sq = limit * limit;
        for r in lo..=hi {
            for &i in &self.rows[r] {
                let (a, b) = self.edges[i as usize];
                if point_segment_distance_sq(p, a, b) <= limit_sq {
                    return true;
                }
            }
        }
        false
    }

    /// How many flattened edges the index holds.
    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}

/// Squared distance from a point to a line segment, in `f64` millipoints.
fn point_segment_distance_sq(p: Point, a: Point, b: Point) -> f64 {
    let (px, py) = p.to_f64();
    let (ax, ay) = a.to_f64();
    let (bx, by) = b.to_f64();
    let (dx, dy) = (bx - ax, by - ay);
    let len_sq = dx * dx + dy * dy;
    let t = if len_sq == 0.0 { 0.0 } else { (((px - ax) * dx + (py - ay) * dy) / len_sq).clamp(0.0, 1.0) };
    let (cx, cy) = (ax + t * dx, ay + t * dy);
    (px - cx) * (px - cx) + (py - cy) * (py - cy)
}
