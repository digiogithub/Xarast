//! Thumbnails and previews (`research/06 §3.2.5`, `§3.2.6`).
//!
//! The format crate never renders: pixels come from a [`ThumbnailProvider`]
//! the application injects (the renderer lives in `xarast-render`, which this
//! crate must not depend on, `research/06 §13.1`). What lives here is the
//! contract — the size caps and the PNG shape — and the check that enforces
//! it on write.

#![deny(clippy::arithmetic_side_effects)]

use thiserror::Error;

/// Recommended longer side of `thumbnail.png`.
pub const THUMBNAIL_PX: u32 = 256;
/// Hard cap on the longer side of `thumbnail.png`.
pub const THUMBNAIL_MAX_PX: u32 = 512;
/// Recommended longer side of a preview.
pub const PREVIEW_PX: u32 = 512;
/// Hard cap on the longer side of a preview.
pub const PREVIEW_MAX_PX: u32 = 1024;

/// Why a provider could not produce an image.
#[derive(Debug, Error)]
#[error("thumbnail generation failed: {0}")]
pub struct ThumbnailError(pub String);

/// Renders thumbnails and per-spread previews. Implemented outside this
/// crate, over the renderer.
pub trait ThumbnailProvider: Send + Sync {
    /// A PNG of the first spread's page area: RGBA8, transparent background
    /// with the page colour composited beneath if it is opaque, longer side
    /// at most `max_px`.
    fn thumbnail(&self, doc: &xarast_doc::Document, max_px: u32)
    -> Result<Vec<u8>, ThumbnailError>;

    /// A PNG of spread `spread` (1-based), longer side at most `max_px`.
    fn preview(
        &self,
        doc: &xarast_doc::Document,
        spread: u32,
        max_px: u32,
    ) -> Result<Vec<u8>, ThumbnailError>;
}

/// What the IHDR chunk of a PNG says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PngHeader {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bits per channel.
    pub bit_depth: u8,
    /// PNG colour type (6 is RGBA).
    pub colour_type: u8,
}

/// Reads the IHDR of a PNG without decoding it. `None` if the bytes do not
/// start with a PNG signature and an IHDR chunk.
pub fn png_header(png: &[u8]) -> Option<PngHeader> {
    const SIG: &[u8] = b"\x89PNG\r\n\x1a\n";
    if png.get(..8)? != SIG || png.get(8..12)? != [0, 0, 0, 13] || png.get(12..16)? != b"IHDR" {
        return None;
    }
    let be = |at: usize| -> Option<u32> {
        Some(u32::from_be_bytes(
            png.get(at..at.checked_add(4)?)?.try_into().ok()?,
        ))
    };
    Some(PngHeader {
        width: be(16)?,
        height: be(20)?,
        bit_depth: *png.get(24)?,
        colour_type: *png.get(25)?,
    })
}

/// Checks a thumbnail or preview against the contract: a PNG, RGBA at 8 bits
/// per channel, non-empty, longer side at most `max_px`.
pub fn check_png(png: &[u8], max_px: u32) -> Result<PngHeader, &'static str> {
    let h = png_header(png).ok_or("not a PNG")?;
    if h.colour_type != 6 || h.bit_depth != 8 {
        return Err("not RGBA8");
    }
    if h.width == 0 || h.height == 0 {
        return Err("empty image");
    }
    if h.width.max(h.height) > max_px {
        return Err("too large");
    }
    Ok(h)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The IHDR prefix of a PNG; enough for [`check_png`], which never
    /// decodes.
    pub(crate) fn fake_png(w: u32, h: u32, colour_type: u8) -> Vec<u8> {
        let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        v.extend_from_slice(&w.to_be_bytes());
        v.extend_from_slice(&h.to_be_bytes());
        v.extend_from_slice(&[8, colour_type, 0, 0, 0]);
        v.extend_from_slice(&[0, 0, 0, 0]); // CRC, unchecked
        v
    }

    #[test]
    fn contract() {
        assert!(check_png(&fake_png(256, 180, 6), THUMBNAIL_MAX_PX).is_ok());
        assert_eq!(
            check_png(&fake_png(513, 10, 6), THUMBNAIL_MAX_PX),
            Err("too large")
        );
        assert_eq!(
            check_png(&fake_png(10, 10, 2), THUMBNAIL_MAX_PX),
            Err("not RGBA8")
        );
        assert_eq!(
            check_png(&fake_png(0, 10, 6), THUMBNAIL_MAX_PX),
            Err("empty image")
        );
        assert_eq!(check_png(b"GIF89a", THUMBNAIL_MAX_PX), Err("not a PNG"));
        assert!(check_png(&fake_png(1024, 1, 6), PREVIEW_MAX_PX).is_ok());
    }
}
