//! Measurement fields: units, the pica notation and small expressions.
//!
//! `research/04 §3` item 16 lists "a numeric field that accepts `10mm`, `1in`
//! or `3p6`" among the eighteen traits that made Xara feel like Xara, so the
//! parser is part of the user interface rather than a nicety bolted on later.
//!
//! [`xarast_geom::Mp`] already parses a number with a unit suffix. This
//! module adds the two things a field needs on top of it: the typographic
//! `3p6` notation (three picas and six points) and additive expressions such
//! as `12mm + 3pt`, which is how a user nudges a value without doing the
//! arithmetic in their head.

use std::fmt;
use std::str::FromStr;

use xarast_geom::Mp;

/// The unit a measurement is displayed in.
///
/// Parsing accepts any unit whatever the display unit is; the display unit
/// only decides how a bare number is read and how a value is written back.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum Unit {
    /// Millimetres.
    Millimetre,
    /// Centimetres.
    Centimetre,
    /// Inches.
    Inch,
    /// PostScript points, 1/72 inch. The document's own unit.
    #[default]
    Point,
    /// Picas, 12 points.
    Pica,
    /// CSS pixels at 96 dpi.
    Pixel,
}

impl Unit {
    /// Every unit, in the order a menu should offer them.
    pub const ALL: [Unit; 6] = [
        Unit::Millimetre,
        Unit::Centimetre,
        Unit::Inch,
        Unit::Point,
        Unit::Pica,
        Unit::Pixel,
    ];

    /// The suffix written after a value, and accepted when parsing one.
    pub fn suffix(self) -> &'static str {
        match self {
            Unit::Millimetre => "mm",
            Unit::Centimetre => "cm",
            Unit::Inch => "in",
            Unit::Point => "pt",
            Unit::Pica => "pc",
            Unit::Pixel => "px",
        }
    }

    /// The name shown in a unit menu.
    pub fn label(self) -> &'static str {
        match self {
            Unit::Millimetre => "Millimetres",
            Unit::Centimetre => "Centimetres",
            Unit::Inch => "Inches",
            Unit::Point => "Points",
            Unit::Pica => "Picas",
            Unit::Pixel => "Pixels",
        }
    }

    /// Converts a millipoint value into this unit.
    pub fn from_mp(self, v: Mp) -> f64 {
        match self {
            Unit::Millimetre => v.to_mm(),
            Unit::Centimetre => v.to_mm() / 10.0,
            Unit::Inch => v.to_inch(),
            Unit::Point => v.to_pt(),
            Unit::Pica => v.to_pt() / 12.0,
            Unit::Pixel => v.to_px(96.0),
        }
    }

    /// Converts a value in this unit into millipoints.
    pub fn to_mp(self, v: f64) -> Mp {
        match self {
            Unit::Millimetre => Mp::from_mm(v),
            Unit::Centimetre => Mp::from_mm(v * 10.0),
            Unit::Inch => Mp::from_inch(v),
            Unit::Point => Mp::from_pt(v),
            Unit::Pica => Mp::from_pt(v * 12.0),
            Unit::Pixel => Mp::from_px(v, 96.0),
        }
    }

    /// How many decimals a field shows for this unit.
    ///
    /// Enough to resolve a device pixel at a normal zoom, and no more: a
    /// field that shows `12.700000mm` is unreadable and invites a false
    /// impression of precision.
    pub fn decimals(self) -> usize {
        match self {
            Unit::Millimetre | Unit::Point | Unit::Pixel => 2,
            Unit::Centimetre | Unit::Inch | Unit::Pica => 3,
        }
    }

    /// The step a bump button or one arrow-key press applies.
    pub fn bump(self) -> Mp {
        match self {
            Unit::Millimetre | Unit::Centimetre => Mp::from_mm(0.1),
            Unit::Inch => Mp::from_inch(0.01),
            Unit::Point | Unit::Pica => Mp::from_pt(1.0),
            Unit::Pixel => Mp::from_px(1.0, 96.0),
        }
    }
}

