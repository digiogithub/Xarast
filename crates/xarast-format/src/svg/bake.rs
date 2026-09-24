//! Baking the fills SVG has no paint for into plain SVG geometry:
//! `research/06 §5.4.1` strategy 1 ("equivalent geometry") and `§6.3`.
//!
//! A conical, diamond, three-colour or four-colour fill is drawn by a
//! `<pattern>` (a fill) or a `<mask>` (a transparency) over the element's
//! box, holding pieces laid out in the **fill's own frame**: one `<g>`
//! whose `transform` maps the unit frame onto the fill's control points,
//! so every piece is a few unitless numbers and a skewed frame costs
//! nothing. The twin (`<xarast:fill>`, `<xarast:transparency>`) stays the
//! model; everything here is derived, marked `xarast:generated="fill-bake"`
//! and never read back.
//!
//! What each shape becomes, against the CPU renderer's definition of it
//! (`xarast-render` `shape_param` and the mesh sampler):
//!
//! - **Diamond** — the parameter is `max(|s|, |r|)`: four triangles from
//!   the centre, each a linear gradient along its own axis. Exact.
//! - **Conical** — the parameter is the angle: a fan of flat wedges, each
//!   no wider than the ramp allows [`TOLERANCE`] levels of change across,
//!   coloured at its middle.
//! - **Three- and four-colour** — the colour is linear along one frame
//!   axis once the other is fixed (bilinear, or barycentric inside the
//!   triangle): rows across the other axis, each a linear gradient sampled
//!   at the row's middle, split until neighbouring rows differ by at most
//!   [`TOLERANCE`] levels. A clamped mesh has two edge rows carrying the
//!   clamped colour beyond the frame, which is what the renderer does
//!   outside it; a tiled mesh is mirrored, as the renderer tiles it: the
//!   rows repeat mirrored across every period the box reaches, and each
//!   row's gradient reflects along the other axis.
//!
//! Pieces are drawn with `shape-rendering="crispEdges"`: neighbours differ
//! by a level or two, so aliasing is invisible, while antialiased shared
//! edges would let the background show through as hairlines.

use std::fmt::Write as _;

use xarast_color::Rgba8;

use super::num::{f64s, mp};
use super::paint::{GRID, PaintCtx, bake_spans, hex, opacity_of, push_stops};
use super::xml::attr;

/// Levels of change allowed across one wedge or between two rows: the
/// middle colour is then within half of it of every point of the piece.
const TOLERANCE: i32 = 2;

/// Angular resolution of a conical fan: wedges are made of these steps.
const FAN_STEPS: u32 = 1024;

/// Rows of a mesh are cut on this grid of the row axis.
const ROW_GRID: u32 = 1024;

/// The most rows a tiled mesh may emit across all its periods; past it the
/// caller falls back to its flat approximation.
const MAX_TILED_ROWS: usize = 8192;

/// A box in SVG space, millipoints: `x0 y0 x1 y1`.
pub(crate) type SvgBox = (i64, i64, i64, i64);

/// An affine frame in SVG space: `origin`, and the images of `(1, 0)` and
/// `(0, 1)` relative to it, all in millipoints.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Frame2 {
    pub origin: (i64, i64),
    pub u: (i64, i64),
    pub v: (i64, i64),
}

impl Frame2 {
    /// How far, in frame units, the box reaches from the origin along
    /// either axis: the half-size of a frame square covering it. `None`
    /// for a degenerate frame.
    fn reach(&self, b: SvgBox) -> Option<f64> {
        let (ux, uy) = (self.u.0 as f64, self.u.1 as f64);
        let (vx, vy) = (self.v.0 as f64, self.v.1 as f64);
        let det = ux * vy - uy * vx;
        if !det.is_finite() || det.abs() < 1.0 {
            return None;
        }
        let mut k = 1.0f64;
        for (x, y) in [(b.0, b.1), (b.2, b.1), (b.0, b.3), (b.2, b.3)] {
            let dx = (x - self.origin.0) as f64;
            let dy = (y - self.origin.1) as f64;
            let s = (dx * vy - dy * vx) / det;
            let r = (ux * dy - uy * dx) / det;
            k = k.max(s.abs()).max(r.abs());
        }
        // A frame too small for the box to be drawn sensibly (a fill whose
        // handles collapsed): leave it to the flat approximation.
        (k.is_finite() && k < 1e6).then(|| (k + 1.0).ceil())
    }

