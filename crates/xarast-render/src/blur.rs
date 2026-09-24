//! The one blur every live effect shares (phase 13, workstream B).
//!
//! Shadow, feather, bevel and the generic effect chain are different user
//! interfaces over the same two primitives: render a subtree to an
//! offscreen alpha plane ([`crate::layer`]) and blur it. This module is the
//! second primitive, built once so that there is one blur, one set of edge
//! rules and one normalisation, not four.
//!
//! # Two kernels, one API
//!
//! * [`Kernel::Disc`] is what a `.xar` file's appearance was authored
//!   against: `research/03 §2.9` records the original's blur as a
//!   convolution with a **disc** of radius at most 100 px, normalised by the
//!   disc's area. That is the kernel Xarast renders with.
//! * [`Kernel::Gaussian`] is what SVG's `feGaussianBlur` does, which is
//!   what an external viewer draws from a baked `.xarast` filter. The
//!   interchange convention that relates the two is **σ = radius / 2**
//!   (`research/06 §6.8.1`: `xarast:blur="6.2"` bakes to
//!   `stdDeviation="3.1"`), [`sigma_for_disc_radius`].
//!
//! # Determinism
//!
//! Both kernels are exact integer arithmetic over `u8` input: the disc sums
//! coverage with per-row prefix sums and divides by its area with rounding;
//! the Gaussian quantises its weights to sum to exactly 2¹⁶ and keeps a
//! `u16` intermediate between its two passes (`research/03 §3.7`: more
//! than 8 bits between passes, so that wide blurs do not band). Rows run
//! in parallel with `rayon`, and each output row depends only on its inputs,
//! so the thread count cannot change a byte.
//!
//! # The disc
//!
//! A pixel `(dx, dy)` away from the centre is inside the disc when
//! `dx² + dy² ≤ r²`. The disc is centred on a pixel, so it is symmetric and
//! never shifts the picture by half a pixel; the original draws a disc of
//! integer *diameter* and compensates an even diameter's half-pixel shift
//! elsewhere (`Kernel/fthrattr.cpp`, "BlurringWillOffsetBitmap"). The shape
//! is ours; the family (a flat disc, area-normalised, 100 px ceiling) is the
//! original's.
//!
//! Everything outside the plane counts as transparent. A caller that needs
//! a pixel near an edge to be right renders a margin of
//! [`Kernel::reach`] pixels around what it keeps.

use rayon::prelude::*;

use crate::ramp::Profile;

/// The largest blur radius, in device pixels, at generation resolution.
///
/// The original's ceiling (`MAX_SHADOW_BLUR`, `research/03 §2.9`). Above
/// it the caller renders at a reduced resolution and scales up, as the
/// original does for feathers; this module clamps.
pub const MAX_RADIUS_PX: f32 = 100.0;

/// A blur kernel, in device pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kernel {
    /// A flat disc: every pixel within `radius_px` weighs the same. What
    /// Xarast renders with (Xara-compatible).
    Disc {
        /// The disc's radius. Below one pixel the disc is just its centre.
        radius_px: f32,
    },
    /// A separable Gaussian: what SVG's `feGaussianBlur` draws.
    Gaussian {
        /// The standard deviation.
        sigma_px: f32,
    },
}

/// The Gaussian an external viewer draws for a disc of `radius_px`: the
/// `.xarast` interchange convention σ = r / 2 (`research/06 §6.8.1`).
///
/// A deviation from it has to be justified by measurement and recorded in
/// `docs/memory/render.md`.
#[must_use]
pub fn sigma_for_disc_radius(radius_px: f32) -> f32 {
    radius_px * 0.5
}

impl Kernel {
    /// The kernel with its radius clamped into `0 ..= MAX_RADIUS_PX` (σ
    /// into half of that), a NaN or negative size becoming zero.
    #[must_use]
    pub fn clamped(self) -> Kernel {
        let clamp = |v: f32, max: f32| if v.is_nan() { 0.0 } else { v.clamp(0.0, max) };
        match self {
            Kernel::Disc { radius_px } => Kernel::Disc {
                radius_px: clamp(radius_px, MAX_RADIUS_PX),
            },
            Kernel::Gaussian { sigma_px } => Kernel::Gaussian {
                sigma_px: clamp(sigma_px, sigma_for_disc_radius(MAX_RADIUS_PX)),
            },
        }
    }

