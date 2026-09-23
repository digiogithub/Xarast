//! Gradients as PDF shadings (T11.4.4).
//!
//! Every gradient is drawn in its own `(u, v)` frame: the caller clips to
//! the shape, then [`paint_gradient`] concatenates the frame's affine map
//! (`cm`) and paints with `sh`. Inside the frame the shapes are simple:
//!
//! | Shape | PDF construct | Exact? |
//! |---|---|---|
//! | Linear | axial shading (type 2) along `u` | yes |
//! | Radial (elliptical) | radial shading (type 3), circles `r = 0 → 1` | yes |
//! | Diamond | two axial shadings of `|u|` and `|v|`, the second clipped to the quadrants where `|v| > |u|` | yes |
//! | Conical | a Gouraud triangle fan (type 4), [`CONICAL_WEDGES`] wedges | approximated |
//! | Four-colour mesh | function-based shading (type 1) of a 2 × 2 sampled function | yes: PDF's multilinear interpolation is Xara's bilinear blend, and the function clamps its input as the renderer clamps `(u, v)` |
//! | Three-colour mesh | function-based shading of a [`MESH3_GRID`]² sampled function | approximated |
//!
//! The ramp — stops, profile and effect space already baked into the
//! renderer's 256- or 2048-entry table — becomes a sampled function
//! (type 0), so a gradient's profile is carried exactly rather than
//! re-derived from its stops. Repeating gradients widen the function's
//! domain to cover the shape and fold the parameter while sampling.
//!
//! Perspective mappings, and ramps or corners with transparency, are not
//! handled here: the caller rasterises them (the fidelity ladder's last
//! step; transparency is T11.4.5).

use kurbo::{Affine, BezPath, Point};
use xarast_color::Rgba8;
use xarast_render::paint::{apply_repeat, ramp_index};
use xarast_render::{GradMapping, GradRamp, GradShape, RampCache, Repeat};

use super::writer::{Canvas, PdfWriter, Rule, Vertex};

/// Wedges in a conical gradient's fan: 1.4° each.
pub const CONICAL_WEDGES: usize = 256;

/// Samples a side in a three-colour mesh's grid.
pub const MESH3_GRID: u32 = 33;

/// The most repetitions of a repeating gradient drawn as a shading; more
/// is rasterised.
pub const MAX_PERIODS: f64 = 4096.0;

/// The most samples in one ramp function.
const MAX_SAMPLES: usize = 1 << 16;

/// The outcome of painting a gradient.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shaded {
    /// Painted exactly, or nothing to paint.
    Exact,
    /// Painted, with a close construct: what it was.
    Approximated(&'static str),
    /// Not painted: the caller must rasterise, for this reason.
    Rasterise(String),
}

/// A gradient's frame: the affine map from `(u, v)` to page space and its
/// inverse.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    to_page: Affine,
    from_page: Affine,
}

impl Frame {
    /// The frame of an affine mapping already in page space, or `None`
    /// when it is degenerate (the renderer paints nothing then either).
    #[must_use]
    pub fn of(mapping: GradMapping) -> Option<Frame> {
        let GradMapping::Affine { a, b, c } = mapping else {
            return None;
        };
        let m = Affine::new([c.x - a.x, c.y - a.y, b.x - a.x, b.y - a.y, a.x, a.y]);
        let det = m.determinant();
        if !det.is_finite() || det.abs() < 1e-12 {
            return None;
        }
        Some(Frame {
            to_page: m,
            from_page: m.inverse(),
        })
    }

    /// The `(u, v)` coordinates of a page rectangle's corners.
    fn corners(&self, r: kurbo::Rect) -> [Point; 4] {
        [
            self.from_page * Point::new(r.x0, r.y0),
            self.from_page * Point::new(r.x1, r.y0),
            self.from_page * Point::new(r.x0, r.y1),
            self.from_page * Point::new(r.x1, r.y1),
        ]
    }
}

/// How many samples a ramp function over `span` parameter units takes.
fn sample_count(span: f64, len: usize) -> usize {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = (span.ceil().max(1.0) as usize).saturating_mul(len.max(2) - 1) + 1;
    n.clamp(2, MAX_SAMPLES)
}

/// A ramp function over `[lo, hi]`: `f(t) = table[repeat(g(t))]`.
fn ramp_function(
    w: &mut PdfWriter,
    table: &[Rgba8],
    repeat: Repeat,
    lo: f64,
    hi: f64,
    g: impl Fn(f64) -> f64,
) -> pdf_writer::Ref {
    let n = sample_count(hi - lo, table.len());
    let mut samples = Vec::with_capacity(n * 3);
    #[allow(clippy::cast_precision_loss)]
    for i in 0..n {
        let t = lo + (hi - lo) * (i as f64) / ((n - 1) as f64);
        let c = table[ramp_index(apply_repeat(g(t), repeat), table.len())];
        samples.extend_from_slice(&[c.r, c.g, c.b]);
    }
    w.sampled_rgb_function(&[lo, hi], &[u32::try_from(n).unwrap_or(u32::MAX)], &samples)
}

