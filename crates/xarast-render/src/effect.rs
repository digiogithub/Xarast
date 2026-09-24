//! Live effects that the renderer computes from pixels: the offscreen
//! pipeline's consumers (phase 13, workstreams B and C).
//!
//! An effect wraps part of a scene, like a layer does
//! ([`crate::SceneBuilder::push_effect`]). The backend renders what it
//! wraps into an offscreen [`LayerTarget`] in **device space**, at the
//! view's own resolution, derives a mask from its silhouette with the
//! shared blur ([`crate::blur`]), and composites the result where the
//! wrapped content would have gone. Nothing is baked into the document:
//! the effect is regenerated at whatever zoom it is drawn at, which is the
//! controller/generated split of `research/02 §6.1` with the generated
//! half living in the renderer.
//!
//! # Exactness under repaint
//!
//! The render thread repaints only the damage of an edit and relies on a
//! rectangle drawn over a frame being exactly that frame's pixels
//! (`docs/memory/render.md`, invariant 15). An effect keeps that property
//! because:
//!
//! * its offscreen region is rendered from the same coverage origin
//!   whatever the draw area is (a left edge fixed by the view and the
//!   effect, bands on an absolute grid), so a pixel's inputs do not depend
//!   on which rectangle is being repainted;
//! * the region reaches [`LayerEffect::reach_px`] beyond the area in every
//!   direction, beyond the viewport included, so every pixel kept has all
//!   the inputs its kernel reads;
//! * the damage of a change inside an effect is inflated by the same reach
//!   (`damage::scene_damage`).
//!
//! [`LayerTarget`]: crate::layer::LayerTarget

use rayon::prelude::*;
use xarast_color::Rgba8;

use crate::blur::{self, Kernel, MAX_RADIUS_PX};
use crate::precision::Transform2D;
use crate::ramp::Profile;

/// One effect the renderer applies to the content it wraps.
#[derive(Debug, Clone, PartialEq)]
pub enum LayerEffect {
    /// A soft edge: the content fades to fully transparent at its outline
    /// over `size` document units (`research/02 §6.12`, `research/03 §2.9`).
    ///
    /// The original's feather is a *diameter*: the object's silhouette is
    /// pulled in by half of it (the outline is stroked, `size` wide, over
    /// the filled silhouette) and then blurred by a disc of half of it, so
    /// the fade runs from fully opaque `size` inside the outline to fully
    /// transparent at the outline itself, and nothing grows
    /// (`Kernel/fthrattr.cpp`, `CreateSilhouetteBitmap`). The profile
    /// reshapes the fade as a bitmap transparency's profile does.
    Feather {
        /// The feather's width in document units (millipoints).
        size: f64,
        /// The fade's bias/gain.
        profile: Profile,
    },
    /// A wall, floor or glow shadow drawn beneath the content it wraps
    /// (`research/02 §6.9`, `research/03 §2.9`).
    Shadow(Box<ShadowEffect>),
}

/// A shadow, in document units: the content's silhouette, grown (a glow),
/// moved by [`map`](Self::map) (a wall's offset, a floor's squash and
/// shear), blurred by a disc of half the penumbra, shaped by the profile
/// and flooded with the colour, under the content.
#[derive(Debug, Clone, PartialEq)]
pub struct ShadowEffect {
    /// Where the silhouette goes, as a document-space affine
    /// `[a, b, c, d, e, f]` (`kurbo`'s order).
    pub map: [f64; 6],
    /// How far [`map`](Self::map) moves any point of the content, in
    /// document units: the offscreen region must reach that far.
    pub displacement: f64,
    /// A glow's growth before the blur, in document units; 0 otherwise.
    pub spread: f64,
    /// The penumbra: the blur's diameter, in document units.
    pub blur: f64,
    /// The profile as the file stores it; its bias is negated when it is
    /// applied, as the original's shadow does.
    pub profile: Profile,
    /// The shadow's colour; its alpha is the opacity where the shadow is
    /// densest (the darkness).
    pub colour: Rgba8,
}

