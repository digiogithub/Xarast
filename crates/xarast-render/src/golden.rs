//! The golden-image harness: PNG in, PNG out, and the comparison metrics
//! the three gates use.
//!
//! # The answer to architecture open question 5
//!
//! *"Is a perceptual diff or exact match the right golden-image gate?"* —
//! **both, at different levels**, and the level is chosen by what the
//! comparison can legitimately promise:
//!
//! | Gate | What is compared | Tolerance |
//! |---|---|---|
//! | A | CPU backend against a committed golden | **exact**, zero differing pixels |
//! | B | GPU against CPU | ΔRMS < 0.5 %, no channel differing by more than 8/255 |
//! | C | Either against the original | mean ΔE₀₀ < 1.0, p99 < 3.0, and flat interiors exact |
//!
//! Gate A is exact because the whole point of having a deterministic
//! backend is that its output is a fact. Gate B is perceptual because GPU
//! results are not reproducible across drivers and pretending otherwise
//! would make the suite a driver-version detector. Gate C is perceptual
//! with a wider band because our coverage is analytic and CDraw's is 17×5
//! supersampled — better, not equal — while *inside* a fill, away from any
//! edge, only the ramp maths and the blend formula are being tested, so
//! there the tolerance is zero.
//!
//! The W0 spike measured what "exact" can mean: `vello_cpu` is
//! byte-identical over 100 runs at a fixed SIMD level and thread count, and
//! differs in 3 of 786,432 channel samples between AVX2 and the baseline
//! path, never by more than 1/255. Gate A therefore pins the SIMD level
//! ([`crate::CpuConfig::deterministic`]) rather than tolerating a pixel.

use std::io::Cursor;
use std::path::Path;

use crate::surface::Surface;

/// Why a golden image could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum GoldenError {
    /// The file could not be read or written.
    #[error("golden image i/o: {0}")]
    Io(#[from] std::io::Error),
    /// The PNG could not be decoded.
    #[error("golden image decode: {0}")]
    Decode(#[from] png::DecodingError),
    /// The PNG could not be encoded.
    #[error("golden image encode: {0}")]
    Encode(#[from] png::EncodingError),
    /// The PNG is not 8-bit RGBA.
    #[error("golden images must be 8-bit RGBA, found {0:?} at {1} bits")]
    UnsupportedFormat(png::ColorType, u8),
}

/// Encodes a surface as a PNG. The surface is premultiplied; the PNG is
/// not, so the encoder unpremultiplies on the way out and the decoder
/// premultiplies on the way in. That round trip is exact for every alpha
/// value of 0 or 255, which every golden in the suite uses.
///
/// # Errors
///
/// Fails only if the encoder does.
pub fn encode_png(s: &Surface) -> Result<Vec<u8>, GoldenError> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(Cursor::new(&mut out), s.width(), s.height());
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header()?;
        let straight: Vec<u8> = s
            .data()
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|px| {
                let a = px[3];
                if a == 0 {
                    [0, 0, 0, 0]
                } else if a == 255 {
                    [px[0], px[1], px[2], 255]
                } else {
                    let un = |v: u8| -> u8 {
                        u8::try_from((u32::from(v) * 255 + u32::from(a) / 2) / u32::from(a))
                            .unwrap_or(255)
                    };
                    [un(px[0]), un(px[1]), un(px[2]), a]
                }
            })
            .collect();
        writer.write_image_data(&straight)?;
    }
    Ok(out)
}

/// Decodes a PNG into a premultiplied surface.
///
/// # Errors
///
/// Fails on a malformed PNG or one that is not 8-bit RGBA or RGB.
pub fn decode_png(bytes: &[u8]) -> Result<Surface, GoldenError> {
    let decoder = png::Decoder::new(Cursor::new(bytes));
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buf)?;
    if info.bit_depth != png::BitDepth::Eight {
        return Err(GoldenError::UnsupportedFormat(
            info.color_type,
            info.bit_depth as u8,
        ));
    }
    let mut s = Surface::new(info.width, info.height);
    let src = &buf[..info.buffer_size()];
    match info.color_type {
        png::ColorType::Rgba => {
            for (dst, px) in s
                .data_mut()
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(src.as_chunks::<4>().0.iter())
            {
                let a = px[3];
                dst[0] = crate::blend::mul(px[0], a);
                dst[1] = crate::blend::mul(px[1], a);
                dst[2] = crate::blend::mul(px[2], a);
                dst[3] = a;
            }
        }
        png::ColorType::Rgb => {
            for (dst, px) in s
                .data_mut()
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(src.as_chunks::<3>().0.iter())
            {
                dst[0] = px[0];
                dst[1] = px[1];
                dst[2] = px[2];
                dst[3] = 255;
            }
        }
        other => return Err(GoldenError::UnsupportedFormat(other, 8)),
    }
    Ok(s)
}

