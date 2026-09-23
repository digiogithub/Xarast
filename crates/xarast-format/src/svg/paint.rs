//! Fills, strokes and transparency: `research/06 §6.3–§6.6`.
//!
//! The rule that shapes everything here is **rule 5 of §5.1**: what SVG
//! expresses exactly is written as plain SVG and nothing more. A flat fill
//! is `fill="#c33"`; a two-colour linear gradient is a `<linearGradient>`
//! with two stops. The `xarast:` twin appears only where SVG cannot say it
//! — a non-linear ramp profile, a rainbow ramp, a conical fill.

use std::fmt::Write as _;

use xarast_color::{Colour, ColourTable, ColourValue, FillEffect, Rgba8, TranspMode, Transparency};
use xarast_doc::fill::{FillGeometry, Paint, Ramp, RampMapping, Tiling, TranspPaint};
use xarast_geom::{BiasGain, Point};

use super::Stats;
use super::defs::Defs;
use super::frame::Frame;
use super::num::{f64s, mp};
use super::xml::attr;

/// The largest per-channel error a baked ramp may have (`§6.4`): 2/255.
const MAX_RAMP_ERROR: f32 = 2.0 / 255.0;
/// Baked stops per key span, at least and at most (`§6.4`).
const MIN_SEGMENTS: usize = 8;
const MAX_DEPTH: u32 = 5;

/// Everything a paint needs besides the paint.
pub(crate) struct PaintCtx<'a> {
    pub colours: &'a ColourTable,
    pub defs: &'a mut Defs,
    pub frame: Frame,
    pub stats: &'a mut Stats,
    /// Palette ids, for `xarast:fill-ref`.
    pub palette: &'a std::collections::HashMap<xarast_color::ColourId, String>,
    /// Resolves a bitmap to its package path, when it has one.
    pub bitmap_href: &'a mut dyn FnMut(xarast_doc::BitmapId) -> Option<BitmapRef>,
}

/// A bitmap that made it into the package.
#[derive(Debug, Clone)]
pub(crate) struct BitmapRef {
    pub href: String,
    pub width: u32,
    pub height: u32,
}

/// The SVG for one paint (a fill or a stroke).
#[derive(Debug, Default)]
pub(crate) struct PaintOut {
    /// `none`, `#rrggbb` or `url(#id)`.
    pub value: String,
    /// Opacity from the colour's own alpha, for a flat paint.
    pub opacity: Option<f64>,
    /// The palette reference, for `xarast:fill-ref` / `xarast:stroke-ref`.
    pub palette_ref: Option<String>,
    /// Attributes and child elements of the `xarast:` twin, when SVG could
    /// not express the paint exactly.
    pub sidecar: Option<String>,
}

/// `#rgb` when it is exact, `#rrggbb` otherwise.
pub(crate) fn hex(c: Rgba8) -> String {
    let short = |v: u8| v >> 4 == v & 0xF;
    if short(c.r) && short(c.g) && short(c.b) {
        format!("#{:x}{:x}{:x}", c.r & 0xF, c.g & 0xF, c.b & 0xF)
    } else {
        format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
    }
}

/// `#rrggbb`, or `#rrggbbaa` when not opaque: the key-stop spelling.
fn hex_alpha(c: Rgba8) -> String {
    if c.a == 255 {
        format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
    } else {
        format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a)
    }
}

fn rgba(c: &Colour, t: &ColourTable) -> Rgba8 {
    c.resolve(t).to_rgba8()
}

fn opacity_of(a: u8) -> Option<f64> {
    (a < 255).then(|| f64::from(a) / 255.0)
}

fn palette_ref(c: &Colour, ctx: &PaintCtx<'_>) -> Option<String> {
    match c {
        Colour::Indexed { id, tint: None } => ctx.palette.get(id).map(|p| format!("#{p}")),
        _ => None,
    }
}

fn repeat_name(t: Tiling) -> &'static str {
    match t {
        Tiling::None => "none",
        Tiling::Simple => "simple",
        Tiling::Repeat => "repeat",
        Tiling::RepeatInverted => "reflect",
        Tiling::RepeatExtra => "repeat-extra",
    }
}

fn effect_name(e: FillEffect) -> &'static str {
    match e {
        FillEffect::Fade => "fade",
        FillEffect::Rainbow => "rainbow",
        FillEffect::AltRainbow => "alt-rainbow",
    }
}

