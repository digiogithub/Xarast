//! Stroke expansion, dashing and offsetting.

use crate::{Mp, Path, Point, Segment, Tolerance};

/// How a stroke terminates at an open end. The discriminants match `.xar`.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[repr(u8)]
pub enum Cap {
    /// Stops flat at the endpoint.
    #[default]
    Butt = 1,
    /// A half-disc centred on the endpoint.
    Round = 2,
    /// A half-square extending half the width past the endpoint.
    Square = 3,
}

/// How a stroke turns a corner. The discriminants match `.xar`.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[repr(u8)]
pub enum Join {
    /// Extends both edges to their intersection, falling back to a bevel
    /// past the mitre limit.
    #[default]
    Mitre = 1,
    /// An arc centred on the corner.
    Round = 2,
    /// A straight cut across the corner.
    Bevel = 3,
}

/// Which regions a path encloses. The discriminants match `.xar`'s
/// `TAG_WINDINGRULE`.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[repr(u8)]
pub enum FillRule {
    /// Filled where the winding number is not zero.
    #[default]
    NonZero = 1,
    /// Filled where the winding number is negative.
    Negative = 2,
    /// Filled where the winding number is odd.
    EvenOdd = 3,
    /// Filled where the winding number is positive.
    Positive = 4,
}

impl FillRule {
    /// Whether a winding number counts as inside under this rule.
    #[inline]
    #[must_use]
    pub const fn covers(self, winding: i32) -> bool {
        match self {
            FillRule::NonZero => winding != 0,
            FillRule::Negative => winding < 0,
            FillRule::EvenOdd => winding % 2 != 0,
            FillRule::Positive => winding > 0,
        }
    }

    /// Reads the `.xar` `TAG_WINDINGRULE` byte. Unknown values become
    /// `NonZero`, which is the format's own default and the least surprising
    /// rendering of a corrupt file.
    #[inline]
    #[must_use]
    pub const fn from_byte(v: u8) -> FillRule {
        match v {
            2 => FillRule::Negative,
            3 => FillRule::EvenOdd,
            4 => FillRule::Positive,
            _ => FillRule::NonZero,
        }
    }
}

/// A dash pattern: alternating on and off lengths.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct DashPattern {
    /// Alternating on/off lengths in millipoints, starting with "on".
    pub elements: Vec<Mp>,
    /// How far into the pattern the first subpath starts.
    pub offset: Mp,
    /// The line width the pattern was authored at, when it scales with the
    /// width (`TAG_DEFINEDASH_SCALED`). `None` means fixed lengths.
    pub reference_width: Option<Mp>,
}

impl DashPattern {
    /// The pattern lengths in millipoints, scaled for the given line width
    /// when the pattern is a scaled one.
    ///
    /// Returns an empty vector when the pattern would produce nothing —
    /// no elements, or every element zero — so that callers can treat "no
    /// dashing" as one case instead of guarding each.
    #[must_use]
    pub fn resolved(&self, line_width: Mp) -> Vec<f64> {
        if self.elements.is_empty() || self.elements.iter().all(|e| e.raw() <= 0) {
            return Vec::new();
        }
        let factor = match self.reference_width {
            Some(rw) if rw.raw() > 0 => line_width.to_f64() / rw.to_f64(),
            _ => 1.0,
        };
        if !factor.is_finite() || factor <= 0.0 {
            return Vec::new();
        }
        self.elements
            .iter()
            .map(|e| (e.to_f64() * factor).max(0.0))
            .collect()
    }
}

/// Everything needed to turn a path into its stroke outline.
#[derive(Clone, PartialEq, Debug)]
pub struct StrokeStyle {
    /// Stroke width in millipoints. [`Mp::ZERO`] means a **hairline**: one
    /// device pixel regardless of zoom, which has no document-space outline.
    pub width: Mp,
    /// Cap at the start of each open subpath.
    pub cap_start: Cap,
    /// Cap at the end of each open subpath.
    ///
    /// Separate from `cap_start` because `.xar` stores `TAG_STARTCAP` and
    /// `TAG_ENDCAP` separately and can therefore express a mismatched pair.
    pub cap_end: Cap,
    /// Join at every interior corner.
    pub join: Join,
    /// How far a mitre may extend, as a multiple of the width, before it
    /// degenerates to a bevel.
    pub mitre_limit: f64,
    /// The dash pattern, if any.
    pub dash: Option<DashPattern>,
}

