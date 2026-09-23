//! Number normalisation: pass 1 of `research/06 §4.5.1`.
//!
//! Coordinates are points with **up to three decimals** — exactly one
//! millipoint, the model's unit — without trailing zeros, without the
//! leading `0` of a fraction. The exponent form the spec allows when it is
//! shorter is not used (see [`push_fixed`]). Because the model is integral millipoints, a coordinate is
//! formatted from its integer, never through a float, so the text is exact.

use std::fmt::Write as _;

/// Appends millipoints as points: `12500` → `12.5`, `-500` → `-.5`.
pub fn push_mp(out: &mut String, mp: i64) {
    push_fixed(out, mp, 3);
}

/// Millipoints as points, as a new string.
#[must_use]
pub fn mp(v: i64) -> String {
    let mut s = String::new();
    push_mp(&mut s, v);
    s
}

/// Appends `value / 10^decimals` in the shortest normalised form.
///
/// `decimals` is at most 9.
pub fn push_fixed(out: &mut String, value: i64, decimals: u32) {
    let decimals = decimals.min(9);
    if value == 0 {
        out.push('0');
        return;
    }
    let neg = value < 0;
    let mag = value.unsigned_abs();
    let scale = 10u64.pow(decimals);
    let int = mag / scale;
    let mut frac = mag % scale;
    let mut frac_digits = decimals;
    while frac_digits > 0 && frac.is_multiple_of(10) {
        frac /= 10;
        frac_digits -= 1;
    }
    if neg {
        out.push('-');
    }
    if frac_digits == 0 {
        // Exponent form (`1e3`) is never written: it only ever shortens a
        // round thousand of points, and a unit suffix after it (`1e3mm`)
        // is a classic parser trap.
        let _ = write!(out, "{int}");
        return;
    }
    if int != 0 {
        let _ = write!(out, "{int}");
    }
    let _ = write!(out, ".{frac:0width$}", width = frac_digits as usize);
}

/// Appends a unitless float rounded to `decimals` places, normalised the
/// same way. Non-finite values become `0`.
pub fn push_f64(out: &mut String, v: f64, decimals: u32) {
    let decimals = decimals.min(9);
    if !v.is_finite() {
        out.push('0');
        return;
    }
    let scaled = (v * 10f64.powi(decimals as i32)).round();
    // Beyond ±9.2e18 the value is not a coordinate anyone meant; clamp.
    let clamped = scaled.clamp(-9.0e18, 9.0e18) as i64;
    push_fixed(out, clamped, decimals);
}

/// A unitless float, as a new string.
#[must_use]
pub fn f64s(v: f64, decimals: u32) -> String {
    let mut s = String::new();
    push_f64(&mut s, v, decimals);
    s
}

/// Appends `next` to a list of numbers with the fewest separators an SVG
/// number parser accepts: none before a `-`, none before a leading `.`
/// when the previous number already has a fraction, a space otherwise.
pub fn push_separated(out: &mut String, prev_needs_sep: &mut bool, next: &str) {
    if *prev_needs_sep {
        let prev_has_dot = last_number_has_dot(out);
        let omit = next.starts_with('-') || (next.starts_with('.') && prev_has_dot);
        if !omit {
            out.push(' ');
        }
    }
    out.push_str(next);
    *prev_needs_sep = true;
}

/// Whether the number at the end of `out` has a `.` and no exponent.
fn last_number_has_dot(out: &str) -> bool {
    for c in out.chars().rev() {
        match c {
            '.' => return true,
            '0'..='9' | '-' => {}
            _ => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn millipoints_format_as_the_spec_asks() {
        let cases = [
            (0, "0"),
            (12_500, "12.5"),
            (500, ".5"),
            (-500, "-.5"),
            (1, ".001"),
            (-1, "-.001"),
            (595_276, "595.276"),
            (841_890, "841.89"),
            (1_000, "1"),
            (10_000, "10"),
            (100_000, "100"),
            (1_000_000, "1000"),
            (-2_000_000, "-2000"),
            (1_000_000_000, "1000000"),
            (i32::MAX as i64, "2147483.647"),
            (i32::MIN as i64, "-2147483.648"),
        ];
        for (v, want) in cases {
            assert_eq!(mp(v), want, "{v}");
        }
    }

    #[test]
    fn floats_round_and_normalise() {
        assert_eq!(f64s(0.5, 4), ".5");
        assert_eq!(f64s(1.0, 4), "1");
        assert_eq!(f64s(0.123_456, 4), ".1235");
        assert_eq!(f64s(-0.000_01, 4), "0");
        assert_eq!(f64s(f64::NAN, 4), "0");
        assert_eq!(f64s(f64::INFINITY, 4), "0");
    }

    #[test]
    fn separators_are_minimal_and_unambiguous() {
        let mut s = String::from("M");
        let mut sep = false;
        for n in ["10", "-5", ".5", ".25", "3", ".5", "1e3", ".5"] {
            push_separated(&mut s, &mut sep, n);
        }
        assert_eq!(s, "M10-5 .5.25 3 .5 1e3 .5");
    }
}
