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
//! And one embeds a standard file with a non-standard meaning:
//!
//! - **68 `TAG_DEFINEBITMAP_PNG`**: when the PNG has an alpha channel
//!   (colour type 4 or 6) that channel holds **transparency**, 0 = opaque —
//!   the original's 32 bpp convention, written without the inversion its
//!   PNG export applies. [`normalise_xar_png`] turns such a file into a
//!   standard PNG; [`decode_xar_bitmap`] inverts while decoding.
//!
//! The importer hands this module the tag, the image bytes and (for 71) the
//! palette; it needs nothing else from `xarast-xar`, which keeps this crate a
//! leaf and lets the fuzzer reach the same code.

use std::io::Read;

use crate::decode::{Native, decode_as_with, decode_native};
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
    /// A PNG whose alpha channel, if it has one, holds transparency.
    PngTransparency,
}

impl XarWrapping {
    /// The wrapping a tag declares, or `None` for a tag that is not a bitmap.
    #[must_use]
    pub const fn from_tag(tag: u32) -> Option<XarWrapping> {
        Some(match tag {
            TAG_DEFINEBITMAP_BMP => XarWrapping::Dib,
            TAG_DEFINEBITMAP_BMPZIP => XarWrapping::DibCompressed,
            TAG_DEFINEBITMAP_JPEG8BPP => XarWrapping::Jpeg8Bpp,
            TAG_DEFINEBITMAP_PNG => XarWrapping::PngTransparency,
            TAG_DEFINEBITMAP_GIF | TAG_DEFINEBITMAP_JPEG => XarWrapping::Plain,
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
        (Some(ImageFormat::Png), XarWrapping::PngTransparency)
            if png_alpha_is_transparency(bytes) =>
        {
            decode_as_with(bytes, ImageFormat::Png, limits, |n: &mut Native| {
                n.invert_alpha();
            })?
        }
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
    // The original reconstructs only a 24 bpp result (`Kernel/bitmap.cpp:817-822`):
    // an opaque JPEG. Anything else found under tag 71 is kept as decoded.
    if wrapping == XarWrapping::Jpeg8Bpp
        && img.format == ImageFormat::Jpeg
        && !img.info.has_alpha
        && !palette.is_empty()
        && palette.len() <= 256
    {
        snap_to_palette(&mut img.data.pixels, palette);
        img.info.depth = 8;
        img.info.palette_entries = palette.len() as u16;
    }
    Ok(img)
}

/// Whether a tag-68 image is a PNG with an alpha channel (IHDR colour type
/// 4, grey + alpha, or 6, RGBA), and so stores transparency there.
///
/// A palette PNG is not: the original writes 8 bpp and below with a single
/// transparent index, which means what it says (`research/01 §4.5`).
#[must_use]
pub fn png_alpha_is_transparency(bytes: &[u8]) -> bool {
    // Signature (8), IHDR length (4), "IHDR" (4), width, height (8),
    // bit depth (1), then the colour type at offset 25.
    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        && bytes.get(12..16) == Some(b"IHDR")
        && matches!(bytes.get(25), Some(4 | 6))
}

/// Rewrites a tag-68 PNG whose alpha channel holds transparency as a
/// standard PNG, so that the bytes mean the same thing in a browser, in a
/// `.xarast` package and to [`crate::decode`].
///
/// Returns `Ok(None)` when the file needs nothing (no alpha channel, or not
/// a PNG). Otherwise the pixels are decoded under `limits` in their own
/// layout, the alpha samples inverted, and the image re-encoded at the same
/// bit depth and colour type — lossless. Every ancillary chunk of the
/// original (`pHYs`, `sRGB`, `iCCP`, text…) is kept, in order, around the
/// new `IHDR` and `IDAT`; the result is never interlaced.
///
/// # Errors
///
/// Any [`DecodeError`] from the decode, or [`DecodeError::Corrupt`] when
/// the encoder refuses the layout.
pub fn normalise_xar_png(
    bytes: &[u8],
    limits: &DecodeLimits,
) -> Result<Option<Vec<u8>>, DecodeError> {
    use image::ImageEncoder;
    if !png_alpha_is_transparency(bytes) {
        return Ok(None);
    }
    let deadline = std::time::Instant::now() + limits.max_duration;
    let mut native = decode_native(bytes, ImageFormat::Png, limits, deadline)?;
    if !native.invert_alpha() {
        return Ok(None);
    }
    let mut fresh = Vec::new();
    image::codecs::png::PngEncoder::new(&mut fresh)
        .write_image(
            &native.buf,
            native.width,
            native.height,
            native.color.into(),
        )
        .map_err(|e| DecodeError::corrupt(format!("PNG re-encode: {e}")))?;
    drop(native);
    let old = png_chunks(bytes).ok_or_else(|| DecodeError::corrupt("PNG chunk list"))?;
    let new = png_chunks(&fresh).ok_or_else(|| DecodeError::corrupt("re-encoded chunk list"))?;
    let mut out = Vec::with_capacity(fresh.len() + bytes.len() / 8);
    out.extend_from_slice(&bytes[..8]);
    let pick = |list: &[(&[u8; 4], &[u8])], ty: &[u8; 4]| -> Vec<u8> {
        list.iter()
            .filter(|(t, _)| *t == ty)
            .flat_map(|(_, c)| c.iter().copied())
            .collect()
    };
    out.extend_from_slice(&pick(&new, b"IHDR"));
    let first_idat = old
        .iter()
        .position(|(t, _)| *t == b"IDAT")
        .unwrap_or(old.len());
    let keep = |(t, _): &&(&[u8; 4], &[u8])| !matches!(*t, b"IHDR" | b"IDAT" | b"IEND");
    for (_, c) in old[..first_idat].iter().filter(keep) {
        out.extend_from_slice(c);
    }
    out.extend_from_slice(&pick(&new, b"IDAT"));
    for (_, c) in old[first_idat..].iter().filter(keep) {
        out.extend_from_slice(c);
    }
    out.extend_from_slice(&pick(&new, b"IEND"));
    Ok(Some(out))
}

/// A PNG's chunks as `(type, whole chunk bytes)`, bounds-checked; `None` on
/// a truncated or overlong chunk.
fn png_chunks(bytes: &[u8]) -> Option<Vec<(&[u8; 4], &[u8])>> {
    let mut out = Vec::new();
    let mut at = 8usize;
    while at < bytes.len() {
        let len = u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?) as usize;
        let end = at.checked_add(12)?.checked_add(len)?;
        let chunk = bytes.get(at..end)?;
        let ty: &[u8; 4] = chunk.get(4..8)?.try_into().ok()?;
        out.push((ty, chunk));
        at = end;
        if ty == b"IEND" {
            break;
        }
    }
    Some(out)
}

/// Maps every pixel to its nearest palette entry by squared RGB
/// distance, lowest index on a tie. Undithered, as the original's
/// reconstruction requests no dithering (`wxOil/dibutil.cpp:3741`).
/// The distance metric of the original lives in the closed rasteriser and
/// is not observable; nearest-RGB is our choice.
pub fn snap_to_palette(pixels: &mut [u8], palette: &[[u8; 3]]) {
    if palette.is_empty() {
        return;
    }
    // Matching is on the straight colour; the result is premultiplied back.
    // For the opaque JPEG of tag 71 both steps are the identity. A
    // direct-mapped cache keyed by the exact colour turns the search into
    // one lookup for every repeated colour; a miss scans only the entries
    // that can win in the colour's cell (`Nearest`), not the whole palette.
    const EMPTY: u32 = u32::MAX;
    let mut search = Nearest::new(palette);
    let mut cache = vec![(EMPTY, [0u8; 3]); 1 << 16];
    for px in pixels.as_chunks_mut::<4>().0 {
        let a = px[3];
        let rgb = [0, 1, 2].map(|i| {
            if a == 255 {
                px[i]
            } else {
                crate::pixels::unpremultiply(px[i], a)
            }
        });
        let key = u32::from(rgb[0]) << 16 | u32::from(rgb[1]) << 8 | u32::from(rgb[2]);
        let slot = (key.wrapping_mul(0x9E37_79B1) >> 16) as usize;
        let out = match cache[slot] {
            (k, o) if k == key => o,
            _ => {
                let o = palette[search.index(rgb)];
                cache[slot] = (key, o);
                o
            }
        };
        for (c, o) in px[..3].iter_mut().zip(out) {
            *c = crate::pixels::premultiply(o, a);
        }
    }
}

/// Bits of each channel that pick a cell of [`Nearest`]'s grid.
const CELL_BITS: u32 = 4;

/// The nearest-entry search of [`snap_to_palette`]. The colour cube is cut
/// into `16^3` cells, and each cell keeps, built the first time a colour
/// falls in it, the entries that can be nearest to some colour of the
/// cell: those no farther from the cell than the farthest point of the
/// cell is from the entry that is best in the worst case. A query scans
/// only its cell's list, so the result is exactly the brute-force one,
/// ties included (an entry at the winning distance is on the list too).
struct Nearest {
    palette: Vec<[i32; 3]>,
    /// Candidate palette indices per cell, ascending.
    cells: Vec<Option<Box<[u32]>>>,
}

impl Nearest {
    fn new(palette: &[[u8; 3]]) -> Self {
        Self {
            palette: palette.iter().map(|p| p.map(i32::from)).collect(),
            cells: vec![None; 1 << (3 * CELL_BITS)],
        }
    }

    /// The palette index of the entry nearest to `rgb` by squared RGB
    /// distance, the lowest index on a tie. The palette is not empty.
    fn index(&mut self, rgb: [u8; 3]) -> usize {
        let shift = 8 - CELL_BITS;
        let cell = rgb
            .iter()
            .fold(0usize, |acc, &c| acc << CELL_BITS | usize::from(c >> shift));
        let q = rgb.map(i32::from);
        let palette = &self.palette;
        let list = self.cells[cell].get_or_insert_with(|| {
            let lo = rgb.map(|c| i32::from(c >> shift << shift));
            candidates(palette, lo, (1 << shift) - 1)
        });
        let mut best = (i32::MAX, u32::MAX);
        for &i in list.iter() {
            let d = dist2(q, palette[i as usize]);
            if d < best.0 {
                best = (d, i);
            }
        }
        best.1 as usize
    }
}

fn dist2(a: [i32; 3], b: [i32; 3]) -> i32 {
    (a[0] - b[0]).pow(2) + (a[1] - b[1]).pow(2) + (a[2] - b[2]).pow(2)
}

/// The palette entries that can be nearest to a colour in the box
/// `lo..=lo + span` on each channel, ascending.
fn candidates(palette: &[[i32; 3]], lo: [i32; 3], span: i32) -> Box<[u32]> {
    // Squared distance from `p` to the box's nearest and farthest points.
    let bounds = |p: [i32; 3]| {
        (0..3).fold((0, 0), |(near, far), c| {
            let (a, b) = (lo[c], lo[c] + span);
            let gap = (a - p[c]).max(p[c] - b).max(0);
            let reach = (p[c] - a).abs().max((p[c] - b).abs());
            (near + gap * gap, far + reach * reach)
        })
    };
    let bound = palette.iter().map(|&p| bounds(p).1).min().unwrap_or(0);
    (0u32..)
        .zip(palette)
        .filter(|&(_, &p)| bounds(p).0 <= bound)
        .map(|(i, _)| i)
        .collect()
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

    /// The first entry at the least squared distance, searched the plain way.
    fn nearest_reference(palette: &[[u8; 3]], rgb: [u8; 3]) -> [u8; 3] {
        let dist = |p: &[u8; 3]| -> u32 {
            (0..3)
                .map(|i| u32::from(rgb[i].abs_diff(p[i])).pow(2))
                .sum()
        };
        let mut best = palette[0];
        for p in palette {
            if dist(p) < dist(&best) {
                best = *p;
            }
        }
        best
    }

    #[test]
    fn the_cell_search_matches_the_plain_one() {
        // A small LCG: deterministic, no dependency.
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 33) as u32
        };
        for (size, coarse) in [1usize, 2, 3, 16, 255, 256, 1000, 20_000]
            .into_iter()
            .flat_map(|n| [(n, true), (n, false)])
        {
            // Coarse entries make duplicates and equal distances occur.
            let palette: Vec<[u8; 3]> = (0..size)
                .map(|_| {
                    [0; 3].map(|_: u8| {
                        if coarse {
                            (next() % 8 * 36) as u8
                        } else {
                            (next() & 0xFF) as u8
                        }
                    })
                })
                .collect();
            let mut search = Nearest::new(&palette);
            for _ in 0..2000 {
                let rgb = [0; 3].map(|_: u8| (next() & 0xFF) as u8);
                assert_eq!(
                    palette[search.index(rgb)],
                    nearest_reference(&palette, rgb),
                    "palette of {size}, colour {rgb:?}"
                );
            }
        }
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
        assert_eq!(
            XarWrapping::from_tag(68),
            Some(XarWrapping::PngTransparency)
        );
        assert_eq!(XarWrapping::from_tag(67), Some(XarWrapping::Plain));
        assert_eq!(XarWrapping::from_tag(61), Some(XarWrapping::Plain));
        assert_eq!(XarWrapping::from_tag(70), None);
    }
}
