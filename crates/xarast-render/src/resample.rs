//! Image resampling: the one sampler behind image fills, placed bitmaps
//! and bitmap transparencies (phase 10, W10.4).
//!
//! # Where the light is averaged
//!
//! Compositing stays in encoded sRGB — every blend family is a table
//! defined there (`research/03 §2.10`). **Minification does not.** A
//! reduction is a weighted average of light, and averaging encoded values
//! visibly darkens a downscaled image (a 1-px black-and-white checker
//! minified twice becomes 128 instead of 188). So every minified sample,
//! and the pyramid, converts its texels to premultiplied linear light
//! through a 256-entry table, weights them there, and encodes the result
//! back with an exact inverse ([`encode`]): the exception `research/03
//! §3.7` grants to blur and high-quality scaling.
//!
//! **Magnification averages in encoded sRGB** ([`MAGNIFY_SPACE`]),
//! premultiplied. The phase document asked for linear light throughout;
//! the comparison harness (`tests/resampling.rs`) measured encoded
//! reconstruction better for every kernel on every test image, with less
//! ringing, and linear light leaks a bright neighbour into a dark texel
//! visibly (5.6 % of light is 66/255 encoded) when a tiny bitmap is blown
//! up. Point sampling converts nothing.
//!
//! # The three filters
//!
//! | [`Filter`] | magnification (≤ 1 texel per pixel) | minification |
//! |---|---|---|
//! | `Nearest` (Draft) | point | point, on the pyramid level the footprint picks |
//! | `Bilinear` (the original's smoothing flag) | bilinear | widened tent below 2×, then trilinear over the pyramid |
//! | `HighQuality` (Final, export) | [`HQ_KERNEL`] | widened tent below 2×, then trilinear over the pyramid |
//!
//! The footprint (texels per device pixel) comes from the Jacobian of the
//! device → texel mapping: once per primitive for an affine mapping, per
//! pixel for a perspective one. The pyramid is a 2 × 2 box in premultiplied
//! linear light (`reduce_level`), built on the walker's decode threads
//! ([`ImageRef::prepare`]) or else the first time an image is minified,
//! and shared by every clone of the [`ImageRef`]. A sampler pins the
//! levels its primitive can reach when it is made; a level the pixel
//! budget evicted comes back byte for byte (`pixel_budget`).
//!
//! # The aligned case
//!
//! An affine mapping that puts texel centres exactly on pixel centres, one
//! texel per pixel along each axis (mirroring allowed), samples every
//! filter as a point: the original smooths only when there is rotation or
//! scaling (`research/03 §2.8`), and no kernel should blur a bitmap drawn
//! at its own size. That case is also the fast path: texel indices are
//! integer arithmetic on the device pixel, no mapping per pixel.

use std::sync::LazyLock;

use xarast_color::Rgba8;

use crate::paint::{Filter, FrameMap, GradMapping, ImageRef, Repeat};
use crate::pixel_budget::{LevelBuf, MissingLevels, Pinned};
use crate::precision::Point64;
use crate::ramp::EffectSpace;

/// The magnification kernel of [`Filter::HighQuality`], chosen by the
/// comparison harness (`tests/resampling.rs`; the numbers are in
/// `docs/memory/render.md`, "Resampling quality"): the least ringing of
/// the cubic-or-better candidates, still sharper than bilinear.
pub const HQ_KERNEL: Kernel = Kernel::MitchellNetravali;

/// Where magnification averages (bilinear and [`HQ_KERNEL`]), chosen by
/// the same harness: encoded sRGB reconstructed better than linear light
/// for every kernel and rang less. Minification always averages in
/// linear light.
pub const MAGNIFY_SPACE: Space = Space::Encoded;

/// The sRGB transfer function, encoded `0..=1` to linear `0..=1`.
fn srgb_decode(c: f64) -> f64 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Encoded byte to linear light.
static TO_LINEAR: LazyLock<[f32; 256]> = LazyLock::new(|| {
    let mut t = [0.0f32; 256];
    for (i, v) in t.iter_mut().enumerate() {
        // f32-ok: a light level in 0..=1, not a coordinate.
        *v = srgb_decode(i as f64 / 255.0) as f32;
    }
    t
});

/// `THRESHOLDS[k]` is the linear value of the encoded level `k + ½`: a
/// linear value encodes to the number of thresholds at or below it, which
/// is rounding to the nearest encoded level, measured in encoded space.
static THRESHOLDS: LazyLock<[f32; 255]> = LazyLock::new(|| {
    let mut t = [0.0f32; 255];
    for (k, v) in t.iter_mut().enumerate() {
        // f32-ok: a light level in 0..=1, not a coordinate.
        *v = srgb_decode((k as f64 + 0.5) / 255.0) as f32;
    }
    t
});

/// The linear value of an encoded byte.
#[must_use]
#[inline]
pub fn to_linear(c: u8) -> f32 {
    TO_LINEAR[usize::from(c)]
}

/// Encoded byte to itself, as a fraction: the table of [`Space::Encoded`].
static TO_ENCODED: LazyLock<[f32; 256]> = LazyLock::new(|| {
    let mut t = [0.0f32; 256];
    for (i, v) in t.iter_mut().enumerate() {
        *v = f32::from(u8::try_from(i).unwrap_or(u8::MAX)) / 255.0;
    }
    t
});

