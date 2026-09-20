//! Boolean operations on paths.
//!
//! # Engine and pipeline
//!
//! The engine is **`i_overlay` 9** (`MIT OR Apache-2.0`), driven through its
//! **32-bit integer API**. That choice is the load-bearing one here, and it is
//! stronger than the float-plus-`grid_size` arrangement the phase document
//! anticipated, for one reason: `i_overlay`'s `i32` engine accepts coordinates
//! in `-2^30..=2^30 - 1`, which is *exactly* [`Mp::EXTENT`]. Millipoints go in
//! untouched, so there is no scaling step, no grid to choose, no dependence on
//! the input's magnitude, and the result is bit-reproducible by construction
//! rather than by configuration. Golden-image tests and "undo then redo gives
//! the same document" both rest on that.
//!
//! The pipeline:
//!
//! 1. [`flatten_traced`](crate::flatten_traced) both operands at
//!    [`Tolerance::BOOLEAN`](crate::Tolerance::BOOLEAN), keeping the trace.
//! 2. Run `i_overlay` on the integer millipoints.
//! 3. Refit: an output run that came wholly from one untouched input segment,
//!    endpoints included, has its original cubic restored verbatim. Anything
//!    else stays a polyline.
//! 4. Clean: drop repeated and zero-length vertices, drop subpaths whose area
//!    is below a square millipoint, and normalise orientation.
//!
//! Alternatives considered and rejected: `flo_curves` 0.8 (Apache-2.0, works
//! on curves directly, but weaker in the degenerate cases that dominate real
//! documents) stays named as the fallback if `i_overlay` ever blocks us, and
//! `i_curve` — booleans natively on cubics, built on `i_overlay` — is the
//! 2027 candidate, to be reconsidered once it reaches 1.0 with six months of
//! release history. Because everything goes through [`boolean`], adopting it
//! is a contained change.
//!
//! [`Mp::EXTENT`]: crate::Mp::EXTENT_MAX

use crate::{FillRule, Mp, Path, PathBuilder, Point, Polyline, SegmentTrace, Tolerance};
use i_overlay::core::fill_rule::FillRule as IoFillRule;
use i_overlay::core::overlay::{ContourDirection, IntOverlayOptions};
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::core::simplify::Simplify;
use i_overlay::core::single::SingleIntOverlay;
use i_overlay::i_float::int::point::IntPoint;
use std::collections::HashMap;

/// Which boolean operation to perform.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum BoolOp {
    /// Everything covered by either operand.
    Union,
    /// Only what both cover.
    Intersection,
    /// What the first covers and the second does not.
    Difference,
    /// What exactly one of them covers.
    Xor,
}

impl BoolOp {
    /// The equivalent `i_overlay` rule.
    fn to_overlay(self) -> OverlayRule {
        match self {
            BoolOp::Union => OverlayRule::Union,
            BoolOp::Intersection => OverlayRule::Intersect,
            BoolOp::Difference => OverlayRule::Difference,
            BoolOp::Xor => OverlayRule::Xor,
        }
    }
}

impl FillRule {
    /// The equivalent `i_overlay` fill rule.
    fn to_overlay(self) -> IoFillRule {
        match self {
            FillRule::NonZero => IoFillRule::NonZero,
            FillRule::Negative => IoFillRule::Negative,
            FillRule::EvenOdd => IoFillRule::EvenOdd,
            FillRule::Positive => IoFillRule::Positive,
        }
    }
}

/// Combines two paths.
///
/// Deterministic by construction: the integer engine sees exact millipoints,
/// so the same inputs always produce the same output, in the same order, on
/// every platform.
///
/// `tol` controls only the flattening of the *input* curves; the boolean
/// itself is exact on the resulting polygons.
#[must_use]
pub fn boolean(a: &Path, b: &Path, op: BoolOp, rule: FillRule, tol: Tolerance) -> Path {
    let (pa, ta) = crate::flatten_traced(a, tol);
    let (pb, tb) = crate::flatten_traced(b, tol);
    let subj = to_contours(&pa);
    let clip = to_contours(&pb);
    if subj.is_empty() && clip.is_empty() {
        return Path::new();
    }
    let shapes = subj.overlay(&clip, op.to_overlay(), rule.to_overlay());
    let restore = RestoreTable::build(&[(a, &pa, &ta), (b, &pb, &tb)]);
    from_shapes(&shapes, &restore)
}

/// Removes self-intersections from a single path, resolving it under `rule`
/// into a set of non-overlapping, correctly oriented subpaths.
///
/// This is what [`offset`](crate::offset) uses to delete the loops that
/// offsetting a concave region always produces, and what makes
/// "`offset` output has no self-intersections" a testable claim.
#[must_use]
pub fn self_union(a: &Path, rule: FillRule, tol: Tolerance) -> Path {
    let (pa, ta) = crate::flatten_traced(a, tol);
    let subj = to_contours(&pa);
    if subj.is_empty() {
        return Path::new();
    }
    let shapes = subj.simplify(rule.to_overlay(), options());
    let restore = RestoreTable::build(&[(a, &pa, &ta)]);
    from_shapes(&shapes, &restore)
}

/// The engine options this crate always uses.
///
/// `min_output_area` is one square millipoint: below that a contour is
/// numerical noise, and keeping it would make `difference(A, A)` return a
/// sliver instead of nothing. Collinear points are not preserved, because a
/// canonical output is worth more here than a minimal diff against the input.
fn options() -> IntOverlayOptions<u64> {
    IntOverlayOptions {
        preserve_input_collinear: false,
        output_direction: ContourDirection::CounterClockwise,
        preserve_output_collinear: false,
        min_output_area: 1,
        ogc: false,
    }
}