impl ShadowEffect {
    /// The blur radius in device pixels: half the penumbra, capped at
    /// the original's 100 px.
    #[must_use]
    pub fn radius_px(&self, scale: f64) -> f32 {
        LayerEffect::feather_radius_px(self.blur, scale)
    }

    /// The glow's growth in device pixels, capped like a blur radius.
    #[must_use]
    pub fn spread_px(&self, scale: f64) -> f32 {
        LayerEffect::feather_radius_px(self.spread * 2.0, scale)
    }

    fn reach_px(&self, scale: f64) -> i32 {
        let blur = Kernel::Disc {
            radius_px: self.radius_px(scale),
        }
        .reach();
        let spread = Kernel::Disc {
            radius_px: self.spread_px(scale),
        }
        .reach();
        let d = self.displacement * scale;
        let moved = if d.is_finite() && d > 0.0 {
            // Clamped: a displacement past the pixel budget is not a
            // shadow anyone can see whole.
            d.ceil().min(f64::from(1 << 20)) as i32
        } else {
            0
        };
        // Two pixels of slack: the bilinear sample and the antialiased
        // edge of the moved silhouette.
        let slack = 2i64;
        let r = i64::from(moved) + i64::from(blur) + i64::from(spread) + slack;
        i32::try_from(r).unwrap_or(i32::MAX)
    }

    /// Draws the shadow beneath `colour`. `origin` is the device position of
    /// the planes' top-left pixel and `xf` the document-to-device transform.
    fn apply(
        &self,
        colour: &mut [u8],
        silhouette: &[u8],
        width: usize,
        height: usize,
        xf: Transform2D,
        origin: (i32, i32),
    ) {
        let scale = xf.max_scale();
        let mut plane = silhouette.to_vec();
        let spread = self.spread_px(scale);
        if spread > 0.0 {
            blur::dilate_plane(&mut plane, width, height, spread);
        }
        let map = Transform2D::new(self.map);
        if map != Transform2D::IDENTITY {
            // Device → device: a pixel of the shadow takes the silhouette
            // from where the map brings it from.
            let back = xf
                .invert()
                .zip(map.invert())
                .map(|(d_inv, m_inv)| d_inv.then(m_inv).then(xf));
            plane = match back {
                Some(b) => warp(&plane, width, height, b, origin),
                // A singular map (a floor of height 0) casts nothing.
                None => vec![0; plane.len()],
            };
        }
        blur::blur_plane(
            &mut plane,
            width,
            height,
            Kernel::Disc {
                radius_px: self.radius_px(scale),
            },
        );
        // The original maps the blurred silhouette's transparency through
        // the profile with its bias negated.
        let table = (self.profile != Profile::IDENTITY).then(|| {
            blur::profile_table(Profile {
                bias: -self.profile.bias,
                gain: self.profile.gain,
            })
        });
        let alpha = u32::from(self.colour.a);
        let rgb = [
            u32::from(self.colour.r),
            u32::from(self.colour.g),
            u32::from(self.colour.b),
        ];
        colour
            .par_chunks_mut(width * 4)
            .zip(plane.par_chunks(width))
            .for_each(|(row, mask)| {
                for (px, m) in row.as_chunks_mut::<4>().0.iter_mut().zip(mask) {
                    let m = match &table {
                        Some(t) => 255 - u32::from(t[usize::from(255 - *m)]),
                        None => u32::from(*m),
                    };
                    let sa = (m * alpha + 127) / 255;
                    if sa == 0 {
                        continue;
                    }
                    let under = 255 - u32::from(px[3]);
                    if under == 0 {
                        continue;
                    }
                    for (c, s) in px[..3].iter_mut().zip(rgb) {
                        let s = (s * sa + 127) / 255;
                        let v = u32::from(*c) + (s * under + 127) / 255;
                        *c = u8::try_from(v.min(255)).unwrap_or(u8::MAX);
                    }
                    let a = u32::from(px[3]) + (sa * under + 127) / 255;
                    px[3] = u8::try_from(a.min(255)).unwrap_or(u8::MAX);
                }
            });
    }
}