fn profile_attr(p: BiasGain) -> String {
    format!("{} {}", f64s(p.bias, 6), f64s(p.gain, 6))
}

/// `x y` in points, SVG space.
fn pt_pair(ctx: &PaintCtx<'_>, p: Point) -> String {
    let (x, y) = ctx.frame.pt(p);
    format!("{} {}", mp(x), mp(y))
}

/// A resolved colour ramp: key stops in the parameter space *after* the
/// profile, plus what shapes it.
struct KeyRamp {
    from: ColourValue,
    to: ColourValue,
    ramp: Ramp<ColourValue>,
    effect: FillEffect,
}

impl KeyRamp {
    fn from_paint(
        from: &Colour,
        to: &Colour,
        ramp: &Ramp<Colour>,
        effect: FillEffect,
        t: &ColourTable,
    ) -> KeyRamp {
        let mut r: Ramp<ColourValue> = Ramp::new();
        r.profile = ramp.profile;
        r.mapping = ramp.mapping;
        for s in ramp.stops() {
            r.insert(xarast_doc::RampStop {
                pos: s.pos,
                value: s.value.resolve(t),
            });
        }
        KeyRamp {
            from: from.resolve(t),
            to: to.resolve(t),
            ramp: r,
            effect,
        }
    }

    fn sample(&self, t: f32) -> Rgba8 {
        self.ramp
            .sample(&self.from, &self.to, t, self.effect)
            .to_rgba8()
    }

    /// Whether plain SVG stops at the key positions reproduce it exactly:
    /// no profile, no easing, RGB interpolation.
    fn is_linear(&self) -> bool {
        self.ramp.profile == BiasGain::IDENTITY
            && self.ramp.mapping == RampMapping::Linear
            && self.effect == FillEffect::Fade
    }

    fn keys(&self) -> Vec<(f32, Rgba8)> {
        let mut v = vec![(0.0, self.from.to_rgba8())];
        v.extend(
            self.ramp
                .stops()
                .iter()
                .map(|s| (s.pos, s.value.to_rgba8())),
        );
        v.push((1.0, self.to.to_rgba8()));
        v
    }
}

/// Samples `f` over `0..=1` until piecewise-linear interpolation is within
/// [`MAX_RAMP_ERROR`] per channel: at least [`MIN_SEGMENTS`] segments, each
/// bisected at most [`MAX_DEPTH`] times (so at most 33 stops per eighth).
fn bake(f: &dyn Fn(f32) -> Rgba8) -> Vec<(f32, Rgba8)> {
    fn err(a: Rgba8, b: Rgba8, mid: Rgba8) -> f32 {
        let ch = |x: u8, y: u8, m: u8| {
            ((f32::from(x) + f32::from(y)) / 2.0 - f32::from(m)).abs() / 255.0
        };
        ch(a.r, b.r, mid.r)
            .max(ch(a.g, b.g, mid.g))
            .max(ch(a.b, b.b, mid.b))
            .max(ch(a.a, b.a, mid.a))
    }
    fn split(
        f: &dyn Fn(f32) -> Rgba8,
        t0: f32,
        c0: Rgba8,
        t1: f32,
        c1: Rgba8,
        depth: u32,
        out: &mut Vec<(f32, Rgba8)>,
    ) {
        let tm = (t0 + t1) / 2.0;
        let cm = f(tm);
        if depth < MAX_DEPTH && err(c0, c1, cm) > MAX_RAMP_ERROR {
            split(f, t0, c0, tm, cm, depth + 1, out);
            split(f, tm, cm, t1, c1, depth + 1, out);
        } else {
            out.push((t1, c1));
        }
    }
    let mut out = vec![(0.0, f(0.0))];
    for i in 0..MIN_SEGMENTS {
        let t0 = i as f32 / MIN_SEGMENTS as f32;
        let t1 = (i + 1) as f32 / MIN_SEGMENTS as f32;
        let c0 = out.last().map_or_else(|| f(t0), |l| l.1);
        let c1 = f(t1);
        split(f, t0, c0, t1, c1, 0, &mut out);
    }
    out
}

fn push_stops(body: &mut String, stops: &[(f32, Rgba8)]) {
    for (pos, c) in stops {
        body.push_str("<stop");
        attr(body, "offset", &f64s(f64::from(*pos), 4));
        attr(body, "stop-color", &hex(*c));
        if let Some(o) = opacity_of(c.a) {
            attr(body, "stop-opacity", &f64s(o, 3));
        }
        body.push_str("/>");
    }
}

