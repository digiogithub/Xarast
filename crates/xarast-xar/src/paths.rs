//! Path decoding: the relative interleaved format and the absolute one.
//!
//! # The two representations, and the trap in the first
//!
//! Xara X always exports paths **relative and interleaved**, so tags 115 and
//! 116 are 11 % of every record in the corpus and tags 100–103 occur zero
//! times. The absolute codec is still needed, because reformed regular
//! shapes, `TAG_MOULD_PATH` and `TAG_BLEND_PATH` embed absolute paths inside
//! their own payloads.
//!
//! The relative format has **no point counter**: `points = size / 9`, one
//! verb byte plus eight coordinate bytes each. If `size % 9 != 0` the record
//! is corrupt or from a variant nobody has seen, and the right answer is to
//! drop the path rather than guess (`research/01 §11` item 4).
//!
//! Point 0 holds an absolute coordinate. Every later point holds an
//! **inverted delta**:
//!
//! ```text
//! P[i] = P[i-1] - D
//! ```
//!
//! Not plus. This is the single easiest thing to get wrong in the whole
//! format, because the result is mirrored geometry that looks completely
//! plausible on a symmetric shape and only shows up on an asymmetric one.
//! The worked example from `testfiles/OneLine.xar` is a unit test below for
//! exactly that reason.
//!
//! # Verbs are a bitfield
//!
//! `verb & 0x06` is the type — 2 line, 4 cubic, 6 move — and `verb & 0x01`
//! says this point **closes** its subpath. Zero is not a valid verb. Cubics
//! occupy three consecutive points: two controls then the endpoint.
//!
//! [`xarast_geom::Path`] spells closure as a `Close` verb that consumes no
//! point, so the two representations agree on `points.len()`, which is what
//! lets `TAG_PATH_FLAGS` — one byte per point — map straight across.

use crate::cur::Cur;
use crate::diag::{DiagCode, DiagSink, Diagnostic};
use crate::error::XarError;
use xarast_geom::{Mp, Path, PathError, Point, PointFlags, Verb};

/// `PT_CLOSEFIGURE`: this point closes its subpath.
const PT_CLOSEFIGURE: u8 = 0x01;
/// `PT_PATHELEMENT`: the mask that extracts the verb type.
const PT_PATHELEMENT: u8 = 0x06;
/// `PT_LINETO`.
const PT_LINETO: u8 = 0x02;
/// `PT_BEZIERTO`.
const PT_BEZIERTO: u8 = 0x04;
/// `PT_MOVETO`, which is `PT_LINETO | PT_BEZIERTO`.
const PT_MOVETO: u8 = 0x06;

/// Nine bytes per point: one verb and eight interleaved coordinate bytes.
const RELATIVE_STRIDE: usize = 9;

/// What a path tag says should happen to the path.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct PathStyleBits {
    /// The path is filled with the inherited fill attribute.
    pub filled: bool,
    /// The path is stroked with the inherited line attributes.
    pub stroked: bool,
    /// The coordinates were stored relative and interleaved.
    pub relative: bool,
}

impl PathStyleBits {
    /// Decodes tags 100–103 and 113–116.
    ///
    /// The fill and stroke bits do **not** replace the attributes: they say
    /// whether the inherited ones apply.
    #[must_use]
    pub const fn from_tag(tag: u32) -> Option<PathStyleBits> {
        let (filled, stroked, relative) = match tag {
            100 => (false, false, false),
            101 => (true, false, false),
            102 => (false, true, false),
            103 => (true, true, false),
            113 => (false, false, true),
            114 => (true, false, true),
            115 => (false, true, true),
            116 => (true, true, true),
            _ => return None,
        };
        Some(PathStyleBits {
            filled,
            stroked,
            relative,
        })
    }
}

