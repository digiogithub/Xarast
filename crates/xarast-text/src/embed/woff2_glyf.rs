//! The WOFF2 table transforms (W3C "WOFF File Format 2.0" §5.1–§5.4),
//! written from the specification: the `glyf`/`loca` transform both ways,
//! and the reconstruction of a transformed `hmtx`.
//!
//! The transformed `glyf` splits the glyphs into streams — contour counts,
//! point counts, flags, packed coordinate triplets, composite records,
//! explicit bounding boxes, instructions — that Brotli compresses better
//! than the table itself. Reconstruction rebuilds an equivalent table
//! (the same outlines, instructions and bounding boxes; its own flag
//! packing) and `loca` from it. Everything is bounds-checked: the input
//! is another program's file.

/// The reconstructed tables grow at most to this size.
const MAX_TABLE: usize = 64 << 20;
/// A simple glyph holds at most this many points.
const MAX_POINTS: usize = 0xFFFF;

// Simple-glyph flags (OpenType `glyf`).
const ON_CURVE: u8 = 0x01;
const X_SHORT: u8 = 0x02;
const Y_SHORT: u8 = 0x04;
const REPEAT: u8 = 0x08;
const X_SAME_OR_POSITIVE: u8 = 0x10;
const Y_SAME_OR_POSITIVE: u8 = 0x20;
const OVERLAP_SIMPLE: u8 = 0x40;

// Composite-glyph flags.
const ARG_1_AND_2_ARE_WORDS: u16 = 0x0001;
const WE_HAVE_A_SCALE: u16 = 0x0008;
const MORE_COMPONENTS: u16 = 0x0020;
const WE_HAVE_AN_X_AND_Y_SCALE: u16 = 0x0040;
const WE_HAVE_A_TWO_BY_TWO: u16 = 0x0080;
const WE_HAVE_INSTRUCTIONS: u16 = 0x0100;

/// The fixed part of a transformed `glyf`: four `u16` and seven `u32`.
const HEADER_LEN: usize = 36;

/// A bounds-checked big-endian reader.
struct Cursor<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn new(b: &'a [u8]) -> Cursor<'a> {
        Cursor { b, at: 0 }
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }

    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    fn u16(&mut self) -> Option<u16> {
        let s = self.take(2)?;
        Some(u16::from_be_bytes([s[0], s[1]]))
    }

    fn i16(&mut self) -> Option<i16> {
        let s = self.take(2)?;
        Some(i16::from_be_bytes([s[0], s[1]]))
    }

    fn u32(&mut self) -> Option<u32> {
        let s = self.take(4)?;
        Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }

    /// `255UInt16` (§4.1).
    fn u255(&mut self) -> Option<u16> {
        match self.u8()? {
            253 => self.u16(),
            254 => Some(u16::from(self.u8()?) + 506),
            255 => Some(u16::from(self.u8()?) + 253),
            c => Some(u16::from(c)),
        }
    }
}

/// Writes a `255UInt16` in its shortest form.
fn push_u255(out: &mut Vec<u8>, v: u16) {
    match v {
        0..253 => out.push(v as u8),
        253..506 => out.extend_from_slice(&[255, (v - 253) as u8]),
        506..762 => out.extend_from_slice(&[254, (v - 506) as u8]),
        _ => {
            out.push(253);
            out.extend_from_slice(&v.to_be_bytes());
        }
    }
}

/// `v` with the sign a triplet flag bit gives it: set is positive.
fn signed(bit: u8, v: i32) -> i32 {
    if bit & 1 == 1 { v } else { -v }
}