impl Default for StrokeStyle {
    fn default() -> StrokeStyle {
        StrokeStyle {
            width: Mp::ONE_PT,
            cap_start: Cap::Butt,
            cap_end: Cap::Butt,
            join: Join::Mitre,
            mitre_limit: 4.0,
            dash: None,
        }
    }
}

/// Why a stroke has no outline.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StrokeError {
    /// The width is zero, meaning a hairline.
    #[error("hairline strokes have no document-space outline; the renderer must handle them")]
    Hairline,
    /// The parameters cannot produce an outline.
    #[error("degenerate stroke parameters: {0}")]
    Degenerate(&'static str),
}

/// Expands a stroke into the filled path that draws it.
///
/// # Hairlines
///
/// A width of zero means a hairline — one device pixel wide at any zoom — and
/// that is not expressible as a path in document space at all. This returns
/// [`StrokeError::Hairline`] rather than inventing a width, so the case
/// cannot be ignored by accident; the renderer handles hairlines as a
/// distinct primitive.
pub fn stroke_to_path(
    path: &Path,
    style: &StrokeStyle,
    tol: Tolerance,
) -> Result<Path, StrokeError> {
    if style.width == Mp::ZERO {
        return Err(StrokeError::Hairline);
    }
    if style.width.raw() < 0 {
        return Err(StrokeError::Degenerate("negative stroke width"));
    }
    if !style.mitre_limit.is_finite() || style.mitre_limit < 1.0 {
        return Err(StrokeError::Degenerate(
            "mitre limit below 1.0 or not finite",
        ));
    }
    if path.is_empty() {
        return Ok(Path::new());
    }

    // `kurbo::Stroke` carries a start cap and an end cap of its own and
    // applies each to its own end of every open subpath (and of every dash),
    // which is exactly `.xar`'s `TAG_STARTCAP`/`TAG_ENDCAP` pair. One pass is
    // therefore enough even when the two differ. (An earlier version stroked
    // twice with the caps swapped and unioned the results, which gave *both*
    // ends the union of the two caps.)
    let bez = path.to_bez_path();
    let opts = kurbo::StrokeOpts::default();
    let base = |start: Cap, end: Cap| {
        let mut s = kurbo::Stroke::new(style.width.to_f64())
            .with_join(to_kurbo_join(style.join))
            .with_miter_limit(style.mitre_limit)
            .with_start_cap(to_kurbo_cap(start))
            .with_end_cap(to_kurbo_cap(end));
        if let Some(d) = &style.dash {
            let elements = d.resolved(style.width);
            if !elements.is_empty() {
                s = s.with_dashes(reduced_dash_offset(d.offset.to_f64(), &elements), elements);
            }
        }
        s
    };

    let out = kurbo::stroke(
        bez.iter(),
        &base(style.cap_start, style.cap_end),
        &opts,
        tol.get(),
    );
    Ok(Path::from_bez_path(&out).0)
}

/// Reduces a dash offset modulo the pattern's period.
///
/// kurbo walks the offset through the pattern one element at a time, so an
/// offset many periods long costs time proportional to its size; the
/// result is the same phase either way. Elements alternate on and off, so an
/// odd-length pattern only repeats after two passes.
///
/// `elements` are the resolved lengths ([`DashPattern::resolved`]). Every
/// stroker that honours [`DashPattern::offset`] goes through this, so the
/// renderer and [`stroke_to_path`] start the pattern at the same phase.
#[must_use]
pub fn reduced_dash_offset(offset: f64, elements: &[f64]) -> f64 {
    let pass: f64 = elements.iter().sum();
    let period = if elements.len().is_multiple_of(2) {
        pass
    } else {
        2.0 * pass
    };
    if period > 0.0 && period.is_finite() && offset.is_finite() {
        offset.rem_euclid(period)
    } else {
        offset
    }
}