/// The stops and twin attributes of a colour ramp.
fn ramp_body(k: &KeyRamp, ext: &mut String, stats: &mut Stats) -> Vec<(f32, Rgba8)> {
    if k.is_linear() {
        return k.keys();
    }
    stats.ramps_baked += 1;
    if k.ramp.profile != BiasGain::IDENTITY {
        attr(ext, "xarast:profile", &profile_attr(k.ramp.profile));
    }
    if k.ramp.mapping == RampMapping::Sin {
        attr(ext, "xarast:ramp-mapping", "sin");
    }
    if k.effect != FillEffect::Fade {
        attr(ext, "xarast:fill-effect", effect_name(k.effect));
    }
    let keys: Vec<String> = k
        .keys()
        .iter()
        .map(|(p, c)| format!("{}:{}", f64s(f64::from(*p), 6), hex_alpha(*c)))
        .collect();
    attr(ext, "xarast:stops", &keys.join(" "));
    bake(&|t| k.sample(t))
}

/// The graduated-fill repeat SVG can express: only the "extra" repeat tiles.
fn spread_method(t: Tiling) -> Option<&'static str> {
    (t == Tiling::RepeatExtra).then_some("repeat")
}

/// Builds the gradient element body of a linear or radial fill.
#[allow(clippy::too_many_arguments)]
fn gradient(
    ctx: &mut PaintCtx<'_>,
    geometry: &str,
    linear: bool,
    persp: Option<&xarast_doc::Perspective>,
    tiling: Tiling,
    stops: &[(f32, Rgba8)],
    ext: &str,
    extra: &str,
) -> String {
    let mut body = String::new();
    attr(&mut body, "gradientUnits", "userSpaceOnUse");
    body.push_str(geometry);
    if let Some(sm) = spread_method(tiling) {
        attr(&mut body, "spreadMethod", sm);
    }
    body.push_str(ext);
    body.push_str(extra);
    if let Some(p) = persp {
        // SVG has no projective gradient: the affine part is drawn, the two
        // extra corners are kept for Xarast.
        ctx.stats.perspective_approximated += 1;
        attr(
            &mut body,
            "xarast:persp",
            &format!("{} {}", pt_pair(ctx, p.p2), pt_pair(ctx, p.p3)),
        );
    }
    if tiling != Tiling::None {
        attr(&mut body, "xarast:fill-repeat", repeat_name(tiling));
    }
    body.push('>');
    push_stops(&mut body, stops);
    let element = if linear {
        "linearGradient"
    } else {
        "radialGradient"
    };
    let _ = write!(body, "</{element}>");
    ctx.stats.gradients += 1;
    let id = ctx.defs.add('g', element, &body);
    format!("url(#{id})")
}

fn linear_geometry(ctx: &PaintCtx<'_>, a: Point, b: Point) -> String {
    let (x1, y1) = ctx.frame.pt(a);
    let (x2, y2) = ctx.frame.pt(b);
    let mut g = String::new();
    attr(&mut g, "x1", &mp(x1));
    attr(&mut g, "y1", &mp(y1));
    attr(&mut g, "x2", &mp(x2));
    attr(&mut g, "y2", &mp(y2));
    g
}

/// A radial gradient over the frame `centre`, `u`, `v`: the unit circle
/// mapped by the matrix. Circles are written as `cx cy r`.
fn radial_geometry(
    ctx: &PaintCtx<'_>,
    centre: Point,
    u_end: Point,
    v_end: Point,
    circle: bool,
) -> String {
    let (cx, cy) = ctx.frame.pt(centre);
    let (ux, uy) = ctx.frame.pt(u_end);
    let (vx, vy) = ctx.frame.pt(v_end);
    let mut g = String::new();
    if circle {
        let r = (((ux - cx) as f64).powi(2) + ((uy - cy) as f64).powi(2)).sqrt();
        attr(&mut g, "cx", &mp(cx));
        attr(&mut g, "cy", &mp(cy));
        attr(&mut g, "r", &mp(r.round() as i64));
    } else {
        // With `userSpaceOnUse`, a missing `cx`/`cy` means 50 % of the
        // viewport, not 0: they must be explicit.
        attr(&mut g, "cx", "0");
        attr(&mut g, "cy", "0");
        attr(&mut g, "r", "1");
        attr(
            &mut g,
            "gradientTransform",
            &format!(
                "matrix({} {} {} {} {} {})",
                mp(ux - cx),
                mp(uy - cy),
                mp(vx - cx),
                mp(vy - cy),
                mp(cx),
                mp(cy)
            ),
        );
    }
    g
}