    /// How far, in whole pixels, an input pixel can move an output pixel:
    /// the margin a caller renders around the area it keeps.
    #[must_use]
    pub fn reach(self) -> u32 {
        match self.clamped() {
            Kernel::Disc { radius_px } => disc_extent(radius_px),
            Kernel::Gaussian { sigma_px } => gaussian_half_width(sigma_px),
        }
    }

    /// Whether the kernel leaves every plane unchanged.
    #[must_use]
    pub fn is_identity(self) -> bool {
        self.reach() == 0
    }
}

/// `floor(r)` for a radius already clamped to `0 ..= MAX_RADIUS_PX`.
fn disc_extent(radius_px: f32) -> u32 {
    let mut n = 0u32;
    while n < 100 && f64::from(n + 1) <= f64::from(radius_px) {
        n += 1;
    }
    n
}

/// A disc, as the half-width of each of its rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disc {
    /// `half[k]` is the half-width of row `dy = k − extent`: the row covers
    /// `−half ..= half`.
    half: Vec<u32>,
    extent: u32,
    area: u32,
}

impl Disc {
    /// The disc of `radius_px` (clamped).
    #[must_use]
    pub fn new(radius_px: f32) -> Disc {
        let r = match (Kernel::Disc { radius_px }).clamped() {
            Kernel::Disc { radius_px } => radius_px,
            Kernel::Gaussian { .. } => 0.0,
        };
        let r2 = f64::from(r) * f64::from(r);
        let extent = disc_extent(r);
        let e = i64::from(extent);
        let mut half = Vec::with_capacity(2 * extent as usize + 1);
        let mut area = 0u32;
        for dy in -e..=e {
            // Exact: every operand is a small integer.
            let dy2 = (dy * dy) as f64;
            let mut h = 0u32;
            while h < extent && dy2 + f64::from((h + 1) * (h + 1)) <= r2 {
                h += 1;
            }
            half.push(h);
            area += 2 * h + 1;
        }
        Disc { half, extent, area }
    }

    /// How many pixels the disc covers: its normalisation.
    #[must_use]
    pub const fn area(&self) -> u32 {
        self.area
    }

    /// Its radius in whole rows.
    #[must_use]
    pub const fn extent(&self) -> u32 {
        self.extent
    }

    /// The half-widths of its rows, top to bottom.
    #[must_use]
    pub fn rows(&self) -> &[u32] {
        &self.half
    }
}

/// Per-row prefix sums: `p[y * (w + 1) + x]` is the sum of the first `x`
/// pixels of row `y`.
fn prefix_rows(plane: &[u8], w: usize, h: usize) -> Vec<u32> {
    let mut p = vec![0u32; (w + 1) * h];
    p.par_chunks_mut(w + 1)
        .zip(plane.par_chunks(w))
        .for_each(|(dst, src)| {
            let mut acc = 0u32;
            for (d, s) in dst[1..].iter_mut().zip(src) {
                acc += u32::from(*s);
                *d = acc;
            }
        });
    p
}

/// For every pixel, the sum of the plane over the disc centred on it.
fn disc_sums(
    plane: &[u8],
    w: usize,
    h: usize,
    disc: &Disc,
    f: impl Fn(u32) -> u8 + Sync,
) -> Vec<u8> {
    let prefix = prefix_rows(plane, w, h);
    let mut out = vec![0u8; w * h];
    let e = disc.extent as i64;
    let hh = h as i64;
    let ww = w as i64;
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let y = y as i64;
        for (x, o) in row.iter_mut().enumerate() {
            let x = x as i64;
            let mut sum = 0u32;
            for (k, &hw) in disc.half.iter().enumerate() {
                let sy = y + k as i64 - e;
                if sy < 0 || sy >= hh {
                    continue;
                }
                let lo = (x - i64::from(hw)).clamp(0, ww) as usize;
                let hi = (x + i64::from(hw) + 1).clamp(0, ww) as usize;
                let base = sy as usize * (w + 1);
                sum += prefix[base + hi] - prefix[base + lo];
            }
            *o = f(sum);
        }
    });
    out
}

