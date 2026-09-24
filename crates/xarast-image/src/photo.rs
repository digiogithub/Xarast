//! The pixel side of non-destructive photo adjustments (phase 10 W10.6,
//! T10.6.2–T10.6.4).
//!
//! This module knows nothing about documents: the chain itself, its
//! canonical order and its storage are `xarast_doc::photo`'s. What is here
//! is how a chain, once normalised, turns a master's pixels into the
//! derived image:
//!
//! 1. **Geometry** — a crop in master pixels, then a mirror and quarter
//!    turns ([`Recipe::crop`], [`Recipe::flip`], [`Recipe::turns`]). Pure
//!    copies: no pixel value changes.
//! 2. **One lookup table per channel** ([`Lut`], T10.6.4). Levels, gamma,
//!    brightness and contrast act on each channel alone, so any run of them
//!    is one function `u8 → u8` per channel. [`fuse`] evaluates the whole
//!    run in `f64` for each of the 256 inputs and rounds **once**, so a
//!    fused table is not the composition of eight-bit steps: it is more
//!    exact, and the order the user set the controls in does not matter
//!    once the chain is normalised.
//! 3. **Channel mixes** ([`Mix`]): saturation and greyscale read all three
//!    channels, so they run after the table, per pixel.
//!
//! Pixels are **straight** (not premultiplied) RGBA8, as the renderer's
//! `ImageRef` takes them; alpha is never changed. Everything is
//! deterministic: the same input gives the same bytes on every run, which
//! the renderer's pixel budget relies on to re-create an evicted derived
//! image.

/// One per-channel point operation, as [`fuse`] evaluates it. Values are
/// fractions of full scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointOp {
    /// Stretch `in_lo..=in_hi` onto `out_lo..=out_hi`, clipping outside.
    /// `channel` is `Some(0..=2)` for red, green or blue, `None` for all.
    Levels {
        /// Which channel, or all.
        channel: Option<u8>,
        /// Input black point.
        in_lo: u8,
        /// Input white point.
        in_hi: u8,
        /// Output black point.
        out_lo: u8,
        /// Output white point.
        out_hi: u8,
    },
    /// `v^(1/γ)`.
    Gamma(f32),
    /// `v + b`, clipped.
    Brightness(f32),
    /// `(v − ½)(1 + c) + ½`, clipped.
    Contrast(f32),
}

impl PointOp {
    /// The operation on one channel value in `0..=1`.
    #[must_use]
    pub fn apply(&self, channel: u8, v: f64) -> f64 {
        match *self {
            PointOp::Levels {
                channel: ch,
                in_lo,
                in_hi,
                out_lo,
                out_hi,
            } => {
                if ch.is_some_and(|c| c != channel) || in_hi <= in_lo {
                    return v;
                }
                let (il, ih) = (f64::from(in_lo), f64::from(in_hi));
                let t = ((v * 255.0 - il) / (ih - il)).clamp(0.0, 1.0);
                let (ol, oh) = (f64::from(out_lo), f64::from(out_hi));
                (ol + t * (oh - ol)) / 255.0
            }
            PointOp::Gamma(g) => {
                if g > 0.0 {
                    v.powf(1.0 / f64::from(g))
                } else {
                    v
                }
            }
            PointOp::Brightness(b) => (v + f64::from(b)).clamp(0.0, 1.0),
            PointOp::Contrast(c) => ((v - 0.5) * (1.0 + f64::from(c)) + 0.5).clamp(0.0, 1.0),
        }
    }
}

/// A lookup table per colour channel (red, green, blue).
#[derive(Clone, PartialEq, Eq)]
pub struct Lut(pub [[u8; 256]; 3]);

impl std::fmt::Debug for Lut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Lut(..)")
    }
}

impl Lut {
    /// The table that changes nothing.
    #[must_use]
    pub fn identity() -> Lut {
        let mut t = [0u8; 256];
        for (i, v) in t.iter_mut().enumerate() {
            *v = i as u8;
        }
        Lut([t; 3])
    }

    /// Whether it changes nothing.
    #[must_use]
    pub fn is_identity(&self) -> bool {
        *self == Lut::identity()
    }
}

/// The run of point operations `ops`, in order, as one table per channel:
/// each input level goes through every operation in `f64` and is rounded
/// once.
#[must_use]
pub fn fuse(ops: &[PointOp]) -> Lut {
    let mut lut = [[0u8; 256]; 3];
    for (ch, table) in lut.iter_mut().enumerate() {
        for (i, out) in table.iter_mut().enumerate() {
            let mut v = i as f64 / 255.0;
            for op in ops {
                v = op.apply(ch as u8, v);
            }
            *out = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    Lut(lut)
}

/// A channel-mixing operation, run per pixel after the table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mix {
    /// Distance from the luma scaled by `1 + s`.
    Saturation(f32),
    /// Every channel set to the luma.
    Greyscale,
}

/// Rec. 601 luma of a straight colour, in levels (`0.0..=255.0`): the
/// weights the contone fill uses too.
#[inline]
fn luma(r: u8, g: u8, b: u8) -> f32 {
    0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b)
}