/// Decodes one point's triplet (§5.2): the flag's low seven bits say how
/// many bytes of `glyph` follow and how they split into `dx`, `dy`.
fn read_triplet(flag: u8, glyph: &mut Cursor<'_>) -> Option<(i32, i32)> {
    let f = flag & 0x7F;
    Some(if f < 10 {
        let b = i32::from(glyph.u8()?);
        (0, signed(f, (i32::from(f >> 1) << 8) + b))
    } else if f < 20 {
        let b = i32::from(glyph.u8()?);
        (signed(f, (i32::from((f - 10) >> 1) << 8) + b), 0)
    } else if f < 84 {
        let k = i32::from(f - 20);
        let b = i32::from(glyph.u8()?);
        (
            signed(f, 1 + (k & 0x30) + (b >> 4)),
            signed(f >> 1, 1 + ((k & 0x0C) << 2) + (b & 0x0F)),
        )
    } else if f < 120 {
        let k = i32::from(f - 84);
        let s = glyph.take(2)?;
        (
            signed(f, 1 + ((k / 12) << 8) + i32::from(s[0])),
            signed(f >> 1, 1 + (((k % 12) >> 2) << 8) + i32::from(s[1])),
        )
    } else if f < 124 {
        let s = glyph.take(3)?;
        let (a, b, c) = (i32::from(s[0]), i32::from(s[1]), i32::from(s[2]));
        (
            signed(f, (a << 4) + (b >> 4)),
            signed(f >> 1, ((b & 0x0F) << 8) + c),
        )
    } else {
        let s = glyph.take(4)?;
        let x = (i32::from(s[0]) << 8) + i32::from(s[1]);
        let y = (i32::from(s[2]) << 8) + i32::from(s[3]);
        (signed(f, x), signed(f >> 1, y))
    })
}

/// Encodes one point's move in the shortest triplet: pushes the flag
/// (bit 7 set for an off-curve point) and the data bytes.
fn push_triplet(flags: &mut Vec<u8>, glyph: &mut Vec<u8>, dx: i32, dy: i32, on: bool) {
    let (ax, ay) = (dx.unsigned_abs(), dy.unsigned_abs());
    let xs = u8::from(dx >= 0);
    let ys = u8::from(dy >= 0);
    let off = if on { 0 } else { 0x80 };
    // The casts below take values the branch conditions bound.
    if dx == 0 && ay < 1280 {
        flags.push(off | (((ay >> 8) as u8) << 1) | ys);
        glyph.push((ay & 0xFF) as u8);
    } else if dy == 0 && ax < 1280 {
        flags.push(off | (10 + (((ax >> 8) as u8) << 1) + xs));
        glyph.push((ax & 0xFF) as u8);
    } else if (1..65).contains(&ax) && (1..65).contains(&ay) {
        let (x, y) = (ax - 1, ay - 1);
        flags.push(off | (20 + (x & 0x30) as u8 + ((y & 0x30) >> 2) as u8 + xs + 2 * ys));
        glyph.push((((x & 0x0F) << 4) | (y & 0x0F)) as u8);
    } else if (1..769).contains(&ax) && (1..769).contains(&ay) {
        let (x, y) = (ax - 1, ay - 1);
        flags.push(off | (84 + 12 * (x >> 8) as u8 + (((y >> 8) as u8) << 2) + xs + 2 * ys));
        glyph.extend_from_slice(&[(x & 0xFF) as u8, (y & 0xFF) as u8]);
    } else if ax < 4096 && ay < 4096 {
        flags.push(off | (120 + xs + 2 * ys));
        glyph.extend_from_slice(&[
            (ax >> 4) as u8,
            (((ax & 0x0F) << 4) | (ay >> 8)) as u8,
            (ay & 0xFF) as u8,
        ]);
    } else {
        flags.push(off | (124 + xs + 2 * ys));
        let (x, y) = (ax.min(0xFFFF) as u16, ay.min(0xFFFF) as u16);
        glyph.extend_from_slice(&x.to_be_bytes());
        glyph.extend_from_slice(&y.to_be_bytes());
    }
}

/// A simple glyph, decoded.
struct Simple {
    end_points: Vec<u16>,
    instructions: Vec<u8>,
    /// Absolute coordinates and on-curve flags.
    points: Vec<(i32, i32, bool)>,
    overlap: bool,
}

fn bbox_of(points: &[(i32, i32, bool)]) -> [i32; 4] {
    let mut b = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
    for &(x, y, _) in points {
        b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
    }
    if points.is_empty() { [0; 4] } else { b }
}

