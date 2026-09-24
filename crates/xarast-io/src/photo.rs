//! Photo adjustments baked into pixels (phase 10 W10.6).
//!
//! The one translation from a document's chain (`xarast_doc::PhotoOps`,
//! the model) to the pixel pipeline (`xarast_image::photo`, the
//! arithmetic). It lives here because this is the lowest crate that sees
//! both: the scene walker (`xarast-app`) evaluates through it for the
//! screen and every raster and PDF export, and the SVG exporter bakes
//! through it. So an adjusted bitmap is the same pixels everywhere.

use xarast_doc::photo::{LevelsChannel, PhotoOp, PhotoOps};
use xarast_image::photo::{Lut, Mix, PointOp, Recipe, evaluate, fuse};

/// What evaluating `ops` means: its evaluable part (unknown operations
/// render as if absent), normalised, as a [`Recipe`]. The point
/// operations are fused into one table (T10.6.4).
#[must_use]
pub fn recipe(ops: &PhotoOps) -> Recipe {
    let ops = ops.evaluable();
    let mut r = Recipe::default();
    let mut points: Vec<PointOp> = Vec::new();
    for op in &ops.ops {
        match op {
            PhotoOp::Crop(c) => r.crop = Some((c.x, c.y, c.width, c.height)),
            PhotoOp::Orient(o) => {
                r.turns = o.turns % 4;
                r.flip = o.flip;
            }
            PhotoOp::Levels(l) => points.push(PointOp::Levels {
                channel: match l.channel {
                    LevelsChannel::All => None,
                    LevelsChannel::Red => Some(0),
                    LevelsChannel::Green => Some(1),
                    LevelsChannel::Blue => Some(2),
                },
                in_lo: l.in_lo,
                in_hi: l.in_hi,
                out_lo: l.out_lo,
                out_hi: l.out_hi,
            }),
            PhotoOp::Gamma(g) => points.push(PointOp::Gamma(*g)),
            PhotoOp::Brightness(b) => points.push(PointOp::Brightness(*b)),
            PhotoOp::Contrast(c) => points.push(PointOp::Contrast(*c)),
            PhotoOp::Saturation(s) => r.mix.push(Mix::Saturation(*s)),
            PhotoOp::Greyscale => r.mix.push(Mix::Greyscale),
            PhotoOp::Unknown { .. } => {}
        }
    }
    if !points.is_empty() {
        let lut: Lut = fuse(&points);
        if !lut.is_identity() {
            r.lut = Some(lut);
        }
    }
    r
}

/// The derived image of a `width` × `height` straight-RGBA8 master under
/// `ops`: `(width, height, pixels)`. `None` when the master is empty or
/// malformed.
#[must_use]
pub fn bake(width: u32, height: u32, rgba: &[u8], ops: &PhotoOps) -> Option<(u32, u32, Vec<u8>)> {
    evaluate(width, height, rgba, &recipe(ops))
}

#[cfg(test)]
mod tests {
    use super::*;
    use xarast_doc::photo::{PhotoOrient, PixelRect};

    #[test]
    fn the_recipe_follows_the_normalised_chain() {
        let ops = PhotoOps {
            ops: vec![
                PhotoOp::Contrast(0.1),
                PhotoOp::Orient(PhotoOrient::CW),
                PhotoOp::Greyscale,
                PhotoOp::Crop(PixelRect {
                    x: 1,
                    y: 2,
                    width: 3,
                    height: 4,
                }),
                PhotoOp::Brightness(0.1),
            ],
        };
        let r = recipe(&ops);
        assert_eq!(r.crop, Some((1, 2, 3, 4)));
        assert_eq!((r.turns, r.flip), (1, false));
        assert_eq!(r.mix, vec![Mix::Greyscale]);
        // Brightness before contrast whatever order they were set in.
        let expect = fuse(&[PointOp::Brightness(0.1), PointOp::Contrast(0.1)]);
        assert_eq!(r.lut, Some(expect));
        assert!(recipe(&PhotoOps::new()).is_identity(10, 10));
    }

    /// The model's placement geometry (`PhotoOps::unit_to_master`, which
    /// `SetPhotoOps` moves the object with) and the pixels `bake` makes
    /// agree: every derived pixel is the master pixel under its centre,
    /// for all eight orientations and several crops.
    #[test]
    fn baked_pixels_sit_where_the_model_says() {
        let (w, h) = (7u32, 5u32);
        let master: Vec<u8> = (0..w * h)
            .flat_map(|i| [i as u8, (i * 3) as u8, 9, 255])
            .collect();
        let crops = [
            None,
            Some(PixelRect {
                x: 1,
                y: 1,
                width: 4,
                height: 3,
            }),
            Some(PixelRect {
                x: 5,
                y: 0,
                width: 9,
                height: 2,
            }),
        ];
        for crop in crops {
            for i in 0..8u8 {
                let mut ops = PhotoOps::new();
                if let Some(c) = crop {
                    ops.ops.push(PhotoOp::Crop(c));
                }
                ops.ops.push(PhotoOp::Orient(PhotoOrient {
                    turns: i % 4,
                    flip: i >= 4,
                }));
                let (dw, dh, out) = bake(w, h, &master, &ops).unwrap();
                assert_eq!((dw, dh), ops.derived_size(w, h));
                let m = ops.unit_to_master(w, h).unwrap();
                for y in 0..dh {
                    for x in 0..dw {
                        let (u, v) = (
                            (f64::from(x) + 0.5) / f64::from(dw),
                            (f64::from(y) + 0.5) / f64::from(dh),
                        );
                        let mx = (m[0] * u + m[2] * v + m[4]).floor() as usize;
                        let my = (m[1] * u + m[3] * v + m[5]).floor() as usize;
                        let d = ((y * dw + x) * 4) as usize;
                        let s = (my * w as usize + mx) * 4;
                        assert_eq!(out[d..d + 4], master[s..s + 4], "{ops:?} at {x},{y}");
                    }
                }
            }
        }
    }
}