/// Writes a surface to a PNG file.
///
/// # Errors
///
/// Fails on an encoding or filesystem error.
pub fn write_png(s: &Surface, path: &Path) -> Result<(), GoldenError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, encode_png(s)?)?;
    Ok(())
}

/// Reads a surface from a PNG file.
///
/// # Errors
///
/// Fails if the file is missing or malformed.
pub fn read_png(path: &Path) -> Result<Surface, GoldenError> {
    decode_png(&std::fs::read(path)?)
}

/// The result of comparing two surfaces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Comparison {
    /// How many pixels differ in any channel.
    pub differing_pixels: u64,
    /// The largest single-channel difference, in 8-bit steps.
    pub max_channel_delta: u8,
    /// Root-mean-square channel difference, as a fraction of full scale.
    pub rms: f64,
    /// Mean CIEDE2000 colour difference.
    pub mean_delta_e: f64,
    /// 99th-percentile CIEDE2000 colour difference.
    pub p99_delta_e: f64,
    /// Total pixels compared.
    pub pixels: u64,
}

impl Comparison {
    /// Whether the two images are byte-identical: gate A.
    #[must_use]
    pub fn is_exact(&self) -> bool {
        self.differing_pixels == 0
    }

    /// Whether the difference is within the GPU-versus-CPU band: gate B.
    #[must_use]
    pub fn passes_parity(&self) -> bool {
        self.rms < 0.005 && self.max_channel_delta <= 8
    }

    /// Whether the difference is within the against-the-original band:
    /// gate C.
    #[must_use]
    pub fn passes_perceptual(&self) -> bool {
        self.mean_delta_e < 1.0 && self.p99_delta_e < 3.0
    }
}

/// Compares two surfaces of the same size.
///
/// # Panics
///
/// Panics if the two surfaces have different dimensions, which is a test
/// authoring error rather than a result.
#[must_use]
pub fn compare(a: &Surface, b: &Surface) -> Comparison {
    assert_eq!(
        (a.width(), a.height()),
        (b.width(), b.height()),
        "golden comparison needs equal dimensions"
    );
    let n = u64::from(a.width()) * u64::from(a.height());
    let mut differing = 0u64;
    let mut max_delta = 0u8;
    let mut sq = 0f64;
    let mut deltas: Vec<f64> = Vec::with_capacity(n as usize);
    let mut sum_de = 0f64;
    for (pa, pb) in a
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.data().as_chunks::<4>().0.iter())
    {
        let mut diff = false;
        for c in 0..4 {
            let d = pa[c].abs_diff(pb[c]);
            if d != 0 {
                diff = true;
            }
            max_delta = max_delta.max(d);
            sq += f64::from(d) * f64::from(d);
        }
        if diff {
            differing += 1;
        }
        let de = delta_e00([pa[0], pa[1], pa[2]], [pb[0], pb[1], pb[2]]);
        sum_de += de;
        deltas.push(de);
    }
    deltas.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((deltas.len() as f64 * 0.99) as usize).min(deltas.len().saturating_sub(1));
    Comparison {
        differing_pixels: differing,
        max_channel_delta: max_delta,
        rms: if n == 0 {
            0.0
        } else {
            (sq / (n as f64 * 4.0)).sqrt() / 255.0
        },
        mean_delta_e: if n == 0 { 0.0 } else { sum_de / n as f64 },
        p99_delta_e: deltas.get(idx).copied().unwrap_or(0.0),
        pixels: n,
    }
}