/// Where a filter averages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Space {
    /// Linear light: the right average for minification.
    Linear,
    /// Encoded sRGB, as stored.
    Encoded,
}

impl Space {
    fn table(self) -> &'static [f32; 256] {
        match self {
            Space::Linear => &TO_LINEAR,
            Space::Encoded => &TO_ENCODED,
        }
    }

    #[inline]
    fn encode(self, x: f32) -> u8 {
        match self {
            Space::Linear => encode(x),
            // Clamped to 0..=255.5 first: the cast then rounds.
            Space::Encoded => {
                if x.is_nan() {
                    0
                } else {
                    (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
                }
            }
        }
    }
}

/// The encoded byte nearest a linear value: the exact inverse of
/// [`to_linear`] on its 256 values, and monotone everywhere. NaN encodes
/// to 0.
#[must_use]
#[inline]
pub fn encode(x: f32) -> u8 {
    // At most 255, so the cast is exact.
    THRESHOLDS.partition_point(|&t| t <= x) as u8
}

/// A premultiplied linear-light sample, `[r, g, b, a]`.
pub type Linear = [f32; 4];

/// Encodes a premultiplied linear sample as a straight sRGB colour.
/// Overshoot (a cubic kernel's ringing) is clamped here, not before.
#[must_use]
#[inline]
pub fn encode_premul(p: Linear) -> Rgba8 {
    encode_premul_in(p, Space::Linear)
}

/// [`encode_premul`] for a sample averaged in either space.
#[must_use]
#[inline]
pub fn encode_premul_in(p: Linear, space: Space) -> Rgba8 {
    let a = if p[3].is_nan() {
        0.0
    } else {
        p[3].clamp(0.0, 1.0)
    };
    // Non-negative and at most 255.5: truncation after adding ½ rounds.
    let a8 = (a * 255.0 + 0.5) as u8;
    if a8 == 0 {
        return Rgba8::TRANSPARENT;
    }
    let inv = 1.0 / a;
    Rgba8 {
        r: space.encode(p[0] * inv),
        g: space.encode(p[1] * inv),
        b: space.encode(p[2] * inv),
        a: a8,
    }
}

/// A straight sRGB texel in premultiplied linear light.
#[inline]
fn lin(t: &[f32; 256], c: Rgba8) -> Linear {
    let a = f32::from(c.a) / 255.0;
    [
        t[usize::from(c.r)] * a,
        t[usize::from(c.g)] * a,
        t[usize::from(c.b)] * a,
        a,
    ]
}

#[inline]
fn madd(acc: &mut Linear, p: Linear, w: f32) {
    acc[0] += p[0] * w;
    acc[1] += p[1] * w;
    acc[2] += p[2] * w;
    acc[3] += p[3] * w;
}

#[inline]
fn mix(a: Linear, b: Linear, t: f32) -> Linear {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

/// `floor` without the libm call the baseline x86-64 target makes, for a
/// value the caller has bounded; saturates like `as`.
#[inline]
fn floor_i64(x: f64) -> i64 {
    let t = x as i64;
    if (t as f64) > x { t - 1 } else { t }
}

/// A fraction in `0..1` as an `f32` weight.
#[inline]
fn weight32(x: f64) -> f32 {
    // f32-ok: a filter weight in -1..=1, not a coordinate.
    x as f32
}

/// Wraps a texel index into `0..n` according to the repeat mode.
#[inline]
pub(crate) fn wrap(v: i64, n: i64, repeat: Repeat) -> i64 {
    match repeat {
        Repeat::Simple => v.clamp(0, n - 1),
        Repeat::Repeat | Repeat::RepeatHq => v.rem_euclid(n),
        Repeat::Mirror => {
            let m = v.rem_euclid(2 * n);
            if m >= n { 2 * n - 1 - m } else { m }
        }
    }
}

/// One level of an image: straight sRGB RGBA8 rows, top row first.
#[derive(Debug, Clone, Copy)]
pub struct Level<'a> {
    /// Width in texels, at least 1.
    pub width: u32,
    /// Height in texels, at least 1.
    pub height: u32,
    /// `width · height · 4` bytes.
    pub data: &'a [u8],
}

impl Level<'_> {
    /// One texel, with the repeat mode applied to both axes.
    #[inline]
    #[must_use]
    pub fn texel(&self, x: i64, y: i64, repeat: Repeat) -> Rgba8 {
        let x = wrap(x, i64::from(self.width), repeat);
        let y = wrap(y, i64::from(self.height), repeat);
        let o = (y as usize * self.width as usize + x as usize) * 4;
        Rgba8 {
            r: self.data[o],
            g: self.data[o + 1],
            b: self.data[o + 2],
            a: self.data[o + 3],
        }
    }
}

