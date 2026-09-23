//! Bounding the work of a stroke to the region it can touch.
//!
//! A stroke is expanded into an outline in document space before it is
//! rasterised, so its cost follows the whole path, not the part that is
//! visible. At deep zoom a page-sized path is thousands of screens long, and
//! a finely dashed, round-capped stroke along it expands into millions of
//! dashes: `fuzz_display_list` found two 1.8 GB allocations that way
//! (gintrack XARA-T-0022).
//!
//! [`cull_stroke_input`] cuts the path down to what lies within a window —
//! the band being drawn, grown by the stroke's reach, in device space —
//! before the path is dashed and stroked. The tests transform the centre
//! line into device space rather than the window back into the document,
//! so that a degenerate view transform, which has no inverse, still culls:
//!
//! * A segment whose control box misses the rectangle is dropped.
//! * A line crossing it is clipped to it; a curve crossing it is split
//!   until each piece is either outside or no larger than the rectangle.
//! * The **dash phase** survives: every kept run is dashed on its own,
//!   starting at the arc length it has along its subpath, so a dash lands
//!   where it would have landed on the whole path.
//! * A closed subpath cut open keeps its seam: the run ending at the start
//!   vertex and the run leaving it are emitted back to back, and joined
//!   there when both are inside a dash, as the dasher does for an intact
//!   closed subpath.
//!
//! A subpath that is kept whole is emitted exactly as it was, so a stroke
//! that lies inside the rectangle expands to the same outline as before.
//!
//! On top of the culling there is a **work budget**, [`MAX_DASH_WORK`]:
//! the dashes left, times what each costs to flatten (a round cap on a
//! stroke a thousand pixels wide is hundreds of segments), may not exceed
//! it, or the pattern is dropped and the stroke drawn solid. A pattern only
//! gets there when its dashes are far below a pixel apart or far too many
//! to see, so the solid stroke is what they average to anyway. Culling
//! alone cannot bound this: a stroke wide enough reaches the window from
//! anywhere.

use kurbo::{
    Affine, BezPath, Line, ParamCurve, ParamCurveArclen, PathEl, PathSeg, Point, Rect, Shape,
};

/// The most flattened segments the dashes of one stroke may cost; see the
/// module documentation and [`dash_cost`].
pub const MAX_DASH_WORK: f64 = 1_000_000.0;

/// What one dash costs to flatten, in segments: its two caps at the
/// device-space half width and flatness, plus a few for its sides.
#[must_use]
pub fn dash_cost(half_width_px: f64, tol_px: f64) -> f64 {
    let r = half_width_px.max(0.0);
    let tol = tol_px.max(1e-6);
    4.0 + 2.0 * (r / tol).sqrt()
}

/// Where a stroke is drawn: the document-to-device transform, and the
/// device rectangle, already grown by the stroke's reach, that its centre
/// line has to come near to matter.
#[derive(Debug, Clone, Copy)]
pub struct Window {
    /// Document to device.
    pub to_dev: Affine,
    /// The device rectangle.
    pub rect: Rect,
}

/// How precisely a dropped stretch of path is measured to carry the dash
/// phase across it, in document units (millipoints). The dasher itself
/// measures to 1e-6.
const PHASE_ACCURACY: f64 = 1e-3;

/// How deep a curve is split looking for the part inside the rectangle.
/// 2^-24 of a curve is below a millipoint for any curve a document holds.
const MAX_SPLIT_DEPTH: u32 = 24;

/// Where a kept piece came from: segment index and parameter range.
#[derive(Debug, Clone, Copy)]
struct Piece {
    seg: usize,
    t0: f64,
    t1: f64,
}

/// A run of contiguous kept pieces and its arc length along the subpath.
type Run = (Vec<Piece>, f64);

/// One subpath, as segments.
struct Subpath<'a> {
    /// The original elements, for emitting it untouched.
    els: &'a [PathEl],
    segs: Vec<PathSeg>,
    closed: bool,
}