/// Splits a path into its dashes, returning an open path per dash.
///
/// The result is not a stroke outline: it is the same centreline cut into
/// pieces, which is what the renderer and the arrowhead placement both want.
#[must_use]
pub fn dash(path: &Path, pattern: &DashPattern, line_width: Mp, tol: Tolerance) -> Path {
    let elements = pattern.resolved(line_width);
    if elements.is_empty() || path.is_empty() {
        return path.clone();
    }
    let _ = tol;
    let bez = path.to_bez_path();
    let out: kurbo::BezPath = kurbo::dash(
        bez.iter(),
        reduced_dash_offset(pattern.offset.to_f64(), &elements),
        &elements,
    )
    .collect();
    Path::from_bez_path(&out).0
}

/// Offsets a path outwards (positive `distance`) or inwards (negative).
///
/// Offsetting a concave region always produces self-intersecting loops, so
/// this offsets each segment, rebuilds the joins, and then runs the result
/// through [`self_union`](crate::self_union) to delete them. That dependency
/// on the boolean engine is intrinsic to the operation, not an implementation
/// shortcut.
///
/// Each subpath is treated as closed, because an offset of an open path is
/// only well defined as one side of a stroke, which is what
/// [`stroke_to_path`] is for.
#[must_use]
pub fn offset(path: &Path, distance: Mp, join: Join, tol: Tolerance) -> Path {
    if distance == Mp::ZERO || path.is_empty() {
        return path.clone();
    }
    let d = distance.to_f64();
    let t = tol.get();
    let mut b = Path::builder();

    for sp in path.subpaths() {
        let segs = path.subpath_segments(&sp);
        if segs.is_empty() {
            continue;
        }
        // Offset each segment independently, in `f64`.
        let mut pieces: Vec<kurbo::BezPath> = Vec::with_capacity(segs.len());
        for seg in &segs {
            pieces.push(offset_segment(*seg, d, t));
        }
        let mut started = false;
        for (i, piece) in pieces.iter().enumerate() {
            let Some(kurbo::PathEl::MoveTo(first)) = piece.elements().first().copied() else {
                continue;
            };
            if !started {
                b.move_to(Point::from_kurbo(first));
                started = true;
            } else {
                // Rebuild the join between the previous offset segment's end
                // and this one's start.
                let prev_end = pieces[..i]
                    .iter()
                    .rev()
                    .find_map(|p| p.elements().last().and_then(el_end));
                if let Some(prev) = prev_end {
                    append_join(&mut b, prev, first, segs[i].start().to_kurbo(), d, join, t);
                }
            }
            for el in piece.elements().iter().skip(1) {
                append_el(&mut b, *el);
            }
        }
        if started {
            b.close();
        }
    }

    let raw = b.build();
    crate::self_union(&raw, FillRule::NonZero, tol)
}

/// Offsets a single segment by `d`, in `f64`.
fn offset_segment(seg: Segment, d: f64, tol: f64) -> kurbo::BezPath {
    match seg {
        Segment::Line { p0, p1 } => {
            let (a, b) = (p0.to_kurbo(), p1.to_kurbo());
            let v = b - a;
            let len = v.hypot();
            if len == 0.0 {
                return kurbo::BezPath::new();
            }
            // The outward normal in a Y-up frame, for a counter-clockwise
            // outline, is the tangent rotated a quarter turn clockwise.
            let n = kurbo::Vec2::new(v.y, -v.x) / len * d;
            let mut p = kurbo::BezPath::new();
            p.move_to(a + n);
            p.line_to(b + n);
            p
        }
        Segment::Cubic { p0, p1, p2, p3 } => {
            let c =
                kurbo::CubicBez::new(p0.to_kurbo(), p1.to_kurbo(), p2.to_kurbo(), p3.to_kurbo());
            let mut out = kurbo::BezPath::new();
            kurbo::offset::offset_cubic(c, d, tol, &mut out);
            out
        }
    }
}

