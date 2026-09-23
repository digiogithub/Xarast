//! Format sniffing and the few header facts the `image` decoders do not
//! expose: resolution, the PNG colour chunks, the JPEG scan count.
//!
//! Every function here is a bounded walk over a borrowed slice: no
//! allocation, no panic, whatever the bytes.

use crate::ImageFormat;

/// Identifies the container from its magic bytes. A headerless DIB cannot be
/// sniffed; only the `.xar` wrapper knows one is there.
#[must_use]
pub fn sniff(b: &[u8]) -> Option<ImageFormat> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageFormat::Png)
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(ImageFormat::Jpeg)
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some(ImageFormat::Gif)
    } else if b.len() >= 12 && b.starts_with(b"RIFF") && b.get(8..12) == Some(b"WEBP") {
        Some(ImageFormat::WebP)
    } else if b.starts_with(b"II*\0") || b.starts_with(b"MM\0*") {
        Some(ImageFormat::Tiff)
    } else if b.starts_with(b"BM") {
        Some(ImageFormat::Bmp)
    } else if b.len() >= 3
        && b[0] == b'P'
        && (b'1'..=b'7').contains(&b[1])
        && b[2].is_ascii_whitespace()
    {
        Some(ImageFormat::Pnm)
    } else {
        None
    }
}

/// Facts read from the header chunks, all optional.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub(crate) struct HeaderFacts {
    /// Resolution in dots per inch, when declared.
    pub dpi: Option<(u32, u32)>,
    /// PNG `sRGB` present.
    pub srgb: bool,
    /// PNG `gAMA`, as the decoding gamma.
    pub gamma: Option<f32>,
    /// PNG colour type is greyscale (0 or 4).
    pub grey: bool,
    /// Source bits per pixel, when the header states it.
    pub depth: Option<u8>,
    /// Palette entries, when the source is indexed.
    pub palette: Option<u16>,
    /// JPEG `SOS` markers counted.
    pub jpeg_scans: u32,
    /// JPEG frame dimensions from the `SOF` segment.
    pub jpeg_dims: Option<(u32, u32)>,
}

/// The EXIF block (without the `Exif\0\0` prefix) and the reassembled ICC
/// profile of a JPEG, read from its APP1/APP2 segments without decoding.
pub(crate) fn jpeg_metadata(b: &[u8]) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let mut exif = None;
    let mut icc: Vec<(u8, &[u8])> = Vec::new();
    let mut i = 2usize;
    while i + 3 < b.len() {
        if b[i] != 0xFF {
            break;
        }
        let m = b[i + 1];
        if m == 0xFF {
            i += 1;
            continue;
        }
        if m == 0xDA || m == 0xD9 {
            break;
        }
        let Some(len) = be16(b, i + 2).map(usize::from) else {
            break;
        };
        let seg = b.get(i + 4..(i + 2).saturating_add(len)).unwrap_or(&[]);
        if m == 0xE1 && exif.is_none() && seg.starts_with(b"Exif\0\0") {
            exif = Some(seg[6..].to_vec());
        } else if m == 0xE2 && seg.starts_with(b"ICC_PROFILE\0") && seg.len() >= 14 {
            icc.push((seg[12], &seg[14..]));
        }
        i = (i + 2).saturating_add(len.max(2));
    }
    let icc = (!icc.is_empty()).then(|| {
        icc.sort_by_key(|(seq, _)| *seq);
        icc.into_iter()
            .flat_map(|(_, d)| d.iter().copied())
            .collect()
    });
    (exif, icc)
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn le16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Pixels per metre to dots per inch, rounded; zero or absurd is `None`.
fn ppm_to_dpi(ppm: u32) -> Option<u32> {
    let dpi = (f64::from(ppm) * 0.0254).round();
    (1.0..=1_000_000.0).contains(&dpi).then_some(dpi as u32)
}

/// `count_scans` walks a JPEG's entropy-coded data to count every scan; it
/// is linear in the file size, so [`crate::probe`] skips it and only the
/// decode pays for it.
pub(crate) fn header_facts(format: ImageFormat, b: &[u8], count_scans: bool) -> HeaderFacts {
    match format {
        ImageFormat::Png => png_facts(b),
        ImageFormat::Jpeg => jpeg_facts(b, count_scans),
        ImageFormat::Bmp => dib_facts(b.get(14..).unwrap_or(&[])),
        ImageFormat::Dib => dib_facts(b),
        ImageFormat::Gif => gif_facts(b),
        _ => HeaderFacts::default(),
    }
}

fn png_facts(b: &[u8]) -> HeaderFacts {
    let mut f = HeaderFacts::default();
    let mut at = 8usize;
    // Bounded: every chunk advances by at least 12 bytes.
    while let (Some(len), Some(kind)) = (be32(b, at), b.get(at + 4..at + 8)) {
        let data_at = at + 8;
        let len = len as usize;
        let data = b.get(data_at..data_at.saturating_add(len)).unwrap_or(&[]);
        match kind {
            b"IHDR" => {
                let bit_depth = data.get(8).copied().unwrap_or(8);
                let colour = data.get(9).copied().unwrap_or(2);
                f.grey = matches!(colour, 0 | 4);
                let channels: u8 = match colour {
                    0 | 3 => 1,
                    4 => 2,
                    2 => 3,
                    _ => 4,
                };
                f.depth = Some(bit_depth.saturating_mul(channels));
            }
            b"PLTE" => f.palette = u16::try_from(len / 3).ok(),
            b"sRGB" => f.srgb = true,
            b"gAMA" => {
                if let Some(g) = be32(data, 0).filter(|g| *g > 0) {
                    f.gamma = Some(100_000.0 / g as f32);
                }
            }
            b"pHYs" => {
                if data.get(8) == Some(&1)
                    && let (Some(x), Some(y)) = (be32(data, 0), be32(data, 4))
                    && let (Some(x), Some(y)) = (ppm_to_dpi(x), ppm_to_dpi(y))
                {
                    f.dpi = Some((x, y));
                }
            }
            b"IDAT" | b"IEND" => break,
            _ => {}
        }
        match data_at.checked_add(len).and_then(|e| e.checked_add(4)) {
            Some(next) if next > at => at = next,
            _ => break,
        }
    }
    f
}