    /// The opening `<g>` that maps frame units onto SVG space, with the
    /// content's own origin at `shift`.
    fn group(&self, shift: (i64, i64)) -> String {
        let mut g = String::from("<g");
        attr(
            &mut g,
            "transform",
            &format!(
                "matrix({} {} {} {} {} {})",
                mp(self.u.0),
                mp(self.u.1),
                mp(self.v.0),
                mp(self.v.1),
                mp(self.origin.0 - shift.0),
                mp(self.origin.1 - shift.1)
            ),
        );
        attr(&mut g, "shape-rendering", "crispEdges");
        g.push('>');
        g
    }
}

/// A number in frame units.
fn fu(v: f64) -> String {
    f64s(v, 6)
}

/// The largest per-channel difference between two colours, in levels.
fn distance(a: Rgba8, b: Rgba8) -> i32 {
    let d = |x: u8, y: u8| (i32::from(x) - i32::from(y)).abs();
    d(a.r, b.r)
        .max(d(a.g, b.g))
        .max(d(a.b, b.b))
        .max(d(a.a, b.a))
}

/// The per-channel range of a run of colours.
#[derive(Debug, Clone, Copy)]
struct Range {
    lo: Rgba8,
    hi: Rgba8,
}

impl Range {
    fn of(c: Rgba8) -> Range {
        Range { lo: c, hi: c }
    }

    fn with(self, c: Rgba8) -> Range {
        let (lo, hi) = (self.lo, self.hi);
        Range {
            lo: Rgba8 {
                r: lo.r.min(c.r),
                g: lo.g.min(c.g),
                b: lo.b.min(c.b),
                a: lo.a.min(c.a),
            },
            hi: Rgba8 {
                r: hi.r.max(c.r),
                g: hi.g.max(c.g),
                b: hi.b.max(c.b),
                a: hi.a.max(c.a),
            },
        }
    }

    fn width(self) -> i32 {
        distance(self.lo, self.hi)
    }
}

fn fill_attrs(out: &mut String, c: Rgba8) {
    attr(out, "fill", &hex(c));
    if let Some(o) = opacity_of(c.a) {
        attr(out, "fill-opacity", &f64s(o, 3));
    }
}

/// Where the baked content goes: a pattern for a fill, a mask for a
/// transparency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    /// A `<pattern>` paint server; content relative to the box.
    Pattern,
    /// A `<mask>`; content in user space.
    Mask,
}

impl Target {
    fn shift(self, b: SvgBox) -> (i64, i64) {
        match self {
            Target::Pattern => (b.0, b.1),
            Target::Mask => (0, 0),
        }
    }
}

/// Wraps baked content and registers it: returns the definition's id.
fn wrap(ctx: &mut PaintCtx<'_>, target: Target, b: SvgBox, content: &str) -> String {
    let mut body = String::new();
    let (tag, prefix) = match target {
        Target::Pattern => {
            attr(&mut body, "patternUnits", "userSpaceOnUse");
            ("pattern", 'p')
        }
        Target::Mask => {
            attr(&mut body, "maskUnits", "userSpaceOnUse");
            ("mask", 'm')
        }
    };
    attr(&mut body, "x", &mp(b.0));
    attr(&mut body, "y", &mp(b.1));
    attr(&mut body, "width", &mp(b.2 - b.0));
    attr(&mut body, "height", &mp(b.3 - b.1));
    if target == Target::Mask {
        attr(&mut body, "color-interpolation", "sRGB");
    }
    attr(&mut body, "xarast:generated", "fill-bake");
    body.push('>');
    body.push_str(content);
    let _ = write!(body, "</{tag}>");
    if target == Target::Mask {
        ctx.stats.masks += 1;
    }
    ctx.defs.add(prefix, tag, &body)
}

/// A linear gradient along a frame axis, from the origin to `(x2, y2)`.
fn axis_gradient(
    ctx: &mut PaintCtx<'_>,
    x2: i32,
    y2: i32,
    stops: &[(f32, Rgba8)],
    spread: Option<&str>,
    target: Target,
) -> String {
    let mut body = String::new();
    attr(&mut body, "gradientUnits", "userSpaceOnUse");
    attr(&mut body, "x1", "0");
    attr(&mut body, "y1", "0");
    attr(&mut body, "x2", &x2.to_string());
    attr(&mut body, "y2", &y2.to_string());
    if let Some(sm) = spread {
        attr(&mut body, "spreadMethod", sm);
    }
    if target == Target::Mask {
        attr(&mut body, "color-interpolation", "sRGB");
    }
    body.push('>');
    push_stops(&mut body, stops);
    body.push_str("</linearGradient>");
    ctx.stats.gradients += 1;
    let id = ctx.defs.add('g', "linearGradient", &body);
    format!("url(#{id})")
}