/// The path to hand to an **undashed** stroker, and whether it has
/// already been dashed.
#[derive(Debug)]
pub struct CulledStroke {
    /// The kept geometry, dashed when `dashed` is true.
    pub path: BezPath,
    /// Whether the dash pattern was applied. When it is false and a
    /// pattern was given, the budget dropped it.
    pub dashed: bool,
}

/// Cuts a stroke's centre line down to what can reach the window.
///
/// The path is in document space; `keep.rect` is in device space and must
/// already include the stroke's reach: half the width times the mitre
/// limit, and the square cap's diagonal. `dashes` is the resolved pattern
/// in document units, empty for a solid stroke. `offset` is the pattern's
/// starting phase, already reduced modulo the period
/// (`xarast_geom::reduced_dash_offset`); like the stroker's own dashing,
/// it restarts at every subpath.
/// `per_dash` is [`dash_cost`] for this stroke.
#[must_use]
pub fn cull_stroke_input(
    path: &BezPath,
    keep: Window,
    dashes: &[f64],
    offset: f64,
    per_dash: f64,
) -> CulledStroke {
    let subpaths = split_subpaths(path.elements());
    let period: f64 = dashes.iter().sum();
    let mut dashed = !dashes.is_empty() && period > 0.0 && period.is_finite();
    // Runs of kept pieces per subpath, with each run's starting arc length.
    let mut kept: Vec<(usize, Vec<Run>, bool)> = Vec::new();
    let mut kept_len = 0.0;
    for (i, sp) in subpaths.iter().enumerate() {
        let (runs, whole) = cull_subpath(sp, keep, dashed);
        if dashed {
            kept_len += runs
                .iter()
                .flat_map(|(r, _)| r.iter())
                .map(|p| polygon_len(&sp.segs[p.seg].subsegment(p.t0..p.t1)))
                .sum::<f64>();
        }
        if !runs.is_empty() {
            kept.push((i, runs, whole));
        }
    }
    if dashed && kept_len / period * dashes.len() as f64 * per_dash > MAX_DASH_WORK {
        dashed = false;
    }

    let mut out = BezPath::new();
    for (i, runs, whole) in kept {
        let sp = &subpaths[i];
        if whole {
            emit(&mut out, sp.els.iter().copied(), offset, dashes, dashed);
            continue;
        }
        let wraps = sp.closed
            && runs.len() >= 2
            && runs
                .first()
                .and_then(|(r, _)| r.first())
                .is_some_and(|p| p.seg == 0 && p.t0 == 0.0)
            && runs
                .last()
                .and_then(|(r, _)| r.last())
                .is_some_and(|p| p.seg + 1 == sp.segs.len() && p.t1 == 1.0);
        if wraps {
            let (first, first_s) = &runs[0];
            let (last, last_s) = &runs[runs.len() - 1];
            let mut tail = BezPath::new();
            emit(
                &mut tail,
                run_elements(sp, last),
                offset + *last_s,
                dashes,
                dashed,
            );
            let mut head = BezPath::new();
            emit(
                &mut head,
                run_elements(sp, first),
                offset + *first_s,
                dashes,
                dashed,
            );
            // The seam is the subpath's start vertex. The tail is inside a
            // dash there when its last drawn element ends on it; the head's
            // first dash is the one that starts on it (the dasher emits it
            // last, having held it back for exactly this join).
            let vertex = piece_seg(sp, &first[0]).start();
            let near = |p: Point| (p - vertex).hypot() <= 1e-9 * (1.0 + vertex.to_vec2().hypot());
            let tail_els = tail.elements();
            // The tail's dashes, as element ranges. The one inside a dash at
            // the seam is not always the last: when the tail itself starts
            // inside a dash, the dasher holds that first dash back and emits
            // it last. Move the one ending on the vertex to the end.
            let mut dash_ranges: Vec<(usize, usize)> = Vec::new();
            for (k, el) in tail_els.iter().enumerate() {
                if matches!(el, PathEl::MoveTo(_)) || dash_ranges.is_empty() {
                    dash_ranges.push((k, k + 1));
                } else if let Some(last) = dash_ranges.last_mut() {
                    last.1 = k + 1;
                }
            }
            let at_seam = dash_ranges
                .iter()
                .rposition(|&(a, b)| b - a >= 2 && tail_els[b - 1].end_point().is_some_and(near));
            let tail_active = at_seam.is_some();
            for (k, &(a, b)) in dash_ranges.iter().enumerate() {
                if Some(k) != at_seam {
                    out.extend(tail_els[a..b].iter().copied());
                }
            }
            if let Some(k) = at_seam {
                let (a, b) = dash_ranges[k];
                out.extend(tail_els[a..b].iter().copied());
            }
            let head_els = head.elements();
            let first_dash = head_els.iter().enumerate().position(|(k, el)| {
                matches!(el, PathEl::MoveTo(p) if near(*p))
                    && head_els
                        .get(k + 1)
                        .is_some_and(|n| !matches!(n, PathEl::MoveTo(_)))
            });
            match first_dash {
                Some(k) if tail_active => {
                    let end = head_els[k + 1..]
                        .iter()
                        .position(|el| matches!(el, PathEl::MoveTo(_)))
                        .map_or(head_els.len(), |e| k + 1 + e);
                    // Continue the tail's dash into the first one, then the
                    // rest of the head in its own order.
                    out.extend(head_els[k + 1..end].iter().copied());
                    out.extend(head_els[..k].iter().copied());
                    out.extend(head_els[end..].iter().copied());
                }
                _ => out.extend(head_els.iter().copied()),
            }
            for (run, s) in &runs[1..runs.len() - 1] {
                emit(&mut out, run_elements(sp, run), offset + *s, dashes, dashed);
            }
        } else {
            for (run, s) in &runs {
                emit(&mut out, run_elements(sp, run), offset + *s, dashes, dashed);
            }
        }
    }
    CulledStroke { path: out, dashed }
}