/// The parameter range a repeating gradient needs, folded to whole
/// periods, or a reason to rasterise.
fn periods(lo: f64, hi: f64) -> Result<(f64, f64), String> {
    let (lo, mut hi) = (lo.floor(), hi.ceil());
    if hi <= lo {
        hi = lo + 1.0;
    }
    if !(lo.is_finite() && hi.is_finite()) || hi - lo > MAX_PERIODS {
        return Err(format!(
            "a repeating gradient spans more than {MAX_PERIODS} periods"
        ));
    }
    Ok((lo, hi))
}

/// Paints a gradient over the current clip, which the caller has set to
/// the shape; `bbox` bounds that shape in page space. Leaves the canvas's
/// transform changed: call inside `q`/`Q`.
#[allow(clippy::too_many_lines)]
#[allow(clippy::too_many_arguments)]
pub fn paint_gradient(
    w: &mut PdfWriter,
    c: &mut Canvas,
    shape: GradShape,
    mapping: GradMapping,
    repeat: Repeat,
    ramp: &GradRamp,
    ramps: &RampCache,
    bbox: kurbo::Rect,
) -> Shaded {
    if matches!(mapping, GradMapping::Perspective { .. }) {
        return Shaded::Rasterise("gradient with a perspective mapping".into());
    }
    let Some(frame) = Frame::of(mapping) else {
        // Degenerate: the renderer draws nothing.
        return Shaded::Exact;
    };
    let corners = frame.corners(bbox);
    let fold = |f: fn(Point) -> f64| {
        corners
            .iter()
            .map(|p| f(*p))
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| {
                (a.min(v), b.max(v))
            })
    };
    match (shape, ramp) {
        (GradShape::Mesh3 | GradShape::Mesh4, GradRamp::Mesh3(_) | GradRamp::Mesh4(_)) => {
            mesh(w, c, &frame, ramp, fold(|p| p.x), fold(|p| p.y))
        }
        (_, GradRamp::Table(id)) => {
            let Some(table) = ramps.try_get(*id) else {
                return Shaded::Exact;
            };
            if table.is_empty() {
                return Shaded::Exact;
            }
            if table.iter().any(|c| c.a < 255) {
                return Shaded::Rasterise("gradient colours with transparency".into());
            }
            table_gradient(w, c, shape, &frame, repeat, table, &corners)
        }
        // A shape given the wrong kind of ramp: the renderer refuses it.
        _ => Shaded::Exact,
    }
}

/// Paints a graduated transparency's opacity as grey — white opaque,
/// black transparent — over the current clip: the content of a
/// luminosity soft mask (T11.4.5). `levels` is the transparency ramp
/// (0 opaque).
#[allow(clippy::too_many_arguments)]
pub fn paint_opacity(
    w: &mut PdfWriter,
    c: &mut Canvas,
    shape: GradShape,
    mapping: GradMapping,
    repeat: Repeat,
    levels: &[u8],
    bbox: kurbo::Rect,
) -> Shaded {
    if matches!(mapping, GradMapping::Perspective { .. }) {
        return Shaded::Rasterise("graduated transparency with a perspective mapping".into());
    }
    if !shape.is_scalar() {
        return Shaded::Rasterise("mesh-shaped graduated transparency".into());
    }
    let (Some(frame), false) = (Frame::of(mapping), levels.is_empty()) else {
        // The renderer reads a degenerate transparency as opaque.
        c.fill_rgb([255, 255, 255]);
        c.rect(bbox.x0, bbox.y0, bbox.x1, bbox.y1);
        c.fill(Rule::NonZero);
        return Shaded::Exact;
    };
    let table: Vec<Rgba8> = levels
        .iter()
        .map(|&t| Rgba8::rgb(255 - t, 255 - t, 255 - t))
        .collect();
    let corners = frame.corners(bbox);
    table_gradient(w, c, shape, &frame, repeat, &table, &corners)
}