/// A heat map of where two surfaces differ, for the per-pull-request
/// artefact: black where they agree, rising through red to white.
#[must_use]
pub fn diff_heatmap(a: &Surface, b: &Surface) -> Surface {
    let mut out = Surface::new(a.width(), a.height());
    for ((pa, pb), dst) in a
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.data().as_chunks::<4>().0.iter())
        .zip(out.data_mut().as_chunks_mut::<4>().0.iter_mut())
    {
        let d = (0..4).map(|c| pa[c].abs_diff(pb[c])).max().unwrap_or(0);
        let hot = d.saturating_mul(8);
        dst[0] = hot;
        dst[1] = hot.saturating_sub(128).saturating_mul(2);
        dst[2] = hot.saturating_sub(192).saturating_mul(4);
        dst[3] = 255;
    }
    out
}

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn to_lab(rgb: [u8; 3]) -> [f64; 3] {
    let r = srgb_to_linear(f64::from(rgb[0]) / 255.0);
    let g = srgb_to_linear(f64::from(rgb[1]) / 255.0);
    let b = srgb_to_linear(f64::from(rgb[2]) / 255.0);
    let x = 0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b;
    let y = 0.212_672_9 * r + 0.715_152_2 * g + 0.072_175_0 * b;
    let z = 0.019_333_9 * r + 0.119_192 * g + 0.950_304_1 * b;
    let f = |t: f64| {
        if t > 216.0 / 24389.0 {
            t.cbrt()
        } else {
            (24389.0 / 27.0 * t + 16.0) / 116.0
        }
    };
    let (fx, fy, fz) = (f(x / 0.950_47), f(y), f(z / 1.088_83));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// CIEDE2000, the metric the perceptual gates are stated in.
#[must_use]
pub fn delta_e00(a: [u8; 3], b: [u8; 3]) -> f64 {
    if a == b {
        return 0.0;
    }
    let [l1, a1, b1] = to_lab(a);
    let [l2, a2, b2] = to_lab(b);
    let c1 = (a1 * a1 + b1 * b1).sqrt();
    let c2 = (a2 * a2 + b2 * b2).sqrt();
    let cb = (c1 + c2) / 2.0;
    let cb7 = cb.powi(7);
    let g = 0.5 * (1.0 - (cb7 / (cb7 + 25f64.powi(7))).sqrt());
    let ap1 = (1.0 + g) * a1;
    let ap2 = (1.0 + g) * a2;
    let cp1 = (ap1 * ap1 + b1 * b1).sqrt();
    let cp2 = (ap2 * ap2 + b2 * b2).sqrt();
    let hp = |ap: f64, bp: f64| {
        if ap == 0.0 && bp == 0.0 {
            0.0
        } else {
            let h = bp.atan2(ap).to_degrees();
            if h < 0.0 { h + 360.0 } else { h }
        }
    };
    let hp1 = hp(ap1, b1);
    let hp2 = hp(ap2, b2);
    let dlp = l2 - l1;
    let dcp = cp2 - cp1;
    let dh = if cp1 * cp2 == 0.0 {
        0.0
    } else {
        let d = hp2 - hp1;
        if d > 180.0 {
            d - 360.0
        } else if d < -180.0 {
            d + 360.0
        } else {
            d
        }
    };
    let dhp = 2.0 * (cp1 * cp2).sqrt() * (dh.to_radians() / 2.0).sin();
    let lbp = (l1 + l2) / 2.0;
    let cbp = (cp1 + cp2) / 2.0;
    let hbp = if cp1 * cp2 == 0.0 {
        hp1 + hp2
    } else if (hp1 - hp2).abs() <= 180.0 {
        (hp1 + hp2) / 2.0
    } else if hp1 + hp2 < 360.0 {
        (hp1 + hp2 + 360.0) / 2.0
    } else {
        (hp1 + hp2 - 360.0) / 2.0
    };
    let t = 1.0 - 0.17 * (hbp - 30.0).to_radians().cos()
        + 0.24 * (2.0 * hbp).to_radians().cos()
        + 0.32 * (3.0 * hbp + 6.0).to_radians().cos()
        - 0.20 * (4.0 * hbp - 63.0).to_radians().cos();
    let dtheta = 30.0 * (-(((hbp - 275.0) / 25.0).powi(2))).exp();
    let cbp7 = cbp.powi(7);
    let rc = 2.0 * (cbp7 / (cbp7 + 25f64.powi(7))).sqrt();
    let sl = 1.0 + (0.015 * (lbp - 50.0).powi(2)) / (20.0 + (lbp - 50.0).powi(2)).sqrt();
    let sc = 1.0 + 0.045 * cbp;
    let sh = 1.0 + 0.015 * cbp * t;
    let rt = -(2.0 * dtheta.to_radians()).sin() * rc;
    ((dlp / sl).powi(2) + (dcp / sc).powi(2) + (dhp / sh).powi(2) + rt * (dcp / sc) * (dhp / sh))
        .sqrt()
}

/// Box-filters a surface down by an integer factor.
///
/// Rendering a scene at `n` times the size and averaging `n × n` samples
/// is the antialiasing reference the W0 spike settled on: a *painter's
/// model* reference, in which each shape's coverage is resolved exactly
/// and the shapes are composited in order. Full sub-pixel visibility
/// supersampling, which resolves overlap between shapes as well, is a
/// reference no painter's-algorithm compositor can reach — the spike
/// measured a p99 ΔE₀₀ of 7.8 against it and 0.65 against this one, on the
/// same renderer. Measuring against the unreachable reference would have
/// failed a gate for a property nothing has.
///
/// # Panics
///
/// Panics if `factor` is zero or does not divide both dimensions.
#[must_use]
pub fn downsample(s: &Surface, factor: u32) -> Surface {
    assert!(factor > 0, "the downsampling factor must be positive");
    assert!(
        s.width().is_multiple_of(factor) && s.height().is_multiple_of(factor),
        "the factor must divide both dimensions"
    );
    let (w, h) = (s.width() / factor, s.height() / factor);
    let mut out = Surface::new(w, h);
    let n = u32::from(u16::try_from(factor * factor).unwrap_or(u16::MAX));
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0u32; 4];
            for sy in 0..factor {
                for sx in 0..factor {
                    let px = s
                        .pixel((x * factor + sx) as i32, (y * factor + sy) as i32)
                        .unwrap_or([0; 4]);
                    for c in 0..4 {
                        acc[c] += u32::from(px[c]);
                    }
                }
            }
            let mut px = [0u8; 4];
            for c in 0..4 {
                // Round half away from zero, the crate's rounding rule.
                px[c] = u8::try_from((acc[c] * 2 + n) / (2 * n)).unwrap_or(255);
            }
            out.set_pixel(x as i32, y as i32, px);
        }
    }
    out
}