/// The colour paint of a fill or a stroke.
pub(crate) fn colour_paint(
    ctx: &mut PaintCtx<'_>,
    paint: &Paint,
    tiling: Tiling,
    effect: FillEffect,
) -> PaintOut {
    let t = ctx.colours;
    match paint {
        FillGeometry::Flat { value } => {
            let c = rgba(value, t);
            if c.a == 0 {
                return PaintOut {
                    value: "none".into(),
                    ..PaintOut::default()
                };
            }
            PaintOut {
                value: hex(c),
                opacity: opacity_of(c.a),
                palette_ref: palette_ref(value, ctx),
                sidecar: None,
            }
        }
        FillGeometry::Linear {
            start,
            end,
            persp,
            from,
            to,
            ramp,
        } => {
            let k = KeyRamp::from_paint(from, to, ramp, effect, t);
            let mut ext = String::new();
            let stops = ramp_body(&k, &mut ext, ctx.stats);
            let geo = linear_geometry(ctx, *start, *end);
            PaintOut {
                value: gradient(ctx, &geo, true, persp.as_ref(), tiling, &stops, &ext, ""),
                ..PaintOut::default()
            }
        }
        FillGeometry::Radial {
            centre,
            major,
            minor,
            aspect_locked,
            persp,
            from,
            to,
            ramp,
        } => {
            let k = KeyRamp::from_paint(from, to, ramp, effect, t);
            let mut ext = String::new();
            let stops = ramp_body(&k, &mut ext, ctx.stats);
            let circle = *aspect_locked || minor == major;
            let geo = if circle {
                radial_geometry(ctx, *centre, *major, *major, true)
            } else {
                radial_geometry(ctx, *centre, *major, *minor, false)
            };
            let mut extra = String::new();
            if circle && minor != major {
                attr(&mut extra, "xarast:minor", &pt_pair(ctx, *minor));
            }
            if !*aspect_locked && minor == major {
                attr(&mut extra, "xarast:aspect-locked", "false");
            }
            PaintOut {
                value: gradient(
                    ctx,
                    &geo,
                    false,
                    persp.as_ref(),
                    tiling,
                    &stops,
                    &ext,
                    &extra,
                ),
                ..PaintOut::default()
            }
        }
        FillGeometry::Diamond {
            centre,
            corner1,
            corner2,
            persp,
            from,
            to,
            ramp,
        } => {
            // §6.3: a radial gradient through the diamond's frame is the
            // accepted approximation (round rather than straight corners).
            ctx.stats.fills_approximated += 1;
            let k = KeyRamp::from_paint(from, to, ramp, effect, t);
            let mut ext = String::new();
            let stops = ramp_body(&k, &mut ext, ctx.stats);
            let geo = radial_geometry(ctx, *centre, *corner1, *corner2, false);
            let mut extra = String::new();
            attr(&mut extra, "xarast:fill", "diamond");
            PaintOut {
                value: gradient(
                    ctx,
                    &geo,
                    false,
                    persp.as_ref(),
                    tiling,
                    &stops,
                    &ext,
                    &extra,
                ),
                ..PaintOut::default()
            }
        }
        FillGeometry::Conical {
            centre,
            zero_dir,
            from,
            to,
            ramp,
        } => {
            ctx.stats.fills_approximated += 1;
            let k = KeyRamp::from_paint(from, to, ramp, effect, t);
            let mid = k.sample(0.5);
            let mut side = String::from("<xarast:fill");
            attr(&mut side, "xarast:type", "conical");
            attr(&mut side, "xarast:centre", &pt_pair(ctx, *centre));
            attr(&mut side, "xarast:zero-dir", &pt_pair(ctx, *zero_dir));
            ramp_twin(&mut side, &k, tiling);
            side.push_str("/>");
            flat_approx(mid, side)
        }
        FillGeometry::ThreeColour {
            origin,
            axis1,
            axis2,
            c0,
            c1,
            c2,
        } => {
            ctx.stats.fills_approximated += 1;
            let cs = [rgba(c0, t), rgba(c1, t), rgba(c2, t)];
            let mut side = String::from("<xarast:fill");
            attr(&mut side, "xarast:type", "three-point");
            attr(
                &mut side,
                "xarast:points",
                &format!(
                    "{} {} {}",
                    pt_pair(ctx, *origin),
                    pt_pair(ctx, *axis1),
                    pt_pair(ctx, *axis2)
                ),
            );
            attr(&mut side, "xarast:colours", &join_hex(&cs));
            if tiling != Tiling::None {
                attr(&mut side, "xarast:repeat", repeat_name(tiling));
            }
            side.push_str("/>");
            flat_approx(mean(&cs), side)
        }
        FillGeometry::FourColour {
            origin,
            axis1,
            axis2,
            axis3,
            c0,
            c1,
            c2,
            c3,
        } => {
            ctx.stats.fills_approximated += 1;
            let cs = [rgba(c0, t), rgba(c1, t), rgba(c2, t), rgba(c3, t)];
            let mut side = String::from("<xarast:fill");
            attr(&mut side, "xarast:type", "four-point");
            attr(
                &mut side,
                "xarast:points",
                &format!(
                    "{} {} {} {}",
                    pt_pair(ctx, *origin),
                    pt_pair(ctx, *axis1),
                    pt_pair(ctx, *axis2),
                    pt_pair(ctx, *axis3)
                ),
            );
            attr(&mut side, "xarast:colours", &join_hex(&cs));
            if tiling != Tiling::None {
                attr(&mut side, "xarast:repeat", repeat_name(tiling));
            }
            side.push_str("/>");
            flat_approx(mean(&cs), side)
        }
        FillGeometry::Fractal {
            params,
            from,
            to,
            profile,
        }
        | FillGeometry::Noise {
            params,
            from,
            to,
            profile,
        } => {
            ctx.stats.fills_approximated += 1;
            let kind = if matches!(paint, FillGeometry::Fractal { .. }) {
                "fractal-clouds"
            } else {
                "noise"
            };
            let (a, b) = (rgba(from, t), rgba(to, t));
            let mut side = String::from("<xarast:fill");
            attr(&mut side, "xarast:type", kind);
            attr(&mut side, "xarast:colours", &join_hex(&[a, b]));
            attr(&mut side, "xarast:seed", &params.seed.to_string());
            attr(
                &mut side,
                "xarast:graininess",
                &f64s(f64::from(params.graininess), 6),
            );
            attr(
                &mut side,
                "xarast:gravity",
                &f64s(f64::from(params.gravity), 6),
            );
            attr(
                &mut side,
                "xarast:squash",
                &f64s(f64::from(params.squash), 6),
            );
            attr(&mut side, "xarast:dpi", &params.dpi.to_string());
            if params.tileable {
                attr(&mut side, "xarast:tileable", "true");
            }
            if *profile != BiasGain::IDENTITY {
                attr(&mut side, "xarast:profile", &profile_attr(*profile));
            }
            side.push_str("/>");
            flat_approx(mean(&[a, b]), side)
        }
        FillGeometry::Bitmap {
            image,
            origin,
            axis_x,
            axis_y,
            persp,
            tiling: own,
            dpi,
            contone,
            profile,
        } => {
            let Some(bm) = (ctx.bitmap_href)(*image) else {
                ctx.stats.images_missing += 1;
                return PaintOut {
                    value: "none".into(),
                    ..PaintOut::default()
                };
            };
            // The unit square of the image, top row first, onto the
            // parallelogram: its top-left corner is `origin + (axis_y -
            // origin)` in document space, which is up.
            let (ox, oy) = ctx.frame.pt(*origin);
            let (xx, xy) = ctx.frame.pt(*axis_x);
            let (yx, yy) = ctx.frame.pt(*axis_y);
            let (ux, uy) = (xx - ox, xy - oy);
            let (vx, vy) = (yx - ox, yy - oy);
            let mut body = String::new();
            attr(&mut body, "patternUnits", "userSpaceOnUse");
            attr(&mut body, "width", "1");
            attr(&mut body, "height", "1");
            attr(
                &mut body,
                "patternTransform",
                &format!(
                    "matrix({} {} {} {} {} {})",
                    mp(ux),
                    mp(uy),
                    mp(-vx),
                    mp(-vy),
                    mp(ox + vx),
                    mp(oy + vy)
                ),
            );
            attr(&mut body, "xarast:fill", "bitmap");
            if *own != Tiling::None {
                attr(&mut body, "xarast:tile-mode", repeat_name(*own));
            }
            if *dpi != 0 {
                attr(&mut body, "xarast:dpi", &dpi.to_string());
            }
            if let Some((a, b)) = contone {
                attr(
                    &mut body,
                    "xarast:contone",
                    &join_hex(&[rgba(a, t), rgba(b, t)]),
                );
            }
            if *profile != BiasGain::IDENTITY {
                attr(&mut body, "xarast:profile", &profile_attr(*profile));
            }
            if let Some(p) = persp {
                ctx.stats.perspective_approximated += 1;
                attr(
                    &mut body,
                    "xarast:persp",
                    &format!("{} {}", pt_pair(ctx, p.p2), pt_pair(ctx, p.p3)),
                );
            }
            body.push_str("><image");
            attr(&mut body, "width", "1");
            attr(&mut body, "height", "1");
            attr(&mut body, "preserveAspectRatio", "none");
            attr(&mut body, "href", &bm.href);
            attr(&mut body, "xlink:href", &bm.href);
            body.push_str("/></pattern>");
            let id = ctx.defs.add('p', "pattern", &body);
            PaintOut {
                value: format!("url(#{id})"),
                ..PaintOut::default()
            }
        }
    }
}