/// The endpoint of a path element, if it has one.
fn el_end(el: &kurbo::PathEl) -> Option<kurbo::Point> {
    match *el {
        kurbo::PathEl::MoveTo(p) | kurbo::PathEl::LineTo(p) => Some(p),
        kurbo::PathEl::QuadTo(_, p) | kurbo::PathEl::CurveTo(_, _, p) => Some(p),
        kurbo::PathEl::ClosePath => None,
    }
}

/// Appends a `kurbo` element to our builder.
fn append_el(b: &mut crate::PathBuilder, el: kurbo::PathEl) {
    match el {
        kurbo::PathEl::MoveTo(p) | kurbo::PathEl::LineTo(p) => {
            b.line_to(Point::from_kurbo(p));
        }
        kurbo::PathEl::QuadTo(c, p) => {
            b.quad_to(Point::from_kurbo(c), Point::from_kurbo(p));
        }
        kurbo::PathEl::CurveTo(c1, c2, p) => {
            b.cubic_to(
                Point::from_kurbo(c1),
                Point::from_kurbo(c2),
                Point::from_kurbo(p),
            );
        }
        kurbo::PathEl::ClosePath => {}
    };
}

/// Bridges the gap between two consecutive offset segments.
fn append_join(
    b: &mut crate::PathBuilder,
    from: kurbo::Point,
    to: kurbo::Point,
    corner: kurbo::Point,
    d: f64,
    join: Join,
    tol: f64,
) {
    if from == to {
        return;
    }
    match join {
        Join::Bevel | Join::Mitre => {
            // A mitre that is longer than the offset distance is exactly the
            // case the mitre limit exists to reject, and at a join between
            // two independently offset pieces the bevel is always valid; the
            // subsequent self-union removes any resulting notch.
            b.line_to(Point::from_kurbo(to));
        }
        Join::Round => {
            let r = d.abs();
            if r == 0.0 {
                b.line_to(Point::from_kurbo(to));
                return;
            }
            let a0 = (from - corner).atan2();
            let a1 = (to - corner).atan2();
            let mut sweep = a1 - a0;
            while sweep > std::f64::consts::PI {
                sweep -= std::f64::consts::TAU;
            }
            while sweep < -std::f64::consts::PI {
                sweep += std::f64::consts::TAU;
            }
            let arc = kurbo::Arc::new((corner.x, corner.y), (r, r), a0, sweep, 0.0);
            arc.to_cubic_beziers(tol, |c1, c2, p| {
                b.cubic_to(
                    Point::from_kurbo(c1),
                    Point::from_kurbo(c2),
                    Point::from_kurbo(p),
                );
            });
        }
    }
}

/// Maps our cap to `kurbo`'s.
const fn to_kurbo_cap(c: Cap) -> kurbo::Cap {
    match c {
        Cap::Butt => kurbo::Cap::Butt,
        Cap::Round => kurbo::Cap::Round,
        Cap::Square => kurbo::Cap::Square,
    }
}

/// Maps our join to `kurbo`'s.
const fn to_kurbo_join(j: Join) -> kurbo::Join {
    match j {
        Join::Mitre => kurbo::Join::Miter,
        Join::Round => kurbo::Join::Round,
        Join::Bevel => kurbo::Join::Bevel,
    }
}

#[cfg(test)]
mod dash_offset_tests {
    use super::reduced_dash_offset;

    #[test]
    fn an_even_pattern_reduces_by_one_pass() {
        assert_eq!(reduced_dash_offset(35.0, &[10.0, 5.0]), 5.0);
        assert_eq!(reduced_dash_offset(-5.0, &[10.0, 5.0]), 10.0);
    }

    #[test]
    fn an_odd_pattern_reduces_by_two_passes() {
        // [10 on, 5 off, 5 on] then [10 off, 5 on, 5 off]: period 40.
        assert_eq!(reduced_dash_offset(45.0, &[10.0, 5.0, 5.0]), 5.0);
        assert_eq!(reduced_dash_offset(25.0, &[10.0, 5.0, 5.0]), 25.0);
    }

    #[test]
    fn a_huge_offset_is_cheap_and_finite() {
        let r = reduced_dash_offset(1.0e15, &[3.0, 4.0]);
        assert!((0.0..7.0).contains(&r));
    }
}
