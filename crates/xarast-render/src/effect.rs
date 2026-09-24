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

use crate::blur::{self, Kernel, MAX_RADIUS_PX};
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
        }
    }

    /// How far the effect's output can reach beyond its content, in device
    /// pixels. A feather draws nothing outside what it wraps.
    #[must_use]
    pub fn growth_px(&self, _scale: f64) -> i32 {
        match self {
            LayerEffect::Feather { .. } => 0,
        }
    }

    /// Whether the effect needs its content's silhouette as well as its
    /// colour.
    #[must_use]
    pub const fn needs_silhouette(&self) -> bool {
        match self {
            LayerEffect::Feather { .. } => true,
        }
    }

    /// Applies the effect to rendered content, in place.
    ///
    /// `colour` is premultiplied RGBA8, `silhouette` one coverage byte per
    /// pixel (every paint opaque), both `width × height`. `scale` is device
    /// pixels per document unit.
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
}