/// Blurs an 8-bit plane in place: `plane` is `width × height`, row-major.
///
/// Anything outside the plane counts as zero. A plane whose length is not
/// `width × height`, or a kernel that is the identity, is left alone.
pub fn blur_plane(plane: &mut [u8], width: usize, height: usize, kernel: Kernel) {
    if width == 0 || height == 0 || plane.len() != width * height || kernel.is_identity() {
        return;
    }
    match kernel.clamped() {
        Kernel::Disc { radius_px } => {
            let disc = Disc::new(radius_px);
            let area = disc.area;
            let out = disc_sums(plane, width, height, &disc, |sum| {
                // Rounded mean: the sum is at most 255 × area, so the
                // quotient is at most 255.
                u8::try_from((sum + area / 2) / area).unwrap_or(u8::MAX)
            });
            plane.copy_from_slice(&out);
        }
        Kernel::Gaussian { sigma_px } => gaussian(plane, width, height, sigma_px),
    }
}

/// Erodes an 8-bit coverage plane by a disc, softly: a pixel keeps full
/// coverage when the disc around it is fully covered, and loses one level
/// for each level of coverage the disc lacks.
///
/// That is a minimum filter for hard-edged input, and for an antialiased
/// edge it moves the edge inwards by the radius (less a fraction of a
/// pixel) while keeping it antialiased. Feathering uses it to pull the
/// silhouette in before blurring it, which is how the original's feather
/// ends fully transparent exactly at the outline (`docs/memory/render.md`,
/// "Live effects: the offscreen pipeline").
pub fn erode_plane(plane: &mut [u8], width: usize, height: usize, radius_px: f32) {
    if width == 0 || height == 0 || plane.len() != width * height {
        return;
    }
    let disc = Disc::new(radius_px);
    if disc.extent == 0 {
        return;
    }
    let full = 255 * disc.area;
    let out = disc_sums(plane, width, height, &disc, |sum| {
        let deficit = full.saturating_sub(sum);
        u8::try_from(255u32.saturating_sub(deficit)).unwrap_or(0)
    });
    plane.copy_from_slice(&out);
}

/// Half the width of the Gaussian's support: three standard deviations,
/// rounded up.
fn gaussian_half_width(sigma_px: f32) -> u32 {
    if sigma_px.is_nan() || sigma_px <= 0.0 {
        return 0;
    }
    let reach = (f64::from(sigma_px) * 3.0).ceil();
    // Clamped by the caller to at most 50 px, so at most 150.
    let mut n = 0u32;
    while n < 150 && f64::from(n) < reach {
        n += 1;
    }
    n
}

/// The Gaussian's weights for offsets `−half ..= half`, quantised to sum to
/// exactly 2¹⁶ (the remainder goes to the centre tap).
#[must_use]
pub fn gaussian_weights(sigma_px: f32) -> Vec<u32> {
    let half = gaussian_half_width(sigma_px);
    if half == 0 {
        return vec![1 << 16];
    }
    let s = f64::from(sigma_px);
    let raw: Vec<f64> = (-i64::from(half)..=i64::from(half))
        .map(|k| {
            let k = k as f64;
            (-(k * k) / (2.0 * s * s)).exp()
        })
        .collect();
    let total: f64 = raw.iter().sum();
    let mut w: Vec<u32> = raw
        .iter()
        .map(|v| {
            let q = (v / total * 65536.0).round();
            if q <= 0.0 { 0 } else { q as u32 }
        })
        .collect();
    let sum: u32 = w.iter().sum();
    let centre = half as usize;
    if sum > 65536 {
        w[centre] -= sum - 65536;
    } else {
        w[centre] += 65536 - sum;
    }
    w
}

fn gaussian(plane: &mut [u8], w: usize, h: usize, sigma_px: f32) {
    let weights = gaussian_weights(sigma_px);
    let half = (weights.len() / 2) as i64;
    // Horizontal: u8 → u16 at 8 extra bits of precision.
    let mut mid = vec![0u16; w * h];
    mid.par_chunks_mut(w)
        .zip(plane.par_chunks(w))
        .for_each(|(dst, src)| {
            for (x, d) in dst.iter_mut().enumerate() {
                let mut acc = 0u32;
                for (k, wt) in weights.iter().enumerate() {
                    let sx = x as i64 + k as i64 - half;
                    if sx >= 0 && (sx as usize) < w {
                        acc += wt * u32::from(src[sx as usize]);
                    }
                }
                // At most 255 × 2¹⁶ before, 65 280 after.
                *d = u16::try_from((acc + 128) >> 8).unwrap_or(u16::MAX);
            }
        });
    // Vertical: u16 → u8.
    plane.par_chunks_mut(w).enumerate().for_each(|(y, dst)| {
        for (x, d) in dst.iter_mut().enumerate() {
            let mut acc = 0u64;
            for (k, wt) in weights.iter().enumerate() {
                let sy = y as i64 + k as i64 - half;
                if sy >= 0 && (sy as usize) < h {
                    acc += u64::from(*wt) * u64::from(mid[sy as usize * w + x]);
                }
            }
            *d = u8::try_from((acc + (1 << 23)) >> 24).unwrap_or(u8::MAX);
        }
    });
}

