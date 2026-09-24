//! Live effects baked as SVG filters (`research/06 §6.8`), so that a
//! browser, resvg or Inkscape draws what Xarast draws.
//!
//! The model keeps the effect's parameters (`xarast:feather` and friends);
//! a filter is derived from them at every save, in both dialects, and the
//! reader ignores it (it recognises its own by `xarast:filter`, the way it
//! recognises the bitmap filters of `paint`). A filter carries no data of
//! its own.
//!
//! [`FilterChain`] is the shared part: a region, primitives appended in
//! order, and the registration in `<defs>` with
//! `color-interpolation-filters="sRGB"` (mandatory, §6.8.1: the SVG default
//! of `linearRGB` blurs visibly differently). An effect is a function that
//! builds a chain; [`feather`] is the first.

use super::defs::Defs;
use super::num::{f64s, mp};
use super::xml::attr;

/// Where a filter draws, in the user space of the element it is set on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Region {
    /// A box in SVG millipoints (`filterUnits="userSpaceOnUse"`): for an
    /// element whose user space is the spread's.
    User { x0: i64, y0: i64, x1: i64, y1: i64 },
    /// The element's own box grown by `margin` of its size on every side
    /// (`objectBoundingBox`): for an element with a `transform`, whose box
    /// is never degenerate (a placed image).
    Object { margin: f64 },
}

/// A radius or deviation in user units, per axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Radius {
    pub x: f64,
    pub y: f64,
}

impl Radius {
    fn text(self, scale: f64) -> String {
        let (x, y) = (self.x * scale, self.y * scale);
        if (x - y).abs() < 1e-9 {
            f64s(x, 4)
        } else {
            format!("{} {}", f64s(x, 4), f64s(y, 4))
        }
    }
}

/// A filter under construction.
#[derive(Debug)]
pub(crate) struct FilterChain {
    body: String,
}

impl FilterChain {
    /// A chain for the effect `kind` (written as `xarast:filter`, which is
    /// how the reader knows the filter is derived) over `region`.
    pub(crate) fn new(kind: &str, region: Region) -> FilterChain {
        let mut body = String::new();
        attr(&mut body, "xarast:filter", kind);
        match region {
            Region::User { x0, y0, x1, y1 } => {
                attr(&mut body, "filterUnits", "userSpaceOnUse");
                attr(&mut body, "x", &mp(x0));
                attr(&mut body, "y", &mp(y0));
                attr(&mut body, "width", &mp(x1.saturating_sub(x0).max(0)));
                attr(&mut body, "height", &mp(y1.saturating_sub(y0).max(0)));
            }
            Region::Object { margin } => {
                attr(&mut body, "x", &f64s(-margin, 4));
                attr(&mut body, "y", &f64s(-margin, 4));
                attr(&mut body, "width", &f64s(2.0f64.mul_add(margin, 1.0), 4));
                attr(&mut body, "height", &f64s(2.0f64.mul_add(margin, 1.0), 4));
            }
        }
        attr(&mut body, "color-interpolation-filters", "sRGB");
        body.push('>');
        FilterChain { body }
    }

    /// Appends one primitive, written whole (`<feX …/>`).
    pub(crate) fn push(&mut self, primitive: &str) {
        self.body.push_str(primitive);
    }

    /// Registers the filter and returns its id, for `filter="url(#…)"`.
    pub(crate) fn finish(mut self, defs: &mut Defs) -> String {
        self.body.push_str("</filter>");
        defs.add('f', "filter", &self.body)
    }
}

/// The angles (degrees) of the rectangles whose union stands in for a
/// disc: `(r cos φ, r sin φ)` half-sides, 22.5° apart and 11.25° off the
/// axes, so every half-side is above zero (a zero `feMorphology` radius
/// disables the primitive) and the union reaches at least `r cos 11.25°`
/// (0.98 r) in every direction.
const DISC_ANGLES: [f64; 4] = [11.25, 33.75, 56.25, 78.75];

/// Appends primitives that erode `SourceAlpha` by a disc of radius `r`,
/// leaving the result in the alpha channel.
///
/// `feMorphology` erodes by a rectangle, which pulls a diagonal edge in by
/// up to √2 r; a feather cut from a star or a curve then fades too early
/// (measured on `feathers.xar`). Eroding by a union of shapes is taking
/// the minimum of the erosions by each, and `feBlend mode="darken"` is a
/// per-channel minimum over opaque inputs: so the alpha is moved into the
/// (opaque) colour channels, eroded by four rectangles whose union is
/// close to the disc, the four combined with `darken`, and moved back.
fn erode_disc(s: &mut String, r: Radius) {
    s.push_str("<feColorMatrix");
    attr(s, "in", "SourceAlpha");
    attr(s, "type", "matrix");
    attr(s, "values", "0 0 0 1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 0 1");
    attr(s, "result", "a");
    s.push_str("/>");
    for (i, deg) in DISC_ANGLES.iter().enumerate() {
        let (sin, cos) = deg.to_radians().sin_cos();
        let rect = Radius {
            x: r.x * cos,
            y: r.y * sin,
        };
        s.push_str("<feMorphology");
        attr(s, "in", "a");
        attr(s, "operator", "erode");
        attr(
            s,
            "radius",
            &format!("{} {}", f64s(rect.x, 4), f64s(rect.y, 4)),
        );
        attr(s, "result", &format!("e{i}"));
        s.push_str("/>");
    }
    s.push_str("<feBlend");
    attr(s, "in", "e0");
    attr(s, "in2", "e1");
    attr(s, "mode", "darken");
    attr(s, "result", "d");
    s.push_str("/><feBlend");
    attr(s, "in", "e2");
    attr(s, "in2", "d");
    attr(s, "mode", "darken");
    attr(s, "result", "d");
    s.push_str("/><feBlend");
    attr(s, "in", "e3");
    attr(s, "in2", "d");
    attr(s, "mode", "darken");
    s.push_str("/><feColorMatrix");
    attr(s, "type", "matrix");
    attr(s, "values", "0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 1 0 0 0 0");
    s.push_str("/>");
}

