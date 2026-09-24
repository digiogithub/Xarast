//! A minimal OpenType container: split a font file into its tables, add or
//! replace tables, and write a font file back (table directory, checksums,
//! `head.checkSumAdjustment`).
//!
//! Only what embedding needs. Every read is bounds-checked: the input is a
//! font file from the system or from our own subsetter, never trusted.

use std::collections::BTreeMap;

/// A four-byte table tag.
pub(crate) type Tag = [u8; 4];

/// `true` for a TrueType-outline font (`0x00010000` or `true`).
pub(crate) const FLAVOR_TRUETYPE: u32 = 0x0001_0000;
/// `OTTO`: CFF outlines.
pub(crate) const FLAVOR_CFF: u32 = 0x4F54_544F;

/// A font file as its tables, sorted by tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Sfnt {
    /// The sfnt version (flavour) of the file.
    pub flavor: u32,
    /// Table data by tag.
    pub tables: BTreeMap<Tag, Vec<u8>>,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    let s = b.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes([s[0], s[1]]))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

impl Sfnt {
    /// Parses face `index` of a font file or collection.
    pub(crate) fn parse(data: &[u8], index: u32) -> Option<Sfnt> {
        let mut base = 0usize;
        if data.get(0..4) == Some(b"ttcf") {
            let n = u32_at(data, 8)?;
            if index >= n {
                return None;
            }
            base = u32_at(data, 12usize.checked_add(4 * index as usize)?)? as usize;
        } else if index != 0 {
            return None;
        }
        let flavor = u32_at(data, base)?;
        if flavor != FLAVOR_TRUETYPE && flavor != FLAVOR_CFF && flavor != 0x7472_7565 {
            return None;
        }
        let count = usize::from(u16_at(data, base + 4)?);
        let mut tables = BTreeMap::new();
        for i in 0..count {
            let rec = base.checked_add(12)?.checked_add(i.checked_mul(16)?)?;
            let tag: Tag = data.get(rec..rec + 4)?.try_into().ok()?;
            let off = u32_at(data, rec + 8)? as usize;
            let len = u32_at(data, rec + 12)? as usize;
            let bytes = data.get(off..off.checked_add(len)?)?;
            tables.insert(tag, bytes.to_vec());
        }
        Some(Sfnt { flavor, tables })
    }

    /// The table `tag`, if present.
    pub(crate) fn table(&self, tag: &Tag) -> Option<&[u8]> {
        self.tables.get(tag).map(Vec::as_slice)
    }

    /// Writes a single-face font file: tables in tag order, each padded to
    /// four bytes, checksums and `head.checkSumAdjustment` computed.
    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        let n = self.tables.len();
        let count = u16::try_from(n).unwrap_or(u16::MAX);
        let mut entry_selector = 0u16;
        while (2u32 << entry_selector) <= u32::from(count) {
            entry_selector += 1;
        }
        let search_range = (1u16 << entry_selector).saturating_mul(16);
        let range_shift = count.saturating_mul(16).saturating_sub(search_range);
        let mut out = Vec::new();
        out.extend_from_slice(&self.flavor.to_be_bytes());
        out.extend_from_slice(&count.to_be_bytes());
        out.extend_from_slice(&search_range.to_be_bytes());
        out.extend_from_slice(&entry_selector.to_be_bytes());
        out.extend_from_slice(&range_shift.to_be_bytes());
        let mut offset = 12 + 16 * n;
        let mut head_at = None;
        let mut body = Vec::new();
        for (tag, data) in &self.tables {
            let mut data = data.clone();
            if tag == b"head" && data.len() >= 12 {
                data[8..12].fill(0);
                head_at = Some(offset + 8);
            }
            out.extend_from_slice(tag);
            out.extend_from_slice(&checksum(&data).to_be_bytes());
            out.extend_from_slice(&u32::try_from(offset).unwrap_or(0).to_be_bytes());
            out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(0).to_be_bytes());
            let padded = data.len().div_ceil(4) * 4;
            body.extend_from_slice(&data);
            body.resize(body.len() + (padded - data.len()), 0);
            offset += padded;
        }
        out.extend_from_slice(&body);
        if let Some(at) = head_at {
            let adj = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
            out[at..at + 4].copy_from_slice(&adj.to_be_bytes());
        }
        out
    }
}

/// The OpenType table checksum: the sum of big-endian `u32`s, the last one
/// padded with zeros.
pub(crate) fn checksum(data: &[u8]) -> u32 {
    let mut sum = 0u32;
    for chunk in data.chunks(4) {
        let mut w = [0u8; 4];
        w[..chunk.len()].copy_from_slice(chunk);
        sum = sum.wrapping_add(u32::from_be_bytes(w));
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_written_font_parses_back_to_the_same_tables() {
        let mut tables = BTreeMap::new();
        tables.insert(*b"head", vec![0u8; 54]);
        tables.insert(*b"abcd", vec![1, 2, 3]);
        let s = Sfnt {
            flavor: FLAVOR_TRUETYPE,
            tables,
        };
        let bytes = s.to_bytes();
        assert_eq!(bytes.len() % 4, 0);
        let back = Sfnt::parse(&bytes, 0).unwrap();
        assert_eq!(back.table(b"abcd"), Some(&[1u8, 2, 3][..]));
        // The whole file sums to the magic number once the adjustment is in.
        assert_eq!(checksum(&bytes), 0xB1B0_AFBA);
    }

    #[test]
    fn truncated_input_is_refused() {
        assert!(Sfnt::parse(&[0, 1, 0, 0, 0, 5], 0).is_none());
        assert!(Sfnt::parse(b"ttcf", 0).is_none());
        assert!(Sfnt::parse(&[], 0).is_none());
    }
}
