//! The legacy `.xar` format's bitmap wrappings (`research/01 §4.5`).
//!
//! A `TAG_DEFINEBITMAP_*` record carries a name and then an embedded image.
//! Most tags embed a complete, standard file; three do not:
//!
//! - **65 `TAG_DEFINEBITMAP_BMP`** embeds a Windows **DIB without its
//!   `BITMAPFILEHEADER`**: the record's image starts at the
//!   `BITMAPINFOHEADER`. We synthesise the 14-byte file header.
//! - **69 `TAG_DEFINEBITMAP_BMPZIP`** is that DIB behind the file's stream
//!   compression. We inflate it (zlib or raw DEFLATE), bounded.
//! - **71 `TAG_DEFINEBITMAP_JPEG8BPP`** is a 24 bpp JPEG of an image that
//!   was 8 bpp, plus the original palette; the 8 bpp image is reconstructed
//!   by mapping every pixel to its nearest palette entry, undithered.
//!
//! The importer hands this module the tag, the image bytes and (for 71) the
//! palette; it needs nothing else from `xarast-xar`, which keeps this crate a
//! leaf and lets the fuzzer reach the same code.

use std::io::Read;

use crate::limits::{DecodeError, DecodeLimits};
use crate::model::ImageFormat;
use crate::{DecodedImage, decode_as, sniff};

/// `TAG_PREVIEWBITMAP_BMP` … `TAG_PREVIEWBITMAP_TIFFLZW`, the document
/// thumbnail. Standard files, no name.
pub const TAG_PREVIEW_FIRST: u32 = 60;
/// Last preview tag.
pub const TAG_PREVIEW_LAST: u32 = 64;
/// Headerless DIB.
pub const TAG_DEFINEBITMAP_BMP: u32 = 65;
/// GIF file.
pub const TAG_DEFINEBITMAP_GIF: u32 = 66;
/// JPEG file.
pub const TAG_DEFINEBITMAP_JPEG: u32 = 67;
/// PNG file.
pub const TAG_DEFINEBITMAP_PNG: u32 = 68;
/// Compressed headerless DIB.
pub const TAG_DEFINEBITMAP_BMPZIP: u32 = 69;
/// 24 bpp JPEG + palette standing for an 8 bpp bitmap.
pub const TAG_DEFINEBITMAP_JPEG8BPP: u32 = 71;

/// How the bytes of one `.xar` bitmap record are wrapped.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum XarWrapping {
    /// A standard file; decode as sniffed.
    Plain,
    /// A headerless DIB.
    Dib,
    /// A compressed headerless DIB.
    DibCompressed,
    /// A JPEG to snap back onto its palette.
    Jpeg8Bpp,
}

impl XarWrapping {
    /// The wrapping a tag declares, or `None` for a tag that is not a bitmap.
    #[must_use]
    pub const fn from_tag(tag: u32) -> Option<XarWrapping> {
        Some(match tag {
            TAG_DEFINEBITMAP_BMP => XarWrapping::Dib,
            TAG_DEFINEBITMAP_BMPZIP => XarWrapping::DibCompressed,
            TAG_DEFINEBITMAP_JPEG8BPP => XarWrapping::Jpeg8Bpp,
            TAG_DEFINEBITMAP_GIF | TAG_DEFINEBITMAP_JPEG | TAG_DEFINEBITMAP_PNG => {
                XarWrapping::Plain
            }
            TAG_PREVIEW_FIRST..=TAG_PREVIEW_LAST => XarWrapping::Plain,
            _ => return None,
        })
    }
}