fn ramp_twin(side: &mut String, k: &KeyRamp, tiling: Tiling) {
    let keys: Vec<String> = k
        .keys()
        .iter()
        .map(|(p, c)| format!("{}:{}", f64s(f64::from(*p), 6), hex_alpha(*c)))
        .collect();
    attr(side, "xarast:stops", &keys.join(" "));
    if k.ramp.profile != BiasGain::IDENTITY {
        attr(side, "xarast:profile", &profile_attr(k.ramp.profile));
    }
    if k.ramp.mapping == RampMapping::Sin {
        attr(side, "xarast:ramp-mapping", "sin");
    }
    if k.effect != FillEffect::Fade {
        attr(side, "xarast:fill-effect", effect_name(k.effect));
    }
    if tiling != Tiling::None {
        attr(side, "xarast:repeat", repeat_name(tiling));
    }
}

fn join_hex(cs: &[Rgba8]) -> String {
    cs.iter()
        .map(|c| hex_alpha(*c))
        .collect::<Vec<_>>()
        .join(" ")
}

fn mean(cs: &[Rgba8]) -> Rgba8 {
    let n = cs.len().max(1) as u32;
    let avg = |f: fn(&Rgba8) -> u8| (cs.iter().map(|c| u32::from(f(c))).sum::<u32>() / n) as u8;
    Rgba8 {
        r: avg(|c| c.r),
        g: avg(|c| c.g),
        b: avg(|c| c.b),
        a: avg(|c| c.a),
    }
}