/// Why a measurement field could not read what was typed into it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MeasureError {
    /// The field was empty, or held only whitespace and signs.
    #[error("no measurement was given")]
    Empty,
    /// A term was neither a number, a `3p6` pica measurement, nor a number
    /// with a known unit.
    #[error("`{0}` is not a measurement")]
    Malformed(String),
    /// The unit suffix was not one this application knows.
    #[error("unknown unit `{0}`; expected mm, cm, in, pt, pc, px, mp or the `3p6` pica form")]
    UnknownUnit(String),
    /// The sum of the terms left the representable coordinate range.
    #[error("the measurement is outside the representable range")]
    OutOfRange,
}

/// Reads a measurement, in `display` unit when no unit is given.
///
/// Accepted forms, in the spirit of `research/04 §3` item 16:
///
/// * a bare number — read in the display unit, so a field showing
///   millimetres reads `12` as `12mm`;
/// * a number with a unit — `10mm`, `1in`, `3pc`, `96px`, `250mp`;
/// * the typographic pica form `3p6`, meaning three picas and six points,
///   including `p6` for a bare six points;
/// * a sum or difference of any of the above — `12mm + 3pt`, `1in - 2mm`.
///
/// # Examples
///
/// ```
/// use xarast_ui::units::{Unit, parse_measure};
/// use xarast_geom::Mp;
///
/// assert_eq!(parse_measure("1in", Unit::Point).unwrap(), Mp::from_pt(72.0));
/// assert_eq!(parse_measure("3p6", Unit::Point).unwrap(), Mp::from_pt(42.0));
/// assert_eq!(parse_measure("12", Unit::Millimetre).unwrap(), Mp::from_mm(12.0));
/// ```
pub fn parse_measure(text: &str, display: Unit) -> Result<Mp, MeasureError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(MeasureError::Empty);
    }

    let mut total = Mp::ZERO;
    let mut terms = 0usize;
    for (sign, term) in split_terms(text)? {
        let value = parse_term(term, display)?;
        let signed = if sign < 0 {
            Mp::ZERO
                .checked_sub(value)
                .ok_or(MeasureError::OutOfRange)?
        } else {
            value
        };
        total = total.checked_add(signed).ok_or(MeasureError::OutOfRange)?;
        terms += 1;
    }
    if terms == 0 {
        return Err(MeasureError::Empty);
    }
    if !total.is_in_extent() {
        return Err(MeasureError::OutOfRange);
    }
    Ok(total)
}

/// Splits `12mm + 3pt` into signed terms.
///
/// Only `+` and `-` join terms, and only when they separate them: the sign
/// of a leading negative number stays with the number, and an exponent's
/// `e-3` is not a separator because a separator must be surrounded by the
/// end of one term and the start of another.
fn split_terms(text: &str) -> Result<Vec<(i32, &str)>, MeasureError> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut sign = 1i32;
    let mut start = 0usize;
    let mut i = 0usize;
    // A leading sign belongs to the first term's value.
    if bytes[0] == b'+' || bytes[0] == b'-' {
        sign = if bytes[0] == b'-' { -1 } else { 1 };
        start = 1;
        i = 1;
    }
    while i < bytes.len() {
        let c = bytes[i];
        if (c == b'+' || c == b'-') && i > start {
            let prev = bytes[i - 1];
            // `1e-3` keeps its sign; a separator follows a digit or a unit
            // letter and is itself followed by something.
            if prev == b'e' || prev == b'E' {
                i += 1;
                continue;
            }
            let term = text[start..i].trim();
            if term.is_empty() {
                return Err(MeasureError::Malformed(text.to_owned()));
            }
            out.push((sign, term));
            sign = if c == b'-' { -1 } else { 1 };
            start = i + 1;
        }
        i += 1;
    }
    let last = text[start..].trim();
    if last.is_empty() {
        return Err(MeasureError::Malformed(text.to_owned()));
    }
    out.push((sign, last));
    Ok(out)
}

/// Reads one term: a pica form, a unit-bearing number or a bare number.
fn parse_term(term: &str, display: Unit) -> Result<Mp, MeasureError> {
    if let Some(v) = parse_pica_form(term)? {
        return Ok(v);
    }
    let has_unit = term
        .chars()
        .last()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '%');
    if has_unit {
        return Mp::from_str(term).map_err(|e| match e {
            xarast_geom::ParseMpError::UnknownUnit(u) => MeasureError::UnknownUnit(u),
            xarast_geom::ParseMpError::Malformed(s) => MeasureError::Malformed(s),
        });
    }
    let v: f64 = term
        .parse()
        .map_err(|_| MeasureError::Malformed(term.to_owned()))?;
    if !v.is_finite() {
        return Err(MeasureError::Malformed(term.to_owned()));
    }
    Ok(display.to_mp(v))
}

