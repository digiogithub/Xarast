//! WOFF2 (W3C Recommendation "WOFF File Format 2.0"), written from the
//! specification.
//!
//! The encoder applies the **`glyf` transform** (§5.1, transform version 0
//! for `glyf` and `loca`) to TrueType fonts, stores every other table with
//! the null transform, and compresses the table data as one Brotli stream
//! in font mode. A `glyf` that does not parse is stored untransformed
//! (version 3). Output depends on the input only (single-threaded Brotli,
//! tables in a fixed order).
//!
//! [`decode`] reads single-font WOFF2 files from any conforming writer: the
//! null transforms, the `glyf`/`loca` transform and the `hmtx` transform
//! (§5.4). It is what the `.xarast` reader's document fonts, the tests and
//! the tools that render an exported SVG with its embedded fonts use.
//! Collections (`ttcf`) are refused.

use std::io::{Read, Write};

use super::sfnt::{Sfnt, Tag};
use super::woff2_glyf;

/// The "known table" tags of WOFF2 §5.1, in flag-index order.
const KNOWN: [&[u8; 4]; 63] = [
    b"cmap", b"head", b"hhea", b"hmtx", b"maxp", b"name", b"OS/2", b"post", b"cvt ", b"fpgm",
    b"glyf", b"loca", b"prep", b"CFF ", b"VORG", b"EBDT", b"EBLC", b"gasp", b"hdmx", b"kern",
    b"LTSH", b"PCLT", b"VDMX", b"vhea", b"vmtx", b"BASE", b"GDEF", b"GPOS", b"GSUB", b"EBSC",
    b"JSTF", b"MATH", b"CBDT", b"CBLC", b"COLR", b"CPAL", b"SVG ", b"sbix", b"acnt", b"avar",
    b"bdat", b"bloc", b"bsln", b"cvar", b"fdsc", b"feat", b"fmtx", b"fvar", b"gvar", b"hsty",
    b"just", b"lcar", b"mort", b"morx", b"opbd", b"prop", b"trak", b"Zapf", b"Silf", b"Glat",
    b"Gloc", b"Feat", b"Sill",
];

const SIGNATURE: u32 = 0x774F_4632; // "wOF2"
const HEADER_LEN: usize = 48;
/// The largest font we are willing to rebuild when decoding.
const MAX_DECODED: usize = 64 << 20;

/// Brotli quality for the table stream: the specification's recommended
/// maximum. Subsets are small, so the cost is milliseconds.
const QUALITY: i32 = 11;

/// `UIntBase128`: big-endian groups of 7 bits, high bit = more follow.
fn push_base128(out: &mut Vec<u8>, v: u32) {
    let mut groups = [0u8; 5];
    let mut n = 0;
    let mut v = v;
    loop {
        groups[n] = (v & 0x7F) as u8;
        n += 1;
        v >>= 7;
        if v == 0 {
            break;
        }
    }
    for i in (0..n).rev() {
        let more = if i > 0 { 0x80 } else { 0 };
        out.push(groups[i] | more);
    }
}