/// A scalar gradient from an opaque ramp table.
#[allow(clippy::too_many_lines)]
fn table_gradient(
    w: &mut PdfWriter,
    c: &mut Canvas,
    shape: GradShape,
    frame: &Frame,
    repeat: Repeat,
    table: &[Rgba8],
    corners: &[Point; 4],
) -> Shaded {
    let fold = |f: fn(Point) -> f64| {
        corners
            .iter()
            .map(|p| f(*p))
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| {
                (a.min(v), b.max(v))
            })
    };
    let simple = repeat == Repeat::Simple;
    match shape {
        GradShape::Linear => {
            let (lo, hi) = if simple {
                (0.0, 1.0)
            } else {
                match periods(fold(|p| p.x).0, fold(|p| p.x).1) {
                    Ok(r) => r,
                    Err(e) => return Shaded::Rasterise(e),
                }
            };
            let f = ramp_function(w, table, repeat, lo, hi, |t| t);
            let sh = w.axial(f, [lo, 0.0, hi, 0.0], [lo, hi], [simple, simple]);
            c.transform(frame.to_page.as_coeffs());
            c.shading(sh);
            Shaded::Exact
        }
        GradShape::Radial => {
            let r = corners
                .iter()
                .map(|p| p.to_vec2().hypot())
                .fold(0.0, f64::max);
            let hi = if simple {
                1.0
            } else {
                match periods(0.0, r.max(1.0)) {
                    Ok((_, h)) => h,
                    Err(e) => return Shaded::Rasterise(e),
                }
            };
            let f = ramp_function(w, table, repeat, 0.0, hi, |t| t);
            let sh = w.radial(f, [0.0, 0.0, 0.0, 0.0, 0.0, hi], [0.0, hi], [true, true]);
            c.transform(frame.to_page.as_coeffs());
            c.shading(sh);
            Shaded::Exact
        }
        GradShape::Diamond => {
            let m = corners
                .iter()
                .map(|p| p.x.abs().max(p.y.abs()))
                .fold(0.0, f64::max);
            let d = if simple {
                1.0
            } else {
                match periods(0.0, m.max(1.0)) {
                    Ok((_, h)) => h,
                    Err(e) => return Shaded::Rasterise(e),
                }
            };
            let f = ramp_function(w, table, repeat, -d, d, f64::abs);
            let across = w.axial(f, [-d, 0.0, d, 0.0], [-d, d], [true, true]);
            let along = w.axial(f, [0.0, -d, 0.0, d], [-d, d], [true, true]);
            c.transform(frame.to_page.as_coeffs());
            // |u| everywhere, then |v| where it is the larger.
            c.shading(across);
            let l = m.max(d) + 1.0;
            let mut bow = BezPath::new();
            bow.move_to((0.0, 0.0));
            bow.line_to((-l, l));
            bow.line_to((l, l));
            bow.close_path();
            bow.move_to((0.0, 0.0));
            bow.line_to((l, -l));
            bow.line_to((-l, -l));
            bow.close_path();
            c.path(&bow);
            c.clip(Rule::NonZero);
            c.shading(along);
            Shaded::Exact
        }
        GradShape::Conical => {
            let r = corners
                .iter()
                .map(|p| p.to_vec2().hypot())
                .fold(0.0, f64::max);
            let r = r * 1.02 + 1.0;
            let colour = |s: f64| {
                let c = table[ramp_index(apply_repeat(s, repeat), table.len())];
                [c.r, c.g, c.b]
            };
            #[allow(clippy::cast_precision_loss)]
            let n = CONICAL_WEDGES as f64;
            let mut tris: Vec<[Vertex; 3]> = Vec::with_capacity(CONICAL_WEDGES);
            for k in 0..CONICAL_WEDGES {
                #[allow(clippy::cast_precision_loss)]
                let (s0, s1) = (k as f64 / n, (k + 1) as f64 / n);
                // The last rim vertex sits just before the seam,
                // where the parameter approaches one from below.
                let s1c = if k + 1 == CONICAL_WEDGES {
                    1.0 - 1e-9
                } else {
                    s1
                };
                let at = |s: f64| {
                    let a = s * std::f64::consts::TAU;
                    Point::new(r * a.cos(), r * a.sin())
                };
                tris.push([
                    (Point::ZERO, colour((s0 + s1c) * 0.5)),
                    (at(s0), colour(s0)),
                    (at(s1), colour(s1c)),
                ]);
            }
            let sh = w.gouraud(&tris);
            c.transform(frame.to_page.as_coeffs());
            c.shading(sh);
            Shaded::Approximated("conical gradient drawn as a 256-wedge Gouraud fan")
        }
        GradShape::Mesh3 | GradShape::Mesh4 => Shaded::Exact,
    }
}