/// One pyramid step: the level below a `width × height` level, each texel
/// the mean of a 2 × 2 block in premultiplied linear light. An odd edge
/// repeats its last row or column, so the result is `⌈w/2⌉ × ⌈h/2⌉`.
/// Returns `(width, height, data)`.
///
/// Deterministic: a level rebuilt after an eviction (`pixel_budget`) is
/// the same bytes as the first time.
pub(crate) fn reduce_level(w: u32, h: u32, src: &[u8]) -> (u32, u32, Vec<u8>) {
    let t = &*TO_LINEAR;
    let (nw, nh) = (w.div_ceil(2), h.div_ceil(2));
    let mut next = Vec::with_capacity(nw as usize * nh as usize * 4);
    let at = |x: u32, y: u32| -> Rgba8 {
        let o = (y as usize * w as usize + x as usize) * 4;
        Rgba8 {
            r: src[o],
            g: src[o + 1],
            b: src[o + 2],
            a: src[o + 3],
        }
    };
    for y in 0..nh {
        let (y0, y1) = (2 * y, (2 * y + 1).min(h - 1));
        for x in 0..nw {
            let (x0, x1) = (2 * x, (2 * x + 1).min(w - 1));
            let mut acc = [0.0f32; 4];
            madd(&mut acc, lin(t, at(x0, y0)), 0.25);
            madd(&mut acc, lin(t, at(x1, y0)), 0.25);
            madd(&mut acc, lin(t, at(x0, y1)), 0.25);
            madd(&mut acc, lin(t, at(x1, y1)), 0.25);
            let c = encode_premul(acc);
            next.extend_from_slice(&[c.r, c.g, c.b, c.a]);
        }
    }
    (nw, nh, next)
}

/// A reconstruction kernel. The product uses [`Kernel::Triangle`]
/// (bilinear) and [`HQ_KERNEL`]; the others are the candidates the
/// comparison harness measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kernel {
    /// The tent: bilinear.
    Triangle,
    /// Mitchell–Netravali, B = C = 1/3.
    MitchellNetravali,
    /// Catmull–Rom, B = 0, C = 1/2: interpolating.
    CatmullRom,
    /// Lanczos, three lobes.
    Lanczos3,
}

/// Every kernel, for the harness.
pub const ALL_KERNELS: [Kernel; 4] = [
    Kernel::Triangle,
    Kernel::MitchellNetravali,
    Kernel::CatmullRom,
    Kernel::Lanczos3,
];

impl Kernel {
    /// The support's half-width, in texels.
    #[must_use]
    pub const fn radius(self) -> f64 {
        match self {
            Kernel::Triangle => 1.0,
            Kernel::MitchellNetravali | Kernel::CatmullRom => 2.0,
            Kernel::Lanczos3 => 3.0,
        }
    }

    /// The kernel's weight at a distance.
    #[must_use]
    pub fn weight(self, x: f64) -> f64 {
        let x = x.abs();
        match self {
            Kernel::Triangle => (1.0 - x).max(0.0),
            Kernel::MitchellNetravali => bc_cubic(1.0 / 3.0, 1.0 / 3.0, x),
            Kernel::CatmullRom => bc_cubic(0.0, 0.5, x),
            Kernel::Lanczos3 => {
                if x >= 3.0 {
                    0.0
                } else if x < 1e-12 {
                    1.0
                } else {
                    let px = std::f64::consts::PI * x;
                    3.0 * px.sin() * (px / 3.0).sin() / (px * px)
                }
            }
        }
    }

    /// A short name for reports.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Kernel::Triangle => "bilinear",
            Kernel::MitchellNetravali => "mitchell",
            Kernel::CatmullRom => "catmull-rom",
            Kernel::Lanczos3 => "lanczos3",
        }
    }
}

/// The Mitchell–Netravali family of cubics at `x ≥ 0`.
fn bc_cubic(b: f64, c: f64, x: f64) -> f64 {
    let (x2, x3) = (x * x, x * x * x);
    if x < 1.0 {
        ((12.0 - 9.0 * b - 6.0 * c) * x3 + (-18.0 + 12.0 * b + 6.0 * c) * x2 + (6.0 - 2.0 * b))
            / 6.0
    } else if x < 2.0 {
        ((-b - 6.0 * c) * x3
            + (6.0 * b + 30.0 * c) * x2
            + (-12.0 * b - 48.0 * c) * x
            + (8.0 * b + 24.0 * c))
            / 6.0
    } else {
        0.0
    }
}

/// Texel coordinates beyond this are not sampled (and NaN is not): far
/// inside `i64`, so no index arithmetic can overflow.
const COORD_LIMIT: f64 = 1e15;

/// The most taps per axis [`kernel_sample`] takes; a wider support is
/// narrowed to fit.
const MAX_TAPS: usize = 128;

/// One axis's taps: first index, count, normalised weights.
fn taps(kernel: Kernel, x: f64, stretch: f64, w: &mut [f32; MAX_TAPS]) -> (i64, usize) {
    let max_stretch = (MAX_TAPS as f64 - 2.0) / (2.0 * kernel.radius());
    let s = if stretch.is_finite() {
        stretch.clamp(1.0, max_stretch)
    } else {
        1.0
    };
    let r = kernel.radius() * s;
    // The caller keeps |x| below 2^50, so none of this saturates.
    let first = floor_i64(x - r) + 1;
    let last = floor_i64(x + r);
    let n = usize::try_from(last - first + 1).unwrap_or(0).min(MAX_TAPS);
    let mut sum = 0.0;
    for (i, slot) in w.iter_mut().enumerate().take(n) {
        let k = kernel.weight(((first + i as i64) as f64 - x) / s);
        *slot = weight32(k);
        sum += k;
    }
    if sum.abs() > 1e-12 && (sum - 1.0).abs() > 1e-12 {
        let inv = 1.0 / sum;
        for slot in w.iter_mut().take(n) {
            *slot = weight32(f64::from(*slot) * inv);
        }
    }
    (first, n)
}