/// An upper bound on how many pieces a dash pattern cuts a path into,
/// from the control polygon's length. Zero for a solid stroke.
#[must_use]
pub fn dash_count_estimate(path: &BezPath, dashes: &[f64]) -> f64 {
    let period: f64 = dashes.iter().sum();
    if dashes.is_empty() || period <= 0.0 || !period.is_finite() {
        return 0.0;
    }
    let len: f64 = path.segments().map(|seg| polygon_len(&seg)).sum();
    len / period * dashes.len() as f64
}

/// Appends elements, dashed from arc length `phase` when `dashed`.
fn emit(
    out: &mut BezPath,
    els: impl Iterator<Item = PathEl>,
    phase: f64,
    dashes: &[f64],
    dashed: bool,
) {
    if dashed {
        for el in kurbo::dash(els, phase, dashes) {
            out.push(el);
        }
    } else {
        for el in els {
            out.push(el);
        }
    }
}

/// The elements of one run of pieces.
fn run_elements<'a>(sp: &'a Subpath<'_>, run: &'a [Piece]) -> impl Iterator<Item = PathEl> + 'a {
    let start = run.first().map(|p| piece_seg(sp, p).start());
    start
        .into_iter()
        .map(PathEl::MoveTo)
        .chain(run.iter().map(|p| seg_to_el(piece_seg(sp, p))))
}

/// A piece as a segment, exactly the original when it is whole.
fn piece_seg(sp: &Subpath<'_>, p: &Piece) -> PathSeg {
    let seg = sp.segs[p.seg];
    if p.t0 == 0.0 && p.t1 == 1.0 {
        seg
    } else {
        seg.subsegment(p.t0..p.t1)
    }
}

