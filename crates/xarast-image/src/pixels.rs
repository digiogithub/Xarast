//! Sample-layout conversion into premultiplied RGBA8.

use image::ColorType;

/// `round(c * a / 255)`, exact for every `u8` pair.
#[inline]
#[must_use]
pub fn premultiply(c: u8, a: u8) -> u8 {
    let t = u32::from(c) * u32::from(a) + 128;
    ((t + (t >> 8)) >> 8) as u8
}

/// The straight value that premultiplies back to `c` at alpha `a`, rounded
/// to nearest. Zero alpha yields zero.
#[inline]
#[must_use]
pub fn unpremultiply(c: u8, a: u8) -> u8 {
    if a == 0 {
        return 0;
    }
    let v = (u32::from(c) * 255 + u32::from(a) / 2) / u32::from(a);
    v.min(255) as u8
}

/// A 16-bit sample rounded to 8 bits.
#[inline]
fn narrow16(v: u16) -> u8 {
    ((u32::from(v) * 255 + 32_767) / 65_535) as u8
}

/// A float sample clamped to `0..=1` and rounded to 8 bits. NaN is zero.
#[inline]
fn narrow_f32(v: f32) -> u8 {
    if v.is_nan() {
        return 0;
    }
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Converts one decoded buffer, as an `image` decoder wrote it, into
/// premultiplied RGBA8. `out` must be `pixels * 4` bytes and `src` exactly
/// `pixels * bytes_per_pixel`. Returns whether any pixel had alpha < 255.
///
/// Multi-byte samples are native-endian, which is how `image` writes them.
pub(crate) fn to_premul_rgba8(color: ColorType, src: &[u8], out: &mut [u8]) -> bool {
    let mut translucent = false;
    let mut put = |o: &mut [u8; 4], r: u8, g: u8, b: u8, a: u8| {
        if a == 255 {
            *o = [r, g, b, 255];
        } else {
            translucent = true;
            *o = [premultiply(r, a), premultiply(g, a), premultiply(b, a), a];
        }
    };
    let u16_at = |s: &[u8], i: usize| u16::from_ne_bytes([s[2 * i], s[2 * i + 1]]);
    let f32_at = |s: &[u8], i: usize| {
        f32::from_ne_bytes([s[4 * i], s[4 * i + 1], s[4 * i + 2], s[4 * i + 3]])
    };
    let n = color.bytes_per_pixel() as usize;
    for (s, o) in src
        .chunks_exact(n)
        .zip(out.as_chunks_mut::<4>().0.iter_mut())
    {
        match color {
            ColorType::L8 => put(o, s[0], s[0], s[0], 255),
            ColorType::La8 => put(o, s[0], s[0], s[0], s[1]),
            ColorType::Rgb8 => put(o, s[0], s[1], s[2], 255),
            ColorType::Rgba8 => put(o, s[0], s[1], s[2], s[3]),
            ColorType::L16 => {
                let l = narrow16(u16_at(s, 0));
                put(o, l, l, l, 255);
            }
            ColorType::La16 => {
                let l = narrow16(u16_at(s, 0));
                put(o, l, l, l, narrow16(u16_at(s, 1)));
            }
            ColorType::Rgb16 => put(
                o,
                narrow16(u16_at(s, 0)),
                narrow16(u16_at(s, 1)),
                narrow16(u16_at(s, 2)),
                255,
            ),
            ColorType::Rgba16 => put(
                o,
                narrow16(u16_at(s, 0)),
                narrow16(u16_at(s, 1)),
                narrow16(u16_at(s, 2)),
                narrow16(u16_at(s, 3)),
            ),
            ColorType::Rgb32F => put(
                o,
                narrow_f32(f32_at(s, 0)),
                narrow_f32(f32_at(s, 1)),
                narrow_f32(f32_at(s, 2)),
                255,
            ),
            ColorType::Rgba32F => put(
                o,
                narrow_f32(f32_at(s, 0)),
                narrow_f32(f32_at(s, 1)),
                narrow_f32(f32_at(s, 2)),
                narrow_f32(f32_at(s, 3)),
            ),
            // `ColorType` is non-exhaustive; a future layout is refused
            // upstream by `supported`, so this is unreachable
            // in practice and harmless if not: the pixel stays transparent.
            _ => {}
        }
    }
    translucent
}

/// Whether [`to_premul_rgba8`] understands this layout.
pub(crate) fn supported(color: ColorType) -> bool {
    matches!(
        color,
        ColorType::L8
            | ColorType::La8
            | ColorType::Rgb8
            | ColorType::Rgba8
            | ColorType::L16
            | ColorType::La16
            | ColorType::Rgb16
            | ColorType::Rgba16
            | ColorType::Rgb32F
            | ColorType::Rgba32F
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiply_is_exactly_rounded() {
        for a in 0..=255u8 {
            for c in 0..=255u8 {
                let want = (f64::from(c) * f64::from(a) / 255.0).round() as u8;
                assert_eq!(premultiply(c, a), want, "c={c} a={a}");
            }
        }
    }

    #[test]
    fn unpremultiply_round_trips_through_premultiply() {
        for a in 1..=255u8 {
            for c in 0..=a {
                let s = unpremultiply(c, a);
                assert_eq!(premultiply(s, a), c, "c={c} a={a}");
            }
        }
    }

    #[test]
    fn sixteen_bit_narrows_to_nearest() {
        assert_eq!(narrow16(0), 0);
        assert_eq!(narrow16(65_535), 255);
        assert_eq!(narrow16(257 * 128), 128);
    }
}