/// Samples a level with a separable kernel at texel coordinates `(x, y)`
/// (texel centres at integers), the kernel widened by `stretch` ≥ 1 (a
/// prefilter for minification). Returns premultiplied linear light,
/// **unclamped**, so that ringing can be measured.
#[must_use]
pub fn kernel_sample(
    level: Level<'_>,
    repeat: Repeat,
    kernel: Kernel,
    x: f64,
    y: f64,
    stretch: f64,
) -> Linear {
    kernel_sample_remapped(level, repeat, None, kernel, x, y, stretch, Space::Linear)
}

/// [`kernel_sample`] averaging in either space; encode the result with
/// [`encode_premul_in`] in the same space.
#[must_use]
pub fn kernel_sample_in(
    level: Level<'_>,
    repeat: Repeat,
    kernel: Kernel,
    x: f64,
    y: f64,
    stretch: f64,
    space: Space,
) -> Linear {
    kernel_sample_remapped(level, repeat, None, kernel, x, y, stretch, space)
}

#[allow(
    clippy::too_many_arguments,
    reason = "the public entries fix most of them"
)]
fn kernel_sample_remapped(
    level: Level<'_>,
    repeat: Repeat,
    remap: Option<&[Rgba8; 256]>,
    kernel: Kernel,
    x: f64,
    y: f64,
    stretch: f64,
    space: Space,
) -> Linear {
    if !(x.abs() < COORD_LIMIT && y.abs() < COORD_LIMIT) {
        return [0.0; 4];
    }
    let t = space.table();
    let mut wx = [0.0f32; MAX_TAPS];
    let mut wy = [0.0f32; MAX_TAPS];
    let (fx, nx) = taps(kernel, x, stretch, &mut wx);
    let (fy, ny) = taps(kernel, y, stretch, &mut wy);
    let mut acc = [0.0f32; 4];
    for (j, &wyj) in wy.iter().enumerate().take(ny) {
        if wyj == 0.0 {
            continue;
        }
        let mut row = [0.0f32; 4];
        let ty = fy.saturating_add(j as i64);
        for (i, &wxi) in wx.iter().enumerate().take(nx) {
            if wxi == 0.0 {
                continue;
            }
            let c = fetch(level, remap, fx.saturating_add(i as i64), ty, repeat);
            madd(&mut row, lin(t, c), wxi);
        }
        madd(&mut acc, row, wyj);
    }
    acc
}

#[inline]
fn fetch(level: Level<'_>, remap: Option<&[Rgba8; 256]>, x: i64, y: i64, repeat: Repeat) -> Rgba8 {
    let c = level.texel(x, y, repeat);
    match remap {
        Some(lut) => remap_texel(lut, c),
        None => c,
    }
}

#[inline]
fn remap_texel(lut: &[Rgba8; 256], c: Rgba8) -> Rgba8 {
    let y = crate::blend::LumaWeights::BT601.luma(c);
    Rgba8 {
        a: c.a,
        ..lut[usize::from(y)]
    }
}

/// Bilinear at texel coordinates, in premultiplied linear light.
#[inline]
fn bilinear(
    t: &[f32; 256],
    level: Level<'_>,
    remap: Option<&[Rgba8; 256]>,
    x: f64,
    y: f64,
    repeat: Repeat,
) -> Linear {
    let (x0, y0) = (floor_i64(x), floor_i64(y));
    let (fx, fy) = (weight32(x - x0 as f64), weight32(y - y0 as f64));
    let (x1, y1) = (x0.saturating_add(1), y0.saturating_add(1));
    let top = mix(
        lin(t, fetch(level, remap, x0, y0, repeat)),
        lin(t, fetch(level, remap, x1, y0, repeat)),
        fx,
    );
    let bottom = mix(
        lin(t, fetch(level, remap, x0, y1, repeat)),
        lin(t, fetch(level, remap, x1, y1, repeat)),
        fx,
    );
    mix(top, bottom, fy)
}

/// Texels per device pixel for a Jacobian `[du/dx, du/dy, dv/dx, dv/dy]`
/// of the device → unit-square mapping and an image of `w × h`: the
/// longer of the two pixel axes' footprints.
#[must_use]
pub fn footprint(j: [f64; 4], w: f64, h: f64) -> f64 {
    let (a, b, c, d) = (j[0] * w, j[1] * w, j[2] * h, j[3] * h);
    (a * a + c * c).max(b * b + d * d).sqrt()
}

/// Where a footprint samples.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Lod {
    /// At most one texel per pixel: the magnification kernel on the base.
    Magnify,
    /// Between one and two texels per pixel: the tent widened by the
    /// footprint, on the base. The harness measured trilinear worse than
    /// this at ÷1.5 on all four test images; see `render.md`.
    Widen(f64),
    /// Two texels per pixel or more: trilinear between `level` and
    /// `level + 1`.
    Minify { level: usize, frac: f32 },
}

