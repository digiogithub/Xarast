//! The bounds-checked cursor every record payload is read through.
//!
//! Everything in `.xar` is little-endian. The single exception is the
//! *interleaved* coordinate pair of relative paths, which permutes the bytes
//! of X and Y within an eight-byte field so that their near-constant high
//! bytes end up adjacent and compress better — see
//! [`Cur::point_interleaved`]. That is a permutation, not a byte order.
//!
//! # Two habits this type exists to enforce
//!
//! **Never assume a declared size.** Records grew between versions:
//! `TAG_LINEARFILL` went from 24 to 40 bytes when bias and gain were added,
//! `TAG_FEATHER` from 4 to 20. A reader that demands the current size cannot
//! open a 2001 file, and one that demands the old size throws away the
//! profile. So the tolerant reads — [`Cur::opt_f64`] and friends — return
//! `None` at end of payload and the caller substitutes a documented default.
//!
//! **Points are translated, vectors are not.** Record coordinates are
//! relative to the spread's coordinate origin, but the major and minor axes
//! of a regular shape are vectors written untranslated
//! (`research/01 §5.3`). [`Cur::point`] takes the origin and [`Cur::vector`]
//! cannot, so the two cannot be confused at a call site.

use crate::error::XarError;
use xarast_color::Fixed24;
use xarast_geom::{Matrix, Mp, Point, Vector};