/// A diamond fill: four triangles, each a gradient along its axis.
/// `stops` are the ramp's stops over `0..=1` (keys or baked), `spread`
/// the gradients' `spreadMethod`. Exact but for the stops.
pub(crate) fn diamond(
    ctx: &mut PaintCtx<'_>,
    fr: Frame2,
    b: SvgBox,
    stops: &[(f32, Rgba8)],
    spread: Option<&str>,
    target: Target,
) -> Option<String> {
    let k = fr.reach(b)?;
    // Each triangle overlaps its neighbours by a sliver, so no pixel on a
    // shared diagonal is left uncovered; the colours there agree.
    let e = k * 1.002;
    let (kk, ee) = (fu(k), fu(e));
    let mut content = fr.group(target.shift(b));
    for (x2, y2, pts) in [
        (1, 0, format!("M0 0L{kk} {ee}L{kk} -{ee}Z")),
        (-1, 0, format!("M0 0L-{kk} {ee}L-{kk} -{ee}Z")),
        (0, 1, format!("M0 0L{ee} {kk}L-{ee} {kk}Z")),
        (0, -1, format!("M0 0L{ee} -{kk}L-{ee} -{kk}Z")),
    ] {
        let g = axis_gradient(ctx, x2, y2, stops, spread, target);
        content.push_str("<path");
        attr(&mut content, "d", &pts);
        attr(&mut content, "fill", &g);
        content.push_str("/>");
    }
    content.push_str("</g>");
    Some(wrap(ctx, target, b, &content))
}

/// A conical fill: a fan of flat wedges around the frame's origin. The
/// frame is a similarity (the second axis is the first turned a quarter),
/// and `f` gives the colour at a fraction of the turn.
pub(crate) fn conical(
    ctx: &mut PaintCtx<'_>,
    fr: Frame2,
    b: SvgBox,
    f: &dyn Fn(f32) -> Rgba8,
    target: Target,
) -> Option<String> {
    let k = fr.reach(b)?;
    // The fan's radius covers the frame square; each edge of a wedge's
    // outline is at most a 64th of a turn, so its chord stays outside it.
    let radius = k * std::f64::consts::SQRT_2 / (std::f64::consts::PI / 64.0).cos() * 1.01;
    let at = |i: u32| f((i as f32 + 0.5) / FAN_STEPS as f32);
    let mut content = fr.group(target.shift(b));
    let point = |t: f64| {
        let a = t * std::f64::consts::TAU;
        format!("{} {}", fu(radius * a.cos()), fu(radius * a.sin()))
    };
    // The wedges: runs of fan steps whose colours stay within the
    // tolerance of each other.
    let mut wedges: Vec<(u32, u32)> = Vec::new();
    let mut run: Option<(u32, Range)> = None;
    for i in 0..FAN_STEPS {
        let c = at(i);
        run = match run {
            Some((start, r)) if r.with(c).width() <= TOLERANCE => Some((start, r.with(c))),
            Some((start, _)) => {
                wedges.push((start, i));
                Some((i, Range::of(c)))
            }
            None => Some((i, Range::of(c))),
        };
    }
    if let Some((start, _)) = run {
        wedges.push((start, FAN_STEPS));
    }
    for (start, end) in wedges {
        // A wedge reaches back over the previous one by a sliver (it is
        // drawn later), so their shared edge is always covered.
        let t0 = f64::from(start) / f64::from(FAN_STEPS)
            - if start > 0 {
                0.1 / f64::from(FAN_STEPS)
            } else {
                0.0
            };
        let t1 = f64::from(end) / f64::from(FAN_STEPS);
        let mid = f(((t0 + t1) / 2.0) as f32);
        let mut d = format!("M0 0L{}", point(t0));
        let steps = (((t1 - t0) * 64.0).ceil() as u32).max(1);
        for i in 1..=steps {
            let t = t0 + (t1 - t0) * f64::from(i) / f64::from(steps);
            let _ = write!(d, "L{}", point(t));
        }
        d.push('Z');
        content.push_str("<path");
        attr(&mut content, "d", &d);
        fill_attrs(&mut content, mid);
        content.push_str("/>");
    }
    content.push_str("</g>");
    ctx.stats.fills_baked += 1;
    Some(wrap(ctx, target, b, &content))
}