#[inline]
fn level(v: f32) -> u8 {
    v.clamp(0.0, 255.0).round() as u8
}

impl Mix {
    #[inline]
    fn apply(self, px: &mut [u8]) {
        let (r, g, b) = (px[0], px[1], px[2]);
        let y = luma(r, g, b);
        match self {
            Mix::Greyscale => {
                let y = level(y);
                px[0] = y;
                px[1] = y;
                px[2] = y;
            }
            Mix::Saturation(s) => {
                let k = 1.0 + s;
                px[0] = level(y + (f32::from(r) - y) * k);
                px[1] = level(y + (f32::from(g) - y) * k);
                px[2] = level(y + (f32::from(b) - y) * k);
            }
        }
    }
}

/// A chain, normalised and translated for evaluation.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Recipe {
    /// `(x, y, width, height)` in master pixels; clamped to the master.
    pub crop: Option<(u32, u32, u32, u32)>,
    /// Mirror left to right (before turning).
    pub flip: bool,
    /// Quarter turns clockwise, `0..=3`.
    pub turns: u8,
    /// The fused point operations; `None` when there are none.
    pub lut: Option<Lut>,
    /// Channel mixes, in order.
    pub mix: Vec<Mix>,
}

impl Recipe {
    /// Whether evaluating it would copy the master unchanged.
    #[must_use]
    pub fn is_identity(&self, width: u32, height: u32) -> bool {
        self.crop
            .is_none_or(|(x, y, w, h)| x == 0 && y == 0 && w >= width && h >= height)
            && !self.flip
            && self.turns.is_multiple_of(4)
            && self.lut.as_ref().is_none_or(Lut::is_identity)
            && self.mix.is_empty()
    }

    /// The size of the image it makes of a `width` × `height` master.
    #[must_use]
    pub fn output_size(&self, width: u32, height: u32) -> Option<(u32, u32)> {
        let (_, _, w, h) = self.crop_rect(width, height)?;
        Some(if self.turns % 2 == 1 { (h, w) } else { (w, h) })
    }

    fn crop_rect(&self, width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
        let (x, y, w, h) = self.crop.unwrap_or((0, 0, width, height));
        let x1 = x.saturating_add(w).min(width);
        let y1 = y.saturating_add(h).min(height);
        if x >= x1 || y >= y1 {
            // A crop outside the image leaves the whole image.
            return (width > 0 && height > 0).then_some((0, 0, width, height));
        }
        Some((x, y, x1 - x, y1 - y))
    }
}

/// Runs `recipe` over a `width` × `height` straight-RGBA8 master:
/// `(width, height, pixels)` of the derived image. `None` when the master
/// is empty or `rgba` is not `width · height · 4` bytes.
#[must_use]
pub fn evaluate(
    width: u32,
    height: u32,
    rgba: &[u8],
    recipe: &Recipe,
) -> Option<(u32, u32, Vec<u8>)> {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || rgba.len() != w.checked_mul(h)?.checked_mul(4)? {
        return None;
    }
    let (cx, cy, cw, ch) = recipe.crop_rect(width, height)?;
    let (cx, cy, cw, ch) = (cx as usize, cy as usize, cw as usize, ch as usize);
    // Crop and mirror in one copy.
    let mut img = Vec::with_capacity(cw * ch * 4);
    for row in rgba.chunks_exact(w * 4).skip(cy).take(ch) {
        let src = &row[cx * 4..(cx + cw) * 4];
        if recipe.flip {
            for px in src.as_chunks::<4>().0.iter().rev() {
                img.extend_from_slice(px);
            }
        } else {
            img.extend_from_slice(src);
        }
    }
    let (mut iw, mut ih) = (cw, ch);
    match recipe.turns % 4 {
        0 => {}
        2 => rotate_180(&mut img),
        t => {
            img = rotate_quarter(&img, iw, ih, t == 1);
            std::mem::swap(&mut iw, &mut ih);
        }
    }
    let lut = recipe.lut.as_ref().filter(|l| !l.is_identity());
    if lut.is_some() || !recipe.mix.is_empty() {
        for px in img.as_chunks_mut::<4>().0 {
            if let Some(Lut([r, g, b])) = lut {
                px[0] = r[usize::from(px[0])];
                px[1] = g[usize::from(px[1])];
                px[2] = b[usize::from(px[2])];
            }
            for m in &recipe.mix {
                m.apply(px);
            }
        }
    }
    Some((iw as u32, ih as u32, img))
}

fn rotate_180(img: &mut [u8]) {
    let n = img.len() / 4;
    for i in 0..n / 2 {
        let j = n - 1 - i;
        for k in 0..4 {
            img.swap(i * 4 + k, j * 4 + k);
        }
    }
}