fn seg_to_el(seg: PathSeg) -> PathEl {
    match seg {
        PathSeg::Line(l) => PathEl::LineTo(l.p1),
        PathSeg::Quad(q) => PathEl::QuadTo(q.p1, q.p2),
        PathSeg::Cubic(c) => PathEl::CurveTo(c.p1, c.p2, c.p3),
    }
}

/// Splits a path into subpaths, turning a `ClosePath` that moves into an
/// explicit closing line.
fn split_subpaths(els: &[PathEl]) -> Vec<Subpath<'_>> {
    let mut out = Vec::new();
    let mut begin = 0;
    let mut start = Point::ORIGIN;
    let mut last = Point::ORIGIN;
    let mut segs = Vec::new();
    let mut closed = false;
    for (i, el) in els.iter().enumerate() {
        match *el {
            PathEl::MoveTo(p) => {
                if i > begin {
                    out.push(Subpath {
                        els: &els[begin..i],
                        segs: std::mem::take(&mut segs),
                        closed,
                    });
                }
                begin = i;
                closed = false;
                start = p;
                last = p;
            }
            PathEl::LineTo(p) => {
                segs.push(PathSeg::Line(Line::new(last, p)));
                last = p;
            }
            PathEl::QuadTo(p1, p2) => {
                segs.push(PathSeg::Quad(kurbo::QuadBez::new(last, p1, p2)));
                last = p2;
            }
            PathEl::CurveTo(p1, p2, p3) => {
                segs.push(PathSeg::Cubic(kurbo::CubicBez::new(last, p1, p2, p3)));
                last = p3;
            }
            PathEl::ClosePath => {
                if last != start {
                    segs.push(PathSeg::Line(Line::new(last, start)));
                }
                last = start;
                closed = true;
            }
        }
    }
    if els.len() > begin {
        out.push(Subpath {
            els: &els[begin..],
            segs,
            closed,
        });
    }
    out
}

/// The kept runs of one subpath, each with its starting arc length, and
/// whether the subpath is kept whole.
fn cull_subpath(sp: &Subpath<'_>, keep: Window, measure: bool) -> (Vec<Run>, bool) {
    if sp.segs.is_empty() {
        // A lone point: kept verbatim if it is inside, so that a round
        // cap's dot, if the stroker draws one, is unchanged.
        let inside = match sp.els.first() {
            Some(PathEl::MoveTo(p)) => keep.rect.contains(keep.to_dev * *p),
            _ => false,
        };
        // A placeholder run: `whole` makes the caller emit the original
        // elements, so its pieces are never read.
        return if inside {
            (vec![(Vec::new(), 0.0)], true)
        } else {
            (Vec::new(), false)
        };
    }
    let mut runs: Vec<Run> = Vec::new();
    let mut current: Vec<Piece> = Vec::new();
    let mut current_s = 0.0;
    let mut s = 0.0;
    let mut whole = true;
    for (i, seg) in sp.segs.iter().enumerate() {
        let mut intervals = Vec::new();
        kept_intervals(*seg, keep, &mut intervals);
        if !(intervals.len() == 1 && intervals[0] == (0.0, 1.0)) {
            whole = false;
        }
        for (t0, t1) in intervals {
            let continues = t0 == 0.0
                && current
                    .last()
                    .is_some_and(|p| p.seg + 1 == i && p.t1 == 1.0);
            if !continues && !current.is_empty() {
                runs.push((std::mem::take(&mut current), current_s));
            }
            if current.is_empty() {
                current_s = if measure && t0 > 0.0 {
                    s + seg.subsegment(0.0..t0).arclen(PHASE_ACCURACY)
                } else {
                    s
                };
            }
            current.push(Piece { seg: i, t0, t1 });
        }
        if measure {
            s += seg.arclen(PHASE_ACCURACY);
        }
    }
    if !current.is_empty() {
        runs.push((current, current_s));
    }
    (runs, whole)
}