fn flat_approx(c: Rgba8, sidecar: String) -> PaintOut {
    PaintOut {
        value: if c.a == 0 { "none".into() } else { hex(c) },
        opacity: opacity_of(c.a),
        palette_ref: None,
        sidecar: Some(sidecar),
    }
}

/// A blend mode: the CSS keyword when there is one, and the Xarast name.
pub(crate) fn blend_of(m: TranspMode) -> Option<(Option<&'static str>, &'static str)> {
    Some(match m {
        TranspMode::None | TranspMode::Mix => return None,
        TranspMode::StainedGlass => (Some("multiply"), "stained-glass"),
        TranspMode::Bleach => (Some("screen"), "bleach"),
        TranspMode::Darken => (Some("darken"), "darken"),
        TranspMode::Lighten => (Some("lighten"), "lighten"),
        TranspMode::Saturation => (Some("saturation"), "saturation"),
        TranspMode::Luminosity => (Some("luminosity"), "luminosity"),
        TranspMode::Contrast => (None, "contrast"),
        TranspMode::Brightness => (None, "brightness"),
    })
}

/// The SVG for a transparency.
#[derive(Debug, Default)]
pub(crate) struct TranspOut {
    /// A flat alpha to multiply into the paint's opacity.
    pub alpha: Option<f64>,
    /// The compositing mode.
    pub mode: TranspMode,
    /// A mask, for a graduated transparency.
    pub mask: Option<String>,
    /// The `xarast:` twin, when SVG could not express it.
    pub sidecar: Option<String>,
}