fn i16_of(v: i32) -> Option<i16> {
    i16::try_from(v).ok()
}

/// `v + d` as a glyph coordinate: `None` outside the `i16` range every
/// coordinate of a glyph (and its box) must fit.
fn coord(v: i32, d: i32) -> Option<i32> {
    let n = v.checked_add(d)?;
    i16_of(n).map(i32::from)
}

impl Simple {
    /// Parses an OpenType simple glyph whose contour count is `n`.
    fn parse(g: &[u8], n: usize) -> Option<Simple> {
        let mut c = Cursor::new(g);
        c.take(10)?;
        let mut end_points = Vec::with_capacity(n);
        for _ in 0..n {
            end_points.push(c.u16()?);
        }
        let count = end_points.last().map_or(0, |e| usize::from(*e) + 1);
        let ilen = usize::from(c.u16()?);
        let instructions = c.take(ilen)?.to_vec();
        let mut flags = Vec::with_capacity(count);
        while flags.len() < count {
            let f = c.u8()?;
            flags.push(f);
            if f & REPEAT != 0 {
                for _ in 0..c.u8()? {
                    flags.push(f);
                }
            }
        }
        flags.truncate(count);
        let mut xs = Vec::with_capacity(count);
        let mut v = 0i32;
        for &f in &flags {
            v = coord(
                v,
                match (f & X_SHORT != 0, f & X_SAME_OR_POSITIVE != 0) {
                    (true, true) => i32::from(c.u8()?),
                    (true, false) => -i32::from(c.u8()?),
                    (false, true) => 0,
                    (false, false) => i32::from(c.i16()?),
                },
            )?;
            xs.push(v);
        }
        let mut points = Vec::with_capacity(count);
        let mut v = 0i32;
        for (&f, x) in flags.iter().zip(xs) {
            v = coord(
                v,
                match (f & Y_SHORT != 0, f & Y_SAME_OR_POSITIVE != 0) {
                    (true, true) => i32::from(c.u8()?),
                    (true, false) => -i32::from(c.u8()?),
                    (false, true) => 0,
                    (false, false) => i32::from(c.i16()?),
                },
            )?;
            points.push((x, v, f & ON_CURVE != 0));
        }
        Some(Simple {
            end_points,
            instructions,
            points,
            overlap: flags.first().is_some_and(|f| f & OVERLAP_SIMPLE != 0),
        })
    }

    /// Writes the glyph as OpenType `glyf` data with bounding box `bbox`.
    fn write(&self, bbox: [i32; 4], out: &mut Vec<u8>) -> Option<()> {
        out.extend_from_slice(&u16::try_from(self.end_points.len()).ok()?.to_be_bytes());
        for v in bbox {
            out.extend_from_slice(&i16_of(v)?.to_be_bytes());
        }
        for e in &self.end_points {
            out.extend_from_slice(&e.to_be_bytes());
        }
        out.extend_from_slice(&u16::try_from(self.instructions.len()).ok()?.to_be_bytes());
        out.extend_from_slice(&self.instructions);
        let mut flags = Vec::with_capacity(self.points.len());
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        let (mut px, mut py) = (0i32, 0i32);
        for (i, &(x, y, on)) in self.points.iter().enumerate() {
            let (dx, dy) = (x - px, y - py);
            (px, py) = (x, y);
            let mut f = if on { ON_CURVE } else { 0 };
            if i == 0 && self.overlap {
                f |= OVERLAP_SIMPLE;
            }
            if dx == 0 {
                f |= X_SAME_OR_POSITIVE;
            } else if dx.unsigned_abs() < 256 {
                f |= X_SHORT | if dx > 0 { X_SAME_OR_POSITIVE } else { 0 };
                xs.push(dx.unsigned_abs() as u8);
            } else {
                xs.extend_from_slice(&i16_of(dx)?.to_be_bytes());
            }
            if dy == 0 {
                f |= Y_SAME_OR_POSITIVE;
            } else if dy.unsigned_abs() < 256 {
                f |= Y_SHORT | if dy > 0 { Y_SAME_OR_POSITIVE } else { 0 };
                ys.push(dy.unsigned_abs() as u8);
            } else {
                ys.extend_from_slice(&i16_of(dy)?.to_be_bytes());
            }
            flags.push(f);
        }
        // Runs of equal flags use the repeat count.
        let mut i = 0;
        while i < flags.len() {
            let f = flags[i];
            let mut run = 1;
            while run < 256 && flags.get(i + run) == Some(&f) {
                run += 1;
            }
            if run > 1 {
                out.push(f | REPEAT);
                out.push((run - 1) as u8);
            } else {
                out.push(f);
            }
            i += run;
        }
        out.extend_from_slice(&xs);
        out.extend_from_slice(&ys);
        Some(())
    }
}