/// The parameter intervals of a segment that can reach `keep`, in order.
///
/// The tests run on the segment in device space; an affine map keeps the
/// parameterisation, so the parameters apply to the document segment.
fn kept_intervals(seg: PathSeg, keep: Window, out: &mut Vec<(f64, f64)>) {
    let dev = keep.to_dev * seg;
    let bb = dev.bounding_box();
    if !overlaps(bb, keep.rect) {
        return;
    }
    if contains(keep.rect, bb) {
        out.push((0.0, 1.0));
        return;
    }
    match dev {
        PathSeg::Line(l) => {
            if let Some((t0, t1)) = clip_line(l, keep.rect) {
                out.push((t0, t1));
            }
        }
        _ => split_curve(dev, 0.0, 1.0, keep.rect, MAX_SPLIT_DEPTH, out),
    }
}

/// Splits a curve until each piece is outside `keep`, inside it, or no
/// bigger than it; keeps the ones that are not outside, merging neighbours.
fn split_curve(seg: PathSeg, t0: f64, t1: f64, keep: Rect, depth: u32, out: &mut Vec<(f64, f64)>) {
    let piece = seg.subsegment(t0..t1);
    let bb = piece.bounding_box();
    if !overlaps(bb, keep) {
        return;
    }
    let small = bb.width() <= keep.width() && bb.height() <= keep.height();
    if depth == 0 || small || contains(keep, bb) {
        match out.last_mut() {
            Some(last) if last.1 == t0 => last.1 = t1,
            _ => out.push((t0, t1)),
        }
        return;
    }
    let tm = 0.5 * (t0 + t1);
    split_curve(seg, t0, tm, keep, depth - 1, out);
    split_curve(seg, tm, t1, keep, depth - 1, out);
}

/// Liang–Barsky: the parameter range of a line inside a rectangle.
fn clip_line(l: Line, r: Rect) -> Option<(f64, f64)> {
    let d = l.p1 - l.p0;
    let mut t0: f64 = 0.0;
    let mut t1: f64 = 1.0;
    for (p, q) in [
        (-d.x, l.p0.x - r.x0),
        (d.x, r.x1 - l.p0.x),
        (-d.y, l.p0.y - r.y0),
        (d.y, r.y1 - l.p0.y),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
    }
    (t0 < t1).then_some((t0, t1))
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x0 <= b.x1 && b.x0 <= a.x1 && a.y0 <= b.y1 && b.y0 <= a.y1
}

fn contains(outer: Rect, inner: Rect) -> bool {
    outer.x0 <= inner.x0 && outer.y0 <= inner.y0 && inner.x1 <= outer.x1 && inner.y1 <= outer.y1
}