/// A bounds-checked little-endian cursor over one record payload.
#[derive(Clone, Debug)]
pub struct Cur<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cur<'a> {
    /// A cursor at the start of `bytes`.
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Cur<'a> {
        Cur { bytes, pos: 0 }
    }

    /// How many bytes are left.
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.pos)
    }

    /// The current offset into the payload.
    #[must_use]
    pub const fn position(&self) -> usize {
        self.pos
    }

    /// Whether every byte has been consumed.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    /// The bytes not yet read, without consuming them.
    #[must_use]
    pub fn peek_rest(&self) -> &'a [u8] {
        self.bytes.get(self.pos..).unwrap_or(&[])
    }

    /// Consumes and returns the rest of the payload.
    pub fn rest(&mut self) -> &'a [u8] {
        let out = self.peek_rest();
        self.pos = self.bytes.len();
        out
    }

    /// Consumes exactly `n` bytes.
    ///
    /// # Errors
    ///
    /// [`XarError::ShortRecord`] when fewer than `n` bytes remain. Nothing is
    /// consumed in that case, so a caller can fall back to a default.
    pub fn take(&mut self, n: usize) -> Result<&'a [u8], XarError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or(XarError::ShortRecord(self.pos))?;
        let slice = self
            .bytes
            .get(self.pos..end)
            .ok_or(XarError::ShortRecord(self.pos))?;
        self.pos = end;
        Ok(slice)
    }

    /// Skips `n` bytes, or the rest of the payload if fewer remain.
    pub fn skip(&mut self, n: usize) {
        self.pos = self.pos.saturating_add(n).min(self.bytes.len());
    }

    /// Reads a `BYTE`.
    ///
    /// # Errors
    ///
    /// [`XarError::ShortRecord`] at end of payload.
    pub fn u8(&mut self) -> Result<u8, XarError> {
        let b = self.take(1)?;
        b.first().copied().ok_or(XarError::ShortRecord(self.pos))
    }

    /// Reads a `UINT16`.
    ///
    /// # Errors
    ///
    /// [`XarError::ShortRecord`] at end of payload.
    pub fn u16(&mut self) -> Result<u16, XarError> {
        let b: [u8; 2] = self
            .take(2)?
            .try_into()
            .map_err(|_| XarError::ShortRecord(self.pos))?;
        Ok(u16::from_le_bytes(b))
    }

    /// Reads an `INT16`.
    ///
    /// # Errors
    ///
    /// [`XarError::ShortRecord`] at end of payload.
    pub fn i16(&mut self) -> Result<i16, XarError> {
        Ok(self.u16()? as i16)
    }

    /// Reads a `UINT32`.
    ///
    /// # Errors
    ///
    /// [`XarError::ShortRecord`] at end of payload.
    pub fn u32(&mut self) -> Result<u32, XarError> {
        let b: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| XarError::ShortRecord(self.pos))?;
        Ok(u32::from_le_bytes(b))
    }

    /// Reads an `INT32`.
    ///
    /// # Errors
    ///
    /// [`XarError::ShortRecord`] at end of payload.
    pub fn i32(&mut self) -> Result<i32, XarError> {
        Ok(self.u32()? as i32)
    }

    /// Reads a `FLOAT` (IEEE-754 binary32).
    ///
    /// # Errors
    ///
    /// [`XarError::ShortRecord`] at end of payload.
    pub fn f32(&mut self) -> Result<f32, XarError> {
        Ok(f32::from_bits(self.u32()?))
    }

    /// Reads a `DOUBLE` (IEEE-754 binary64).
    ///
    /// # Errors
    ///
    /// [`XarError::ShortRecord`] at end of payload.
    pub fn f64(&mut self) -> Result<f64, XarError> {
        let b: [u8; 8] = self
            .take(8)?
            .try_into()
            .map_err(|_| XarError::ShortRecord(self.pos))?;
        Ok(f64::from_le_bytes(b))
    }

    /// Reads a `FIXED16`: an `i32` with 16 fractional bits.
    ///
    /// # Errors
    ///
    /// [`XarError::ShortRecord`] at end of payload.
    pub fn fixed16(&mut self) -> Result<f64, XarError> {
        Ok(f64::from(self.i32()?) / 65_536.0)
    }

    /// Reads an `ANGLE`: a [`Cur::fixed16`] in radians.
    ///
    /// Not every angle in the format is one of these. `TAG_SHADOWCONTROLLER`
    /// uses a bespoke integer encoding and `TAG_BEVEL` whole degrees
    /// (`research/01 §11` item 14), so check the record before reaching for
    /// this.
    ///
    /// # Errors
    ///
    /// [`XarError::ShortRecord`] at end of payload.
    pub fn angle(&mut self) -> Result<f64, XarError> {
        self.fixed16()
    }

    /// Reads a `FIXED24` colour component, sentinel and all.
    ///
    /// # Errors
    ///
    /// [`XarError::ShortRecord`] at end of payload.
    pub fn fixed24(&mut self) -> Result<Fixed24, XarError> {
        Ok(Fixed24(self.i32()?))
    }

    /// Reads a `REFERENCE`.
    ///
    /// # Errors
    ///
    /// [`XarError::ShortRecord`] at end of payload.
    pub fn reference(&mut self) -> Result<crate::Ref, XarError> {
        Ok(crate::Ref::parse(self.i32()?))
    }

    /// Reads a `MILLIPOINT` scalar, such as a line width.
    ///
    /// # Errors
    ///
    /// [`XarError::ShortRecord`] at end of payload.
    pub fn mp(&mut self) -> Result<Mp, XarError> {
        Ok(Mp::new(self.i32()?).clamp_to_extent().0)
    }

    /// Reads a **point**: the spread coordinate origin *is* added.
    ///
    /// # Errors
    ///
    /// [`XarError::ShortRecord`] at end of payload.
    pub fn point(&mut self, origin: Point) -> Result<Point, XarError> {
        let x = Mp::new(self.i32()?);
        let y = Mp::new(self.i32()?);
        Ok(translate(x, y, origin))
    }

    /// Reads a **vector**: the origin is *not* added.
    ///
    /// # Errors
    ///
    /// [`XarError::ShortRecord`] at end of payload.
    pub fn vector(&mut self) -> Result<Vector, XarError> {
        let dx = Mp::new(self.i32()?).clamp_to_extent().0;
        let dy = Mp::new(self.i32()?).clamp_to_extent().0;
        Ok(Vector::new(dx, dy))
    }

    /// Reads the eight interleaved bytes of a relative-path coordinate and
    /// returns the raw `(x, y)` pair, untranslated and unclamped.
    ///
    /// ```text
    /// b0 = X>>24  b1 = Y>>24  b2 = X>>16  b3 = Y>>16
    /// b4 = X>>8   b5 = Y>>8   b6 = X      b7 = Y
    /// ```
    ///
    /// # Errors
    ///
    /// [`XarError::ShortRecord`] at end of payload.
    pub fn point_interleaved(&mut self) -> Result<(i32, i32), XarError> {
        let b: [u8; 8] = self
            .take(8)?
            .try_into()
            .map_err(|_| XarError::ShortRecord(self.pos))?;
        let x = i32::from_be_bytes([b[0], b[2], b[4], b[6]]);
        let y = i32::from_be_bytes([b[1], b[3], b[5], b[7]]);
        Ok((x, y))
    }

    /// Reads a 24-byte `Matrix`: four `FIXED16` then two translation
    /// `INT32`s, which do carry the coordinate origin.
    ///
    /// # Errors
    ///
    /// [`XarError::ShortRecord`] at end of payload.
    pub fn matrix(&mut self, origin: Point) -> Result<Matrix, XarError> {
        let a = self.fixed16()?;
        let b = self.fixed16()?;
        let c = self.fixed16()?;
        let d = self.fixed16()?;
        let e = Mp::new(self.i32()?);
        let f = Mp::new(self.i32()?);
        let t = translate(e, f, origin);
        Ok(Matrix {
            a,
            b,
            c,
            d,
            e: t.x,
            f: t.y,
        })
    }

    /// Reads a NUL-terminated ASCII string. Invalid bytes become U+FFFD.
    ///
    /// # Errors
    ///
    /// [`XarError::ShortRecord`] when no terminator is found before the end
    /// of the payload.
    pub fn ascii_z(&mut self) -> Result<String, XarError> {
        let start = self.pos;
        let rest = self.peek_rest();
        let n = rest
            .iter()
            .position(|&c| c == 0)
            .ok_or(XarError::ShortRecord(start))?;
        let text = rest.get(..n).unwrap_or(&[]);
        let out = String::from_utf8_lossy(text).into_owned();
        self.skip(n.saturating_add(1));
        Ok(out)
    }

    /// Reads a NUL-terminated UTF-16LE string. Unpaired surrogates become
    /// U+FFFD.
    ///
    /// # Errors
    ///
    /// [`XarError::ShortRecord`] when no terminator is found before the end
    /// of the payload.
    pub fn utf16_z(&mut self) -> Result<String, XarError> {
        let start = self.pos;
        let mut units: Vec<u16> = Vec::new();
        loop {
            let u = self.u16().map_err(|_| XarError::ShortRecord(start))?;
            if u == 0 {
                break;
            }
            if units.len() >= MAX_STRING_UNITS {
                return Err(XarError::ShortRecord(start));
            }
            units.push(u);
        }
        Ok(String::from_utf16_lossy(&units))
    }

    /// Reads the rest of the payload as UTF-16LE with **no** terminator.
    ///
    /// `TAG_TEXT_STRING` (2201) is the only record in the format that stores
    /// a string this way, and so the only caller of this method. A NUL in the
    /// middle of it is a character, not a terminator; treating it as one
    /// desynchronises the cursor and yields garbage that is hard to trace
    /// back (`research/01 §11` item 16).
    pub fn utf16_rest(&mut self) -> String {
        let bytes = self.rest();
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .take(MAX_STRING_UNITS)
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        String::from_utf16_lossy(&units)
    }

    /// Reads an `INT32` if the bytes are there.
    ///
    /// This is the ergonomic path for a field a later version appended:
    /// `cur.opt_i32().unwrap_or(0)`.
    pub fn opt_i32(&mut self) -> Option<i32> {
        self.i32().ok()
    }

    /// Reads a `UINT32` if the bytes are there.
    pub fn opt_u32(&mut self) -> Option<u32> {
        self.u32().ok()
    }

    /// Reads a `DOUBLE` if the bytes are there.
    pub fn opt_f64(&mut self) -> Option<f64> {
        self.f64().ok()
    }

    /// Reads a `BYTE` if the bytes are there.
    pub fn opt_u8(&mut self) -> Option<u8> {
        self.u8().ok()
    }

    /// Reads a `(bias, gain)` profile pair, defaulting to `(0, 0)`.
    ///
    /// Every two-colour gradient grew this suffix in Xara X; the multi-stage
    /// variants never had it (`research/01 §8.3`).
    pub fn opt_profile(&mut self) -> (f64, f64) {
        let bias = self.opt_f64().unwrap_or(0.0);
        let gain = self.opt_f64().unwrap_or(0.0);
        (bias, gain)
    }
}