/// The level of detail for a footprint; `levels` (which builds the
/// pyramid) is only asked for at two texels per pixel or more.
fn lod_of(rho: f64, levels: impl FnOnce() -> usize) -> Lod {
    if rho.is_nan() || rho <= 1.0 {
        return Lod::Magnify;
    }
    if rho < 2.0 {
        return Lod::Widen(rho);
    }
    let l = rho.log2();
    let last = levels().saturating_sub(1);
    if !l.is_finite() || l >= last as f64 {
        return Lod::Minify {
            level: last,
            frac: 0.0,
        };
    }
    // 1 <= l < last.
    let level = l as usize;
    Lod::Minify {
        level,
        frac: weight32(l - level as f64),
    }
}

/// The level a `Nearest` sampler points into: the one trilinear would
/// start from, so a minified Draft frame reads a reduction the size of
/// what it draws and never the base (XARA-T-0281).
fn nearest_level(lod: Lod) -> usize {
    match lod {
        Lod::Minify { level, .. } => level,
        Lod::Magnify | Lod::Widen(_) => 0,
    }
}

/// How a sampler maps pixels to texels.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Plan {
    /// Point sampling through the mapping (the `Nearest` filter), in one
    /// pyramid level chosen by the footprint: the base unless minified by
    /// two or more.
    Nearest { level: usize },
    /// Point sampling through a perspective mapping, the level chosen by
    /// the footprint per pixel.
    NearestPerPixel,
    /// Texel centres on pixel centres: `tx = sx·x + ox`, `ty = sy·y + oy`.
    Aligned { sx: i64, ox: i64, sy: i64, oy: i64 },
    /// An affine mapping: one level of detail for the whole primitive.
    Fixed(Lod),
    /// A perspective mapping: the level of detail per pixel.
    PerPixel,
    /// The levels the plan needed were evicted and the render would not
    /// wait for them ([`MissingLevels::Substitute`]): the only pinned
    /// level, a smaller resident one, point-sampled for `Nearest` and
    /// bilinear otherwise. Never byte-identical by design; the caller
    /// redraws once the base is back.
    Fallback,
}

/// How close to exact the aligned case must be, relative per axis: over
/// the 65 535-texel maximum image this drifts a thousandth of a texel.
const ALIGN_EPS: f64 = 1.5e-8;
/// How close to a texel centre the first pixel centre must land.
const ALIGN_OFFSET_EPS: f64 = 1e-6;

/// Recognises the aligned case from an affine frame; see the module docs.
fn aligned(frame: &FrameMap, w: f64, h: f64) -> Option<(i64, i64, i64, i64)> {
    let origin = Point64::new(0.5, 0.5);
    let j = frame.jacobian(origin)?;
    let (a, b, c, d) = (j[0] * w, j[1] * w, j[2] * h, j[3] * h);
    let unit = |s: f64| (s.abs() - 1.0).abs() <= ALIGN_EPS;
    if !(unit(a) && unit(d) && b.abs() <= ALIGN_EPS && c.abs() <= ALIGN_EPS) {
        return None;
    }
    let (u, v) = frame.apply(origin)?;
    let (tx, ty) = (u * w - 0.5, v * h - 0.5);
    if !(tx.abs() < 1e15 && ty.abs() < 1e15) {
        return None;
    }
    let (rx, ry) = (floor_i64(tx + 0.5), floor_i64(ty + 0.5));
    if (tx - rx as f64).abs() > ALIGN_OFFSET_EPS || (ty - ry as f64).abs() > ALIGN_OFFSET_EPS {
        return None;
    }
    let (sx, sy) = (if a > 0.0 { 1 } else { -1 }, if d > 0.0 { 1 } else { -1 });
    Some((sx, rx, sy, ry))
}

/// The contone remap as a table over luminance: one entry per level,
/// alpha taken from the texel.
fn contone_table(start: Rgba8, end: Rgba8, space: EffectSpace) -> Box<[Rgba8; 256]> {
    let effect = match space {
        EffectSpace::Rgb => xarast_color::FillEffect::Fade,
        EffectSpace::HsvShort => xarast_color::FillEffect::Rainbow,
        EffectSpace::HsvLong => xarast_color::FillEffect::AltRainbow,
    };
    let (s, e) = (
        xarast_color::ColourValue::from_rgba8(start),
        xarast_color::ColourValue::from_rgba8(end),
    );
    let mut lut = Box::new([Rgba8::TRANSPARENT; 256]);
    for (y, slot) in lut.iter_mut().enumerate() {
        let t = f32::from(u8::try_from(y).unwrap_or(u8::MAX)) / 255.0;
        *slot = xarast_color::interpolate(s, e, t, effect).to_rgba8();
    }
    lut
}

/// An image readied for sampling at many device points: the mapping
/// inverted, the level of detail chosen and the contone table built once
/// per primitive.
#[derive(Debug, Clone)]
pub struct ImageSampler<'a> {
    img: &'a ImageRef,
    /// The levels this primitive can reach, pinned once: `first..` of the
    /// image's pyramid. Holding them keeps them alive through an eviction
    /// (`pixel_budget`), and sampling takes no lock.
    pins: Vec<LevelBuf>,
    first: usize,
    frame: FrameMap,
    repeat: Repeat,
    filter: Filter,
    remap: Option<Box<[Rgba8; 256]>>,
    plan: Plan,
}