/// Resamples a coverage plane through `back` (device output → device
/// input), bilinearly; outside the plane is uncovered. Every output pixel
/// is a function of its own position alone, so the result does not depend
/// on how the work is split.
fn warp(
    plane: &[u8],
    width: usize,
    height: usize,
    back: Transform2D,
    origin: (i32, i32),
) -> Vec<u8> {
    let c = back.to_affine().as_coeffs();
    let (ox, oy) = (i64::from(origin.0), i64::from(origin.1));
    // `x`, `y` are absolute device pixel indices.
    let at = |x: i64, y: i64| -> f64 {
        let (x, y) = (x - ox, y - oy);
        if x < 0 || y < 0 || x >= width as i64 || y >= height as i64 {
            0.0
        } else {
            f64::from(plane[y as usize * width + x as usize])
        }
    };
    let (lo_x, lo_y) = (ox as f64 - 1.0, oy as f64 - 1.0);
    let (hi_x, hi_y) = (ox as f64 + width as f64, oy as f64 + height as f64);
    let mut out = vec![0u8; plane.len()];
    out.par_chunks_mut(width.max(1))
        .enumerate()
        .for_each(|(j, row)| {
            // Everything below is computed from absolute device positions
            // only: where the region starts must not move a sample by an
            // ulp (a strip's region starts elsewhere than a frame's).
            let dy = (oy + j as i64) as f64 + 0.5;
            for (i, o) in row.iter_mut().enumerate() {
                let dx = (ox + i as i64) as f64 + 0.5;
                let sx = c[0] * dx + c[2] * dy + c[4] - 0.5;
                let sy = c[1] * dx + c[3] * dy + c[5] - 0.5;
                if !(sx.is_finite() && sy.is_finite())
                    || sx < lo_x
                    || sy < lo_y
                    || sx > hi_x
                    || sy > hi_y
                {
                    continue;
                }
                let (x0, y0) = (sx.floor(), sy.floor());
                let (fx, fy) = (sx - x0, sy - y0);
                let (x0, y0) = (x0 as i64, y0 as i64);
                let v = at(x0, y0) * (1.0 - fx) * (1.0 - fy)
                    + at(x0 + 1, y0) * fx * (1.0 - fy)
                    + at(x0, y0 + 1) * (1.0 - fx) * fy
                    + at(x0 + 1, y0 + 1) * fx * fy;
                *o = v.round().clamp(0.0, 255.0) as u8;
            }
        });
    out
}

impl LayerEffect {
    /// The feather's blur radius in device pixels at `scale` device pixels
    /// per document unit: half its size, capped at the original's 100 px.
    #[must_use]
    pub fn feather_radius_px(size: f64, scale: f64) -> f32 {
        let r = size * scale * 0.5;
        if r.is_nan() || r <= 0.0 {
            return 0.0;
        }
        if r >= f64::from(MAX_RADIUS_PX) {
            return MAX_RADIUS_PX;
        }
        // f32-ok: a radius in device pixels, at most 100.
        r as f32
    }

    /// How far, in device pixels, an input pixel can move an output pixel
    /// at `scale` device pixels per document unit. The offscreen region is
    /// rendered this far beyond the pixels it keeps, and a change under the
    /// effect damages this far around itself.
    #[must_use]
    pub fn reach_px(&self, scale: f64) -> i32 {
        match self {
            LayerEffect::Feather { size, .. } => {
                let r = LayerEffect::feather_radius_px(*size, scale);
                if r <= 0.0 {
                    return 0;
                }
                // Erosion by r, then a blur by r, plus a pixel of slack for
                // the antialiased edge of the eroded silhouette.
                let k = Kernel::Disc { radius_px: r }.reach();
                i32::try_from(2 * k + 1).unwrap_or(i32::MAX)
            }
            LayerEffect::Shadow(s) => s.reach_px(scale),
        }
    }

