//! Number normalisation: pass 1 of `research/06 §4.5.1`.
//!
//! Coordinates are points with **up to three decimals** — exactly one
//! millipoint, the model's unit — without trailing zeros, without the
//! leading `0` of a fraction. The exponent form the spec allows when it is
//! shorter is not used (see [`push_fixed`]). Because the model is integral millipoints, a coordinate is
//! formatted from its integer, never through a float, so the text is exact.

/// One formatted number, on the stack: the path writer formats both
/// spellings of every segment to pick the shorter, so a heap string per
/// number was most of its time (ProbeX16: 518 k nodes, ~250 ms).
#[derive(Debug, Clone, Copy)]
pub struct Num {
    b: [u8; 24],
    n: u8,
}

impl Num {
    /// Millipoints as points: `12500` → `12.5`, `-500` → `-.5`.
    #[must_use]
    pub fn mp(v: i64) -> Num {
        Num::fixed(v, 3)
    }

    /// `value / 10^decimals` in the shortest normalised form; `decimals`
    /// is at most 9. At most 21 bytes: 19 digits, a sign and a point.
    #[must_use]
    pub fn fixed(value: i64, decimals: u32) -> Num {
        let decimals = decimals.min(9);
        let mut s = Num { b: [0; 24], n: 0 };
        if value == 0 {
            s.push(b'0');
            return s;
        }
        let mag = value.unsigned_abs();
        let scale = 10u64.pow(decimals);
        let int = mag / scale;
        let mut frac = mag % scale;
        let mut frac_digits = decimals;
        while frac_digits > 0 && frac.is_multiple_of(10) {
            frac /= 10;
            frac_digits -= 1;
        }
        if value < 0 {
            s.push(b'-');
        }
        // Exponent form (`1e3`) is never written: it only ever shortens a
        // round thousand of points, and a unit suffix after it (`1e3mm`) is
        // a classic parser trap.
        if int != 0 || frac_digits == 0 {
            s.digits(int, 0);
        }
        if frac_digits > 0 {
            s.push(b'.');
            s.digits(frac, frac_digits);
        }
        s
    }

    fn push(&mut self, c: u8) {
        if let Some(slot) = self.b.get_mut(usize::from(self.n)) {
            *slot = c;
            self.n += 1;
        }
    }

    /// Appends `v` in decimal, zero-padded to `width` digits.
    fn digits(&mut self, mut v: u64, width: u32) {
        let mut tmp = [0u8; 20];
        let mut k = 0usize;
        loop {
            if let Some(d) = tmp.get_mut(k) {
                *d = b'0' + (v % 10) as u8;
            }
            k += 1;
            v /= 10;
            if v == 0 && k >= width as usize {
                break;
            }
        }
        for d in tmp.iter().take(k).rev() {
            self.push(*d);
        }
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(self.b.get(..usize::from(self.n)).unwrap_or(&[])).unwrap_or("")
    }

    /// Its length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.n)
    }

    /// Whether it is empty (never, for a formatted number).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// Whether it has a fractional part.
    #[must_use]
    pub fn has_dot(&self) -> bool {
        self.b.iter().take(usize::from(self.n)).any(|&c| c == b'.')
    }
}

/// Appends millipoints as points: `12500` → `12.5`, `-500` → `-.5`.
pub fn push_mp(out: &mut String, mp: i64) {
    out.push_str(Num::mp(mp).as_str());
}

/// Millipoints as points, as a new string.
#[must_use]
pub fn mp(v: i64) -> String {
    Num::mp(v).as_str().to_owned()
}

/// Appends `value / 10^decimals` in the shortest normalised form.
///
/// `decimals` is at most 9.
pub fn push_fixed(out: &mut String, value: i64, decimals: u32) {
    out.push_str(Num::fixed(value, decimals).as_str());
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

/// An `f32` in the shortest decimal that parses back to the same `f32`
/// (no exponent; `0.5` as `.5`, as [`f64s`] spells it). Used where the
/// model's value must survive a reload bit for bit: palette components
/// (XARA-T-0110). Non-finite values become `0`.
#[must_use]
pub fn f32s(v: f32) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    // `Display` for `f32` is the shortest round-trip form, never an
    // exponent.
    let s = format!("{v}");
    if let Some(rest) = s.strip_prefix("0.") {
        format!(".{rest}")
    } else if let Some(rest) = s.strip_prefix("-0.") {
        format!("-.{rest}")
    } else if s == "-0" {
        "0".into()
    } else {
        s
    }
}

/// An `f64` in the shortest decimal that parses back to the same `f64`,
/// spelt as [`f32s`] spells an `f32`. Used for the fill profile, whose
/// bias and gain must survive a reload bit for bit (phase 8, T8.8.2).
#[must_use]
pub fn f64s_exact(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    // `Display` for `f64` is the shortest round-trip form, never an
    // exponent.
    let s = format!("{v}");
    if let Some(rest) = s.strip_prefix("0.") {
        format!(".{rest}")
    } else if let Some(rest) = s.strip_prefix("-0.") {
        format!("-.{rest}")
    } else if s == "-0" {
        "0".into()
    } else {
        s
    }
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
            '0'..='9' => {}
            // A sign starts the number: what precedes it is the previous
            // one (`1.5-5` then `.5` needs its space).
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
        // The dot of the number before a signed one does not count.
        let mut s = String::from("M");
        let mut sep = false;
        for n in ["1.5", "-5", ".5"] {
            push_separated(&mut s, &mut sep, n);
        }
        assert_eq!(s, "M1.5-5 .5");
    }

    #[test]
    fn stack_numbers_match_the_rules() {
        for (v, d, want) in [
            (0, 3, "0"),
            (-1_000, 3, "-1"),
            (1_234_567, 3, "1234.567"),
            (7, 9, ".000000007"),
            (-10, 6, "-.00001"),
            (i64::MIN, 3, "-9223372036854775.808"),
        ] {
            assert_eq!(Num::fixed(v, d).as_str(), want, "{v}");
        }
    }

    #[test]
    fn f32s_reads_back_bit_for_bit() {
        assert_eq!(f32s(0.5), ".5");
        assert_eq!(f32s(-0.25), "-.25");
        assert_eq!(f32s(1.0), "1");
        assert_eq!(f32s(-0.0), "0");
        assert_eq!(f32s(f32::NAN), "0");
        // A walk over the unit interval, where palette components live,
        // and a few magnitudes beyond it.
        let mut bits = 0x3380_0000u32; // ~6e-8
        while bits < 0x3f80_0000 {
            let v = f32::from_bits(bits);
            let s = f32s(v);
            assert!(!s.contains('e'), "{s}");
            assert_eq!(s.parse::<f32>().ok(), Some(v), "{s}");
            bits += 9_973;
        }
        for v in [255.0f32, 1234.567, 0.466_783_6, 1e-7] {
            assert_eq!(f32s(v).parse::<f32>().ok(), Some(v));
        }
    }
}