/// Converts flattened polylines into integer contours.
///
/// Every subpath becomes a closed contour, open ones included: a fill rule
/// has no notion of an open region, and `i_overlay` closes them implicitly
/// anyway. Contours with fewer than three vertices enclose nothing and are
/// dropped before they can confuse the engine.
fn to_contours(polys: &[Polyline]) -> Vec<Vec<IntPoint<i32>>> {
    polys
        .iter()
        .filter(|p| p.points.len() >= 3)
        .map(|p| {
            p.points
                .iter()
                .map(|q| {
                    let (x, _) = q.x.clamp_to_extent();
                    let (y, _) = q.y.clamp_to_extent();
                    IntPoint::new(x.raw(), y.raw())
                })
                .collect()
        })
        .collect()
}

/// A lookup from an output vertex pair back to an input cubic that has both as
/// its endpoints and contributed every vertex between them.
///
/// Keyed by the ordered endpoint pair, because that is the only information
/// the engine's output carries: `i_overlay` returns coordinates, not
/// provenance. Coincident endpoints from different segments collide, which is
/// why the value records the full expected vertex run and the caller checks it.
#[derive(Debug, Default)]
struct RestoreTable {
    /// `start -> (interior vertices in order, the cubic's control points and
    /// endpoint)`. Keyed by the start alone so that a lookup costs one hash
    /// and a handful of comparisons; keying by the whole run would mean
    /// scanning every possible run length at every output vertex, which is
    /// quadratic on the 100 000-segment paths the budget covers.
    runs: HashMap<(i32, i32), Vec<(Vec<Point>, [Point; 3])>>,
}

impl RestoreTable {
    /// Indexes every cubic of every operand that survived flattening intact.
    fn build(operands: &[(&Path, &Vec<Polyline>, &SegmentTrace)]) -> RestoreTable {
        let mut t = RestoreTable::default();
        for (path, polys, trace) in operands {
            let segs = path.indexed_segments();
            for (pi, poly) in polys.iter().enumerate() {
                let Some(src) = trace.polyline_sources(pi) else { continue };
                let mut i = 0usize;
                while i < src.len() {
                    let seg_id = src[i].segment;
                    if seg_id == u32::MAX {
                        i += 1;
                        continue;
                    }
                    let mut j = i;
                    while j + 1 < src.len() && src[j + 1].segment == seg_id {
                        j += 1;
                    }
                    // The run covering one source segment is
                    // `src[i - 1 ..= j]`: the previous vertex is the
                    // segment's start.
                    if i > 0
                        && let Some(&(_, _, crate::Segment::Cubic { p1, p2, p3, .. })) = segs
                            .iter()
                            .find(|(sp, si, _)| *sp == pi && *si == seg_id as usize)
                    {
                        let start = poly.points[i - 1];
                        let end = poly.points[j];
                        let interior = poly.points[i..j].to_vec();
                        debug_assert_eq!(key(end), key(p3));
                        t.runs.entry(key(start)).or_default().push((interior, [p1, p2, p3]));
                    }
                    i = j + 1;
                }
            }
        }
        t
    }

    /// If the output vertices from `at` onwards reproduce a stored run,
    /// returns how many vertices it consumes and the cubic to emit.
    ///
    /// The run must match exactly, endpoints included. A run that the boolean
    /// cut, or whose endpoints moved, falls through to line segments — losing
    /// curve fidelity there is correct, because the output really is a new
    /// curve and pretending otherwise would move the geometry.
    fn lookup(&self, contour: &[Point], at: usize) -> Option<(usize, [Point; 3])> {
        let list = self.runs.get(&key(contour[at]))?;
        for (interior, ctrl) in list {
            let span = interior.len() + 1;
            if at + span >= contour.len() {
                continue;
            }
            if contour[at + span] == ctrl[2] && contour[at + 1..at + span] == interior[..] {
                return Some((span, *ctrl));
            }
        }
        None
    }
}

/// Integer key for a point.
#[inline]
fn key(p: Point) -> (i32, i32) {
    (p.x.raw(), p.y.raw())
}

/// Rebuilds a [`Path`] from the engine's shapes, restoring cubics where the
/// trace allows and cleaning the result.
fn from_shapes(
    shapes: &[Vec<Vec<IntPoint<i32>>>],
    restore: &RestoreTable,
) -> Path {
    let mut b = PathBuilder::new();
    for shape in shapes {
        for contour in shape {
            let pts = clean_contour(contour);
            if pts.len() < 3 {
                continue;
            }
            b.move_to(pts[0]);
            let mut i = 0usize;
            while i + 1 < pts.len() {
                if let Some((span, [c1, c2, p3])) = restore.lookup(&pts, i) {
                    b.cubic_to(c1, c2, p3);
                    i += span;
                } else {
                    b.line_to(pts[i + 1]);
                    i += 1;
                }
            }
            b.close();
        }
    }
    b.build()
}

/// Drops repeated vertices. Coordinates are integers, so "closer than 1 mp"
/// and "identical" are the same test, and there is nothing to merge beyond it.
fn clean_contour(contour: &[IntPoint<i32>]) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::with_capacity(contour.len());
    for q in contour {
        let p = Point::new(Mp::new(q.x), Mp::new(q.y));
        if out.last() != Some(&p) {
            out.push(p);
        }
    }
    while out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}