/// How many distinct grey levels appear in the red channel of a surface.
///
/// On an antialiasing ramp this is the effective coverage resolution.
/// CDraw manages 85 levels in its normal mode and 132 in high quality
/// (`research/03 §2.3`); 132 is the bar Xarast has to clear.
#[must_use]
pub fn coverage_levels(s: &Surface) -> usize {
    let mut seen = [false; 256];
    for px in s.data().as_chunks::<4>().0.iter() {
        seen[px[0] as usize] = true;
    }
    seen.iter().filter(|v| **v).count()
}

/// A SHA-256 digest of a surface, for the determinism test.
#[must_use]
pub fn digest(s: &Surface) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(s.width().to_le_bytes());
    h.update(s.height().to_le_bytes());
    h.update(s.data());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker(w: u32, h: u32) -> Surface {
        let mut s = Surface::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let v = if (x + y) % 2 == 0 { 255 } else { 32 };
                s.set_pixel(x as i32, y as i32, [v, v / 2, v / 4, 255]);
            }
        }
        s
    }

    #[test]
    fn png_round_trips_exactly_for_opaque_images() {
        let s = checker(9, 7);
        let bytes = encode_png(&s).unwrap();
        let back = decode_png(&bytes).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn comparing_an_image_with_itself_is_exact() {
        let s = checker(8, 8);
        let c = compare(&s, &s);
        assert!(c.is_exact());
        assert_eq!(c.max_channel_delta, 0);
        assert_eq!(c.mean_delta_e, 0.0);
        assert!(c.passes_parity() && c.passes_perceptual());
    }

    #[test]
    fn one_changed_pixel_is_detected_but_still_passes_the_perceptual_band() {
        let a = checker(64, 64);
        let mut b = a.clone();
        b.set_pixel(3, 3, [250, 120, 60, 255]);
        let c = compare(&a, &b);
        assert!(!c.is_exact());
        assert_eq!(c.differing_pixels, 1);
        assert!(c.passes_perceptual(), "one pixel cannot move the mean");
    }

    #[test]
    fn a_wholly_different_image_fails_every_gate() {
        let a = Surface::filled(16, 16, [255, 255, 255, 255]);
        let b = Surface::filled(16, 16, [0, 0, 0, 255]);
        let c = compare(&a, &b);
        assert!(!c.is_exact());
        assert!(!c.passes_parity());
        assert!(!c.passes_perceptual());
        assert_eq!(c.max_channel_delta, 255);
    }

    #[test]
    fn delta_e_is_zero_for_equal_colours_and_positive_otherwise() {
        assert_eq!(delta_e00([10, 20, 30], [10, 20, 30]), 0.0);
        assert!(delta_e00([10, 20, 30], [10, 20, 31]) > 0.0);
        // Two 8-bit steps of red on mid grey is just about the
        // just-noticeable difference.
        let jnd = delta_e00([128, 128, 128], [130, 128, 128]);
        assert!((0.5..1.5).contains(&jnd), "two steps of red gave dE {jnd}");
    }

    #[test]
    fn the_heatmap_is_black_where_the_images_agree() {
        let a = checker(8, 8);
        let mut b = a.clone();
        b.set_pixel(1, 1, [0, 0, 0, 255]);
        let hm = diff_heatmap(&a, &b);
        assert_eq!(hm.pixel(0, 0), Some([0, 0, 0, 255]));
        assert_ne!(hm.pixel(1, 1), Some([0, 0, 0, 255]));
    }

    #[test]
    fn digests_differ_when_a_single_byte_does() {
        let a = checker(4, 4);
        let mut b = a.clone();
        b.set_pixel(0, 0, [1, 1, 1, 255]);
        assert_ne!(digest(&a), digest(&b));
        assert_eq!(digest(&a), digest(&a.clone()));
    }
}