/// The length of a composite glyph's component records starting at the
/// start of `c`, and whether any asks for instructions.
fn composite_len(b: &[u8]) -> Option<(usize, bool)> {
    let mut c = Cursor::new(b);
    let mut instructions = false;
    loop {
        let flags = c.u16()?;
        c.take(2)?;
        c.take(if flags & ARG_1_AND_2_ARE_WORDS != 0 {
            4
        } else {
            2
        })?;
        if flags & WE_HAVE_A_SCALE != 0 {
            c.take(2)?;
        } else if flags & WE_HAVE_AN_X_AND_Y_SCALE != 0 {
            c.take(4)?;
        } else if flags & WE_HAVE_A_TWO_BY_TWO != 0 {
            c.take(8)?;
        }
        instructions |= flags & WE_HAVE_INSTRUCTIONS != 0;
        if flags & MORE_COMPONENTS == 0 {
            return Some((c.at, instructions));
        }
    }
}

/// The glyphs of an OpenType `glyf` as `loca` slices them.
fn glyphs<'a>(glyf: &'a [u8], loca: &[u8], long: bool) -> Option<Vec<&'a [u8]>> {
    let step = if long { 4 } else { 2 };
    let n = (loca.len() / step).checked_sub(1)?;
    let at = |i: usize| -> Option<usize> {
        let s = loca.get(i * step..i * step + step)?;
        Some(if long {
            u32::from_be_bytes([s[0], s[1], s[2], s[3]]) as usize
        } else {
            usize::from(u16::from_be_bytes([s[0], s[1]])) * 2
        })
    };
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (a, b) = (at(i)?, at(i + 1)?);
        out.push(glyf.get(a..b.max(a))?);
    }
    Some(out)
}