/// Decodes a relative interleaved path from the rest of the payload.
///
/// Never fails on a malformed payload: a bad size or a bad verb produces a
/// diagnostic and a shorter — possibly empty — path.
///
/// # Errors
///
/// [`XarError::ShortRecord`] only if the payload ends mid-point, which the
/// `% 9` check already rules out for a well-formed record.
pub fn decode_relative(
    cur: &mut Cur<'_>,
    origin: Point,
    diags: &mut DiagSink,
    at: (u32, u32),
) -> Result<Path, XarError> {
    let avail = cur.remaining();
    if !avail.is_multiple_of(RELATIVE_STRIDE) {
        diags.push(
            Diagnostic::new(DiagCode::BadRelativePathSize)
                .at(at.0, at.1)
                .with_detail(avail as u64),
        );
        cur.rest();
        return Ok(Path::new());
    }
    let n = avail / RELATIVE_STRIDE;
    let mut raw: Vec<(u8, Point)> = Vec::with_capacity(n.min(4096));
    let mut prev = Point::ORIGIN;
    for i in 0..n {
        let verb = cur.u8()?;
        let (dx, dy) = cur.point_interleaved()?;
        let p = if i == 0 {
            let x = Mp::new(dx).saturating_add(origin.x);
            let y = Mp::new(dy).saturating_add(origin.y);
            clamped(x, y, diags, at)
        } else {
            // P[i] = P[i-1] - D. Subtract, never add.
            let x = prev.x.saturating_sub(Mp::new(dx));
            let y = prev.y.saturating_sub(Mp::new(dy));
            clamped(x, y, diags, at)
        };
        prev = p;
        raw.push((verb, p));
    }
    Ok(assemble(&raw, diags, at))
}

/// Decodes an absolute path: `i32 count`, then `count` verb bytes, then
/// `count` coordinate pairs.
///
/// # Errors
///
/// [`XarError::ShortRecord`] when the payload ends before the declared
/// points do.
pub fn decode_absolute(
    cur: &mut Cur<'_>,
    origin: Point,
    diags: &mut DiagSink,
    at: (u32, u32),
) -> Result<Path, XarError> {
    let declared = cur.i32()?;
    if declared <= 0 {
        return Ok(Path::new());
    }
    // Allocation follows what is actually there, never the declared count:
    // `4 + n * 9` bytes are needed, so a count larger than that is a lie.
    let n = usize::try_from(declared).unwrap_or(0);
    let possible = cur.remaining() / RELATIVE_STRIDE;
    if n > possible {
        diags.push(
            Diagnostic::new(DiagCode::TruncatedRecord)
                .at(at.0, at.1)
                .with_detail(cur.remaining() as u64),
        );
        return Err(XarError::ShortRecord(cur.position()));
    }
    let mut verbs: Vec<u8> = Vec::with_capacity(n);
    for _ in 0..n {
        verbs.push(cur.u8()?);
    }
    let mut raw: Vec<(u8, Point)> = Vec::with_capacity(n);
    for v in verbs {
        let p = cur.point(origin)?;
        raw.push((v, p));
    }
    Ok(assemble(&raw, diags, at))
}

/// Applies a `TAG_PATH_FLAGS` payload to a path.
///
/// One byte per point, in point order. A length that disagrees with the
/// point count is a warning and the prefix is applied: the flags are editing
/// metadata, so losing some of them costs nothing that renders.
pub fn apply_path_flags(path: &mut Path, payload: &[u8], diags: &mut DiagSink, at: (u32, u32)) {
    let want = path.points().len();
    if payload.len() != want {
        diags.push(
            Diagnostic::new(DiagCode::PathFlagsMismatch)
                .at(at.0, at.1)
                .with_detail(payload.len() as u64),
        );
    }
    let flags: Vec<PointFlags> = (0..want)
        .map(|i| {
            payload
                .get(i)
                .map_or(PointFlags::empty(), |b| PointFlags::from_bits_truncate(*b))
        })
        .collect();
    // Only fails on a length mismatch, which cannot happen: `want` is the
    // point count.
    if path.set_flags(flags).is_err() {
        diags.push(Diagnostic::new(DiagCode::PathFlagsMismatch).at(at.0, at.1));
    }
}