    /// How far the effect's output can reach beyond its content, in device
    /// pixels. A feather draws nothing outside what it wraps; a shadow
    /// reaches as far as it moves, grows and blurs.
    #[must_use]
    pub fn growth_px(&self, scale: f64) -> i32 {
        match self {
            LayerEffect::Feather { .. } => 0,
            LayerEffect::Shadow(s) => s.reach_px(scale),
        }
    }

    /// Whether the effect needs its content's silhouette as well as its
    /// colour.
    #[must_use]
    pub const fn needs_silhouette(&self) -> bool {
        match self {
            LayerEffect::Feather { .. } | LayerEffect::Shadow(_) => true,
        }
    }

    /// A short name for reports ("a live effect (shadow) has no PDF
    /// equivalent").
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            LayerEffect::Feather { .. } => "feather",
            LayerEffect::Shadow(_) => "shadow",
        }
    }

    /// Applies the effect to rendered content in device space, in place.
    ///
    /// `colour` is premultiplied RGBA8, `silhouette` one coverage byte per
    /// pixel (every paint opaque), both `width × height`; `xf` is the
    /// document-to-device transform the effect applies under and `origin`
    /// the device position of the planes' top-left pixel. A shadow needs
    /// both (its offset and its floor are document-space geometry); a
    /// feather only the scale.
    #[allow(
        clippy::too_many_arguments,
        reason = "two planes, their size and where they sit"
    )]
    pub fn apply_at(
        &self,
        colour: &mut [u8],
        silhouette: Option<&[u8]>,
        width: usize,
        height: usize,
        xf: Transform2D,
        origin: (i32, i32),
    ) {
        match self {
            LayerEffect::Feather { .. } => {
                self.apply(colour, silhouette, width, height, xf.max_scale());
            }
            LayerEffect::Shadow(s) => {
                let Some(sil) = silhouette else { return };
                if sil.len() != width * height || colour.len() != sil.len() * 4 {
                    return;
                }
                s.apply(colour, sil, width, height, xf, origin);
            }
        }
    }

    /// Applies the effect to rendered content, in place.
    ///
    /// `colour` is premultiplied RGBA8, `silhouette` one coverage byte per
    /// pixel (every paint opaque), both `width × height`. `scale` is device
    /// pixels per document unit. A shadow is applied as if the planes sat
    /// at the device origin under a plain `scale`, with no flip: callers
    /// that know the view use [`LayerEffect::apply_at`].
    pub fn apply(
        &self,
        colour: &mut [u8],
        silhouette: Option<&[u8]>,
        width: usize,
        height: usize,
        scale: f64,
    ) {
        match self {
            LayerEffect::Feather { size, profile } => {
                let r = LayerEffect::feather_radius_px(*size, scale);
                let Some(sil) = silhouette else { return };
                if r <= 0.0 || sil.len() != width * height || colour.len() != sil.len() * 4 {
                    return;
                }
                let mut mask = sil.to_vec();
                blur::erode_plane(&mut mask, width, height, r);
                blur::blur_plane(&mut mask, width, height, Kernel::Disc { radius_px: r });
                if *profile != Profile::IDENTITY {
                    // The profile shapes the transparency, 0 opaque to 255
                    // clear, as a bitmap transparency's profile does.
                    let table = blur::profile_table(*profile);
                    for m in &mut mask {
                        *m = 255 - table[usize::from(255 - *m)];
                    }
                }
                for (px, m) in colour.as_chunks_mut::<4>().0.iter_mut().zip(&mask) {
                    if *m == 255 {
                        continue;
                    }
                    let m = u32::from(*m);
                    for c in px {
                        *c = u8::try_from((u32::from(*c) * m + 127) / 255).unwrap_or(u8::MAX);
                    }
                }
            }
            LayerEffect::Shadow(_) => self.apply_at(
                colour,
                silhouette,
                width,
                height,
                Transform2D::scale(scale),
                (0, 0),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_feather_radius_is_half_its_size_and_capped() {
        assert!((LayerEffect::feather_radius_px(20_000.0, 0.001) - 10.0).abs() < 1e-6);
        assert_eq!(LayerEffect::feather_radius_px(1e9, 1.0), MAX_RADIUS_PX);
        assert_eq!(LayerEffect::feather_radius_px(-3.0, 1.0), 0.0);
        assert_eq!(LayerEffect::feather_radius_px(f64::NAN, 1.0), 0.0);
    }

    #[test]
    fn a_feather_reaches_its_erosion_and_its_blur() {
        let f = LayerEffect::Feather {
            size: 20.0,
            profile: Profile::IDENTITY,
        };
        assert_eq!(f.reach_px(1.0), 21);
        assert_eq!(f.growth_px(1.0), 0);
        assert_eq!(f.reach_px(0.0), 0);
    }

    #[test]
    fn a_feather_fades_to_clear_at_the_outline_and_is_opaque_deep_inside() {
        let (w, h) = (80usize, 80usize);
        let inside = |x: usize, y: usize| (20..60).contains(&x) && (20..60).contains(&y);
        let sil: Vec<u8> = (0..w * h)
            .map(|i| if inside(i % w, i / w) { 255 } else { 0 })
            .collect();
        let mut colour: Vec<u8> = sil.iter().flat_map(|a| [*a, 0, 0, *a]).collect();
        LayerEffect::Feather {
            size: 12.0,
            profile: Profile::IDENTITY,
        }
        .apply(&mut colour, Some(&sil), w, h, 1.0);
        let a = |x: usize, y: usize| colour[(y * w + x) * 4 + 3];
        // Outside stays clear; nothing grows.
        assert_eq!(a(19, 40), 0);
        // The outermost pixel is nearly clear, the centre untouched.
        assert!(a(20, 40) < 40, "{}", a(20, 40));
        assert_eq!(a(40, 40), 255);
        // Monotone from the edge in.
        for x in 20..40 {
            assert!(a(x, 40) <= a(x + 1, 40), "x = {x}");
        }
        // Fully opaque by one feather size in.
        assert_eq!(a(32, 40), 255);
    }

    /// A 40 × 40 square at (20, 20) in an 100 × 100 plane, drawn opaque red
    /// when `content`, and its silhouette.
    fn square_planes(content: bool) -> (Vec<u8>, Vec<u8>, usize, usize) {
        let (w, h) = (100usize, 100usize);
        let inside = |x: usize, y: usize| (20..60).contains(&x) && (20..60).contains(&y);
        let sil: Vec<u8> = (0..w * h)
            .map(|i| if inside(i % w, i / w) { 255 } else { 0 })
            .collect();
        let colour: Vec<u8> = sil
            .iter()
            .flat_map(|a| if content { [*a, 0, 0, *a] } else { [0; 4] })
            .collect();
        (colour, sil, w, h)
    }

    fn shadow(map: [f64; 6], displacement: f64, spread: f64, blur: f64) -> LayerEffect {
        LayerEffect::Shadow(Box::new(ShadowEffect {
            map,
            displacement,
            spread,
            blur,
            profile: Profile::IDENTITY,
            colour: Rgba8 {
                r: 0,
                g: 0,
                b: 255,
                a: 128,
            },
        }))
    }

    #[test]
    fn a_wall_shadow_is_the_silhouette_moved_blurred_and_under_the_content() {
        let (mut colour, sil, w, h) = square_planes(true);
        // 10 units right and 10 up in document space; the device flips y.
        let e = shadow(
            [1.0, 0.0, 0.0, 1.0, 10.0, 10.0],
            10.0_f64.hypot(10.0),
            0.0,
            4.0,
        );
        let xf = Transform2D::new([1.0, 0.0, 0.0, -1.0, 0.0, 100.0]);
        e.apply_at(&mut colour, Some(&sil), w, h, xf, (0, 0));
        let px = |x: usize, y: usize| {
            let i = (y * w + x) * 4;
            [colour[i], colour[i + 1], colour[i + 2], colour[i + 3]]
        };
        // The content is untouched where it is opaque.
        assert_eq!(px(40, 40), [255, 0, 0, 255]);
        // Right of and above the square (device up): the shadow, at its
        // opacity (a disc of radius 2 leaves the middle of a 40 px square
        // fully covered).
        assert_eq!(px(65, 25), [0, 0, 128, 128]);
        // Nothing on the other side.
        assert_eq!(px(15, 55), [0; 4]);
        // Blurred: the edge is soft, half covered on the moved outline.
        let edge = px(69, 25)[3];
        assert!((40..100).contains(&edge), "{edge}");
        assert!(e.growth_px(1.0) >= 17, "{}", e.growth_px(1.0));
        assert_eq!(e.reach_px(1.0), e.growth_px(1.0));
    }

    #[test]
    fn a_glow_grows_the_silhouette_before_blurring_it() {
        let (mut colour, sil, w, h) = square_planes(false);
        let e = shadow([1.0, 0.0, 0.0, 1.0, 0.0, 0.0], 0.0, 6.0, 0.0);
        e.apply_at(&mut colour, Some(&sil), w, h, Transform2D::IDENTITY, (0, 0));
        let a = |x: usize, y: usize| colour[(y * w + x) * 4 + 3];
        assert_eq!(a(40, 40), 128);
        assert_eq!(a(15, 40), 128, "6 px out of the square");
        assert_eq!(a(12, 40), 0, "8 px out");
    }

    #[test]
    fn a_floor_shadow_is_squashed_towards_the_bottom_edge() {
        let (mut colour, sil, w, h) = square_planes(false);
        // Device space is document space here (no flip): the "bottom" of
        // the square in document terms is y = 20. Half height, no shear.
        let e = shadow([1.0, 0.0, 0.0, 0.5, 0.0, 10.0], 20.0, 0.0, 0.0);
        e.apply_at(&mut colour, Some(&sil), w, h, Transform2D::IDENTITY, (0, 0));
        let a = |x: usize, y: usize| colour[(y * w + x) * 4 + 3];
        assert_eq!(a(40, 25), 128);
        assert_eq!(a(40, 38), 128);
        assert_eq!(a(40, 45), 0, "above half its height");
    }

    #[test]
    fn a_singular_floor_casts_nothing_and_a_shadow_without_silhouette_is_left_alone() {
        let (mut colour, sil, w, h) = square_planes(false);
        let e = shadow([1.0, 0.0, 0.0, 0.0, 0.0, 20.0], 40.0, 0.0, 2.0);
        e.apply_at(&mut colour, Some(&sil), w, h, Transform2D::IDENTITY, (0, 0));
        assert!(colour.iter().all(|v| *v == 0));
        e.apply_at(&mut colour, None, w, h, Transform2D::IDENTITY, (0, 0));
        assert!(colour.iter().all(|v| *v == 0));
    }

    #[test]
    fn the_profile_bias_is_negated_as_the_original_does() {
        let run = |bias: f64| {
            let (mut colour, sil, w, h) = square_planes(false);
            let LayerEffect::Shadow(mut s) = shadow([1.0, 0.0, 0.0, 1.0, 0.0, 0.0], 0.0, 0.0, 16.0)
            else {
                unreachable!()
            };
            s.profile = Profile::new(bias, 0.0);
            LayerEffect::Shadow(s).apply_at(
                &mut colour,
                Some(&sil),
                w,
                h,
                Transform2D::IDENTITY,
                (0, 0),
            );
            colour[(40 * w + 20) * 4 + 3]
        };
        // On the outline the blurred silhouette is half covered; a positive
        // bias as stored darkens it, a negative one lightens it.
        let (neg, zero, pos) = (run(-0.5), run(0.0), run(0.5));
        assert!(neg < zero && zero < pos, "{neg} {zero} {pos}");
    }
}