/// Reads the typographic `3p6` form — three picas and six points.
///
/// Returns `Ok(None)` when the term is not in that form at all, so the
/// caller can try the other readings.
fn parse_pica_form(term: &str) -> Result<Option<Mp>, MeasureError> {
    let lowered = term.trim();
    // Exactly one `p`, with digits or nothing on either side, and no other
    // letter: `3p6`, `p6`, `3p`, `-2p3`.
    let p = match lowered.find(['p', 'P']) {
        Some(p) => p,
        None => return Ok(None),
    };
    let (head, tail) = lowered.split_at(p);
    let tail = &tail[1..];
    let head_ok = head.is_empty()
        || head
            .strip_prefix(['+', '-'])
            .unwrap_or(head)
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.');
    let tail_ok = tail.chars().all(|c| c.is_ascii_digit() || c == '.');
    if !head_ok || !tail_ok || (head.is_empty() && tail.is_empty()) {
        return Ok(None);
    }
    // `3p` alone is picas; `12pt`, `3pc` and `10px` are handled by `Mp`, and
    // their tails are not digits so they never reach here.
    let negative = head.starts_with('-');
    let picas: f64 = match head.trim_start_matches(['+', '-']) {
        "" => 0.0,
        h => h
            .parse()
            .map_err(|_| MeasureError::Malformed(term.to_owned()))?,
    };
    let points: f64 = match tail {
        "" => 0.0,
        t => t
            .parse()
            .map_err(|_| MeasureError::Malformed(term.to_owned()))?,
    };
    let total = picas * 12.0 + points;
    Ok(Some(Mp::from_pt(if negative { -total } else { total })))
}

/// Writes a measurement the way a field shows it: value, then unit suffix.
///
/// The output parses back to the same value, which is the property a field
/// needs when the user tabs through it without editing.
///
/// # Examples
///
/// ```
/// use xarast_ui::units::{Unit, format_measure};
/// use xarast_geom::Mp;
///
/// assert_eq!(format_measure(Mp::from_pt(72.0), Unit::Inch), "1in");
/// ```
pub fn format_measure(v: Mp, unit: Unit) -> String {
    let value = unit.from_mp(v);
    let mut s = format!("{value:.*}", unit.decimals());
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_owned();
    }
    if s == "-0" {
        s = "0".to_owned();
    }
    s.push_str(unit.suffix());
    s
}

/// A measurement shown without its unit, for a table cell that carries the
/// unit in its column heading.
#[derive(Debug, Clone, Copy)]
pub struct Bare(
    /// The value to show.
    pub Mp,
    /// The unit it is shown in.
    pub Unit,
);