/// The control polygon's length: an upper bound on the arc length, cheap
/// enough for the dash budget.
fn polygon_len(seg: &PathSeg) -> f64 {
    match seg {
        PathSeg::Line(l) => l.p0.distance(l.p1),
        PathSeg::Quad(q) => q.p0.distance(q.p1) + q.p1.distance(q.p2),
        PathSeg::Cubic(c) => c.p0.distance(c.p1) + c.p1.distance(c.p2) + c.p2.distance(c.p3),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::{Cap, Join, Stroke, StrokeOpts};

    fn win(rect: Rect) -> Window {
        Window {
            to_dev: Affine::IDENTITY,
            rect,
        }
    }

    fn stroke_of(path: &BezPath, dashes: &[f64], offset: f64) -> BezPath {
        let s = Stroke::new(2.0)
            .with_caps(Cap::Butt)
            .with_join(Join::Miter)
            .with_dashes(offset, dashes.to_vec());
        kurbo::stroke(path.iter(), &s, &StrokeOpts::default(), 0.01)
    }

    /// Samples whether points of `keep` are inside a NonZero outline.
    fn samples(outline: &BezPath, keep: Rect) -> Vec<bool> {
        let mut v = Vec::new();
        for i in 0..40 {
            for j in 0..40 {
                let p = Point::new(
                    keep.x0 + (f64::from(i) + 0.37) * keep.width() / 40.0,
                    keep.y0 + (f64::from(j) + 0.61) * keep.height() / 40.0,
                );
                v.push(outline.winding(p) != 0);
            }
        }
        v
    }

    fn check(path: &BezPath, dashes: &[f64], view: Rect) {
        check_offset(path, dashes, 0.0, view);
    }

    fn check_offset(path: &BezPath, dashes: &[f64], offset: f64, view: Rect) {
        let reach = 2.0;
        let keep = view.inflate(reach, reach);
        let whole = stroke_of(path, dashes, offset);
        let culled = cull_stroke_input(path, win(keep), dashes, offset, 1.0);
        assert!(culled.dashed || dashes.is_empty());
        let s = Stroke::new(2.0).with_caps(Cap::Butt).with_join(Join::Miter);
        let part = kurbo::stroke(culled.path.iter(), &s, &StrokeOpts::default(), 0.01);
        let (a, b) = (samples(&whole, view), samples(&part, view));
        let bad: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).collect();
        assert!(
            bad.is_empty(),
            "{dashes:?} {view:?}: {} samples differ, first {:?}\nculled {:?}",
            bad.len(),
            &bad[..bad.len().min(8)],
            culled.path
        );
    }

    #[test]
    fn a_long_dashed_line_keeps_its_phase_in_the_window() {
        let mut p = BezPath::new();
        p.move_to((-20_000.0, 3.0));
        p.line_to((20_000.0, 7.0));
        check(&p, &[3.0, 2.0, 1.0], Rect::new(-10.0, 0.0, 30.0, 10.0));
        check(&p, &[5.0], Rect::new(12_345.0, 0.0, 12_400.0, 10.0));
    }

    #[test]
    fn a_dash_offset_carries_into_every_kept_run() {
        // XARA-T-0231: the offset shifts the pattern the same whether the
        // stroke is dashed whole or cut to the window first.
        let mut p = BezPath::new();
        p.move_to((-20_000.0, 3.0));
        p.line_to((20_000.0, 7.0));
        for offset in [0.5, 2.0, 4.5] {
            check_offset(&p, &[3.0, 2.0], offset, Rect::new(-10.0, 0.0, 30.0, 10.0));
            check_offset(
                &p,
                &[3.0, 2.0, 1.0],
                offset,
                Rect::new(12_345.0, 0.0, 12_400.0, 10.0),
            );
        }
        let mut sq = BezPath::new();
        sq.move_to((0.0, 0.0));
        sq.line_to((2_000.0, 0.0));
        sq.line_to((2_000.0, 2_000.0));
        sq.line_to((0.0, 2_000.0));
        sq.close_path();
        // Every phase at the seam, including a tail that starts inside a
        // dash (the dasher then emits that dash last, not the one on the
        // seam).
        for k in 0..20 {
            let offset = f64::from(k) * 0.5;
            check_offset(
                &sq,
                &[7.0, 3.0],
                offset,
                Rect::new(-20.0, -20.0, 20.0, 20.0),
            );
        }
    }

    #[test]
    fn a_cut_closed_rectangle_keeps_its_corner_and_seam() {
        let mut p = BezPath::new();
        p.move_to((0.0, 0.0));
        p.line_to((2_000.0, 0.0));
        p.line_to((2_000.0, 2_000.0));
        p.line_to((0.0, 2_000.0));
        p.close_path();
        // The window holds the start vertex, where the dasher joins the
        // last dash to the first.
        let w = Rect::new(-20.0, -20.0, 20.0, 20.0);
        check(&p, &[], w);
        check(&p, &[7.0, 3.0], w);
        check(&p, &[4.0, 4.0], w);
        // And a corner that is not the seam.
        check(
            &p,
            &[7.0, 3.0],
            Rect::new(1_980.0, 1_980.0, 2_020.0, 2_020.0),
        );
        // A pattern that does not divide the perimeter, so the seam lands
        // mid-dash, at windows all the way round.
        for k in 0..16 {
            let x = f64::from(k) * 125.0;
            check(
                &p,
                &[7.0, 4.0, 1.5, 4.0],
                Rect::new(x - 20.0, -20.0, x + 20.0, 20.0),
            );
            check(
                &p,
                &[7.0, 4.0, 1.5],
                Rect::new(-20.0, x - 20.0, 20.0, x + 20.0),
            );
        }
    }

    #[test]
    fn a_long_curve_is_cut_to_the_window() {
        let mut p = BezPath::new();
        p.move_to((0.0, 0.0));
        p.curve_to((3_000.0, 4_000.0), (6_000.0, -4_000.0), (9_000.0, 0.0));
        let culled = cull_stroke_input(
            &p,
            win(Rect::new(4_490.0, -50.0, 4_510.0, 50.0)),
            &[3.0, 1.0],
            0.0,
            1.0,
        );
        // Tens of dashes, not the thousands of the whole curve.
        assert!(
            culled.path.elements().len() < 200,
            "{}",
            culled.path.elements().len()
        );
        check(&p, &[3.0, 1.0], Rect::new(4_490.0, -30.0, 4_510.0, 30.0));
    }

    #[test]
    fn a_path_inside_the_window_is_emitted_unchanged() {
        let mut p = BezPath::new();
        p.move_to((0.0, 0.0));
        p.curve_to((3.0, 4.0), (6.0, -4.0), (9.0, 0.0));
        p.line_to((9.0, 9.0));
        p.close_path();
        let culled = cull_stroke_input(&p, win(Rect::new(-10.0, -10.0, 20.0, 20.0)), &[], 0.0, 1.0);
        assert_eq!(culled.path, p);
        let dashed = cull_stroke_input(
            &p,
            win(Rect::new(-10.0, -10.0, 20.0, 20.0)),
            &[1.0, 0.5],
            0.0,
            1.0,
        );
        let expected: BezPath = kurbo::dash(p.iter(), 0.0, &[1.0, 0.5]).collect();
        assert_eq!(dashed.path, expected);
    }

    #[test]
    fn a_too_dense_pattern_is_drawn_solid() {
        let mut p = BezPath::new();
        p.move_to((0.0, 0.0));
        p.line_to((1_000_000.0, 0.0));
        let culled = cull_stroke_input(
            &p,
            win(Rect::new(-1.0, -1.0, 1_000_001.0, 1.0)),
            &[1.0],
            0.0,
            4.0,
        );
        assert!(!culled.dashed);
        assert_eq!(culled.path, p);
        // Few dashes, but each of them a round cap hundreds of segments
        // long: the same budget.
        let wide = dash_cost(100_000.0, 0.1);
        let culled = cull_stroke_input(
            &p,
            win(Rect::new(-1.0, -1.0, 1_000_001.0, 1.0)),
            &[1_000.0],
            0.0,
            wide,
        );
        assert!(!culled.dashed);
    }

    #[test]
    fn a_degenerate_transform_still_culls() {
        // x collapses onto one device column; only y decides.
        let mut p = BezPath::new();
        p.move_to((0.0, -1_000_000.0));
        p.line_to((0.0, 1_000_000.0));
        let keep = Window {
            to_dev: Affine::new([0.0, 0.0, 0.0, 0.5, 5.0, 0.0]),
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
        };
        let culled = cull_stroke_input(&p, keep, &[], 0.0, 1.0);
        let els = culled.path.elements();
        assert_eq!(els.len(), 2);
        let (PathEl::MoveTo(a), PathEl::LineTo(b)) = (els[0], els[1]) else {
            unreachable!("one clipped line");
        };
        assert!(
            (a.y - 0.0).abs() < 1e-6 && (b.y - 20.0).abs() < 1e-6,
            "{a:?} {b:?}"
        );
    }
}