/// The cap on how many UTF-16 code units one string may hold.
///
/// Nothing in the format needs more, and without a cap a payload with no
/// terminator would be an allocation proportional to a declared length rather
/// than to bytes actually read.
const MAX_STRING_UNITS: usize = 1 << 20;

/// Adds the coordinate origin to a raw pair, clamping to the document extent.
fn translate(x: Mp, y: Mp, origin: Point) -> Point {
    let px = x.saturating_add(origin.x).clamp_to_extent().0;
    let py = y.saturating_add(origin.y).clamp_to_extent().0;
    Point::new(px, py)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitives_read_little_endian() {
        let mut c = Cur::new(&[0x01, 0x02, 0x03, 0x04]);
        assert_eq!(c.u32().unwrap(), 0x0403_0201);
        assert!(c.is_empty());
    }

    #[test]
    fn every_primitive_fails_cleanly_when_truncated() {
        for n in 0..8usize {
            let buf = vec![0u8; n];
            let mut c = Cur::new(&buf);
            let _ = c.u8();
            let mut c = Cur::new(&buf);
            let _ = c.u16();
            let mut c = Cur::new(&buf);
            let _ = c.u32();
            let mut c = Cur::new(&buf);
            let _ = c.f64();
            let mut c = Cur::new(&buf);
            let _ = c.point(Point::ORIGIN);
            let mut c = Cur::new(&buf);
            let _ = c.matrix(Point::ORIGIN);
            let mut c = Cur::new(&buf);
            let _ = c.point_interleaved();
            let mut c = Cur::new(&buf);
            let _ = c.utf16_z();
            let mut c = Cur::new(&buf);
            let _ = c.ascii_z();
        }
    }

    #[test]
    fn take_consumes_nothing_when_it_fails() {
        let mut c = Cur::new(&[1, 2, 3]);
        assert!(c.take(4).is_err());
        assert_eq!(c.remaining(), 3);
    }

    #[test]
    fn fixed16_has_sixteen_fractional_bits() {
        let bytes = 0x0001_8000i32.to_le_bytes();
        let mut c = Cur::new(&bytes);
        assert!((c.fixed16().unwrap() - 1.5).abs() < 1e-12);
    }

    #[test]
    fn interleaved_coordinate_matches_the_worked_example() {
        // research/01 §7.3, the first point of testfiles/OneLine.xar.
        let bytes = [0x00, 0x00, 0x01, 0x02, 0xB5, 0xBA, 0xE5, 0xD3];
        let mut c = Cur::new(&bytes);
        assert_eq!(c.point_interleaved().unwrap(), (112_101, 178_899));
    }

    #[test]
    fn a_point_is_translated_and_a_vector_is_not() {
        let origin = Point::raw(1_000, 2_000);
        let bytes = [10i32.to_le_bytes(), 20i32.to_le_bytes()].concat();
        let mut c = Cur::new(&bytes);
        assert_eq!(c.point(origin).unwrap(), Point::raw(1_010, 2_020));
        let mut c = Cur::new(&bytes);
        assert_eq!(c.vector().unwrap(), Vector::raw(10, 20));
    }

    #[test]
    fn ascii_z_needs_its_terminator() {
        let mut c = Cur::new(b"Xara X\0rest");
        assert_eq!(c.ascii_z().unwrap(), "Xara X");
        assert_eq!(c.remaining(), 4);
        let mut c = Cur::new(b"no terminator");
        assert!(c.ascii_z().is_err());
    }

    #[test]
    fn utf16_rest_treats_an_embedded_nul_as_a_character() {
        let mut units: Vec<u8> = Vec::new();
        for u in "ab".encode_utf16() {
            units.extend_from_slice(&u.to_le_bytes());
        }
        units.extend_from_slice(&0u16.to_le_bytes());
        units.extend_from_slice(&u16::from(b'c').to_le_bytes());
        let mut c = Cur::new(&units);
        assert_eq!(c.utf16_rest().chars().count(), 4);
    }

    #[test]
    fn tolerant_reads_stop_at_the_end_of_a_short_record() {
        let mut c = Cur::new(&[0u8; 4]);
        assert_eq!(c.opt_i32(), Some(0));
        assert_eq!(c.opt_i32(), None);
        assert_eq!(c.opt_profile(), (0.0, 0.0));
    }
}