impl<'a> ImageSampler<'a> {
    /// Readies an image mapped onto the unit square by `mapping` (device
    /// space). `None` for a degenerate mapping or an empty image.
    #[must_use]
    pub fn new(
        img: &'a ImageRef,
        mapping: GradMapping,
        repeat: Repeat,
        filter: Filter,
        contone: Option<(Rgba8, Rgba8, EffectSpace)>,
    ) -> Option<ImageSampler<'a>> {
        ImageSampler::with_missing(
            img,
            mapping,
            repeat,
            filter,
            contone,
            MissingLevels::Materialise,
        )
    }

    /// [`ImageSampler::new`], with what to do when a level it needs was
    /// evicted and needs the base back (`pixel_budget`, "Drawing without
    /// waiting").
    #[must_use]
    pub fn with_missing(
        img: &'a ImageRef,
        mapping: GradMapping,
        repeat: Repeat,
        filter: Filter,
        contone: Option<(Rgba8, Rgba8, EffectSpace)>,
        missing: MissingLevels,
    ) -> Option<ImageSampler<'a>> {
        if img.width() == 0 || img.height() == 0 {
            return None;
        }
        let frame = mapping.frame_map()?;
        let (w, h) = (f64::from(img.width()), f64::from(img.height()));
        let fixed_lod = || {
            let rho = frame
                .jacobian(Point64::new(0.5, 0.5))
                .map_or(1.0, |j| footprint(j, w, h));
            lod_of(rho, || img.level_count())
        };
        let plan = if let Some((sx, ox, sy, oy)) =
            frame.is_affine().then(|| aligned(&frame, w, h)).flatten()
        {
            Plan::Aligned { sx, ox, sy, oy }
        } else if filter == Filter::Nearest {
            if frame.is_affine() {
                Plan::Nearest {
                    level: nearest_level(fixed_lod()),
                }
            } else {
                Plan::NearestPerPixel
            }
        } else if frame.is_affine() {
            Plan::Fixed(fixed_lod())
        } else {
            Plan::PerPixel
        };
        // The levels the plan can touch: the base for everything but a
        // minification, which reads one reduced level (point) or two
        // adjacent ones (trilinear); a perspective plane may reach any
        // level.
        let (first, last) = match plan {
            Plan::Fixed(Lod::Minify { level, .. }) => (level, level + 1),
            Plan::Nearest { level } => (level, level),
            Plan::PerPixel | Plan::NearestPerPixel => (0, img.level_count() - 1),
            Plan::Aligned { .. } | Plan::Fixed(_) | Plan::Fallback => (0, 0),
        };
        let (plan, first, pins) = match img.pin_levels(first, last, missing) {
            Pinned::Levels(pins) => (plan, first, pins),
            Pinned::Substitute(level, pin) => (Plan::Fallback, level, vec![pin]),
        };
        Some(ImageSampler {
            img,
            pins,
            first,
            frame,
            repeat,
            filter,
            remap: contone.map(|(s, e, sp)| contone_table(s, e, sp)),
            plan,
        })
    }

    /// A pinned level; `i` is always in the pinned range by construction.
    #[inline]
    fn lvl(&self, i: usize) -> Level<'_> {
        let k = i.saturating_sub(self.first).min(self.pins.len() - 1);
        self.pins[k].as_level()
    }

    /// Whether the sampler takes the aligned fast path.
    #[must_use]
    pub fn is_aligned(&self) -> bool {
        matches!(self.plan, Plan::Aligned { .. })
    }

    /// The straight sRGB colour at a device point (a pixel centre).
    #[must_use]
    #[inline]
    pub fn sample(&self, p: Point64) -> Rgba8 {
        let remap = self.remap.as_deref();
        match self.plan {
            Plan::Aligned { sx, ox, sy, oy } => {
                let (x, y) = (floor_i64(p.x), floor_i64(p.y));
                fetch(
                    self.lvl(0),
                    remap,
                    sx.saturating_mul(x).saturating_add(ox),
                    sy.saturating_mul(y).saturating_add(oy),
                    self.repeat,
                )
            }
            Plan::Nearest { level } => {
                let Some((u, v)) = self.frame.apply(p) else {
                    return Rgba8::TRANSPARENT;
                };
                self.point(u, v, level)
            }
            Plan::NearestPerPixel => {
                let Some((u, v)) = self.frame.apply(p) else {
                    return Rgba8::TRANSPARENT;
                };
                let (w, h) = (f64::from(self.img.width()), f64::from(self.img.height()));
                let rho = self.frame.jacobian(p).map_or(1.0, |j| footprint(j, w, h));
                self.point(u, v, nearest_level(lod_of(rho, || self.img.level_count())))
            }
            Plan::Fixed(lod) => {
                let Some((u, v)) = self.frame.apply(p) else {
                    return Rgba8::TRANSPARENT;
                };
                self.filtered(u, v, lod)
            }
            Plan::PerPixel => {
                let Some((u, v)) = self.frame.apply(p) else {
                    return Rgba8::TRANSPARENT;
                };
                let (w, h) = (f64::from(self.img.width()), f64::from(self.img.height()));
                let rho = self.frame.jacobian(p).map_or(1.0, |j| footprint(j, w, h));
                let lod = lod_of(rho, || self.img.level_count());
                self.filtered(u, v, lod)
            }
            Plan::Fallback => {
                let Some((u, v)) = self.frame.apply(p) else {
                    return Rgba8::TRANSPARENT;
                };
                if self.filter == Filter::Nearest {
                    return self.point(u, v, self.first);
                }
                if !(u.abs() < 1e9 && v.abs() < 1e9) {
                    return Rgba8::TRANSPARENT;
                }
                let l = self.lvl(self.first);
                let (x, y) = (u * f64::from(l.width) - 0.5, v * f64::from(l.height) - 0.5);
                encode_premul_in(
                    bilinear(MAGNIFY_SPACE.table(), l, remap, x, y, self.repeat),
                    MAGNIFY_SPACE,
                )
            }
        }
    }

    /// The texel of pinned level `level` nearest to `(u, v)`. On the
    /// base this is exactly the old base-only point sampling.
    fn point(&self, u: f64, v: f64, level: usize) -> Rgba8 {
        let l = self.lvl(level);
        let (x, y) = (u * f64::from(l.width) - 0.5, v * f64::from(l.height) - 0.5);
        fetch(
            l,
            self.remap.as_deref(),
            x.round() as i64,
            y.round() as i64,
            self.repeat,
        )
    }

    fn filtered(&self, u: f64, v: f64, lod: Lod) -> Rgba8 {
        // A unit-square coordinate this far out is past any tile the
        // 65 535-texel maximum image can have; it also keeps every texel
        // index below `COORD_LIMIT`.
        if !(u.abs() < 1e9 && v.abs() < 1e9) {
            return Rgba8::TRANSPARENT;
        }
        let remap = self.remap.as_deref();
        let at = |level: usize, space: Space| -> Linear {
            let l = self.lvl(level);
            let (x, y) = (u * f64::from(l.width) - 0.5, v * f64::from(l.height) - 0.5);
            bilinear(space.table(), l, remap, x, y, self.repeat)
        };
        let base = || {
            let l = self.lvl(0);
            let (x, y) = (u * f64::from(l.width) - 0.5, v * f64::from(l.height) - 0.5);
            (l, x, y)
        };
        match lod {
            Lod::Magnify => {
                let p = if self.filter == Filter::HighQuality {
                    let (l, x, y) = base();
                    kernel_sample_remapped(
                        l,
                        self.repeat,
                        remap,
                        HQ_KERNEL,
                        x,
                        y,
                        1.0,
                        MAGNIFY_SPACE,
                    )
                } else {
                    at(0, MAGNIFY_SPACE)
                };
                encode_premul_in(p, MAGNIFY_SPACE)
            }
            Lod::Widen(rho) => {
                let (l, x, y) = base();
                encode_premul(kernel_sample_remapped(
                    l,
                    self.repeat,
                    remap,
                    Kernel::Triangle,
                    x,
                    y,
                    rho,
                    Space::Linear,
                ))
            }
            Lod::Minify { level, frac } => {
                let lo = at(level, Space::Linear);
                encode_premul(if frac == 0.0 || level + 1 >= self.img.level_count() {
                    lo
                } else {
                    mix(lo, at(level + 1, Space::Linear), frac)
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_inverts_the_table_and_is_monotone() {
        for c in 0..=255u8 {
            assert_eq!(encode(to_linear(c)), c);
        }
        let mut last = 0u8;
        for i in 0..=u16::MAX {
            let x = f32::from(i) / 65_535.0;
            let e = encode(x);
            assert!(e >= last);
            last = e;
        }
        assert_eq!(encode(-1.0), 0);
        assert_eq!(encode(2.0), 255);
        assert_eq!(encode(f32::NAN), 0);
    }

    #[test]
    fn encoding_rounds_in_encoded_space() {
        // Halfway, in encoded space, between two levels.
        for k in 0..255u32 {
            let mid = srgb_decode((f64::from(k) + 0.5) / 255.0);
            // f32-ok: a light level, not a coordinate.
            let below = (mid * (1.0 - 1e-5)) as f32;
            // f32-ok: a light level, not a coordinate.
            let above = (mid * (1.0 + 1e-5) + 1e-9) as f32;
            assert_eq!(u32::from(encode(below)), k);
            assert_eq!(u32::from(encode(above)), k + 1);
        }
    }

    #[test]
    fn a_premultiplied_texel_round_trips() {
        let t = &*TO_LINEAR;
        for a in [1u8, 7, 128, 254, 255] {
            for c in [0u8, 1, 17, 128, 200, 255] {
                let px = Rgba8 {
                    r: c,
                    g: 255 - c,
                    b: c / 2,
                    a,
                };
                assert_eq!(encode_premul(lin(t, px)), px, "{px:?}");
            }
        }
        assert_eq!(
            encode_premul(lin(t, Rgba8::TRANSPARENT)),
            Rgba8::TRANSPARENT
        );
    }

    #[test]
    fn a_checker_minifies_to_the_mean_of_its_light() {
        // 2×2 black/white checker → one texel: the mean of the light is
        // 0.5, which encodes to 188, not the 128 of an encoded mean.
        let data = [
            0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255,
        ];
        let (w, h, d) = reduce_level(2, 2, &data);
        assert_eq!((w, h), (1, 1));
        assert_eq!(d, vec![188, 188, 188, 255]);
    }

    #[test]
    fn the_pyramid_goes_down_to_one_texel_through_odd_sizes() {
        let (w, h) = (13u32, 5u32);
        let data = vec![200u8; (w * h * 4) as usize];
        let image = ImageRef::new(w, h, data);
        let dims: Vec<_> = (1..image.level_count())
            .map(|i| {
                let l = image.level(i);
                // A flat image stays flat.
                assert!(l.data.iter().all(|&b| b == 200));
                (l.width, l.height)
            })
            .collect();
        assert_eq!(dims, vec![(7, 3), (4, 2), (2, 1), (1, 1)]);
    }

    #[test]
    fn transparent_texels_do_not_bleed_their_colour() {
        // Red opaque beside a transparent texel whose stored colour is
        // green: a straight-alpha average would tint the edge green.
        let data = [255, 0, 0, 255, 0, 255, 0, 0];
        let level = Level {
            width: 2,
            height: 1,
            data: &data,
        };
        let s = encode_premul(bilinear(&TO_LINEAR, level, None, 0.5, 0.0, Repeat::Simple));
        assert_eq!((s.r, s.g, s.b), (255, 0, 0));
        assert_eq!(s.a, 128);
    }

    #[test]
    fn interpolating_kernels_reproduce_texels_at_their_centres() {
        let data: Vec<u8> = (0..16u8)
            .flat_map(|i| [i * 15, 255 - i * 15, i * 7, 255])
            .collect();
        let level = Level {
            width: 4,
            height: 4,
            data: &data,
        };
        for k in [Kernel::Triangle, Kernel::CatmullRom, Kernel::Lanczos3] {
            for y in 0..4 {
                for x in 0..4 {
                    let s = encode_premul(kernel_sample(
                        level,
                        Repeat::Repeat,
                        k,
                        f64::from(x),
                        f64::from(y),
                        1.0,
                    ));
                    assert_eq!(s, level.texel(i64::from(x), i64::from(y), Repeat::Repeat));
                }
            }
        }
    }

    #[test]
    fn every_kernel_passes_a_flat_image_unchanged() {
        let data = [90u8, 140, 30, 255].repeat(36);
        let level = Level {
            width: 6,
            height: 6,
            data: &data,
        };
        for k in ALL_KERNELS {
            for stretch in [1.0, 1.7, 4.0] {
                let s = encode_premul(kernel_sample(level, Repeat::Simple, k, 2.3, 1.6, stretch));
                assert_eq!((s.r, s.g, s.b, s.a), (90, 140, 30, 255), "{k:?} ×{stretch}");
            }
        }
    }

    #[test]
    fn a_nearest_sampler_points_into_the_level_the_footprint_picks() {
        assert_eq!(nearest_level(Lod::Magnify), 0);
        assert_eq!(nearest_level(Lod::Widen(1.9)), 0);
        assert_eq!(nearest_level(lod_of(3.0, || 8)), 1);
        assert_eq!(nearest_level(lod_of(4.0, || 8)), 2);
        // A 64² image of four flat quadrants on 16 px (four texels per
        // pixel): every sample is a texel of level 2, where each
        // quadrant is still flat, so the picture is the quadrants.
        let mut data = Vec::new();
        for y in 0..64u32 {
            for x in 0..64u32 {
                let q = u8::from(x >= 32) + 2 * u8::from(y >= 32);
                data.extend_from_slice(&[q * 60, 255 - q * 60, 40, 255]);
            }
        }
        let image = ImageRef::new(64, 64, data);
        let mapping = GradMapping::Affine {
            a: Point64::new(0.0, 0.0),
            b: Point64::new(0.0, 16.0),
            c: Point64::new(16.0, 0.0),
        };
        let s = ImageSampler::new(&image, mapping, Repeat::Simple, Filter::Nearest, None)
            .expect("sampler");
        assert_eq!(s.plan, Plan::Nearest { level: 2 });
        assert_eq!((s.first, s.pins.len()), (2, 1), "the base is not pinned");
        let at = |x: f64, y: f64| s.sample(Point64::new(x, y));
        assert_eq!(at(2.5, 2.5), image.texel(0, 0, Repeat::Simple));
        assert_eq!(at(13.5, 2.5), image.texel(63, 0, Repeat::Simple));
        assert_eq!(at(2.5, 13.5), image.texel(0, 63, Repeat::Simple));
        assert_eq!(at(13.5, 13.5), image.texel(63, 63, Repeat::Simple));
    }

    #[test]
    fn the_level_of_detail_follows_the_footprint() {
        let never = || -> usize { panic!("the pyramid is not needed below 2×") };
        assert_eq!(lod_of(0.5, never), Lod::Magnify);
        assert_eq!(lod_of(1.0, never), Lod::Magnify);
        assert_eq!(lod_of(f64::NAN, never), Lod::Magnify);
        assert_eq!(lod_of(1.5, never), Lod::Widen(1.5));
        assert_eq!(
            lod_of(2.0, || 8),
            Lod::Minify {
                level: 1,
                frac: 0.0
            }
        );
        assert_eq!(
            lod_of(4.0, || 8),
            Lod::Minify {
                level: 2,
                frac: 0.0
            }
        );
        match lod_of(3.0, || 8) {
            Lod::Minify { level: 1, frac } => assert!((frac - 0.585).abs() < 1e-3),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            lod_of(1e9, || 4),
            Lod::Minify {
                level: 3,
                frac: 0.0
            }
        );
    }
}
