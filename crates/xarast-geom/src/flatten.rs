//! Adaptive flattening, with traceability back to the source segments.

use crate::{Path, Point, Segment};

/// A flattening tolerance, in **document units** — `f64` millipoints.
///
/// Never pixels: a [`Path`] does not know the zoom, and a tolerance that
/// silently meant "pixels" would produce different geometry at different zoom
/// levels for operations, like booleans, whose result is stored in the
/// document. Callers that start from a device-pixel budget go through
/// [`Tolerance::from_device_px`].
#[derive(Copy, Clone, PartialEq, PartialOrd, Debug)]
pub struct Tolerance(pub f64);

impl Default for Tolerance {
    fn default() -> Tolerance {
        Tolerance::RENDER_DEFAULT
    }
}

impl Tolerance {
    /// 1 mp: for cases where the flattened result is the deliverable rather
    /// than an intermediate.
    pub const EXPORT: Tolerance = Tolerance(1.0);

    /// The input tolerance for the boolean pipeline, 10 mp (0.01 pt, 3.5 um).
    ///
    /// Chosen as the largest value whose worst-case area error over the
    /// regression corpus stays below `1e-4` of the input area; see
    /// `docs/memory/geometry.md` for the sweep. Tighter buys no accuracy that
    /// survives quantisation back to integer millipoints; looser starts to
    /// erode small features.
    pub const BOOLEAN: Tolerance = Tolerance(10.0);

    /// The device-pixel budget rendering uses. Below about a quarter of a
    /// pixel, more subdivision buys no visible quality and costs time in
    /// proportion.
    pub const RENDER_DEVICE_PX: f64 = 0.25;

    /// A safe default for a 100 % zoom at 96 dpi: a quarter pixel is 187.5 mp.
    pub const RENDER_DEFAULT: Tolerance = Tolerance(Tolerance::RENDER_DEVICE_PX * 750.0);

    /// Converts a device-pixel budget into document units.
    ///
    /// `doc_units_per_px` is the document-space length of one device pixel,
    /// which for an anisotropic transform should be
    /// [`Matrix::max_scale`](crate::Matrix::max_scale)'s reciprocal so that
    /// the budget holds in the worst direction.
    #[must_use]
    pub fn from_device_px(px: f64, doc_units_per_px: f64) -> Tolerance {
        let t = px * doc_units_per_px;
        // A non-positive or non-finite tolerance would make `kurbo`'s
        // subdivision loop forever, so it is clamped rather than trusted.
        Tolerance(if t.is_finite() && t > 0.0 { t } else { Tolerance::EXPORT.0 })
    }

    /// The tolerance value, guaranteed finite and positive.
    #[inline]
    #[must_use]
    pub fn get(self) -> f64 {
        if self.0.is_finite() && self.0 > 0.0 { self.0 } else { Tolerance::EXPORT.0 }
    }
}

/// One flattened subpath.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Polyline {
    /// The vertices, starting at the subpath's `MoveTo` point. A closed
    /// polyline does **not** repeat its first point at the end.
    pub points: Vec<Point>,
    /// Whether the subpath was closed.
    pub closed: bool,
}

/// Where one flattened vertex came from.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct VertexSource {
    /// Index of the source segment within its subpath, as
    /// [`Path::indexed_segments`] numbers them. `u32::MAX` marks the
    /// subpath's initial `MoveTo` vertex, which belongs to no segment.
    pub segment: u32,
    /// Whether this vertex is an original on-curve point of the source path
    /// rather than a subdivision point invented by the flattener.
    pub original: bool,
}

/// The map from flattened vertices back to the segments that produced them.
///
/// This is what makes the boolean refit step possible at all. Without it,
/// refitting cannot tell "this whole output run came from one untouched input
/// cubic, so restore the cubic" from "this run is a mixture, so fit it", and
/// repeated boolean operations degrade every curve in the document into a
/// polyline — a failure mode that is slow, cumulative and effectively
/// irreversible for the user.
#[derive(Clone, Debug, Default)]
pub struct SegmentTrace {
    /// One entry per polyline, each parallel to that polyline's `points`.
    sources: Vec<Vec<VertexSource>>,
}

impl SegmentTrace {
    /// The `(subpath, segment)` a vertex came from, or `None` when the index
    /// is out of range or the vertex is a subpath's initial `MoveTo`.
    #[must_use]
    pub fn source_of(&self, polyline: usize, vertex: usize) -> Option<(usize, usize)> {
        let s = self.sources.get(polyline)?.get(vertex)?;
        if s.segment == u32::MAX { None } else { Some((polyline, s.segment as usize)) }
    }

    /// Whether a vertex is an original on-curve point rather than one the
    /// flattener invented.
    #[must_use]
    pub fn is_original_vertex(&self, polyline: usize, vertex: usize) -> bool {
        self.sources
            .get(polyline)
            .and_then(|v| v.get(vertex))
            .is_some_and(|s| s.original)
    }

