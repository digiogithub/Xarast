//! Which ZIP method each entry gets (`research/06 §4.2`, `§4.3`).
//!
//! Do not recompress what is already compressed, compress text, and for
//! anything the table does not name, measure: deflate the first 64 KiB at
//! level 1 and store the entry if that saves less than 5 %.

use std::io::Write;

use crate::Method;

/// Media types whose payload is already compressed: always STORED.
const ALREADY_COMPRESSED: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/webp",
    "image/avif",
    "image/jxl",
    "image/gif",
    "font/woff2",
    "application/zip",
    "application/gzip",
    "application/zstd",
];

/// How many leading bytes the escape heuristic samples.
pub const SAMPLE_LEN: usize = 64 << 10;

/// The ratio below which the escape heuristic stores (5 % saving).
pub const MIN_RATIO: f64 = 1.05;

/// The decision for the table of §4.3, before the escape heuristic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Already compressed.
    Stored,
    /// Text: always compressed.
    Compressed,
    /// Not in the table: the escape heuristic decides.
    Measure,
}

/// Classifies a media type per the table of §4.3.
pub fn classify(media_type: &str) -> Class {
    let mt = media_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if ALREADY_COMPRESSED.contains(&mt.as_str()) {
        return Class::Stored;
    }
    if mt.starts_with("text/")
        || mt.ends_with("+xml")
        || mt.ends_with("/xml")
        || mt.ends_with("+json")
        || mt == "application/json"
        // ICC profiles have repetitive tables: 2:1 to 4:1 (§4.3).
        || mt == "application/vnd.iccprofile"
        // Uncompressed pixels.
        || mt == "image/tiff"
        || mt == "image/bmp"
    {
        return Class::Compressed;
    }
    Class::Measure
}

/// The method for an entry of this media type and content, in the
/// `portable` profile. `deflate_level` is the level the whole entry will be
/// compressed at; the sample is always measured at level 1, as the spec says.
pub fn choose(media_type: &str, content: &[u8]) -> Method {
    match classify(media_type) {
        Class::Stored => Method::Stored,
        Class::Compressed => {
            if content.is_empty() {
                Method::Stored
            } else {
                Method::Deflate
            }
        }
        Class::Measure => {
            if compresses_well(content) {
                Method::Deflate
            } else {
                Method::Stored
            }
        }
    }
}

/// The escape heuristic: deflate the first [`SAMPLE_LEN`] bytes at level 1
/// and compare.
pub fn compresses_well(content: &[u8]) -> bool {
    let sample = content.get(..SAMPLE_LEN).unwrap_or(content);
    if sample.is_empty() {
        return false;
    }
    let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(1));
    let compressed = match enc.write_all(sample).and_then(|()| enc.finish()) {
        Ok(v) => v.len(),
        // Writing into a Vec cannot fail; if it somehow did, storing is the
        // safe choice.
        Err(_) => return false,
    };
    compressed > 0 && (sample.len() as f64) / (compressed as f64) >= MIN_RATIO
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table() {
        for mt in [
            "image/png",
            "image/jpeg",
            "IMAGE/WEBP",
            "font/woff2",
            "image/avif",
            "image/gif",
        ] {
            assert_eq!(choose(mt, b"whatever"), Method::Stored, "{mt}");
        }
        for mt in [
            "image/svg+xml",
            "application/xml",
            "text/plain",
            "application/vnd.iccprofile",
        ] {
            assert_eq!(choose(mt, b"x"), Method::Deflate, "{mt}");
        }
    }

    #[test]
    fn escape_heuristic() {
        // Incompressible: a xorshift stream.
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        let noise: Vec<u8> = (0..200_000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x as u8
            })
            .collect();
        assert_eq!(choose("application/octet-stream", &noise), Method::Stored);
        let text = b"repetitive ".repeat(20_000);
        assert_eq!(choose("application/octet-stream", &text), Method::Deflate);
        assert_eq!(choose("application/octet-stream", b""), Method::Stored);
    }
}