/// How many samples of the profile go into `feFuncA`'s table; the filter
/// interpolates between them.
const PROFILE_SAMPLES: usize = 33;

/// A feather (`research/06 §6.8.6`; the renderer's `LayerEffect::Feather`,
/// `render.md` "Live effects"): the object's alpha pulled in by `r` (half
/// the feather's size), blurred by a Gaussian of σ = r/2 (the interchange
/// stand-in for the renderer's disc of radius `r`, `blur::sigma_for_disc_
/// radius`), shaped by the profile as the renderer shapes it (the profile
/// maps transparency, so alpha `a` becomes `1 − P(1 − a)`), and the object
/// kept where that mask is.
pub(crate) fn feather(r: Radius, profile: xarast_geom::BiasGain, region: Region) -> FilterChain {
    let mut f = FilterChain::new("feather", region);
    let mut s = String::new();
    erode_disc(&mut s, r);
    s.push_str("<feGaussianBlur");
    attr(&mut s, "stdDeviation", &r.text(0.5));
    s.push_str("/>");
    if profile != xarast_geom::BiasGain::default() {
        let d = (PROFILE_SAMPLES - 1) as f64;
        let v: Vec<String> = (0..PROFILE_SAMPLES)
            .map(|i| f64s(1.0 - profile.map(1.0 - i as f64 / d), 4))
            .collect();
        s.push_str("<feComponentTransfer><feFuncA");
        attr(&mut s, "type", "table");
        attr(&mut s, "tableValues", &v.join(" "));
        s.push_str("/></feComponentTransfer>");
    }
    s.push_str("<feComposite");
    attr(&mut s, "in", "SourceGraphic");
    attr(&mut s, "operator", "in");
    s.push_str("/>");
    f.push(&s);
    f
}

#[cfg(test)]
mod tests {
    use super::*;
    use xarast_geom::BiasGain;

    fn written(f: FilterChain) -> String {
        let mut d = Defs::default();
        f.finish(&mut d);
        d.items().collect()
    }

    #[test]
    fn a_feather_erodes_by_half_its_size_and_blurs_by_a_quarter() {
        let region = Region::User {
            x0: 0,
            y0: 0,
            x1: 10_000,
            y1: 5_000,
        };
        let s = written(feather(
            Radius { x: 4.0, y: 4.0 },
            BiasGain::IDENTITY,
            region,
        ));
        assert!(
            s.contains("filterUnits=\"userSpaceOnUse\" x=\"0\" y=\"0\" width=\"10\" height=\"5\"")
        );
        assert!(s.contains("color-interpolation-filters=\"sRGB\""));
        assert!(s.contains("xarast:filter=\"feather\""));
        assert_eq!(s.matches("<feMorphology").count(), 4);
        // The rectangles reach r along their own angle.
        assert!(s.contains("radius=\"3.9231 .7804\""), "{s}");
        assert!(s.contains("stdDeviation=\"2\""));
        assert!(!s.contains("feFuncA"), "no table for the identity profile");
        assert!(s.ends_with("<feComposite in=\"SourceGraphic\" operator=\"in\"/></filter>"));
    }

    #[test]
    fn a_profile_becomes_an_alpha_table_from_clear_to_opaque() {
        let s = written(feather(
            Radius { x: 1.0, y: 2.0 },
            BiasGain::new(0.5, 0.0),
            Region::Object { margin: 0.1 },
        ));
        assert!(
            s.contains("x=\"-.1\" y=\"-.1\" width=\"1.2\" height=\"1.2\""),
            "{s}"
        );
        assert!(s.contains("stdDeviation=\".5 1\""), "{s}");
        let table = s
            .split("tableValues=\"")
            .nth(1)
            .and_then(|t| t.split('"').next())
            .unwrap();
        let v: Vec<f64> = table.split(' ').map(|x| x.parse().unwrap()).collect();
        assert_eq!(v.len(), PROFILE_SAMPLES);
        assert_eq!((v[0], v[PROFILE_SAMPLES - 1]), (0.0, 1.0));
        assert!(v.windows(2).all(|p| p[0] <= p[1]));
    }
}