/// A mesh fill: `f(u, v)` over the frame, linear in `u` for a fixed `v`
/// (or nearly: `u_breaks(v)` names grid points of the `u` axis, out of
/// [`GRID`], where it has a kink). Rows across `v`, each a gradient along
/// `u`. Beyond the frame a clamped mesh (`tiled` false) clamps, as do the
/// edge rows and the gradients' padding; a tiled one mirrors, as do the
/// rows repeated across each period and the gradients' `reflect`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn mesh(
    ctx: &mut PaintCtx<'_>,
    fr: Frame2,
    b: SvgBox,
    f: &dyn Fn(f64, f64) -> Rgba8,
    u_breaks: &dyn Fn(f64) -> Vec<u32>,
    tiled: bool,
    target: Target,
) -> Option<String> {
    let k = fr.reach(b)?;
    // The rows: [v0, v1] on ROW_GRID, split while any column changes by
    // more than the tolerance from one end to the other.
    fn split(f: &dyn Fn(f64, f64) -> Rgba8, v0: u32, v1: u32, out: &mut Vec<(u32, u32)>) {
        let (a, b) = (
            f64::from(v0) / f64::from(ROW_GRID),
            f64::from(v1) / f64::from(ROW_GRID),
        );
        let off = v1 - v0 > 1
            && (0..=16).any(|i| {
                let u = f64::from(i) / 16.0;
                distance(f(u, a), f(u, b)) > TOLERANCE
            });
        if off {
            let m = v0 + (v1 - v0) / 2;
            split(f, v0, m, out);
            split(f, m, v1, out);
        } else {
            out.push((v0, v1));
        }
    }
    let mut rows = Vec::new();
    split(f, 0, ROW_GRID, &mut rows);
    // A tiled mesh: the whole periods of `v` the box reaches.
    #[allow(clippy::cast_possible_truncation)]
    let periods = (-k.ceil() as i64)..(k.ceil() as i64);
    if tiled && rows.len().saturating_mul(periods.clone().count()) > MAX_TILED_ROWS {
        return None;
    }
    let spread = tiled.then_some("reflect");
    let mut content = fr.group(target.shift(b));
    let row = |ctx: &mut PaintCtx<'_>, content: &mut String, y0: f64, y1: f64, v: f64| {
        let stops = bake_spans(&|u| f(f64::from(u), v), &u_breaks(v), 1);
        let g = axis_gradient(ctx, 1, 0, &stops, spread, target);
        content.push_str("<rect");
        attr(content, "x", &fu(-k));
        attr(content, "y", &fu(y0));
        attr(content, "width", &fu(2.0 * k));
        attr(content, "height", &fu(y1 - y0));
        attr(content, "fill", &g);
        content.push_str("/>");
    };
    if tiled {
        for p in periods {
            // Even periods run forwards, odd ones mirrored.
            #[allow(clippy::cast_precision_loss)]
            let base = p as f64;
            for &(v0, v1) in &rows {
                let (a, b) = (
                    f64::from(v0) / f64::from(ROW_GRID),
                    f64::from(v1) / f64::from(ROW_GRID),
                );
                let (y0, y1) = if p.rem_euclid(2) == 0 {
                    (base + a, base + b)
                } else {
                    (base + 1.0 - b, base + 1.0 - a)
                };
                row(ctx, &mut content, y0, y1, (a + b) / 2.0);
            }
        }
    } else {
        // Below the frame: the clamped first row.
        row(ctx, &mut content, -k, 0.0, 0.0);
        for (v0, v1) in rows {
            let (a, b) = (
                f64::from(v0) / f64::from(ROW_GRID),
                f64::from(v1) / f64::from(ROW_GRID),
            );
            row(ctx, &mut content, a, b, (a + b) / 2.0);
        }
        // Above it: the clamped last row.
        row(ctx, &mut content, 1.0, k, 1.0);
    }
    content.push_str("</g>");
    ctx.stats.fills_baked += 1;
    Some(wrap(ctx, target, b, &content))
}