/// A `w` × `h` image turned a quarter turn: clockwise when `cw`.
fn rotate_quarter(img: &[u8], w: usize, h: usize, cw: bool) -> Vec<u8> {
    // The result is `h` wide and `w` tall.
    let mut out = vec![0u8; img.len()];
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = if cw { (h - 1 - y, x) } else { (y, w - 1 - x) };
            let s = (y * w + x) * 4;
            let d = (ny * h + nx) * 4;
            out[d..d + 4].copy_from_slice(&img[s..s + 4]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(w: u32, h: u32) -> Vec<u8> {
        (0..w * h)
            .flat_map(|i| [(i * 7) as u8, (i * 13) as u8, (i * 29) as u8, 200])
            .collect()
    }

    fn px(data: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * w + x) * 4) as usize;
        [data[i], data[i + 1], data[i + 2], data[i + 3]]
    }

    #[test]
    fn crop_and_turns_move_pixels_where_the_model_says() {
        let (w, h) = (5, 4);
        let src = img(w, h);
        let r = Recipe {
            crop: Some((1, 1, 3, 2)),
            turns: 1,
            ..Recipe::default()
        };
        let (ow, oh, out) = evaluate(w, h, &src, &r).unwrap();
        assert_eq!((ow, oh), (2, 3));
        // Clockwise: the output's top-left is the crop's bottom-left.
        assert_eq!(px(&out, ow, 0, 0), px(&src, w, 1, 2));
        assert_eq!(px(&out, ow, 1, 0), px(&src, w, 1, 1));
        assert_eq!(px(&out, ow, 0, 2), px(&src, w, 3, 2));
        let flipped = evaluate(
            w,
            h,
            &src,
            &Recipe {
                flip: true,
                ..Recipe::default()
            },
        )
        .unwrap();
        assert_eq!(px(&flipped.2, w, 0, 0), px(&src, w, 4, 0));
        let half = evaluate(
            w,
            h,
            &src,
            &Recipe {
                turns: 2,
                ..Recipe::default()
            },
        )
        .unwrap();
        assert_eq!(px(&half.2, w, 0, 0), px(&src, w, 4, 3));
        let ccw = evaluate(
            w,
            h,
            &src,
            &Recipe {
                turns: 3,
                ..Recipe::default()
            },
        )
        .unwrap();
        assert_eq!((ccw.0, ccw.1), (4, 5));
        // Anticlockwise: the output's top-left is the source's top-right.
        assert_eq!(px(&ccw.2, 4, 0, 0), px(&src, w, 4, 0));
    }

    #[test]
    fn the_fused_table_is_the_chain_rounded_once() {
        let ops = [
            PointOp::Levels {
                channel: Some(0),
                in_lo: 10,
                in_hi: 240,
                out_lo: 5,
                out_hi: 250,
            },
            PointOp::Gamma(1.3),
            PointOp::Brightness(0.05),
            PointOp::Contrast(0.2),
        ];
        let lut = fuse(&ops);
        for ch in 0..3u8 {
            for i in 0..=255u8 {
                let mut v = f64::from(i) / 255.0;
                for op in &ops {
                    v = op.apply(ch, v);
                }
                assert_eq!(
                    lut.0[usize::from(ch)][usize::from(i)],
                    (v * 255.0).round() as u8
                );
            }
        }
        // Levels on red only leave green's table without it.
        assert_ne!(lut.0[0], lut.0[1]);
        assert_eq!(lut.0[1], lut.0[2]);
        assert!(fuse(&[]).is_identity());
        assert!(fuse(&[PointOp::Gamma(1.0), PointOp::Brightness(0.0)]).is_identity());
    }

    #[test]
    fn mixes_keep_alpha_and_grey_stays_grey() {
        let src = vec![200, 100, 50, 77, 90, 90, 90, 255];
        let r = Recipe {
            mix: vec![Mix::Saturation(0.5)],
            ..Recipe::default()
        };
        let (_, _, out) = evaluate(2, 1, &src, &r).unwrap();
        assert_eq!(out[3], 77);
        assert_eq!(&out[4..8], &[90, 90, 90, 255]);
        assert!(out[0] > 200 && out[2] < 50, "more saturated: {out:?}");
        let g = Recipe {
            mix: vec![Mix::Greyscale],
            ..Recipe::default()
        };
        let (_, _, out) = evaluate(2, 1, &src, &g).unwrap();
        assert_eq!(out[0], out[1]);
        assert_eq!(out[1], out[2]);
        assert_eq!(out[0], level(luma(200, 100, 50)));
        let s = Recipe {
            mix: vec![Mix::Saturation(-1.0)],
            ..Recipe::default()
        };
        assert_eq!(
            evaluate(2, 1, &src, &s).unwrap().2,
            out,
            "−1 saturation is greyscale"
        );
    }

    #[test]
    fn bad_input_is_refused_and_a_crop_outside_is_ignored() {
        assert!(evaluate(0, 1, &[], &Recipe::default()).is_none());
        assert!(evaluate(2, 2, &[0; 15], &Recipe::default()).is_none());
        let src = img(3, 3);
        let r = Recipe {
            crop: Some((10, 10, 5, 5)),
            ..Recipe::default()
        };
        assert_eq!(evaluate(3, 3, &src, &r).unwrap().2, src);
        assert!(Recipe::default().is_identity(3, 3));
        assert!(!r.is_identity(3, 3) || r.crop_rect(3, 3) == Some((0, 0, 3, 3)));
    }
}