/// The transparency of a fill or a stroke. `bounds` is the element's box in
/// SVG space (millipoints), which a mask needs.
pub(crate) fn transparency(
    ctx: &mut PaintCtx<'_>,
    t: &TranspPaint,
    tiling: Tiling,
    bounds: Option<(i64, i64, i64, i64)>,
) -> TranspOut {
    let alpha = |l: u8| f64::from(255 - l) / 255.0;
    let opt_alpha = |l: u8| (l != 0).then(|| alpha(l));
    match t {
        FillGeometry::Flat { value } => {
            if value.mode == TranspMode::None {
                return TranspOut::default();
            }
            TranspOut {
                alpha: opt_alpha(value.level),
                mode: value.mode,
                ..TranspOut::default()
            }
        }
        FillGeometry::Linear {
            start,
            end,
            persp,
            from,
            to,
            ramp,
        } => {
            let geo = linear_geometry(ctx, *start, *end);
            let mask = transparency_mask(
                ctx,
                &geo,
                true,
                persp.as_ref(),
                tiling,
                *from,
                *to,
                ramp,
                bounds,
            );
            TranspOut {
                mode: from.mode,
                mask,
                ..TranspOut::default()
            }
        }
        FillGeometry::Radial {
            centre,
            major,
            minor,
            aspect_locked,
            persp,
            from,
            to,
            ramp,
        } => {
            let circle = *aspect_locked || minor == major;
            let geo = if circle {
                radial_geometry(ctx, *centre, *major, *major, true)
            } else {
                radial_geometry(ctx, *centre, *major, *minor, false)
            };
            let mask = transparency_mask(
                ctx,
                &geo,
                false,
                persp.as_ref(),
                tiling,
                *from,
                *to,
                ramp,
                bounds,
            );
            TranspOut {
                mode: from.mode,
                mask,
                ..TranspOut::default()
            }
        }
        FillGeometry::Diamond {
            centre,
            corner1,
            corner2,
            persp,
            from,
            to,
            ramp,
        } => {
            ctx.stats.fills_approximated += 1;
            let geo = radial_geometry(ctx, *centre, *corner1, *corner2, false);
            let mask = transparency_mask(
                ctx,
                &geo,
                false,
                persp.as_ref(),
                tiling,
                *from,
                *to,
                ramp,
                bounds,
            );
            TranspOut {
                mode: from.mode,
                mask,
                ..TranspOut::default()
            }
        }
        FillGeometry::Conical { from, to, .. }
        | FillGeometry::Fractal { from, to, .. }
        | FillGeometry::Noise { from, to, .. } => {
            ctx.stats.fills_approximated += 1;
            let level = ((u16::from(from.level) + u16::from(to.level)) / 2) as u8;
            TranspOut {
                alpha: opt_alpha(level),
                mode: from.mode,
                mask: None,
                sidecar: Some(transparency_twin(ctx, t)),
            }
        }
        FillGeometry::ThreeColour { c0, c1, c2, .. } => {
            ctx.stats.fills_approximated += 1;
            let level =
                ((u16::from(c0.level) + u16::from(c1.level) + u16::from(c2.level)) / 3) as u8;
            TranspOut {
                alpha: opt_alpha(level),
                mode: c0.mode,
                mask: None,
                sidecar: Some(transparency_twin(ctx, t)),
            }
        }
        FillGeometry::FourColour { c0, c1, c2, c3, .. } => {
            ctx.stats.fills_approximated += 1;
            let sum = u16::from(c0.level)
                + u16::from(c1.level)
                + u16::from(c2.level)
                + u16::from(c3.level);
            TranspOut {
                alpha: opt_alpha((sum / 4) as u8),
                mode: c0.mode,
                mask: None,
                sidecar: Some(transparency_twin(ctx, t)),
            }
        }
        FillGeometry::Bitmap { .. } => {
            ctx.stats.fills_approximated += 1;
            TranspOut {
                sidecar: Some(transparency_twin(ctx, t)),
                ..TranspOut::default()
            }
        }
    }
}

/// The twin of a transparency SVG cannot draw: its kind and control points.
fn transparency_twin(ctx: &PaintCtx<'_>, t: &TranspPaint) -> String {
    let kind = match t {
        FillGeometry::Conical { .. } => "conical",
        FillGeometry::ThreeColour { .. } => "three-point",
        FillGeometry::FourColour { .. } => "four-point",
        FillGeometry::Bitmap { .. } => "bitmap",
        FillGeometry::Fractal { .. } => "fractal-clouds",
        FillGeometry::Noise { .. } => "noise",
        _ => "other",
    };
    let mut s = String::from("<xarast:transparency");
    attr(&mut s, "xarast:type", kind);
    let pts: Vec<String> = t
        .control_points()
        .iter()
        .map(|p| pt_pair(ctx, *p))
        .collect();
    if !pts.is_empty() {
        attr(&mut s, "xarast:points", &pts.join(" "));
    }
    s.push_str("/>");
    s
}