impl fmt::Display for Bare {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = format_measure(self.0, self.1);
        f.write_str(s.trim_end_matches(self.1.suffix()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_with_suffixes_parse_whatever_the_display_unit() {
        assert_eq!(
            parse_measure("10mm", Unit::Inch).unwrap(),
            Mp::from_mm(10.0)
        );
        assert_eq!(
            parse_measure("1in", Unit::Millimetre).unwrap(),
            Mp::from_pt(72.0)
        );
        assert_eq!(
            parse_measure("36pt", Unit::Pixel).unwrap(),
            Mp::from_pt(36.0)
        );
        assert_eq!(
            parse_measure("96px", Unit::Point).unwrap(),
            Mp::from_inch(1.0)
        );
        assert_eq!(
            parse_measure("1pc", Unit::Point).unwrap(),
            Mp::from_pt(12.0)
        );
    }

    #[test]
    fn a_bare_number_is_read_in_the_display_unit() {
        assert_eq!(
            parse_measure("12", Unit::Millimetre).unwrap(),
            Mp::from_mm(12.0)
        );
        assert_eq!(parse_measure("12", Unit::Point).unwrap(), Mp::from_pt(12.0));
        assert_eq!(
            parse_measure(" 2.5 ", Unit::Inch).unwrap(),
            Mp::from_inch(2.5)
        );
    }

    #[test]
    fn the_pica_form_reads_picas_and_points() {
        assert_eq!(
            parse_measure("3p6", Unit::Point).unwrap(),
            Mp::from_pt(42.0)
        );
        assert_eq!(parse_measure("p6", Unit::Point).unwrap(), Mp::from_pt(6.0));
        assert_eq!(parse_measure("3p", Unit::Point).unwrap(), Mp::from_pt(36.0));
        assert_eq!(
            parse_measure("-2p3", Unit::Point).unwrap(),
            Mp::from_pt(-27.0)
        );
    }

    #[test]
    fn the_pica_form_does_not_swallow_pt_pc_or_px() {
        assert_eq!(
            parse_measure("12pt", Unit::Millimetre).unwrap(),
            Mp::from_pt(12.0)
        );
        assert_eq!(
            parse_measure("2pc", Unit::Millimetre).unwrap(),
            Mp::from_pt(24.0)
        );
        assert_eq!(
            parse_measure("8px", Unit::Millimetre).unwrap(),
            Mp::from_px(8.0, 96.0)
        );
    }

    #[test]
    fn sums_and_differences_are_evaluated() {
        assert_eq!(
            parse_measure("12mm + 3pt", Unit::Point).unwrap(),
            Mp::from_mm(12.0).checked_add(Mp::from_pt(3.0)).unwrap()
        );
        assert_eq!(
            parse_measure("1in - 2mm", Unit::Point).unwrap(),
            Mp::from_inch(1.0).checked_sub(Mp::from_mm(2.0)).unwrap()
        );
        // Each term is rounded to whole millipoints before the sum, so a
        // three-term expression may land one millipoint — a thousandth of
        // a point — from the same sum computed in one step.
        let three_terms = parse_measure("1in-2mm+1mm", Unit::Point).unwrap();
        let one_step = Mp::from_inch(1.0).checked_sub(Mp::from_mm(1.0)).unwrap();
        assert!((three_terms.raw() - one_step.raw()).abs() <= 1);
    }

    #[test]
    fn a_leading_sign_belongs_to_the_number() {
        assert_eq!(
            parse_measure("-5mm", Unit::Point).unwrap(),
            Mp::from_mm(-5.0)
        );
        assert_eq!(
            parse_measure("+5mm", Unit::Point).unwrap(),
            Mp::from_mm(5.0)
        );
    }

    #[test]
    fn rubbish_is_rejected_and_never_panics() {
        for s in [
            "",
            "   ",
            "mm",
            "abc",
            "12 fathoms",
            "+",
            "-",
            "12mm +",
            "1..2",
            "1e",
            "**",
        ] {
            let _ = parse_measure(s, Unit::Point);
        }
        assert!(matches!(
            parse_measure("", Unit::Point),
            Err(MeasureError::Empty)
        ));
        assert!(matches!(
            parse_measure("12 fathoms", Unit::Point),
            Err(MeasureError::UnknownUnit(_))
        ));
        assert!(parse_measure("12mm +", Unit::Point).is_err());
    }

    #[test]
    fn huge_values_saturate_into_an_error_not_an_overflow() {
        assert!(parse_measure("100000in + 100000in", Unit::Point).is_err());
    }

    #[test]
    fn formatting_round_trips_through_parsing() {
        for unit in Unit::ALL {
            for raw in [0, 1_000, -1_000, 72_000, 123_456, -987_654] {
                let v = Mp::new(raw);
                let text = format_measure(v, unit);
                let back = parse_measure(&text, unit).expect("formatted value must parse");
                let tolerance = 60; // rounding to the displayed decimals
                assert!(
                    (back.raw() - v.raw()).abs() <= tolerance,
                    "{unit:?}: {v:?} -> {text} -> {back:?}"
                );
            }
        }
    }

    #[test]
    fn a_bare_display_drops_the_suffix() {
        assert_eq!(Bare(Mp::from_pt(72.0), Unit::Inch).to_string(), "1");
    }

    #[test]
    fn bumps_are_a_sensible_size_in_every_unit() {
        for unit in Unit::ALL {
            let bump = unit.bump();
            assert!(bump.raw() > 0, "{unit:?}");
            assert!(bump.to_pt() <= 12.0, "{unit:?}");
        }
    }
}
