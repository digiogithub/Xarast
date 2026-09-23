//! The WebP encoder (T11.2.6), lossless only, over `image-webp`.
//!
//! There is no acceptable pure-Rust lossy WebP encoder (the usable ones
//! bind `libwebp`, a C library), so [`WebPMode::Lossy`] is refused with
//! a message and recorded in `docs/memory/export.md`. WebP without an
//! `ICCP` chunk is sRGB by definition, which is what we composite in.

use image_webp::{ColorType, WebPEncoder};

use crate::options::{WebPMode, WebPOptions};

/// The largest side WebP can store.
pub const MAX_WEBP_SIDE: u32 = 16_383;

/// Why a WebP could not be written.
#[derive(Debug, thiserror::Error)]
pub enum WebPError {
    /// Lossy output needs an encoder this build does not have.
    #[error("lossy WebP needs an encoder this build does not have; use lossless")]
    LossyNotBuilt,
    /// A side is over [`MAX_WEBP_SIDE`].
    #[error("{0}x{1} px is larger than WebP allows ({MAX_WEBP_SIDE} px a side)")]
    TooLarge(u32, u32),
    /// The encoder failed.
    #[error("{0}")]
    Encode(String),
}

/// Encodes straight RGBA8 pixels; `opaque` drops the alpha channel.
///
/// # Errors
///
/// [`WebPError`].
pub fn encode_webp(
    rgba: &[u8],
    width: u32,
    height: u32,
    o: &WebPOptions,
    opaque: bool,
) -> Result<Vec<u8>, WebPError> {
    if let WebPMode::Lossy { .. } = o.mode {
        return Err(WebPError::LossyNotBuilt);
    }
    if width > MAX_WEBP_SIDE || height > MAX_WEBP_SIDE {
        return Err(WebPError::TooLarge(width, height));
    }
    let mut out = Vec::new();
    let enc = WebPEncoder::new(&mut out);
    if opaque {
        let rgb: Vec<u8> = rgba
            .as_chunks::<4>().0.iter()
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect();
        enc.encode(&rgb, width, height, ColorType::Rgb8)
    } else {
        enc.encode(rgba, width, height, ColorType::Rgba8)
    }
    .map_err(|e| WebPError::Encode(e.to_string()))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lossless_round_trips_exactly() {
        let (w, h) = (33u32, 17u32);
        let mut rgba = Vec::new();
        for i in 0..w * h {
            #[allow(clippy::cast_possible_truncation)]
            rgba.extend_from_slice(&[(i * 7) as u8, (i * 3) as u8, 9, (i % 256) as u8]);
        }
        let bytes = encode_webp(&rgba, w, h, &WebPOptions::default(), false).unwrap();
        let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::WebP)
            .unwrap()
            .to_rgba8();
        assert_eq!(img.into_raw(), rgba);
        let bytes = encode_webp(&rgba, w, h, &WebPOptions::default(), true).unwrap();
        let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::WebP).unwrap();
        assert!(!img.color().has_alpha());
    }

    #[test]
    fn lossy_and_oversize_are_refused() {
        let o = WebPOptions {
            mode: WebPMode::Lossy { quality: 80 },
        };
        assert!(matches!(
            encode_webp(&[0; 4], 1, 1, &o, false),
            Err(WebPError::LossyNotBuilt)
        ));
        assert!(matches!(
            encode_webp(&[], 20_000, 1, &WebPOptions::default(), false),
            Err(WebPError::TooLarge(..))
        ));
    }
}