/// Applies the `glyf` transform (§5.1) to a font's `glyf` and `loca`.
/// `index_format` is `head.indexToLocFormat`. `None` when the tables do
/// not parse; the caller then stores them untransformed.
pub(crate) fn transform(glyf: &[u8], loca: &[u8], index_format: u16) -> Option<Vec<u8>> {
    let long = match index_format {
        0 => false,
        1 => true,
        _ => return None,
    };
    let list = glyphs(glyf, loca, long)?;
    let num = u16::try_from(list.len()).ok()?;
    let mut n_contour = Vec::with_capacity(list.len() * 2);
    let mut n_points = Vec::new();
    let mut flag_s = Vec::new();
    let mut glyph_s = Vec::new();
    let mut composite_s = Vec::new();
    let mut bbox_bits = vec![0u8; list.len().div_ceil(32) * 4];
    let mut bbox_s = Vec::new();
    let mut instr_s = Vec::new();
    let mut overlap = vec![0u8; list.len().div_ceil(8)];
    let mut any_overlap = false;
    for (i, g) in list.iter().enumerate() {
        let mut c = Cursor::new(g);
        let n = if g.is_empty() { 0 } else { c.i16()? };
        if n == 0 {
            // No outline: nothing but the count is stored.
            n_contour.extend_from_slice(&0i16.to_be_bytes());
            continue;
        }
        let stored = [
            i32::from(c.i16()?),
            i32::from(c.i16()?),
            i32::from(c.i16()?),
            i32::from(c.i16()?),
        ];
        n_contour.extend_from_slice(&n.to_be_bytes());
        let explicit = if n >= 0 {
            let s = Simple::parse(g, usize::try_from(n).ok()?)?;
            let mut prev = 0u16;
            for (k, e) in s.end_points.iter().enumerate() {
                let count = if k == 0 {
                    e.checked_add(1)?
                } else {
                    e.checked_sub(prev)?
                };
                push_u255(&mut n_points, count);
                prev = *e;
            }
            let (mut px, mut py) = (0i32, 0i32);
            for &(x, y, on) in &s.points {
                push_triplet(&mut flag_s, &mut glyph_s, x - px, y - py, on);
                (px, py) = (x, y);
            }
            push_u255(&mut glyph_s, u16::try_from(s.instructions.len()).ok()?);
            instr_s.extend_from_slice(&s.instructions);
            if s.overlap {
                overlap[i / 8] |= 0x80 >> (i % 8);
                any_overlap = true;
            }
            // The box is stored only when it is not the points' own.
            bbox_of(&s.points) != stored
        } else if n == -1 {
            let rest = g.get(10..)?;
            let (len, has_instr) = composite_len(rest)?;
            composite_s.extend_from_slice(rest.get(..len)?);
            if has_instr {
                let mut c = Cursor::new(rest.get(len..)?);
                let ilen = usize::from(c.u16()?);
                let ins = c.take(ilen)?;
                push_u255(&mut glyph_s, u16::try_from(ilen).ok()?);
                instr_s.extend_from_slice(ins);
            }
            true
        } else {
            return None;
        };
        if explicit {
            bbox_bits[i / 8] |= 0x80 >> (i % 8);
            for v in stored {
                bbox_s.extend_from_slice(&i16_of(v)?.to_be_bytes());
            }
        }
    }
    let mut bbox_stream = bbox_bits;
    bbox_stream.extend_from_slice(&bbox_s);
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&u16::from(any_overlap).to_be_bytes());
    out.extend_from_slice(&num.to_be_bytes());
    out.extend_from_slice(&index_format.to_be_bytes());
    for s in [
        &n_contour,
        &n_points,
        &flag_s,
        &glyph_s,
        &composite_s,
        &bbox_stream,
        &instr_s,
    ] {
        out.extend_from_slice(&u32::try_from(s.len()).ok()?.to_be_bytes());
    }
    for s in [
        n_contour,
        n_points,
        flag_s,
        glyph_s,
        composite_s,
        bbox_stream,
        instr_s,
    ] {
        out.extend_from_slice(&s);
    }
    if any_overlap {
        out.extend_from_slice(&overlap);
    }
    Some(out)
}

/// A reconstructed `glyf` and `loca`, and each glyph's `xMin` (0 for an
/// empty glyph) for the `hmtx` reconstruction.
pub(crate) struct Reconstructed {
    pub glyf: Vec<u8>,
    pub loca: Vec<u8>,
    pub x_mins: Vec<i16>,
}