/// Decodes the image bytes of a `.xar` bitmap record.
///
/// `palette` is the record's reconstruction palette (tag 71 only; ignored
/// otherwise, and ignored when empty or longer than 256 entries, exactly as
/// the original skips the reconstruction then).
///
/// The content is sniffed first: if the bytes are a recognisable standard
/// file, that wins over what the tag declares, so a writer that embedded a
/// full `BM` file under tag 65 still decodes.
///
/// # Errors
///
/// Any [`DecodeError`]; an unknown tag is [`DecodeError::UnknownFormat`].
pub fn decode_xar_bitmap(
    tag: u32,
    bytes: &[u8],
    palette: &[[u8; 3]],
    limits: &DecodeLimits,
) -> Result<DecodedImage, DecodeError> {
    let wrapping = XarWrapping::from_tag(tag).ok_or(DecodeError::UnknownFormat)?;
    let mut img = match (sniff::sniff(bytes), wrapping) {
        (Some(f), _) => decode_as(bytes, f, limits)?,
        (None, XarWrapping::Dib) => decode_as(bytes, ImageFormat::Dib, limits)?,
        (None, XarWrapping::DibCompressed) => {
            let inflated = inflate_bounded(bytes, limits)?;
            match sniff::sniff(&inflated) {
                Some(f) => decode_as(&inflated, f, limits)?,
                None => decode_as(&inflated, ImageFormat::Dib, limits)?,
            }
        }
        (None, _) => return Err(DecodeError::UnknownFormat),
    };
    if wrapping == XarWrapping::Jpeg8Bpp && !palette.is_empty() && palette.len() <= 256 {
        snap_to_palette(&mut img.data.pixels, palette);
        img.info.depth = 8;
        img.info.palette_entries = palette.len() as u16;
    }
    Ok(img)
}

/// Maps every (opaque) pixel to its nearest palette entry by squared RGB
/// distance, lowest index on a tie. Undithered, as the original's
/// reconstruction requests no dithering (`wxOil/dibutil.cpp:3671-3780`).
/// The distance metric of the original lives in the closed rasteriser and
/// is not observable; nearest-RGB is our choice.
pub fn snap_to_palette(pixels: &mut [u8], palette: &[[u8; 3]]) {
    if palette.is_empty() {
        return;
    }
    // JPEG output is opaque, so pixels are straight here. A direct-mapped
    // cache keyed by the exact colour turns the 256-entry search into one
    // lookup for every repeated colour; a miss only costs the search.
    const EMPTY: u32 = u32::MAX;
    let mut cache = vec![(EMPTY, [0u8; 3]); 1 << 16];
    for px in pixels.as_chunks_mut::<4>().0 {
        let key = u32::from(px[0]) << 16 | u32::from(px[1]) << 8 | u32::from(px[2]);
        let slot = (key.wrapping_mul(0x9E37_79B1) >> 16) as usize;
        let out = match cache[slot] {
            (k, o) if k == key => o,
            _ => {
                let o = nearest(palette, [px[0], px[1], px[2]]);
                cache[slot] = (key, o);
                o
            }
        };
        px[..3].copy_from_slice(&out);
    }
}

fn nearest(palette: &[[u8; 3]], rgb: [u8; 3]) -> [u8; 3] {
    let mut best = palette[0];
    let mut best_d = u32::MAX;
    for p in palette {
        let d: u32 = (0..3)
            .map(|i| {
                let e = i32::from(rgb[i]) - i32::from(p[i]);
                e.unsigned_abs() * e.unsigned_abs()
            })
            .sum();
        if d < best_d {
            best_d = d;
            best = *p;
            if d == 0 {
                break;
            }
        }
    }
    best
}

/// Inflates a BMPZIP payload, refusing to grow past what the limits allow
/// for a decoded image. Accepts a zlib stream or raw DEFLATE.
fn inflate_bounded(bytes: &[u8], limits: &DecodeLimits) -> Result<Vec<u8>, DecodeError> {
    let cap = limits.max_decoded_bytes.saturating_add(1 << 20);
    let zlib = bytes.len() >= 2
        && bytes[0] & 0x0F == 8
        && (u16::from(bytes[0]) << 8 | u16::from(bytes[1])) % 31 == 0;
    let mut out = Vec::new();
    let r = if zlib {
        flate2::read::ZlibDecoder::new(bytes)
            .take(cap + 1)
            .read_to_end(&mut out)
    } else {
        flate2::read::DeflateDecoder::new(bytes)
            .take(cap + 1)
            .read_to_end(&mut out)
    };
    r.map_err(|e| DecodeError::Corrupt(Box::new(e)))?;
    if out.len() as u64 > cap {
        return Err(DecodeError::SuspiciousRatio {
            ratio: out.len() as u64 / (bytes.len() as u64).max(1),
        });
    }
    Ok(out)
}