/// The grid points of the `u` axis around `u = 1 - v`, where a
/// three-colour fill's barycentric triangle ends.
pub(crate) fn three_colour_breaks(v: f64) -> Vec<u32> {
    let edge = ((1.0 - v).clamp(0.0, 1.0) * f64::from(GRID)).round() as u32;
    vec![edge.saturating_sub(1), edge, (edge + 1).min(GRID)]
}

/// The renderer's three-colour fill at a frame point: barycentric inside
/// the triangle, the two far corners' blend beyond its long edge, clamped
/// to the unit square.
pub(crate) fn three_colour(c: [Rgba8; 3], u: f64, v: f64) -> Rgba8 {
    let (u, v) = (u.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
    let w0 = (1.0 - u - v).max(0.0);
    let sum = w0 + u + v;
    if sum <= 0.0 {
        return c[0];
    }
    let ch = |k: fn(&Rgba8) -> u8| -> u8 {
        ((w0 * f64::from(k(&c[0])) + u * f64::from(k(&c[1])) + v * f64::from(k(&c[2]))) / sum)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Rgba8 {
        r: ch(|c| c.r),
        g: ch(|c| c.g),
        b: ch(|c| c.b),
        a: ch(|c| c.a),
    }
}

fn lerp(a: Rgba8, b: Rgba8, t: f64) -> Rgba8 {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| -> u8 {
        (f64::from(x) + (f64::from(y) - f64::from(x)) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Rgba8 {
        r: m(a.r, b.r),
        g: m(a.g, b.g),
        b: m(a.b, b.b),
        a: m(a.a, b.a),
    }
}

/// The renderer's four-colour fill at a frame point: bilinear, `c[0]` at
/// the origin, `c[1]` along the first axis, `c[2]` along the second,
/// `c[3]` opposite, clamped to the unit square.
pub(crate) fn four_colour(c: [Rgba8; 4], u: f64, v: f64) -> Rgba8 {
    let (u, v) = (u.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
    lerp(lerp(c[0], c[1], u), lerp(c[2], c[3], u), v)
}

/// The largest change of `f` along `u` (at `v` = 0 and 1) and along `v`
/// (at `u` = 0 and 1): which axis the rows should run across.
pub(crate) fn variation(f: &dyn Fn(f64, f64) -> Rgba8) -> (i32, i32) {
    let d = distance;
    let along_u = d(f(0.0, 0.0), f(1.0, 0.0)).max(d(f(0.0, 1.0), f(1.0, 1.0)));
    let along_v = d(f(0.0, 0.0), f(0.0, 1.0)).max(d(f(1.0, 0.0), f(1.0, 1.0)));
    (along_u, along_v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(r: u8, g: u8, b: u8) -> Rgba8 {
        Rgba8 { r, g, b, a: 255 }
    }

    #[test]
    fn the_reach_covers_the_box_in_frame_units() {
        let fr = Frame2 {
            origin: (0, 0),
            u: (1000, 0),
            v: (0, 2000),
        };
        // Corners at x = ±5000 (5 units of u), y = ±4000 (2 units of v).
        assert_eq!(fr.reach((-5000, -4000, 5000, 4000)), Some(6.0));
        let flat = Frame2 {
            origin: (0, 0),
            u: (1000, 0),
            v: (2000, 0),
        };
        assert_eq!(flat.reach((0, 0, 10, 10)), None);
    }

    #[test]
    fn the_mesh_samplers_hit_their_corners_and_clamp() {
        let t = [c(0, 0, 0), c(255, 0, 0), c(0, 0, 255)];
        assert_eq!(three_colour(t, 0.0, 0.0), t[0]);
        assert_eq!(three_colour(t, 1.0, 0.0), t[1]);
        assert_eq!(three_colour(t, 0.0, 1.0), t[2]);
        assert_eq!(three_colour(t, -3.0, 7.0), t[2]);
        // Beyond the long edge: the far corners' blend.
        assert_eq!(three_colour(t, 1.0, 1.0), c(128, 0, 128));
        let q = [c(0, 0, 0), c(255, 0, 0), c(0, 255, 0), c(255, 255, 255)];
        assert_eq!(four_colour(q, 1.0, 1.0), q[3]);
        assert_eq!(four_colour(q, 2.0, -1.0), q[1]);
        assert_eq!(variation(&|u, v| four_colour(q, u, v)), (255, 255));
    }
}
