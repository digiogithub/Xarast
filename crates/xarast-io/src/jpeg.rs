//! The JPEG encoder (T11.2.5), over `jpeg-encoder`.
//!
//! JPEG has no alpha: the pixels arrive already composited onto an opaque
//! background (the exporter renders onto it), and any alpha left is
//! ignored. The file carries a JFIF density when asked and always an APP1
//! EXIF block with `ColorSpace = 1` (sRGB), so viewers do not guess
//! (phase 11 W11.5).

use jpeg_encoder::{ColorType, Encoder, PixelDensity, PixelDensityUnit, SamplingFactor};

use crate::options::{JpegOptions, Subsampling};

/// The EXIF payload (after `Exif\0\0`): a little-endian TIFF header, IFD0
/// holding only the pointer to the EXIF IFD, and an EXIF IFD holding only
/// `ColorSpace` (0xA001) = 1, sRGB.
#[must_use]
pub fn exif_srgb() -> Vec<u8> {
    let mut v = Vec::with_capacity(44);
    v.extend_from_slice(b"II");
    v.extend_from_slice(&42u16.to_le_bytes());
    v.extend_from_slice(&8u32.to_le_bytes()); // IFD0 offset
    // IFD0: one entry, ExifIFDPointer (0x8769), LONG, count 1, offset 26.
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&0x8769u16.to_le_bytes());
    v.extend_from_slice(&4u16.to_le_bytes());
    v.extend_from_slice(&1u32.to_le_bytes());
    v.extend_from_slice(&26u32.to_le_bytes());
    v.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
    // EXIF IFD at 26: one entry, ColorSpace, SHORT, count 1, value 1.
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&0xA001u16.to_le_bytes());
    v.extend_from_slice(&3u16.to_le_bytes());
    v.extend_from_slice(&1u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&0u16.to_le_bytes());
    v.extend_from_slice(&0u32.to_le_bytes());
    v
}

/// Encodes straight RGBA8 pixels (alpha ignored) as a JPEG.
///
/// # Errors
///
/// A message when the encoder refuses (a side over 65 535 px, a bad
/// quality).
pub fn encode_jpeg(
    rgba: &[u8],
    width: u32,
    height: u32,
    o: &JpegOptions,
    dpi: f64,
) -> Result<Vec<u8>, String> {
    let w = u16::try_from(width).map_err(|_| format!("{width} px is too wide for JPEG"))?;
    let h = u16::try_from(height).map_err(|_| format!("{height} px is too tall for JPEG"))?;
    if !(1..=100).contains(&o.quality) {
        return Err(format!("quality {} is outside 1..=100", o.quality));
    }
    let mut out = Vec::new();
    let mut enc = Encoder::new(&mut out, o.quality);
    enc.set_progressive(o.progressive);
    // The standard Annex K tables, not optimised ones: zune-jpeg 0.5 (our
    // own importer's decoder, behind `image`) misdecodes this encoder's
    // optimised tables in subsampled files, though libjpeg, Pillow and
    // ImageMagick read them. A file we cannot read back ourselves is not
    // worth the ~5 % (docs/memory/export.md).
    enc.set_optimized_huffman_tables(false);
    enc.set_sampling_factor(match o.subsampling {
        Subsampling::S444 => SamplingFactor::F_1_1,
        Subsampling::S422 => SamplingFactor::F_2_1,
        Subsampling::S420 => SamplingFactor::F_2_2,
    });
    if o.write_dpi {
        let d = dpi.round().clamp(1.0, f64::from(u16::MAX));
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let d = d as u16;
        enc.set_density(PixelDensity {
            density: (d, d),
            unit: PixelDensityUnit::Inches,
        });
    }
    enc.add_exif_metadata(&exif_srgb())
        .map_err(|e| e.to_string())?;
    enc.encode(rgba, w, h, ColorType::Rgba)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_image_decodes_to_its_colour_with_metadata() {
        let (w, h) = (64u32, 48u32);
        let rgba: Vec<u8> = [200u8, 40, 90, 255].repeat((w * h) as usize);
        for sub in [Subsampling::S444, Subsampling::S422, Subsampling::S420] {
            for progressive in [false, true] {
                let o = JpegOptions {
                    quality: 95,
                    progressive,
                    subsampling: sub,
                    write_dpi: true,
                };
                let bytes = encode_jpeg(&rgba, w, h, &o, 300.0).unwrap();
                // JFIF density: units=1 (inch), 300 x 300.
                let jfif = bytes.windows(5).position(|x| x == b"JFIF\0").expect("JFIF");
                assert_eq!(bytes[jfif + 7], 1);
                assert_eq!(&bytes[jfif + 8..jfif + 12], &[1, 44, 1, 44]);
                assert!(bytes.windows(6).any(|x| x == b"Exif\0\0"), "EXIF");
                let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg)
                    .expect("decodes")
                    .to_rgb8();
                assert_eq!(img.dimensions(), (w, h));
                let p = img.get_pixel(w / 2, h / 2);
                for (got, want) in p.0.iter().zip([200u8, 40, 90]) {
                    assert!(got.abs_diff(want) <= 3, "{sub:?} {progressive}: {:?}", p.0);
                }
            }
        }
    }

    #[test]
    fn the_exif_block_is_well_formed() {
        let e = exif_srgb();
        assert_eq!(e.len(), 44);
        assert_eq!(&e[..4], b"II*\0");
        // ColorSpace tag at 28, value 1 at 36.
        assert_eq!(u16::from_le_bytes([e[28], e[29]]), 0xA001);
        assert_eq!(u16::from_le_bytes([e[36], e[37]]), 1);
    }

    #[test]
    fn bad_input_is_refused() {
        let o = JpegOptions {
            quality: 0,
            ..JpegOptions::default()
        };
        assert!(encode_jpeg(&[0; 4], 1, 1, &o, 96.0).is_err());
        assert!(encode_jpeg(&[0; 4], 70_000, 1, &JpegOptions::default(), 96.0).is_err());
    }
}