/// A mesh gradient as a function-based shading over the shape's frame
/// bounds, the function clamping its input to the unit square.
fn mesh(
    w: &mut PdfWriter,
    c: &mut Canvas,
    frame: &Frame,
    ramp: &GradRamp,
    (u0, u1): (f64, f64),
    (v0, v1): (f64, f64),
) -> Shaded {
    let domain = [u0.min(0.0), u1.max(1.0), v0.min(0.0), v1.max(1.0)];
    match ramp {
        GradRamp::Mesh4(k) => {
            if k.iter().any(|c| c.a < 255) {
                return Shaded::Rasterise("mesh colours with transparency".into());
            }
            // (0,0), (1,0), (0,1), (1,1): the first input varies fastest.
            let samples: Vec<u8> = k.iter().flat_map(|c| [c.r, c.g, c.b]).collect();
            let f = w.sampled_rgb_function(&[0.0, 1.0, 0.0, 1.0], &[2, 2], &samples);
            let sh = w.function_based(f, domain);
            c.transform(frame.to_page.as_coeffs());
            c.shading(sh);
            Shaded::Exact
        }
        GradRamp::Mesh3(k) => {
            if k.iter().any(|c| c.a < 255) {
                return Shaded::Rasterise("mesh colours with transparency".into());
            }
            let g = MESH3_GRID;
            let mut samples = Vec::with_capacity((g * g * 3) as usize);
            for j in 0..g {
                for i in 0..g {
                    let (u, v) = (
                        f64::from(i) / f64::from(g - 1),
                        f64::from(j) / f64::from(g - 1),
                    );
                    let px = mesh3_at(k, u, v);
                    samples.extend_from_slice(&[px.r, px.g, px.b]);
                }
            }
            let f = w.sampled_rgb_function(&[0.0, 1.0, 0.0, 1.0], &[g, g], &samples);
            let sh = w.function_based(f, domain);
            c.transform(frame.to_page.as_coeffs());
            c.shading(sh);
            Shaded::Approximated("three-colour mesh drawn from a 33 x 33 sampled grid")
        }
        GradRamp::Table(_) => Shaded::Exact,
    }
}

/// The three-colour mesh at a point of the unit square, as the renderer
/// defines it: barycentric weights with the first clamped at zero and the
/// three renormalised.
fn mesh3_at(k: &[Rgba8; 3], u: f64, v: f64) -> Rgba8 {
    let w0 = (1.0 - u - v).max(0.0);
    let sum = w0 + u + v;
    if sum <= 0.0 {
        return k[0];
    }
    let ch = |a: u8, b: u8, c: u8| -> u8 {
        let x = (w0 * f64::from(a) + u * f64::from(b) + v * f64::from(c)) / sum;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let y = x.round().clamp(0.0, 255.0) as u8;
        y
    };
    Rgba8 {
        r: ch(k[0].r, k[1].r, k[2].r),
        g: ch(k[0].g, k[1].g, k[2].g),
        b: ch(k[0].b, k[1].b, k[2].b),
        a: ch(k[0].a, k[1].a, k[2].a),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xarast_render::precision::Point64;

    #[test]
    fn a_frame_maps_the_unit_axes_onto_the_control_points() {
        let f = Frame::of(GradMapping::Affine {
            a: Point64::new(10.0, 20.0),
            b: Point64::new(10.0, 30.0),
            c: Point64::new(50.0, 20.0),
        })
        .unwrap();
        let p = f.to_page * Point::new(1.0, 1.0);
        assert!((p.x - 50.0).abs() < 1e-9 && (p.y - 30.0).abs() < 1e-9);
        let q = f.from_page * Point::new(30.0, 25.0);
        assert!((q.x - 0.5).abs() < 1e-9 && (q.y - 0.5).abs() < 1e-9);
        assert!(
            Frame::of(GradMapping::Affine {
                a: Point64::new(0.0, 0.0),
                b: Point64::new(1.0, 1.0),
                c: Point64::new(2.0, 2.0),
            })
            .is_none()
        );
    }

    #[test]
    fn repeating_ranges_fold_to_whole_periods() {
        assert_eq!(periods(-0.3, 2.2), Ok((-1.0, 3.0)));
        assert_eq!(periods(0.5, 0.5), Ok((0.0, 1.0)));
        assert!(periods(0.0, 1e7).is_err());
        assert_eq!(sample_count(1.0, 256), 256);
        assert_eq!(sample_count(2.0, 256), 511);
        assert_eq!(sample_count(1e9, 2048), MAX_SAMPLES);
    }

    #[test]
    fn the_mesh3_grid_is_the_renderer_formula() {
        let k = [
            Rgba8::rgb(255, 0, 0),
            Rgba8::rgb(0, 255, 0),
            Rgba8::rgb(0, 0, 255),
        ];
        assert_eq!(mesh3_at(&k, 0.0, 0.0), k[0]);
        assert_eq!(mesh3_at(&k, 1.0, 0.0), k[1]);
        assert_eq!(mesh3_at(&k, 0.0, 1.0), k[2]);
        // Past the hypotenuse the first weight is clamped away.
        assert_eq!(mesh3_at(&k, 1.0, 1.0), Rgba8::rgb(0, 128, 128));
    }
}