fn clamped(x: Mp, y: Mp, diags: &mut DiagSink, at: (u32, u32)) -> Point {
    let (p, moved) = Point::new(x, y).clamp_to_extent();
    if moved {
        diags.push(Diagnostic::new(DiagCode::CoordinateClamped).at(at.0, at.1));
    }
    p
}

/// Turns `(verb, point)` pairs into a validated [`Path`].
///
/// Everything malformed is repaired rather than rejected: a path that starts
/// with a `LineTo` starts with a `MoveTo` instead, an empty subpath is
/// dropped, a truncated cubic is dropped. Each repair is a diagnostic, and
/// the result always passes [`Path::validate`].
fn assemble(raw: &[(u8, Point)], diags: &mut DiagSink, at: (u32, u32)) -> Path {
    let mut verbs: Vec<Verb> = Vec::with_capacity(raw.len());
    let mut points: Vec<Point> = Vec::with_capacity(raw.len());
    let mut repaired = false;
    let mut i = 0usize;
    while let Some(&(v, p)) = raw.get(i) {
        let kind = v & PT_PATHELEMENT;
        let mut consumed = 1usize;
        match kind {
            PT_MOVETO => push_move(&mut verbs, &mut points, p),
            PT_LINETO => {
                if verbs.is_empty() || matches!(verbs.last(), Some(Verb::Close)) {
                    repaired = true;
                    push_move(&mut verbs, &mut points, p);
                } else {
                    verbs.push(Verb::LineTo);
                    points.push(p);
                }
            }
            PT_BEZIERTO => {
                let c2 = raw.get(i.saturating_add(1));
                let end = raw.get(i.saturating_add(2));
                match (c2, end) {
                    (Some(&(_, b)), Some(&(_, c))) => {
                        if verbs.is_empty() || matches!(verbs.last(), Some(Verb::Close)) {
                            repaired = true;
                            push_move(&mut verbs, &mut points, p);
                            consumed = 1;
                        } else {
                            verbs.push(Verb::CubicTo);
                            points.push(p);
                            points.push(b);
                            points.push(c);
                            consumed = 3;
                        }
                    }
                    _ => {
                        // A cubic with fewer than three points left.
                        repaired = true;
                        break;
                    }
                }
            }
            _ => {
                // Verb 0 is not defined by the format.
                repaired = true;
                diags.push(
                    Diagnostic::new(DiagCode::UnknownEnumValue)
                        .at(at.0, at.1)
                        .with_detail(u64::from(v)),
                );
            }
        }
        // The close bit sits on the last point of the subpath.
        let last = i.saturating_add(consumed).saturating_sub(1);
        let closes = raw.get(last).is_some_and(|e| e.0 & PT_CLOSEFIGURE != 0);
        if closes && !verbs.is_empty() && !matches!(verbs.last(), Some(Verb::Close)) {
            verbs.push(Verb::Close);
        }
        i = i.saturating_add(consumed);
    }
    if repaired {
        diags.push(Diagnostic::new(DiagCode::MalformedPath).at(at.0, at.1));
    }
    match Path::from_parts(verbs, points, Vec::new()) {
        Ok(p) => p,
        Err(e) => {
            diags.push(
                Diagnostic::new(DiagCode::MalformedPath)
                    .at(at.0, at.1)
                    .with_detail(path_error_code(&e)),
            );
            Path::new()
        }
    }
}

/// Pushes a `MoveTo`, dropping a preceding one so that no empty subpath is
/// produced.
fn push_move(verbs: &mut Vec<Verb>, points: &mut Vec<Point>, p: Point) {
    if matches!(verbs.last(), Some(Verb::MoveTo)) {
        verbs.pop();
        points.pop();
    }
    verbs.push(Verb::MoveTo);
    points.push(p);
}