    /// The raw source records of one polyline.
    #[must_use]
    pub fn polyline_sources(&self, polyline: usize) -> Option<&[VertexSource]> {
        self.sources.get(polyline).map(Vec::as_slice)
    }

    /// The number of polylines this trace covers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sources.len()
    }

    /// Whether the trace covers no polylines.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }
}

/// Flattens every subpath into a polyline whose maximum deviation from the
/// curve is at most `tol`.
///
/// Subdivision is `kurbo`'s adaptive scheme, which places vertices where the
/// curvature needs them instead of uniformly in the parameter, so a long
/// gentle curve costs a handful of vertices and a cusp costs many.
#[must_use]
pub fn flatten(path: &Path, tol: Tolerance) -> Vec<Polyline> {
    flatten_traced(path, tol).0
}

/// Flattens, and records where each output vertex came from.
///
/// Costs one extra `u64` per output vertex over [`flatten`], which is why the
/// untraced form exists for rendering, where nothing reads the trace.
#[must_use]
pub fn flatten_traced(path: &Path, tol: Tolerance) -> (Vec<Polyline>, SegmentTrace) {
    let tol = tol.get();
    let mut polys: Vec<Polyline> = Vec::new();
    let mut sources: Vec<Vec<VertexSource>> = Vec::new();

    for sp in path.subpaths() {
        let segs = path.subpath_segments(&sp);
        let start = match segs.first() {
            Some(s) => s.start(),
            None => match path.subpath_start(&sp) {
                Some(p) => p,
                None => continue,
            },
        };
        let mut pts = vec![start];
        let mut src = vec![VertexSource { segment: u32::MAX, original: true }];

        for (i, seg) in segs.iter().enumerate() {
            let si = u32::try_from(i).unwrap_or(u32::MAX - 1);
            match *seg {
                Segment::Line { p1, .. } => {
                    pts.push(p1);
                    src.push(VertexSource { segment: si, original: true });
                }
                Segment::Cubic { p0, p1, p2, p3 } => {
                    let before = pts.len();
                    kurbo::flatten(
                        [
                            kurbo::PathEl::MoveTo(p0.to_kurbo()),
                            kurbo::PathEl::CurveTo(p1.to_kurbo(), p2.to_kurbo(), p3.to_kurbo()),
                        ],
                        tol,
                        |el| {
                            if let kurbo::PathEl::LineTo(p) = el {
                                pts.push(Point::from_kurbo(p));
                            }
                        },
                    );
                    if pts.len() == before {
                        // A fully degenerate cubic emits nothing; keep the
                        // endpoint so the polyline stays connected.
                        pts.push(p3);
                    } else {
                        // `kurbo` lands on the true endpoint in `f64`, but
                        // quantising it can move it by half a millipoint.
                        // Forcing the exact endpoint is what makes the trace
                        // usable: the refit step asks whether the run ends on
                        // the source segment's own endpoint.
                        let last = pts.len() - 1;
                        pts[last] = p3;
                    }
                    for _ in before..pts.len() {
                        src.push(VertexSource { segment: si, original: false });
                    }
                    let last = src.len() - 1;
                    src[last].original = true;
                }
            }
        }

        // A closed polyline must not repeat its start vertex: the consumers
        // (i_overlay contours, winding, area) all close implicitly, and a
        // duplicated vertex shows up as a zero-length edge in all of them.
        if sp.closed && pts.len() > 1 && pts[pts.len() - 1] == pts[0] {
            pts.pop();
            src.pop();
        }

        polys.push(Polyline { points: pts, closed: sp.closed });
        sources.push(src);
    }

    (polys, SegmentTrace { sources })
}

/// The greatest distance from any of `samples` points on the curve to the
/// polyline that was flattened from it.
///
/// Exposed because it is the only honest way to test a tolerance claim, and
/// the boolean and stroke tests need it as much as the flattening ones do.
#[must_use]
pub fn max_deviation(seg: Segment, polyline: &[Point], samples: usize) -> f64 {
    if polyline.len() < 2 || samples == 0 {
        return 0.0;
    }
    let k = seg.to_kurbo();
    let mut worst: f64 = 0.0;
    for i in 0..=samples {
        let t = i as f64 / samples as f64;
        let p = kurbo::ParamCurve::eval(&k, t);
        let mut best = f64::INFINITY;
        for w in polyline.windows(2) {
            let line = kurbo::Line::new(w[0].to_kurbo(), w[1].to_kurbo());
            let n = kurbo::ParamCurveNearest::nearest(&line, p, 1e-9);
            best = best.min(n.distance_sq);
        }
        worst = worst.max(best.sqrt());
    }
    worst
}