fn read_base128(b: &[u8], at: &mut usize) -> Option<u32> {
    let mut v: u32 = 0;
    for i in 0..5 {
        let byte = *b.get(*at)?;
        *at += 1;
        // No leading zeros.
        if i == 0 && byte == 0x80 {
            return None;
        }
        if v & 0xFE00_0000 != 0 {
            return None;
        }
        v = (v << 7) | u32::from(byte & 0x7F);
        if byte & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

/// The order tables are stored in: by tag, except that `loca` follows
/// `glyf` directly (§5.1 requires it).
fn stored_order(s: &Sfnt) -> Vec<(Tag, &[u8])> {
    let mut out: Vec<(Tag, &[u8])> = Vec::with_capacity(s.tables.len());
    for (tag, data) in &s.tables {
        if tag == b"loca" && s.tables.contains_key(b"glyf") {
            continue;
        }
        out.push((*tag, data.as_slice()));
        if tag == b"glyf"
            && let Some(loca) = s.tables.get(b"loca")
        {
            out.push((*b"loca", loca.as_slice()));
        }
    }
    out
}

/// `head.indexToLocFormat`.
fn index_format(s: &Sfnt) -> Option<u16> {
    let h = s.table(b"head")?.get(50..52)?;
    Some(u16::from_be_bytes([h[0], h[1]]))
}

/// Encodes a font as WOFF2.
pub(crate) fn encode(s: &Sfnt) -> Option<Vec<u8>> {
    encode_with(s, true)
}

/// Encodes a font as WOFF2, with the `glyf` transform when `transform`.
pub(crate) fn encode_with(s: &Sfnt, transform: bool) -> Option<Vec<u8>> {
    let tables = stored_order(s);
    let num = u16::try_from(tables.len()).ok()?;
    let glyf = if transform {
        s.table(b"glyf")
            .zip(s.table(b"loca"))
            .zip(index_format(s))
            .and_then(|((g, l), f)| woff2_glyf::transform(g, l, f))
    } else {
        None
    };
    let mut dir = Vec::new();
    let mut stream = Vec::new();
    let mut sfnt_size: u64 = 12 + 16 * u64::from(num);
    for (tag, data) in &tables {
        let len = u32::try_from(data.len()).ok()?;
        let outline = tag == b"glyf" || tag == b"loca";
        // glyf and loca: version 0 is the transform and 3 the null one;
        // every other table: version 0 is the null transform.
        let version: u8 = if outline && glyf.is_none() { 3 } else { 0 };
        match KNOWN.iter().position(|k| *k == tag) {
            Some(i) => dir.push((i as u8) | (version << 6)),
            None => {
                dir.push(63 | (version << 6));
                dir.extend_from_slice(tag);
            }
        }
        push_base128(&mut dir, len);
        match (&glyf, outline) {
            (Some(t), true) => {
                // The transformed glyf holds loca too: its own length is 0.
                let t: &[u8] = if tag == b"glyf" { t } else { &[] };
                push_base128(&mut dir, u32::try_from(t.len()).ok()?);
                stream.extend_from_slice(t);
            }
            _ => stream.extend_from_slice(data),
        }
        sfnt_size += u64::from(len).div_ceil(4) * 4;
    }
    let params = brotli::enc::BrotliEncoderParams {
        quality: QUALITY,
        lgwin: 22,
        mode: brotli::enc::backward_references::BrotliEncoderMode::BROTLI_MODE_FONT,
        size_hint: stream.len(),
        ..brotli::enc::BrotliEncoderParams::default()
    };
    let mut compressed = Vec::new();
    brotli::BrotliCompress(&mut stream.as_slice(), &mut compressed, &params).ok()?;

    let mut out = Vec::with_capacity(HEADER_LEN + dir.len() + compressed.len() + 3);
    let total = (HEADER_LEN + dir.len() + compressed.len()).div_ceil(4) * 4;
    out.extend_from_slice(&SIGNATURE.to_be_bytes());
    out.extend_from_slice(&s.flavor.to_be_bytes());
    out.extend_from_slice(&u32::try_from(total).ok()?.to_be_bytes());
    out.extend_from_slice(&num.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&u32::try_from(sfnt_size).ok()?.to_be_bytes());
    out.extend_from_slice(&u32::try_from(compressed.len()).ok()?.to_be_bytes());
    // Version of the font data: head.fontRevision's two halves.
    let rev = s
        .table(b"head")
        .and_then(|h| h.get(4..8))
        .unwrap_or(&[0; 4]);
    out.extend_from_slice(rev);
    // No metadata, no private data.
    out.extend_from_slice(&[0u8; 20]);
    out.extend_from_slice(&dir);
    out.extend_from_slice(&compressed);
    out.resize(total, 0);
    Some(out)
}

/// Decodes a single-font WOFF2 file into a plain OpenType font file,
/// undoing the `glyf`/`loca` and `hmtx` transforms. `None` for anything
/// malformed, a collection, or a transform the specification does not
/// define.
#[must_use]
pub fn decode(b: &[u8]) -> Option<Vec<u8>> {
    let u32_at = |at: usize| -> Option<u32> {
        let s = b.get(at..at + 4)?;
        Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    };
    if u32_at(0)? != SIGNATURE {
        return None;
    }
    let flavor = u32_at(4)?;
    if flavor == u32::from_be_bytes(*b"ttcf") {
        return None;
    }
    let num = usize::from(u16::from_be_bytes([*b.get(12)?, *b.get(13)?]));
    let compressed_len = u32_at(20)? as usize;
    let mut at = HEADER_LEN;
    // (tag, stored length, transformed)
    let mut entries: Vec<(Tag, usize, bool)> = Vec::with_capacity(num);
    for _ in 0..num {
        let flags = *b.get(at)?;
        at += 1;
        let tag: Tag = match flags & 0x3F {
            63 => {
                let t = b.get(at..at + 4)?.try_into().ok()?;
                at += 4;
                t
            }
            i => **KNOWN.get(usize::from(i))?,
        };
        let version = flags >> 6;
        let outline = &tag == b"glyf" || &tag == b"loca";
        let transformed = match (outline, version) {
            (true, 3) | (false, 0) => false,
            (true, 0) => true,
            (false, 1) if &tag == b"hmtx" => true,
            _ => return None,
        };
        let orig = read_base128(b, &mut at)? as usize;
        let len = if transformed {
            read_base128(b, &mut at)? as usize
        } else {
            orig
        };
        entries.push((tag, len, transformed));
    }
    let compressed = b.get(at..at.checked_add(compressed_len)?)?;
    let mut data = Vec::new();
    let mut limited = brotli::Decompressor::new(compressed, 4096).take(MAX_DECODED as u64 + 1);
    limited.read_to_end(&mut data).ok()?;
    if data.len() > MAX_DECODED {
        return None;
    }
    let mut s = Sfnt {
        flavor,
        tables: std::collections::BTreeMap::new(),
    };
    let mut off = 0usize;
    let mut transformed: Vec<Tag> = Vec::new();
    for (tag, len, t) in entries {
        let end = off.checked_add(len)?;
        s.tables.insert(tag, data.get(off..end)?.to_vec());
        if t {
            transformed.push(tag);
        }
        off = end;
    }
    // The reference encoder pads the stream; nothing may follow the tables
    // but that padding.
    if data.len().checked_sub(off)? > 3 {
        return None;
    }
    let glyf_t = transformed.contains(b"glyf");
    if glyf_t != transformed.contains(b"loca") {
        return None;
    }
    let mut x_mins = None;
    if glyf_t {
        if s.table(b"loca").is_some_and(|l| !l.is_empty()) {
            return None;
        }
        let r = woff2_glyf::reconstruct(s.table(b"glyf")?)?;
        s.tables.insert(*b"glyf", r.glyf);
        s.tables.insert(*b"loca", r.loca);
        x_mins = Some(r.x_mins);
    }
    if transformed.contains(b"hmtx") {
        let hhea = s.table(b"hhea")?;
        let hmetrics = usize::from(u16::from_be_bytes([*hhea.get(34)?, *hhea.get(35)?]));
        let hmtx = woff2_glyf::reconstruct_hmtx(s.table(b"hmtx")?, hmetrics, x_mins.as_deref()?)?;
        s.tables.insert(*b"hmtx", hmtx);
    }
    let mut out = Vec::new();
    out.write_all(&s.to_bytes()).ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base128_round_trips_and_refuses_leading_zeros() {
        for v in [0u32, 1, 127, 128, 16_383, 16_384, 0x0FFF_FFFF, u32::MAX] {
            let mut b = Vec::new();
            push_base128(&mut b, v);
            let mut at = 0;
            assert_eq!(read_base128(&b, &mut at), Some(v));
            assert_eq!(at, b.len());
        }
        let mut at = 0;
        assert_eq!(read_base128(&[0x80, 0x01], &mut at), None);
    }

    #[test]
    fn a_truetype_font_survives_the_glyf_transform() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts");
        for f in [
            "NotoSans-Regular.subset.ttf",
            "NotoSansArabic-Regular.subset.ttf",
            "XarastTestVariable.ttf",
        ] {
            let bytes = std::fs::read(dir.join(f)).unwrap();
            let s = Sfnt::parse(&bytes, 0).unwrap();
            let plain = encode_with(&s, false).unwrap();
            let packed = encode(&s).unwrap();
            // Real faces shrink; the synthetic one (a few glyphs) may not.
            if f.starts_with("Noto") {
                assert!(
                    packed.len() < plain.len(),
                    "{f}: {} >= {}",
                    packed.len(),
                    plain.len()
                );
            }
            let back = Sfnt::parse(&decode(&packed).unwrap(), 0).unwrap();
            for (tag, data) in &s.tables {
                let mut data = data.clone();
                let mut got = back.table(tag).unwrap().to_vec();
                if tag == b"head" {
                    // checkSumAdjustment covers the rebuilt glyf.
                    data[8..12].fill(0);
                    got[8..12].fill(0);
                }
                if tag != b"glyf" && tag != b"loca" {
                    assert_eq!(got, data, "{f}");
                }
            }
            assert_eq!(back.tables.len(), s.tables.len());
            // A second pass is stable: the rebuilt glyf is canonical.
            let again = Sfnt::parse(&decode(&encode(&back).unwrap()).unwrap(), 0).unwrap();
            assert_eq!(again, back, "{f}");
        }
    }

    #[test]
    fn a_font_survives_encode_and_decode() {
        let mut tables = std::collections::BTreeMap::new();
        tables.insert(*b"head", (0..54u8).collect::<Vec<u8>>());
        tables.insert(*b"glyf", vec![7; 33]);
        tables.insert(*b"loca", vec![0, 0, 0, 33]);
        tables.insert(*b"zzzz", vec![1, 2, 3]);
        let s = Sfnt {
            flavor: super::super::sfnt::FLAVOR_TRUETYPE,
            tables,
        };
        let w = encode(&s).unwrap();
        assert_eq!(&w[0..4], b"wOF2");
        assert_eq!(w.len() % 4, 0);
        let back = Sfnt::parse(&decode(&w).unwrap(), 0).unwrap();
        assert_eq!(back.tables.len(), 4);
        assert_eq!(back.table(b"zzzz"), Some(&[1u8, 2, 3][..]));
        assert_eq!(back.table(b"glyf"), s.table(b"glyf"));
        assert!(decode(&w[..w.len() - 8]).is_none());
        assert!(decode(b"wOF2").is_none());
    }
}