/// Rebuilds `glyf` and `loca` from a transformed `glyf` (§5.1–§5.3).
/// `None` for anything malformed.
pub(crate) fn reconstruct(t: &[u8]) -> Option<Reconstructed> {
    let mut h = Cursor::new(t);
    h.u16()?;
    let options = h.u16()?;
    let num = usize::from(h.u16()?);
    let index_format = h.u16()?;
    let long = match index_format {
        0 => false,
        1 => true,
        _ => return None,
    };
    let mut sizes = [0usize; 7];
    for s in &mut sizes {
        *s = h.u32()? as usize;
    }
    let mut at = HEADER_LEN;
    let mut stream = |len: usize| -> Option<Cursor<'_>> {
        let s = t.get(at..at.checked_add(len)?)?;
        at += len;
        Some(Cursor::new(s))
    };
    let mut n_contour = stream(sizes[0])?;
    let mut n_points = stream(sizes[1])?;
    let mut flags = stream(sizes[2])?;
    let mut glyph = stream(sizes[3])?;
    let mut composite = stream(sizes[4])?;
    let mut bbox = stream(sizes[5])?;
    let mut instr = stream(sizes[6])?;
    let overlap = if options & 1 != 0 {
        Some(stream(num.div_ceil(8))?.b)
    } else {
        None
    };
    let bbox_bits = bbox.take(num.div_ceil(32) * 4)?;
    let bit = |bits: &[u8], i: usize| bits.get(i / 8).is_some_and(|b| b & (0x80 >> (i % 8)) != 0);

    let mut glyf = Vec::new();
    let mut offsets = Vec::with_capacity(num + 1);
    let mut x_mins = Vec::with_capacity(num);
    for i in 0..num {
        offsets.push(glyf.len());
        let n = n_contour.i16()?;
        let explicit = bit(bbox_bits, i);
        if n == 0 {
            if explicit {
                return None;
            }
            x_mins.push(0);
        } else if n > 0 {
            let n = usize::try_from(n).ok()?;
            let mut end_points = Vec::with_capacity(n);
            let mut total = 0usize;
            for _ in 0..n {
                total = total.checked_add(usize::from(n_points.u255()?))?;
                if total == 0 || total > MAX_POINTS {
                    return None;
                }
                end_points.push(u16::try_from(total - 1).ok()?);
            }
            let mut points = Vec::with_capacity(total);
            let (mut x, mut y) = (0i32, 0i32);
            for _ in 0..total {
                let f = flags.u8()?;
                let (dx, dy) = read_triplet(f, &mut glyph)?;
                x = coord(x, dx)?;
                y = coord(y, dy)?;
                points.push((x, y, f & 0x80 == 0));
            }
            let ilen = usize::from(glyph.u255()?);
            let instructions = instr.take(ilen)?.to_vec();
            let b = if explicit {
                [
                    i32::from(bbox.i16()?),
                    i32::from(bbox.i16()?),
                    i32::from(bbox.i16()?),
                    i32::from(bbox.i16()?),
                ]
            } else {
                bbox_of(&points)
            };
            x_mins.push(i16_of(b[0])?);
            Simple {
                end_points,
                instructions,
                points,
                overlap: overlap.is_some_and(|o| bit(o, i)),
            }
            .write(b, &mut glyf)?;
        } else if n == -1 {
            if !explicit {
                return None;
            }
            let b = [bbox.i16()?, bbox.i16()?, bbox.i16()?, bbox.i16()?];
            x_mins.push(b[0]);
            let rest = composite.b.get(composite.at..)?;
            let (len, has_instr) = composite_len(rest)?;
            let records = composite.take(len)?;
            glyf.extend_from_slice(&(-1i16).to_be_bytes());
            for v in b {
                glyf.extend_from_slice(&v.to_be_bytes());
            }
            glyf.extend_from_slice(records);
            if has_instr {
                let ilen = glyph.u255()?;
                glyf.extend_from_slice(&ilen.to_be_bytes());
                glyf.extend_from_slice(instr.take(usize::from(ilen))?);
            }
        } else {
            return None;
        }
        // Padding keeps every offset expressible in the `loca` format.
        let pad = if long { 4 } else { 2 };
        glyf.resize(glyf.len().div_ceil(pad) * pad, 0);
        if glyf.len() > MAX_TABLE {
            return None;
        }
    }
    offsets.push(glyf.len());
    let mut loca = Vec::with_capacity(offsets.len() * if long { 4 } else { 2 });
    for o in offsets {
        if long {
            loca.extend_from_slice(&u32::try_from(o).ok()?.to_be_bytes());
        } else {
            loca.extend_from_slice(&u16::try_from(o / 2).ok()?.to_be_bytes());
        }
    }
    Some(Reconstructed { glyf, loca, x_mins })
}