fn jpeg_facts(b: &[u8], count_scans: bool) -> HeaderFacts {
    let mut f = HeaderFacts {
        depth: Some(24),
        ..HeaderFacts::default()
    };
    let mut i = 2usize;
    while i + 1 < b.len() {
        if b[i] != 0xFF {
            i += 1;
            continue;
        }
        let m = b[i + 1];
        match m {
            // Fill bytes and markers without a length.
            0xFF => {
                i += 1;
                continue;
            }
            0x00 | 0x01 | 0xD0..=0xD7 => {
                i += 2;
                continue;
            }
            0xD8 => {
                i += 2;
                continue;
            }
            0xD9 => break,
            _ => {}
        }
        let Some(len) = be16(b, i + 2).map(usize::from) else {
            break;
        };
        let seg = b.get(i + 4..(i + 2).saturating_add(len)).unwrap_or(&[]);
        match m {
            0xE0 if seg.starts_with(b"JFIF\0") => {
                let units = seg.get(7).copied().unwrap_or(0);
                if let (Some(x), Some(y)) = (be16(seg, 8), be16(seg, 10)) {
                    let (x, y) = (u32::from(x), u32::from(y));
                    f.dpi = match units {
                        1 if x > 0 && y > 0 => Some((x, y)),
                        2 if x > 0 && y > 0 => Some((
                            (f64::from(x) * 2.54).round() as u32,
                            (f64::from(y) * 2.54).round() as u32,
                        )),
                        _ => f.dpi,
                    };
                }
            }
            // Baseline, extended, progressive, lossless SOF: component count.
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                if let Some(n) = seg.get(5) {
                    f.depth = Some(n.saturating_mul(8));
                }
                if let (Some(h), Some(w)) = (be16(seg, 1), be16(seg, 3)) {
                    f.jpeg_dims = Some((u32::from(w), u32::from(h)));
                }
            }
            0xDA => {
                f.jpeg_scans = f.jpeg_scans.saturating_add(1);
                if !count_scans {
                    break;
                }
                // Skip the entropy-coded data up to the next real marker.
                let mut j = (i + 2).saturating_add(len);
                while j + 1 < b.len() {
                    if b[j] == 0xFF && !matches!(b[j + 1], 0x00 | 0xD0..=0xD7 | 0xFF) {
                        break;
                    }
                    j += 1;
                }
                i = j;
                continue;
            }
            _ => {}
        }
        i = (i + 2).saturating_add(len.max(2));
    }
    f
}

/// Facts from a `BITMAPINFOHEADER` (or the 12-byte `BITMAPCOREHEADER`).
fn dib_facts(h: &[u8]) -> HeaderFacts {
    let mut f = HeaderFacts::default();
    let size = le32(h, 0).unwrap_or(0);
    if size == 12 {
        f.depth = le16(h, 10).and_then(|d| u8::try_from(d).ok());
        return f;
    }
    f.depth = le16(h, 14).and_then(|d| u8::try_from(d).ok());
    if let (Some(x), Some(y)) = (le32(h, 24), le32(h, 28))
        && let (Some(x), Some(y)) = (ppm_to_dpi(x), ppm_to_dpi(y))
    {
        f.dpi = Some((x, y));
    }
    if let Some(d) = f.depth.filter(|d| *d <= 8) {
        let used = le32(h, 32).unwrap_or(0);
        let n = if used == 0 { 1u32 << d } else { used.min(256) };
        f.palette = u16::try_from(n).ok();
    }
    f
}

fn gif_facts(b: &[u8]) -> HeaderFacts {
    let mut f = HeaderFacts {
        depth: Some(8),
        ..HeaderFacts::default()
    };
    if let Some(flags) = b.get(10)
        && flags & 0x80 != 0
    {
        f.palette = Some(2u16 << (flags & 7));
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_each_magic() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\nxxxx"), Some(ImageFormat::Png));
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Some(ImageFormat::Jpeg));
        assert_eq!(sniff(b"GIF89a"), Some(ImageFormat::Gif));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some(ImageFormat::WebP));
        assert_eq!(sniff(b"II*\0"), Some(ImageFormat::Tiff));
        assert_eq!(sniff(b"MM\0*"), Some(ImageFormat::Tiff));
        assert_eq!(sniff(b"BM\0\0"), Some(ImageFormat::Bmp));
        assert_eq!(sniff(b"P6\n1 1\n255\n"), Some(ImageFormat::Pnm));
        assert_eq!(sniff(b"P9\n"), None);
        assert_eq!(sniff(b""), None);
        assert_eq!(sniff(b"RIFF"), None);
    }

    #[test]
    fn header_walks_never_panic_on_garbage() {
        let junk: Vec<u8> = (0..4096u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
            .collect();
        for f in [
            ImageFormat::Png,
            ImageFormat::Jpeg,
            ImageFormat::Bmp,
            ImageFormat::Dib,
            ImageFormat::Gif,
        ] {
            for n in 0..64 {
                let _ = header_facts(f, &junk[..n], true);
            }
            let _ = header_facts(f, &junk, true);
        }
    }
}
