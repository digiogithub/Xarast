//! A `cmap` table for a web-font subset: the characters the document draws
//! mapped to their glyphs in the subset.
//!
//! Written from the OpenType specification: a Windows Unicode BMP subtable
//! (platform 3, encoding 1, format 4) for the characters below U+10000,
//! and, when any character lies above, a full-repertoire subtable
//! (platform 3, encoding 10, format 12) as well.

/// Builds the table. `map` holds `(character, glyph)` pairs sorted by
/// character, with no duplicates.
pub(crate) fn build(map: &[(u32, u16)]) -> Vec<u8> {
    let bmp: Vec<(u32, u16)> = map.iter().copied().filter(|(c, _)| *c < 0xFFFF).collect();
    let wide = map.iter().any(|(c, _)| *c > 0xFFFF);
    let f4 = format4(&bmp);
    let f12 = wide.then(|| format12(map));

    let n: u16 = if f12.is_some() { 2 } else { 1 };
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&n.to_be_bytes());
    let mut offset = 4 + 8 * u32::from(n);
    // Encoding records, sorted by platform then encoding.
    out.extend_from_slice(&3u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&offset.to_be_bytes());
    offset += u32::try_from(f4.len()).unwrap_or(0);
    if f12.is_some() {
        out.extend_from_slice(&3u16.to_be_bytes());
        out.extend_from_slice(&10u16.to_be_bytes());
        out.extend_from_slice(&offset.to_be_bytes());
    }
    out.extend_from_slice(&f4);
    if let Some(f) = f12 {
        out.extend_from_slice(&f);
    }
    out
}

/// Runs of consecutive characters mapped to consecutive glyphs.
fn runs(map: &[(u32, u16)]) -> Vec<(u32, u32, u16)> {
    let mut out: Vec<(u32, u32, u16)> = Vec::new();
    for &(c, g) in map {
        match out.last_mut() {
            Some((start, end, g0))
                if c == *end + 1 && u32::from(*g0) + (c - *start) == u32::from(g) =>
            {
                *end = c;
            }
            _ => out.push((c, c, g)),
        }
    }
    out
}

/// Format 4: one segment per run, `idDelta` only, then the required final
/// segment for U+FFFF. A table too long for its 16-bit length keeps only
/// the segments that fit (the format 12 subtable, when present, still has
/// them all).
fn format4(map: &[(u32, u16)]) -> Vec<u8> {
    let mut segs = runs(map);
    // 16 header bytes, 8 per segment including the final one.
    let max = (usize::from(u16::MAX) - 16) / 8 - 1;
    segs.truncate(max);
    // The terminator maps U+FFFF to glyph 0 (a delta of 1).
    segs.push((0xFFFF, 0xFFFF, 0));
    let seg_count = u16::try_from(segs.len()).unwrap_or(u16::MAX);
    let mut entry_selector = 0u16;
    while (2u32 << entry_selector) <= u32::from(seg_count) {
        entry_selector += 1;
    }
    let search_range = 2 * (1u16 << entry_selector);
    let seg_x2 = seg_count.saturating_mul(2);
    let len = 16 + 8 * segs.len();
    let mut out = Vec::with_capacity(len);
    let push = |out: &mut Vec<u8>, v: u16| out.extend_from_slice(&v.to_be_bytes());
    push(&mut out, 4);
    push(&mut out, u16::try_from(len).unwrap_or(u16::MAX));
    push(&mut out, 0);
    push(&mut out, seg_x2);
    push(&mut out, search_range);
    push(&mut out, entry_selector);
    push(&mut out, seg_x2.saturating_sub(search_range));
    for (_, end, _) in &segs {
        push(&mut out, *end as u16);
    }
    push(&mut out, 0);
    for (start, _, _) in &segs {
        push(&mut out, *start as u16);
    }
    for (start, _, g) in &segs {
        // Glyph = character + delta, modulo 65536.
        push(&mut out, g.wrapping_sub(*start as u16));
    }
    for _ in &segs {
        push(&mut out, 0);
    }
    out
}

/// Format 12: sequential map groups.
fn format12(map: &[(u32, u16)]) -> Vec<u8> {
    let groups = runs(map);
    let len = 16 + 12 * groups.len();
    let mut out = Vec::with_capacity(len);
    out.extend_from_slice(&12u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&u32::try_from(len).unwrap_or(0).to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&u32::try_from(groups.len()).unwrap_or(0).to_be_bytes());
    for (start, end, g) in groups {
        out.extend_from_slice(&start.to_be_bytes());
        out.extend_from_slice(&end.to_be_bytes());
        out.extend_from_slice(&u32::from(g).to_be_bytes());
    }
    out
}