/// Rebuilds `hmtx` from its transformed form (§5.4): advances, then the
/// side bearings the flags did not drop; a dropped one is the glyph's
/// `xMin`. `hmetrics` is `hhea.numberOfHMetrics`.
pub(crate) fn reconstruct_hmtx(t: &[u8], hmetrics: usize, x_mins: &[i16]) -> Option<Vec<u8>> {
    let num = x_mins.len();
    if hmetrics == 0 || hmetrics > num {
        return None;
    }
    let mut c = Cursor::new(t);
    let flags = c.u8()?;
    if flags & 0xFC != 0 || flags & 0x03 == 0 {
        return None;
    }
    let mut advances = Vec::with_capacity(hmetrics);
    for _ in 0..hmetrics {
        advances.push(c.u16()?);
    }
    // Proportional bearings (bit 0 drops them), then monospaced ones
    // (bit 1 drops them).
    let mut lsb = Vec::with_capacity(num);
    for (i, x_min) in x_mins.iter().enumerate() {
        let bit = if i < hmetrics { 1 } else { 2 };
        lsb.push(if flags & bit != 0 { *x_min } else { c.i16()? });
    }
    let mut out = Vec::with_capacity(hmetrics * 4 + (num - hmetrics) * 2);
    for (i, l) in lsb.iter().enumerate() {
        if let Some(a) = advances.get(i) {
            out.extend_from_slice(&a.to_be_bytes());
        }
        out.extend_from_slice(&l.to_be_bytes());
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u255_round_trips_at_every_boundary() {
        for v in [0u16, 252, 253, 505, 506, 761, 762, 65_535] {
            let mut b = Vec::new();
            push_u255(&mut b, v);
            let mut c = Cursor::new(&b);
            assert_eq!(c.u255(), Some(v), "{v}");
            assert_eq!(c.at, b.len());
        }
    }

    #[test]
    fn every_triplet_class_round_trips() {
        let moves = [
            (0, 0),
            (0, 5),
            (0, -1279),
            (7, 0),
            (-1279, 0),
            (1, 1),
            (-64, 64),
            (65, -2),
            (768, -768),
            (-769, 3),
            (4095, -4095),
            (4096, 1),
            (-32_768, 32_767),
        ];
        for (dx, dy) in moves {
            for on in [true, false] {
                let (mut f, mut g) = (Vec::new(), Vec::new());
                push_triplet(&mut f, &mut g, dx, dy, on);
                assert_eq!(f.len(), 1);
                assert_eq!(f[0] & 0x80 == 0, on);
                let mut c = Cursor::new(&g);
                assert_eq!(read_triplet(f[0], &mut c), Some((dx, dy)), "{dx},{dy}");
                assert_eq!(c.at, g.len());
            }
        }
    }

    #[test]
    fn a_corrupt_transformed_glyf_never_panics() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts");
        let bytes = std::fs::read(dir.join("NotoSans-Regular.subset.ttf")).unwrap();
        let s = super::super::sfnt::Sfnt::parse(&bytes, 0).unwrap();
        let t = transform(
            s.table(b"glyf").unwrap(),
            s.table(b"loca").unwrap(),
            u16::from_be_bytes([s.table(b"head").unwrap()[50], s.table(b"head").unwrap()[51]]),
        )
        .unwrap();
        assert!(reconstruct(&t).is_some());
        for i in 0..t.len() {
            for flip in [0x01u8, 0x80, 0xFF] {
                let mut bad = t.clone();
                bad[i] ^= flip;
                let _ = reconstruct(&bad);
            }
        }
        for n in 0..t.len() {
            let _ = reconstruct(&t[..n]);
        }
    }

    #[test]
    fn a_transformed_hmtx_takes_the_dropped_bearings_from_the_glyphs() {
        // Three glyphs, two long metrics; proportional bearings dropped,
        // the monospaced one explicit.
        let t = [1u8, 0, 10, 0, 20, 0xFF, 0xFE];
        let h = reconstruct_hmtx(&t, 2, &[5, -3, 9]).unwrap();
        assert_eq!(h, [0, 10, 0, 5, 0, 20, 0xFF, 0xFD, 0xFF, 0xFE]);
        assert!(reconstruct_hmtx(&[0], 2, &[0, 0]).is_none());
        assert!(reconstruct_hmtx(&t, 2, &[5, -3, 9, 1]).is_none());
    }
}