/// Prepends a `BITMAPFILEHEADER` to a headerless DIB, so a standard BMP
/// decoder can read it. The pixel-data offset is computed from the info
/// header: its size, the colour masks of `BI_BITFIELDS`, and the palette.
///
/// # Errors
///
/// [`DecodeError::Corrupt`] when the info header is truncated or declares a
/// palette the bytes cannot hold.
pub fn dib_to_bmp(dib: &[u8]) -> Result<Vec<u8>, DecodeError> {
    let le32 = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(dib.get(at..at + 4)?.try_into().ok()?))
    };
    let le16 = |at: usize| -> Option<u16> {
        Some(u16::from_le_bytes(dib.get(at..at + 2)?.try_into().ok()?))
    };
    let bad = || DecodeError::corrupt("truncated DIB header");
    let hsize = le32(0).ok_or_else(bad)?;
    let (bpp, compression, clr_used, entry) = match hsize {
        12 => (u32::from(le16(10).ok_or_else(bad)?), 0, 0, 3u64),
        40 | 52 | 56 | 64 | 108 | 124 => (
            u32::from(le16(14).ok_or_else(bad)?),
            le32(16).ok_or_else(bad)?,
            le32(32).ok_or_else(bad)?,
            4u64,
        ),
        _ => return Err(DecodeError::corrupt("unknown DIB header size")),
    };
    // BI_BITFIELDS (3) / BI_ALPHABITFIELDS (6) with the 40-byte header keep
    // their masks after it; the larger headers hold them inside.
    let masks: u64 = match (hsize, compression) {
        (40, 3) => 12,
        (40, 6) => 16,
        _ => 0,
    };
    let palette: u64 = if bpp <= 8 && bpp > 0 {
        let n = if clr_used == 0 {
            1u64 << bpp
        } else {
            u64::from(clr_used).min(256)
        };
        n * entry
    } else {
        u64::from(clr_used.min(256)) * entry
    };
    let offset = 14 + u64::from(hsize) + masks + palette;
    if offset > 14 + dib.len() as u64 {
        return Err(DecodeError::corrupt("DIB palette runs past the data"));
    }
    let total = 14 + dib.len() as u64;
    let mut out = Vec::with_capacity(dib.len() + 14);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&u32::try_from(total).unwrap_or(u32::MAX).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(offset as u32).to_le_bytes());
    out.extend_from_slice(dib);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapping_picks_the_nearest_entry_and_the_lowest_on_ties() {
        let pal = [[0, 0, 0], [255, 255, 255], [250, 0, 0], [250, 0, 0]];
        let mut px = vec![10, 10, 10, 255, 200, 200, 200, 255, 240, 10, 5, 255];
        snap_to_palette(&mut px, &pal);
        assert_eq!(px, vec![0, 0, 0, 255, 255, 255, 255, 255, 250, 0, 0, 255]);
    }

    #[test]
    fn dib_header_synthesis_rejects_garbage_without_panicking() {
        for n in 0..64 {
            let junk: Vec<u8> = (0..n as u8).collect();
            let _ = dib_to_bmp(&junk);
        }
        assert!(dib_to_bmp(&[40, 0, 0, 0]).is_err());
    }

    #[test]
    fn tags_map_to_wrappings() {
        assert_eq!(XarWrapping::from_tag(65), Some(XarWrapping::Dib));
        assert_eq!(XarWrapping::from_tag(69), Some(XarWrapping::DibCompressed));
        assert_eq!(XarWrapping::from_tag(71), Some(XarWrapping::Jpeg8Bpp));
        assert_eq!(XarWrapping::from_tag(68), Some(XarWrapping::Plain));
        assert_eq!(XarWrapping::from_tag(61), Some(XarWrapping::Plain));
        assert_eq!(XarWrapping::from_tag(70), None);
    }
}