/// A stable numeric code for a [`PathError`], for the diagnostic detail.
const fn path_error_code(e: &PathError) -> u64 {
    match e {
        PathError::ArityMismatch { .. } => 1,
        PathError::FlagsMismatch { .. } => 2,
        PathError::MissingMoveTo => 3,
        PathError::DanglingClose { .. } => 4,
        PathError::EmptySubPath { .. } => 5,
        PathError::OutOfExtent { .. } => 6,
        PathError::Svg(_) => 7,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_rel(bytes: &[u8]) -> (Path, DiagSink) {
        let mut d = DiagSink::new();
        let mut c = Cur::new(bytes);
        let p = decode_relative(&mut c, Point::ORIGIN, &mut d, (1, 115)).unwrap();
        (p, d)
    }

    /// `testfiles/OneLine.xar` record 28, byte for byte
    /// (`research/01 §7.3`). This is the oracle for the delta sign, the
    /// interleave and the verb decoding at once.
    #[test]
    fn the_one_line_worked_example_decodes_exactly() {
        let bytes = [
            0x06, 0x00, 0x00, 0x01, 0x02, 0xB5, 0xBA, 0xE5, 0xD3, 0x02, 0xFF, 0xFF, 0xFD, 0xFD,
            0x64, 0xD3, 0x08, 0x5C,
        ];
        let (path, diags) = decode_rel(&bytes);
        assert_eq!(diags.total(), 0);
        assert_eq!(path.verbs(), &[Verb::MoveTo, Verb::LineTo]);
        assert_eq!(
            path.points(),
            &[Point::raw(112_101, 178_899), Point::raw(283_101, 321_399)]
        );
    }

    #[test]
    fn the_delta_is_subtracted_not_added() {
        // An asymmetric two-point path: adding instead of subtracting would
        // put the second point at (90, 80) rather than (110, 120).
        let mut bytes = vec![0x06];
        bytes.extend_from_slice(&interleave(100, 100));
        bytes.push(0x02);
        bytes.extend_from_slice(&interleave(-10, -20));
        let (path, _) = decode_rel(&bytes);
        assert_eq!(path.points()[1], Point::raw(110, 120));
    }

    fn interleave(x: i32, y: i32) -> [u8; 8] {
        let xb = x.to_be_bytes();
        let yb = y.to_be_bytes();
        [xb[0], yb[0], xb[1], yb[1], xb[2], yb[2], xb[3], yb[3]]
    }

    #[test]
    fn a_size_that_is_not_a_multiple_of_nine_is_refused() {
        let (path, diags) = decode_rel(&[0u8; 10]);
        assert!(path.is_empty());
        assert!(
            diags
                .items()
                .iter()
                .any(|d| d.code == DiagCode::BadRelativePathSize)
        );
    }

    #[test]
    fn the_close_bit_becomes_a_close_verb_and_consumes_no_point() {
        let mut bytes = vec![0x06];
        bytes.extend_from_slice(&interleave(0, 0));
        bytes.push(0x02);
        bytes.extend_from_slice(&interleave(-10, 0));
        bytes.push(0x03); // LineTo | CLOSEFIGURE
        bytes.extend_from_slice(&interleave(0, -10));
        let (path, _) = decode_rel(&bytes);
        assert_eq!(
            path.verbs(),
            &[Verb::MoveTo, Verb::LineTo, Verb::LineTo, Verb::Close]
        );
        assert_eq!(path.points().len(), 3);
    }

    #[test]
    fn a_cubic_takes_three_points() {
        let mut bytes = vec![0x06];
        bytes.extend_from_slice(&interleave(0, 0));
        for _ in 0..3 {
            bytes.push(0x04);
            bytes.extend_from_slice(&interleave(-10, -10));
        }
        let (path, _) = decode_rel(&bytes);
        assert_eq!(path.verbs(), &[Verb::MoveTo, Verb::CubicTo]);
        assert_eq!(path.points().len(), 4);
    }

    #[test]
    fn every_verb_byte_decodes_without_panicking() {
        for v in 0u8..=255 {
            let mut bytes = vec![v];
            bytes.extend_from_slice(&interleave(1, 2));
            bytes.push(v);
            bytes.extend_from_slice(&interleave(3, 4));
            let (path, _) = decode_rel(&bytes);
            assert!(path.validate().is_ok());
        }
    }

    #[test]
    fn a_path_that_starts_with_a_line_is_repaired() {
        let mut bytes = vec![0x02];
        bytes.extend_from_slice(&interleave(5, 5));
        let (path, diags) = decode_rel(&bytes);
        assert_eq!(path.verbs(), &[Verb::MoveTo]);
        assert!(
            diags
                .items()
                .iter()
                .any(|d| d.code == DiagCode::MalformedPath)
        );
    }

    #[test]
    fn path_flags_apply_in_point_order() {
        let mut bytes = vec![0x06];
        bytes.extend_from_slice(&interleave(0, 0));
        bytes.push(0x02);
        bytes.extend_from_slice(&interleave(-1, -1));
        let (mut path, _) = decode_rel(&bytes);
        let mut d = DiagSink::new();
        apply_path_flags(&mut path, &[0x05, 0x04], &mut d, (1, 111));
        assert_eq!(d.total(), 0);
        assert_eq!(path.flags_at(0), PointFlags::SMOOTH | PointFlags::END_POINT);
        assert_eq!(path.flags_at(1), PointFlags::END_POINT);
    }

    #[test]
    fn path_flags_of_the_wrong_length_warn_and_apply_the_prefix() {
        let mut bytes = vec![0x06];
        bytes.extend_from_slice(&interleave(0, 0));
        bytes.push(0x02);
        bytes.extend_from_slice(&interleave(-1, -1));
        let (mut path, _) = decode_rel(&bytes);
        let mut d = DiagSink::new();
        apply_path_flags(&mut path, &[0x01, 0x02, 0x04, 0x04], &mut d, (1, 111));
        assert_eq!(d.count(crate::Severity::Warning), 1);
        assert_eq!(path.flags_at(0), PointFlags::SMOOTH);
        let mut d = DiagSink::new();
        apply_path_flags(&mut path, &[0x02], &mut d, (1, 111));
        assert_eq!(d.count(crate::Severity::Warning), 1);
        assert_eq!(path.flags_at(1), PointFlags::empty());
    }

    #[test]
    fn a_huge_coordinate_is_clamped_rather_than_wrapped() {
        let mut bytes = vec![0x06];
        bytes.extend_from_slice(&interleave(i32::MAX, i32::MIN));
        bytes.push(0x02);
        bytes.extend_from_slice(&interleave(i32::MIN, i32::MAX));
        let (path, diags) = decode_rel(&bytes);
        assert!(path.validate().is_ok());
        assert!(
            diags
                .items()
                .iter()
                .any(|d| d.code == DiagCode::CoordinateClamped)
        );
    }

    #[test]
    fn the_absolute_codec_reads_a_declared_count() {
        let mut bytes = 2i32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0x06, 0x02]);
        bytes.extend_from_slice(&10i32.to_le_bytes());
        bytes.extend_from_slice(&20i32.to_le_bytes());
        bytes.extend_from_slice(&30i32.to_le_bytes());
        bytes.extend_from_slice(&40i32.to_le_bytes());
        let mut d = DiagSink::new();
        let mut c = Cur::new(&bytes);
        let p = decode_absolute(&mut c, Point::raw(1, 2), &mut d, (1, 100)).unwrap();
        assert_eq!(p.points(), &[Point::raw(11, 22), Point::raw(31, 42)]);
    }

    #[test]
    fn an_absolute_count_larger_than_the_payload_allocates_nothing() {
        let mut bytes = i32::MAX.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0u8; 8]);
        let mut d = DiagSink::new();
        let mut c = Cur::new(&bytes);
        assert!(decode_absolute(&mut c, Point::ORIGIN, &mut d, (1, 100)).is_err());
    }

    #[test]
    fn style_bits_come_from_the_tag() {
        assert_eq!(
            PathStyleBits::from_tag(116),
            Some(PathStyleBits {
                filled: true,
                stroked: true,
                relative: true
            })
        );
        assert_eq!(
            PathStyleBits::from_tag(100),
            Some(PathStyleBits {
                filled: false,
                stroked: false,
                relative: false
            })
        );
        assert_eq!(PathStyleBits::from_tag(117), None);
    }
}