#[allow(clippy::too_many_arguments)]
fn transparency_mask(
    ctx: &mut PaintCtx<'_>,
    geometry: &str,
    linear: bool,
    persp: Option<&xarast_doc::Perspective>,
    tiling: Tiling,
    from: Transparency,
    to: Transparency,
    ramp: &Ramp<Transparency>,
    bounds: Option<(i64, i64, i64, i64)>,
) -> Option<String> {
    let (x0, y0, x1, y1) = bounds?;
    let grey = |level: u8| {
        let a = 255 - level;
        Rgba8 {
            r: a,
            g: a,
            b: a,
            a: 255,
        }
    };
    let exact = ramp.profile == BiasGain::IDENTITY && ramp.mapping == RampMapping::Linear;
    let mut ext = String::new();
    let stops: Vec<(f32, Rgba8)> = if exact {
        let mut v = vec![(0.0, grey(from.level))];
        v.extend(ramp.stops().iter().map(|s| (s.pos, grey(s.value.level))));
        v.push((1.0, grey(to.level)));
        v
    } else {
        ctx.stats.ramps_baked += 1;
        if ramp.profile != BiasGain::IDENTITY {
            attr(&mut ext, "xarast:profile", &profile_attr(ramp.profile));
        }
        if ramp.mapping == RampMapping::Sin {
            attr(&mut ext, "xarast:ramp-mapping", "sin");
        }
        let mut keys = vec![format!("0:{}", from.level)];
        keys.extend(
            ramp.stops()
                .iter()
                .map(|s| format!("{}:{}", f64s(f64::from(s.pos), 6), s.value.level)),
        );
        keys.push(format!("1:{}", to.level));
        attr(&mut ext, "xarast:levels", &keys.join(" "));
        bake(&|t| grey(ramp.sample(&from, &to, t, FillEffect::Fade).level))
    };
    attr(&mut ext, "color-interpolation", "sRGB");
    let g = gradient(ctx, geometry, linear, persp, tiling, &stops, &ext, "");
    let mut body = String::new();
    attr(&mut body, "maskUnits", "userSpaceOnUse");
    let rect = |b: &mut String| {
        attr(b, "x", &mp(x0));
        attr(b, "y", &mp(y0));
        attr(b, "width", &mp(x1 - x0));
        attr(b, "height", &mp(y1 - y0));
    };
    rect(&mut body);
    attr(&mut body, "color-interpolation", "sRGB");
    body.push_str("><rect");
    rect(&mut body);
    attr(&mut body, "fill", &g);
    body.push_str("/></mask>");
    ctx.stats.masks += 1;
    Some(ctx.defs.add('m', "mask", &body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_shortens_only_when_exact() {
        let c = |r, g, b| Rgba8 { r, g, b, a: 255 };
        assert_eq!(hex(c(0xcc, 0x33, 0x33)), "#c33");
        assert_eq!(hex(c(0xcc, 0x33, 0x34)), "#cc3334");
        assert_eq!(hex(c(0, 0, 0)), "#000");
    }

    #[test]
    fn baking_meets_the_error_bound_and_the_stop_limits() {
        let p = BiasGain::new(0.42, -0.15);
        let f = |t: f32| {
            let v = (p.map(f64::from(t)) * 255.0).round() as u8;
            Rgba8 {
                r: v,
                g: 255 - v,
                b: v / 2,
                a: 255,
            }
        };
        let stops = bake(&f);
        assert!(stops.len() >= 9, "{}", stops.len());
        assert!(stops.len() <= 8 * 32 + 1);
        assert_eq!(stops.first().map(|s| s.0), Some(0.0));
        assert_eq!(stops.last().map(|s| s.0), Some(1.0));
        // Check the bound between every pair of baked stops.
        for w in stops.windows(2) {
            let (t0, c0) = w[0];
            let (t1, c1) = w[1];
            for i in 1..8 {
                let t = t0 + (t1 - t0) * i as f32 / 8.0;
                let want = f(t);
                let k = (t - t0) / (t1 - t0);
                let lerp = |a: u8, b: u8| f32::from(a) + (f32::from(b) - f32::from(a)) * k;
                let e = (lerp(c0.r, c1.r) - f32::from(want.r)).abs() / 255.0;
                assert!(e <= 3.0 / 255.0, "error {e} at {t}");
            }
        }
    }
}