/// A 256-entry transfer table applying a bias/gain profile to a
/// transparency plane: `0` (opaque) and `255` (clear) are fixed, and the
/// profile reshapes the ramp between them, as the original applies a
/// feather's or a shadow's profile to its mask (`research/03 §1.3 (i)`,
/// channel 3).
#[must_use]
pub fn profile_table(profile: Profile) -> [u8; 256] {
    let mut t = [0u8; 256];
    for (i, v) in t.iter_mut().enumerate() {
        let x = i as f64 / 255.0;
        let y = (profile.map(x) * 255.0).round();
        *v = if y <= 0.0 {
            0
        } else if y >= 255.0 {
            255
        } else {
            y as u8
        };
    }
    t
}

/// Maps every value of a plane through a table.
pub fn apply_table(plane: &mut [u8], table: &[u8; 256]) {
    for v in plane {
        *v = table[usize::from(*v)];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plane_with(w: usize, h: usize, f: impl Fn(usize, usize) -> u8) -> Vec<u8> {
        (0..w * h).map(|i| f(i % w, i / w)).collect()
    }

    #[test]
    fn a_disc_is_symmetric_and_its_area_counts_its_pixels() {
        for r in [0.0, 0.4, 1.0, 1.5, 2.0, 3.7, 10.0, 25.5, 100.0] {
            let d = Disc::new(r);
            let rows = d.rows();
            assert_eq!(rows.len(), 2 * d.extent() as usize + 1);
            for (a, b) in rows.iter().zip(rows.iter().rev()) {
                assert_eq!(a, b, "r = {r}");
            }
            let area: u32 = rows.iter().map(|h| 2 * h + 1).sum();
            assert_eq!(area, d.area());
            // Every counted pixel is inside the circle, and the next one out
            // on every row is not.
            let e = i64::from(d.extent());
            for (k, &h) in rows.iter().enumerate() {
                let dy = k as i64 - e;
                assert!((dy * dy + i64::from(h * h)) as f64 <= f64::from(r) * f64::from(r) + 1e-9);
                if h < d.extent() {
                    assert!(
                        (dy * dy + i64::from((h + 1) * (h + 1))) as f64
                            > f64::from(r) * f64::from(r)
                    );
                }
            }
        }
        assert_eq!(Disc::new(0.9).area(), 1);
        assert_eq!(Disc::new(1.0).area(), 5);
        // π r² within a few percent at a useful size.
        let a = f64::from(Disc::new(20.0).area());
        assert!(
            (a / (std::f64::consts::PI * 400.0) - 1.0).abs() < 0.02,
            "{a}"
        );
    }

    #[test]
    fn the_radius_is_clamped_to_the_original_ceiling() {
        assert_eq!(Disc::new(1e6).extent(), 100);
        assert_eq!(Disc::new(f32::NAN).area(), 1);
        assert_eq!(Disc::new(-5.0).area(), 1);
        assert_eq!(Kernel::Disc { radius_px: 250.0 }.reach(), 100);
        assert_eq!(Kernel::Gaussian { sigma_px: 1000.0 }.reach(), 150);
    }

    #[test]
    fn blurring_a_constant_plane_away_from_its_edges_keeps_the_constant() {
        let (w, h) = (64, 48);
        for v in [0u8, 1, 77, 128, 254, 255] {
            for k in [
                Kernel::Disc { radius_px: 5.0 },
                Kernel::Gaussian { sigma_px: 2.5 },
            ] {
                let mut p = vec![v; w * h];
                blur_plane(&mut p, w, h, k);
                let r = k.reach() as usize;
                for y in r..h - r {
                    for x in r..w - r {
                        assert_eq!(p[y * w + x], v, "{k:?} at ({x}, {y})");
                    }
                }
            }
        }
    }

    #[test]
    fn a_disc_blur_of_one_opaque_pixel_is_the_disc_itself() {
        let (w, h) = (41, 41);
        let mut p = plane_with(w, h, |x, y| if x == 20 && y == 20 { 255 } else { 0 });
        let disc = Disc::new(6.0);
        blur_plane(&mut p, w, h, Kernel::Disc { radius_px: 6.0 });
        let lit = ((255 + disc.area() / 2) / disc.area()) as u8;
        for y in 0..h {
            for x in 0..w {
                let (dx, dy) = (x as i64 - 20, y as i64 - 20);
                let inside = dx * dx + dy * dy <= 36;
                assert_eq!(p[y * w + x], if inside { lit } else { 0 }, "({x}, {y})");
            }
        }
    }

    #[test]
    fn the_gaussian_weights_sum_to_unity_and_are_symmetric() {
        for s in [0.3f32, 0.5, 1.0, 3.1, 7.5, 50.0] {
            let w = gaussian_weights(s);
            assert_eq!(w.iter().sum::<u32>(), 65536, "σ = {s}");
            for (a, b) in w.iter().zip(w.iter().rev()) {
                assert_eq!(a, b);
            }
            assert_eq!(
                w.len(),
                2 * Kernel::Gaussian { sigma_px: s }.reach() as usize + 1
            );
        }
        assert_eq!(gaussian_weights(0.0), vec![65536]);
    }

    #[test]
    fn blurs_are_deterministic_and_independent_of_the_thread_count() {
        let (w, h) = (97, 61);
        let src = plane_with(w, h, |x, y| ((x * 7 + y * 13) % 256) as u8);
        for k in [
            Kernel::Disc { radius_px: 4.5 },
            Kernel::Gaussian { sigma_px: 3.0 },
        ] {
            let mut a = src.clone();
            blur_plane(&mut a, w, h, k);
            let mut b = src.clone();
            rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .build()
                .unwrap()
                .install(|| blur_plane(&mut b, w, h, k));
            assert_eq!(a, b, "{k:?}");
        }
    }

    #[test]
    fn the_interchange_convention_is_sigma_is_half_the_radius() {
        // research/06 §6.8.1's worked example.
        assert!((sigma_for_disc_radius(6.2) - 3.1).abs() < 1e-6);
    }

    #[test]
    fn a_disc_and_its_gaussian_agree_on_the_edge_position() {
        // A straight edge blurred either way crosses one half at the edge.
        let (w, h) = (120, 40);
        let src = plane_with(w, h, |x, _| if x < 60 { 255 } else { 0 });
        let r = 12.0;
        let mut d = src.clone();
        blur_plane(&mut d, w, h, Kernel::Disc { radius_px: r });
        let mut g = src;
        blur_plane(
            &mut g,
            w,
            h,
            Kernel::Gaussian {
                sigma_px: sigma_for_disc_radius(r),
            },
        );
        let row = 20 * w;
        for x in 40..80 {
            let (a, b) = (i32::from(d[row + x]), i32::from(g[row + x]));
            assert!((a - b).abs() <= 40, "x = {x}: disc {a}, gaussian {b}");
        }
        assert!((i32::from(d[row + 59]) + i32::from(d[row + 60]) - 255).abs() <= 2);
    }

    #[test]
    fn erosion_pulls_a_hard_edge_in_by_the_radius() {
        let (w, h) = (60, 60);
        let mut p = plane_with(w, h, |x, y| {
            if (10..50).contains(&x) && (10..50).contains(&y) {
                255
            } else {
                0
            }
        });
        erode_plane(&mut p, w, h, 4.0);
        for y in 0..h {
            for x in 0..w {
                let inside = (14..46).contains(&x) && (14..46).contains(&y);
                assert_eq!(p[y * w + x], if inside { 255 } else { 0 }, "({x}, {y})");
            }
        }
    }

    #[test]
    fn the_identity_profile_table_is_the_identity() {
        let t = profile_table(Profile::IDENTITY);
        for (i, v) in t.iter().enumerate() {
            assert_eq!(usize::from(*v), i);
        }
        let s = profile_table(Profile::new(0.6, 0.0));
        assert_eq!((s[0], s[255]), (0, 255));
        assert!(s.windows(2).all(|p| p[0] <= p[1]));
        assert_ne!(s, t);
    }

    #[test]
    fn a_malformed_plane_is_left_alone() {
        let mut p = vec![9u8; 10];
        blur_plane(&mut p, 4, 4, Kernel::Disc { radius_px: 3.0 });
        erode_plane(&mut p, 4, 4, 3.0);
        assert_eq!(p, vec![9u8; 10]);
    }
}
