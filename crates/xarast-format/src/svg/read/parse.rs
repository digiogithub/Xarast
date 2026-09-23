//! Value parsers for the reader: numbers, number lists, path data,
//! transforms and colours.
//!
//! Numbers become **millipoints exactly**: a coordinate written with up to
//! three decimals (which is everything the writer produces) is parsed as a
//! decimal, never through a float, so a read–write cycle cannot drift.
//! Anything else SVG allows — more decimals, exponents — is accepted and
//! rounded to the nearest millipoint (`research/06 §5.5`: "the reader MUST
//! accept any valid SVG number").

#![deny(clippy::arithmetic_side_effects)]

use xarast_color::Rgba8;

/// The largest magnitude a parsed coordinate keeps, in millipoints: far
/// outside the document extent (so the builder still reports it), small
/// enough that sums of a few of them never overflow an `i64`.
pub(crate) const MP_CLAMP: i64 = 1 << 40;

/// A scanner over an SVG number list or path data.
pub(crate) struct Scan<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Scan<'a> {
    pub(crate) fn new(s: &'a str) -> Scan<'a> {
        Scan {
            s: s.as_bytes(),
            i: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn bump(&mut self) {
        self.i = self.i.saturating_add(1);
    }

    /// Skips whitespace and at most one comma.
    pub(crate) fn sep(&mut self) {
        self.ws();
        if self.peek() == Some(b',') {
            self.bump();
            self.ws();
        }
    }

    pub(crate) fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r' | b'\x0c')) {
            self.bump();
        }
    }

    pub(crate) fn at_end(&mut self) -> bool {
        self.ws();
        self.i >= self.s.len()
    }

    /// The next command letter, if the next token is one.
    pub(crate) fn letter(&mut self) -> Option<u8> {
        self.ws();
        match self.peek() {
            Some(c) if c.is_ascii_alphabetic() && c != b'e' && c != b'E' => {
                self.bump();
                Some(c)
            }
            _ => None,
        }
    }

    /// An arc flag: a single `0` or `1`, which needs no separator.
    pub(crate) fn flag(&mut self) -> Option<bool> {
        self.sep();
        let f = match self.peek()? {
            b'0' => false,
            b'1' => true,
            _ => return None,
        };
        self.bump();
        Some(f)
    }

    /// The next number, as its decimal parts.
    pub(crate) fn decimal(&mut self) -> Option<Decimal> {
        self.sep();
        let mut neg = false;
        match self.peek()? {
            b'-' => {
                neg = true;
                self.bump();
            }
            b'+' => self.bump(),
            _ => {}
        }
        let mut mant: i128 = 0;
        let mut digits = 0u32;
        let mut scale: i32 = 0;
        let mut any = false;
        let push = |d: u8, frac: bool, mant: &mut i128, digits: &mut u32, scale: &mut i32| {
            // Beyond 30 significant digits the rest cannot change anything
            // a millipoint can hold; only the position still matters.
            if *digits < 30 {
                *mant = mant
                    .saturating_mul(10)
                    .saturating_add(i128::from(d.saturating_sub(b'0')));
                if *mant != 0 {
                    *digits = digits.saturating_add(1);
                }
                if frac {
                    *scale = scale.saturating_sub(1);
                }
            } else if !frac {
                *scale = scale.saturating_add(1);
            }
        };
        while let Some(c @ b'0'..=b'9') = self.peek() {
            push(c, false, &mut mant, &mut digits, &mut scale);
            any = true;
            self.bump();
        }
        if self.peek() == Some(b'.') {
            let save = self.i;
            self.bump();
            let mut frac_any = false;
            while let Some(c @ b'0'..=b'9') = self.peek() {
                push(c, true, &mut mant, &mut digits, &mut scale);
                frac_any = true;
                self.bump();
            }
            if !frac_any && !any {
                self.i = save;
                return None;
            }
            any = true;
        }
        if !any {
            return None;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            let save = self.i;
            self.bump();
            let mut eneg = false;
            match self.peek() {
                Some(b'-') => {
                    eneg = true;
                    self.bump();
                }
                Some(b'+') => self.bump(),
                _ => {}
            }
            let mut e: i32 = 0;
            let mut edigits = false;
            while let Some(c @ b'0'..=b'9') = self.peek() {
                e = e
                    .saturating_mul(10)
                    .saturating_add(i32::from(c.saturating_sub(b'0')));
                edigits = true;
                self.bump();
            }
            if edigits {
                scale = scale.saturating_add(if eneg { e.saturating_neg() } else { e });
            } else {
                // `1em`: the `e` belongs to a unit, not to the number.
                self.i = save;
            }
        }
        Some(Decimal { neg, mant, scale })
    }

    /// The next number as a float.
    pub(crate) fn f64(&mut self) -> Option<f64> {
        self.decimal().map(Decimal::to_f64)
    }

    /// The next number as millipoints (the number is in points).
    pub(crate) fn mp(&mut self) -> Option<i64> {
        self.decimal().map(|d| d.scaled(3))
    }
}

/// A parsed decimal: `(-1)^neg · mant · 10^scale`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Decimal {
    neg: bool,
    mant: i128,
    scale: i32,
}

impl Decimal {
    /// The value as a float.
    ///
    /// Correctly rounded: `.35` is the `f64` nearest to 0.35, so a value
    /// written in its shortest round-trip spelling reads back bit for bit.
    /// Multiplying by `10^scale` would not be (`35 × 0.01` is
    /// `0.35000000000000003`).
    pub(crate) fn to_f64(self) -> f64 {
        const EXACT: i128 = 1 << 53;
        let v = if self.mant < EXACT && (-22..=22).contains(&self.scale) {
            // Both operands are exact, so one IEEE operation rounds once.
            let m = self.mant as f64;
            if self.scale < 0 {
                m / 10f64.powi(self.scale.saturating_neg())
            } else {
                m * 10f64.powi(self.scale)
            }
        } else {
            format!("{}e{}", self.mant, self.scale)
                .parse()
                .unwrap_or(f64::INFINITY)
        };
        if self.neg { -v } else { v }
    }

    /// The value times `10^shift`, rounded half away from zero, clamped to
    /// ±[`MP_CLAMP`]. Exact whenever the result is an integer.
    pub(crate) fn scaled(self, shift: i32) -> i64 {
        let e = self.scale.saturating_add(shift);
        let mag: i128 = if e >= 0 {
            let mut m = self.mant;
            for _ in 0..e.min(40) {
                m = m.saturating_mul(10);
                if m > i128::from(MP_CLAMP) {
                    break;
                }
            }
            m
        } else if e < -38 {
            0
        } else {
            let div = 10i128.checked_pow(e.unsigned_abs()).unwrap_or(i128::MAX);
            let q = self.mant.checked_div(div).unwrap_or(0);
            let r = self.mant.checked_rem(div).unwrap_or(0);
            if r.saturating_mul(2) >= div {
                q.saturating_add(1)
            } else {
                q
            }
        };
        let mag = i64::try_from(mag.min(i128::from(MP_CLAMP))).unwrap_or(MP_CLAMP);
        if self.neg { mag.saturating_neg() } else { mag }
    }
}

/// A whole string as one number in points → millipoints. Trailing
/// whitespace is allowed; anything else is not.
pub(crate) fn mp(s: &str) -> Option<i64> {
    let mut sc = Scan::new(s);
    let v = sc.mp()?;
    sc.at_end().then_some(v)
}

/// A whole string as the nearest `f32`, correctly rounded from the
/// decimal (not through an `f64`, which can round twice): what the writer's
/// shortest round-trip spelling (`num::f32s`) needs to come back bit for
/// bit. Anything but a plain decimal goes through [`float`].
pub(crate) fn f32_exact(s: &str) -> Option<f32> {
    let t = s.trim();
    if !t.is_empty()
        && t.bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'+' | b'e' | b'E'))
        && let Ok(v) = t.parse::<f32>()
        && v.is_finite()
    {
        return Some(v);
    }
    float(s).map(|x| x as f32)
}

/// A whole string as one float.
pub(crate) fn float(s: &str) -> Option<f64> {
    let mut sc = Scan::new(s);
    let v = sc.f64()?;
    (sc.at_end() && v.is_finite()).then_some(v)
}

/// A list of numbers separated by whitespace and/or commas.
pub(crate) fn floats(s: &str) -> Option<Vec<f64>> {
    let mut sc = Scan::new(s);
    let mut out = Vec::new();
    while !sc.at_end() {
        let v = sc.f64()?;
        if !v.is_finite() {
            return None;
        }
        out.push(v);
        if out.len() > 4096 {
            return None;
        }
    }
    Some(out)
}

/// A list of numbers in points → millipoints.
pub(crate) fn mps(s: &str) -> Option<Vec<i64>> {
    let mut sc = Scan::new(s);
    let mut out = Vec::new();
    while !sc.at_end() {
        out.push(sc.mp()?);
        if out.len() > 4096 {
            return None;
        }
    }
    Some(out)
}

/// An affine matrix `[a b c d e f]`, the translation in millipoints.
pub(crate) type Affine = [f64; 6];

/// The identity.
pub(crate) const IDENTITY: Affine = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `m · n`: apply `n` first, then `m`.
pub(crate) fn mul(m: &Affine, n: &Affine) -> Affine {
    let [a, b, c, d, e, f] = *m;
    let [a2, b2, c2, d2, e2, f2] = *n;
    [
        a * a2 + c * b2,
        b * a2 + d * b2,
        a * c2 + c * d2,
        b * c2 + d * d2,
        a * e2 + c * f2 + e,
        b * e2 + d * f2 + f,
    ]
}

/// Applies a matrix to a point.
pub(crate) fn apply(m: &Affine, x: f64, y: f64) -> (f64, f64) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

/// Whether a matrix is the identity.
pub(crate) fn is_identity(m: &Affine) -> bool {
    *m == IDENTITY
}

/// A `transform` attribute. Translations are in points in the text and in
/// millipoints in the result.
pub(crate) fn transform(s: &str) -> Option<Affine> {
    let mut out = IDENTITY;
    let mut rest = s.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == ',');
    let mut n = 0usize;
    while !rest.is_empty() {
        n = n.saturating_add(1);
        if n > 256 {
            return None;
        }
        let open = rest.find('(')?;
        let close = rest.find(')')?;
        if close < open {
            return None;
        }
        let name = rest.get(..open)?.trim();
        let args = floats(rest.get(open.saturating_add(1)..close)?)?;
        let deg = |v: f64| v.to_radians();
        let m: Affine = match (name, args.as_slice()) {
            ("matrix", [a, b, c, d, e, f]) => [*a, *b, *c, *d, e * 1000.0, f * 1000.0],
            ("translate", [x]) => [1.0, 0.0, 0.0, 1.0, x * 1000.0, 0.0],
            ("translate", [x, y]) => [1.0, 0.0, 0.0, 1.0, x * 1000.0, y * 1000.0],
            ("scale", [s]) => [*s, 0.0, 0.0, *s, 0.0, 0.0],
            ("scale", [x, y]) => [*x, 0.0, 0.0, *y, 0.0, 0.0],
            ("rotate", [a]) => {
                let (s, c) = deg(*a).sin_cos();
                [c, s, -s, c, 0.0, 0.0]
            }
            ("rotate", [a, cx, cy]) => {
                let (s, c) = deg(*a).sin_cos();
                let (cx, cy) = (cx * 1000.0, cy * 1000.0);
                let t = [1.0, 0.0, 0.0, 1.0, cx, cy];
                let r = [c, s, -s, c, 0.0, 0.0];
                let u = [1.0, 0.0, 0.0, 1.0, -cx, -cy];
                mul(&mul(&t, &r), &u)
            }
            ("skewX", [a]) => [1.0, 0.0, deg(*a).tan(), 1.0, 0.0, 0.0],
            ("skewY", [a]) => [1.0, deg(*a).tan(), 0.0, 1.0, 0.0, 0.0],
            _ => return None,
        };
        if !m.iter().all(|v| v.is_finite()) {
            return None;
        }
        out = mul(&out, &m);
        rest = rest
            .get(close.saturating_add(1)..)?
            .trim_start_matches(|c: char| c.is_ascii_whitespace() || c == ',');
    }
    Some(out)
}

/// A point of path data, SVG space, millipoints.
pub(crate) type Pt = (i64, i64);

/// One element of parsed path data, still in SVG space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Seg {
    /// Start a subpath.
    Move(Pt),
    /// A line.
    Line(Pt),
    /// A cubic.
    Cubic(Pt, Pt, Pt),
    /// Close the subpath.
    Close,
}

/// Why path data was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PathDataError {
    /// Segments parsed before the error: SVG renders up to the error, and
    /// so does the reader, with a warning.
    pub partial: Vec<Seg>,
}

fn add(a: Pt, b: Pt) -> Pt {
    (
        a.0.saturating_add(b.0).clamp(-MP_CLAMP, MP_CLAMP),
        a.1.saturating_add(b.1).clamp(-MP_CLAMP, MP_CLAMP),
    )
}

fn round_pt(x: f64, y: f64) -> Pt {
    let c = |v: f64| {
        if v.is_finite() {
            (v.round() as i64).clamp(-MP_CLAMP, MP_CLAMP)
        } else {
            0
        }
    };
    (c(x), c(y))
}

/// Parses SVG path data into segments in millipoints, SVG space.
///
/// Every command is supported. Quadratics are elevated to cubics and arcs
/// converted to cubics (both round their control points to millipoints);
/// everything the writer produces (`M L H V C S Z`, absolute or relative)
/// is integer arithmetic and exact. An error keeps what was read before it,
/// as the SVG error-handling rules do.
///
/// # Errors
///
/// [`PathDataError`] at the first malformed token, or when `max_segments`
/// is exceeded.
pub(crate) fn path_data(d: &str, max_segments: usize) -> Result<Vec<Seg>, PathDataError> {
    let mut out: Vec<Seg> = Vec::new();
    let mut sc = Scan::new(d);
    let mut cur: Pt = (0, 0);
    let mut start: Pt = (0, 0);
    let mut prev_c2: Option<Pt> = None;
    let mut prev_q: Option<Pt> = None;
    let mut cmd: Option<u8> = None;
    let mut closed = false;
    macro_rules! fail {
        () => {
            return Err(PathDataError { partial: out })
        };
    }
    // After a close, a drawing command without a moveto starts at the
    // subpath's start: the model needs that moveto explicitly.
    let reopen = |out: &mut Vec<Seg>, closed: &mut bool, start: Pt| {
        if *closed {
            out.push(Seg::Move(start));
            *closed = false;
        }
    };
    loop {
        if out.len() > max_segments {
            fail!();
        }
        let c = match sc.letter() {
            Some(c) => c,
            None => {
                if sc.at_end() {
                    break;
                }
                // Implicit repetition of the previous command.
                match cmd {
                    Some(b'M') => b'L',
                    Some(b'm') => b'l',
                    Some(b'Z' | b'z') | None => fail!(),
                    Some(c) => c,
                }
            }
        };
        if cmd.is_none() && !matches!(c, b'M' | b'm') {
            fail!();
        }
        let rel = c.is_ascii_lowercase();
        let base = if rel { cur } else { (0, 0) };
        let pair = |sc: &mut Scan<'_>| -> Option<Pt> {
            let x = sc.mp()?;
            let y = sc.mp()?;
            Some(add(base, (x, y)))
        };
        match c.to_ascii_uppercase() {
            b'M' => {
                let Some(p) = pair(&mut sc) else { fail!() };
                out.push(Seg::Move(p));
                closed = false;
                cur = p;
                start = p;
                prev_c2 = None;
                prev_q = None;
            }
            b'L' => {
                let Some(p) = pair(&mut sc) else { fail!() };
                reopen(&mut out, &mut closed, start);
                out.push(Seg::Line(p));
                cur = p;
                prev_c2 = None;
                prev_q = None;
            }
            b'H' => {
                let Some(x) = sc.mp() else { fail!() };
                reopen(&mut out, &mut closed, start);
                let p = (
                    if rel {
                        add(cur, (x, 0)).0
                    } else {
                        x.clamp(-MP_CLAMP, MP_CLAMP)
                    },
                    cur.1,
                );
                out.push(Seg::Line(p));
                cur = p;
                prev_c2 = None;
                prev_q = None;
            }
            b'V' => {
                let Some(y) = sc.mp() else { fail!() };
                reopen(&mut out, &mut closed, start);
                let p = (
                    cur.0,
                    if rel {
                        add(cur, (0, y)).1
                    } else {
                        y.clamp(-MP_CLAMP, MP_CLAMP)
                    },
                );
                out.push(Seg::Line(p));
                cur = p;
                prev_c2 = None;
                prev_q = None;
            }
            b'C' => {
                let (Some(c1), Some(c2), Some(p)) = (pair(&mut sc), pair(&mut sc), pair(&mut sc))
                else {
                    fail!()
                };
                reopen(&mut out, &mut closed, start);
                out.push(Seg::Cubic(c1, c2, p));
                cur = p;
                prev_c2 = Some(c2);
                prev_q = None;
            }
            b'S' => {
                let (Some(c2), Some(p)) = (pair(&mut sc), pair(&mut sc)) else {
                    fail!()
                };
                reopen(&mut out, &mut closed, start);
                let c1 = match prev_c2 {
                    Some(q) => add(cur, (cur.0.saturating_sub(q.0), cur.1.saturating_sub(q.1))),
                    None => cur,
                };
                out.push(Seg::Cubic(c1, c2, p));
                cur = p;
                prev_c2 = Some(c2);
                prev_q = None;
            }
            b'Q' | b'T' => {
                let q = if c.eq_ignore_ascii_case(&b'Q') {
                    let Some(q) = pair(&mut sc) else { fail!() };
                    q
                } else {
                    match prev_q {
                        Some(q) => add(cur, (cur.0.saturating_sub(q.0), cur.1.saturating_sub(q.1))),
                        None => cur,
                    }
                };
                let Some(p) = pair(&mut sc) else { fail!() };
                reopen(&mut out, &mut closed, start);
                let (x0, y0) = (cur.0 as f64, cur.1 as f64);
                let (qx, qy) = (q.0 as f64, q.1 as f64);
                let (x1, y1) = (p.0 as f64, p.1 as f64);
                let c1 = round_pt(x0 + 2.0 / 3.0 * (qx - x0), y0 + 2.0 / 3.0 * (qy - y0));
                let c2 = round_pt(x1 + 2.0 / 3.0 * (qx - x1), y1 + 2.0 / 3.0 * (qy - y1));
                out.push(Seg::Cubic(c1, c2, p));
                cur = p;
                prev_c2 = None;
                prev_q = Some(q);
            }
            b'A' => {
                let (Some(rx), Some(ry), Some(rot)) = (sc.mp(), sc.mp(), sc.f64()) else {
                    fail!()
                };
                let (Some(large), Some(sweep)) = (sc.flag(), sc.flag()) else {
                    fail!()
                };
                let Some(p) = pair(&mut sc) else { fail!() };
                reopen(&mut out, &mut closed, start);
                arc(&mut out, cur, p, rx, ry, rot, large, sweep);
                cur = p;
                prev_c2 = None;
                prev_q = None;
            }
            b'Z' => {
                if !closed && !out.is_empty() {
                    out.push(Seg::Close);
                    closed = true;
                }
                cur = start;
                prev_c2 = None;
                prev_q = None;
            }
            _ => fail!(),
        }
        cmd = Some(c);
    }
    Ok(out)
}

/// An elliptical arc as cubics (SVG 1.1 implementation notes F.6).
#[allow(clippy::too_many_arguments, clippy::arithmetic_side_effects)]
fn arc(
    out: &mut Vec<Seg>,
    from: Pt,
    to: Pt,
    rx: i64,
    ry: i64,
    rot_deg: f64,
    large: bool,
    sweep: bool,
) {
    let (x1, y1) = (from.0 as f64, from.1 as f64);
    let (x2, y2) = (to.0 as f64, to.1 as f64);
    let (mut rx, mut ry) = ((rx as f64).abs(), (ry as f64).abs());
    if from == to {
        return;
    }
    if rx == 0.0 || ry == 0.0 || !rot_deg.is_finite() {
        out.push(Seg::Line(to));
        return;
    }
    let (sin, cos) = rot_deg.to_radians().sin_cos();
    let dx = (x1 - x2) / 2.0;
    let dy = (y1 - y2) / 2.0;
    let x1p = cos * dx + sin * dy;
    let y1p = -sin * dx + cos * dy;
    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let mut coef = if den == 0.0 {
        0.0
    } else {
        (num / den).max(0.0).sqrt()
    };
    if large == sweep {
        coef = -coef;
    }
    let cxp = coef * rx * y1p / ry;
    let cyp = -coef * ry * x1p / rx;
    let cx = cos * cxp - sin * cyp + (x1 + x2) / 2.0;
    let cy = sin * cxp + cos * cyp + (y1 + y2) / 2.0;
    let angle = |ux: f64, uy: f64, vx: f64, vy: f64| {
        let a = (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
        if a.is_finite() { a } else { 0.0 }
    };
    let ux = (x1p - cxp) / rx;
    let uy = (y1p - cyp) / ry;
    let vx = (-x1p - cxp) / rx;
    let vy = (-y1p - cyp) / ry;
    let theta1 = angle(1.0, 0.0, ux, uy);
    let mut dtheta = angle(ux, uy, vx, vy);
    if !sweep && dtheta > 0.0 {
        dtheta -= std::f64::consts::TAU;
    } else if sweep && dtheta < 0.0 {
        dtheta += std::f64::consts::TAU;
    }
    let n = (dtheta.abs() / std::f64::consts::FRAC_PI_2)
        .ceil()
        .clamp(1.0, 4.0) as usize;
    let step = dtheta / n as f64;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let point = |t: f64| {
        let (s, c) = t.sin_cos();
        (
            cx + rx * c * cos - ry * s * sin,
            cy + rx * c * sin + ry * s * cos,
        )
    };
    let deriv = |t: f64| {
        let (s, c) = t.sin_cos();
        (-rx * s * cos - ry * c * sin, -rx * s * sin + ry * c * cos)
    };
    let mut t = theta1;
    for i in 0..n {
        let t2 = t + step;
        let (p0x, p0y) = point(t);
        let (d0x, d0y) = deriv(t);
        let (p3x, p3y) = point(t2);
        let (d3x, d3y) = deriv(t2);
        let c1 = round_pt(p0x + k * d0x, p0y + k * d0y);
        let c2 = round_pt(p3x - k * d3x, p3y - k * d3y);
        let end = if i + 1 == n { to } else { round_pt(p3x, p3y) };
        out.push(Seg::Cubic(c1, c2, end));
        t = t2;
    }
}

/// The CSS/SVG colour keywords (CSS Color Module Level 3, §4.3).
const NAMED: &[(&str, u32)] = &[
    ("aliceblue", 0xf0f8ff),
    ("antiquewhite", 0xfaebd7),
    ("aqua", 0x00ffff),
    ("aquamarine", 0x7fffd4),
    ("azure", 0xf0ffff),
    ("beige", 0xf5f5dc),
    ("bisque", 0xffe4c4),
    ("black", 0x000000),
    ("blanchedalmond", 0xffebcd),
    ("blue", 0x0000ff),
    ("blueviolet", 0x8a2be2),
    ("brown", 0xa52a2a),
    ("burlywood", 0xdeb887),
    ("cadetblue", 0x5f9ea0),
    ("chartreuse", 0x7fff00),
    ("chocolate", 0xd2691e),
    ("coral", 0xff7f50),
    ("cornflowerblue", 0x6495ed),
    ("cornsilk", 0xfff8dc),
    ("crimson", 0xdc143c),
    ("cyan", 0x00ffff),
    ("darkblue", 0x00008b),
    ("darkcyan", 0x008b8b),
    ("darkgoldenrod", 0xb8860b),
    ("darkgray", 0xa9a9a9),
    ("darkgreen", 0x006400),
    ("darkgrey", 0xa9a9a9),
    ("darkkhaki", 0xbdb76b),
    ("darkmagenta", 0x8b008b),
    ("darkolivegreen", 0x556b2f),
    ("darkorange", 0xff8c00),
    ("darkorchid", 0x9932cc),
    ("darkred", 0x8b0000),
    ("darksalmon", 0xe9967a),
    ("darkseagreen", 0x8fbc8f),
    ("darkslateblue", 0x483d8b),
    ("darkslategray", 0x2f4f4f),
    ("darkslategrey", 0x2f4f4f),
    ("darkturquoise", 0x00ced1),
    ("darkviolet", 0x9400d3),
    ("deeppink", 0xff1493),
    ("deepskyblue", 0x00bfff),
    ("dimgray", 0x696969),
    ("dimgrey", 0x696969),
    ("dodgerblue", 0x1e90ff),
    ("firebrick", 0xb22222),
    ("floralwhite", 0xfffaf0),
    ("forestgreen", 0x228b22),
    ("fuchsia", 0xff00ff),
    ("gainsboro", 0xdcdcdc),
    ("ghostwhite", 0xf8f8ff),
    ("gold", 0xffd700),
    ("goldenrod", 0xdaa520),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xadff2f),
    ("grey", 0x808080),
    ("honeydew", 0xf0fff0),
    ("hotpink", 0xff69b4),
    ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082),
    ("ivory", 0xfffff0),
    ("khaki", 0xf0e68c),
    ("lavender", 0xe6e6fa),
    ("lavenderblush", 0xfff0f5),
    ("lawngreen", 0x7cfc00),
    ("lemonchiffon", 0xfffacd),
    ("lightblue", 0xadd8e6),
    ("lightcoral", 0xf08080),
    ("lightcyan", 0xe0ffff),
    ("lightgoldenrodyellow", 0xfafad2),
    ("lightgray", 0xd3d3d3),
    ("lightgreen", 0x90ee90),
    ("lightgrey", 0xd3d3d3),
    ("lightpink", 0xffb6c1),
    ("lightsalmon", 0xffa07a),
    ("lightseagreen", 0x20b2aa),
    ("lightskyblue", 0x87cefa),
    ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xb0c4de),
    ("lightyellow", 0xffffe0),
    ("lime", 0x00ff00),
    ("limegreen", 0x32cd32),
    ("linen", 0xfaf0e6),
    ("magenta", 0xff00ff),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66cdaa),
    ("mediumblue", 0x0000cd),
    ("mediumorchid", 0xba55d3),
    ("mediumpurple", 0x9370db),
    ("mediumseagreen", 0x3cb371),
    ("mediumslateblue", 0x7b68ee),
    ("mediumspringgreen", 0x00fa9a),
    ("mediumturquoise", 0x48d1cc),
    ("mediumvioletred", 0xc71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xf5fffa),
    ("mistyrose", 0xffe4e1),
    ("moccasin", 0xffe4b5),
    ("navajowhite", 0xffdead),
    ("navy", 0x000080),
    ("oldlace", 0xfdf5e6),
    ("olive", 0x808000),
    ("olivedrab", 0x6b8e23),
    ("orange", 0xffa500),
    ("orangered", 0xff4500),
    ("orchid", 0xda70d6),
    ("palegoldenrod", 0xeee8aa),
    ("palegreen", 0x98fb98),
    ("paleturquoise", 0xafeeee),
    ("palevioletred", 0xdb7093),
    ("papayawhip", 0xffefd5),
    ("peachpuff", 0xffdab9),
    ("peru", 0xcd853f),
    ("pink", 0xffc0cb),
    ("plum", 0xdda0dd),
    ("powderblue", 0xb0e0e6),
    ("purple", 0x800080),
    ("red", 0xff0000),
    ("rosybrown", 0xbc8f8f),
    ("royalblue", 0x4169e1),
    ("saddlebrown", 0x8b4513),
    ("salmon", 0xfa8072),
    ("sandybrown", 0xf4a460),
    ("seagreen", 0x2e8b57),
    ("seashell", 0xfff5ee),
    ("sienna", 0xa0522d),
    ("silver", 0xc0c0c0),
    ("skyblue", 0x87ceeb),
    ("slateblue", 0x6a5acd),
    ("slategray", 0x708090),
    ("slategrey", 0x708090),
    ("snow", 0xfffafa),
    ("springgreen", 0x00ff7f),
    ("steelblue", 0x4682b4),
    ("tan", 0xd2b48c),
    ("teal", 0x008080),
    ("thistle", 0xd8bfd8),
    ("tomato", 0xff6347),
    ("turquoise", 0x40e0d0),
    ("violet", 0xee82ee),
    ("wheat", 0xf5deb3),
    ("white", 0xffffff),
    ("whitesmoke", 0xf5f5f5),
    ("yellow", 0xffff00),
    ("yellowgreen", 0x9acd32),
];

/// A colour value: `#rgb`, `#rrggbb`, `#rrggbbaa` (the writer's key-stop
/// spelling), `rgb()`/`rgba()`, or a keyword. Opaque unless the text says
/// otherwise.
pub(crate) fn colour(s: &str) -> Option<Rgba8> {
    let s = s.trim();
    if let Some(h) = s.strip_prefix('#') {
        let hexd = |c: u8| (c as char).to_digit(16).and_then(|v| u8::try_from(v).ok());
        let b = h.as_bytes();
        let d: Option<Vec<u8>> = b.iter().map(|c| hexd(*c)).collect();
        let d = d?;
        let pair = |i: usize| -> Option<u8> {
            let hi = *d.get(i)?;
            let lo = *d.get(i.checked_add(1)?)?;
            hi.checked_mul(16)?.checked_add(lo)
        };
        let dup = |i: usize| -> Option<u8> { d.get(i)?.checked_mul(17) };
        return match d.len() {
            3 => Some(Rgba8 {
                r: dup(0)?,
                g: dup(1)?,
                b: dup(2)?,
                a: 255,
            }),
            4 => Some(Rgba8 {
                r: dup(0)?,
                g: dup(1)?,
                b: dup(2)?,
                a: dup(3)?,
            }),
            6 => Some(Rgba8 {
                r: pair(0)?,
                g: pair(2)?,
                b: pair(4)?,
                a: 255,
            }),
            8 => Some(Rgba8 {
                r: pair(0)?,
                g: pair(2)?,
                b: pair(4)?,
                a: pair(6)?,
            }),
            _ => None,
        };
    }
    let lower = s.to_ascii_lowercase();
    if let Some(args) = lower
        .strip_prefix("rgba(")
        .or_else(|| lower.strip_prefix("rgb("))
        .and_then(|r| r.strip_suffix(')'))
    {
        let parts: Vec<&str> = args
            .split(|c: char| c == ',' || c == '/' || c.is_ascii_whitespace())
            .filter(|p| !p.is_empty())
            .collect();
        let chan = |p: &str| -> Option<u8> {
            let v = if let Some(pc) = p.strip_suffix('%') {
                float(pc)? * 2.55
            } else {
                float(p)?
            };
            Some(v.round().clamp(0.0, 255.0) as u8)
        };
        let alpha = |p: &str| -> Option<u8> {
            let v = if let Some(pc) = p.strip_suffix('%') {
                float(pc)? / 100.0
            } else {
                float(p)?
            };
            Some((v.clamp(0.0, 1.0) * 255.0).round() as u8)
        };
        return match parts.as_slice() {
            [r, g, b] => Some(Rgba8 {
                r: chan(r)?,
                g: chan(g)?,
                b: chan(b)?,
                a: 255,
            }),
            [r, g, b, a] => Some(Rgba8 {
                r: chan(r)?,
                g: chan(g)?,
                b: chan(b)?,
                a: alpha(a)?,
            }),
            _ => None,
        };
    }
    NAMED
        .binary_search_by(|(n, _)| (*n).cmp(lower.as_str()))
        .ok()
        .and_then(|i| NAMED.get(i))
        .map(|(_, v)| Rgba8 {
            r: ((v >> 16) & 0xff) as u8,
            g: ((v >> 8) & 0xff) as u8,
            b: (v & 0xff) as u8,
            a: 255,
        })
}

/// Standard base64, padding optional, whitespace ignored.
pub(crate) fn base64(s: &str, max: usize) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(u32::from(match c {
            b'A'..=b'Z' => c.checked_sub(b'A')?,
            b'a'..=b'z' => c.checked_sub(b'a')?.checked_add(26)?,
            b'0'..=b'9' => c.checked_sub(b'0')?.checked_add(52)?,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        }))
    };
    let mut out = Vec::with_capacity((s.len() / 4).saturating_mul(3).min(max));
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    let mut pad = false;
    for c in s.bytes() {
        if c.is_ascii_whitespace() {
            continue;
        }
        if c == b'=' {
            pad = true;
            continue;
        }
        if pad {
            return None;
        }
        acc = (acc << 6) | val(c)?;
        bits = bits.saturating_add(6);
        if bits >= 8 {
            bits = bits.saturating_sub(8);
            out.push(((acc >> bits) & 0xff) as u8);
            if out.len() > max {
                return None;
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_are_correctly_rounded() {
        for v in [
            0.35f64,
            -0.4,
            0.6,
            0.1,
            0.123_456_789_012_345_67,
            1e-7,
            123.456,
        ] {
            let s = crate::svg::num::f64s_exact(v);
            assert_eq!(float(&s), Some(v), "{s}");
        }
        assert_eq!(float(".35"), Some(0.35));
        assert_eq!(float("1e-30"), Some(1e-30));
        assert_eq!(float("35e-2"), Some(0.35));
    }

    #[test]
    fn numbers_are_exact_millipoints() {
        assert_eq!(mp("12.5"), Some(12_500));
        assert_eq!(mp("-.5"), Some(-500));
        assert_eq!(mp("1e3"), Some(1_000_000));
        assert_eq!(mp("0.0005"), Some(1));
        assert_eq!(mp("-0.0005"), Some(-1));
        assert_eq!(mp("595.276"), Some(595_276));
        assert_eq!(mp("1."), Some(1_000));
        assert_eq!(mp("."), None);
        assert_eq!(mp("1x"), None);
        assert_eq!(mp("1e999999999"), Some(MP_CLAMP));
        assert_eq!(mps("1.5.5-2"), Some(vec![1_500, 500, -2_000]));
    }

    #[test]
    fn path_data_writer_syntax() {
        let segs = path_data("M105.123 266.565h678.75v81.75h-678.75v-81.75z", 100).unwrap();
        assert_eq!(
            segs,
            vec![
                Seg::Move((105_123, 266_565)),
                Seg::Line((783_873, 266_565)),
                Seg::Line((783_873, 348_315)),
                Seg::Line((105_123, 348_315)),
                Seg::Line((105_123, 266_565)),
                Seg::Close,
            ]
        );
        let s = path_data("M0 0c1 1 2 2 3 3s4 4 5 5zl1 1", 100).unwrap();
        assert_eq!(
            s.get(2),
            Some(&Seg::Cubic((4_000, 4_000), (7_000, 7_000), (8_000, 8_000)))
        );
        assert_eq!(s.get(4), Some(&Seg::Move((0, 0))));
        assert!(path_data("M0 0 L1", 100).is_err());
        assert!(path_data("L1 1", 100).is_err());
        let arc = path_data("M0 0A10 10 0 1 1 20 0", 100).unwrap();
        assert!(arc.len() >= 3);
        assert!(matches!(arc.last(), Some(Seg::Cubic(_, _, (20_000, 0)))));
        // Flags without separators.
        assert!(path_data("M0 0a10 10 0 0110 10", 100).is_ok());
    }

    #[test]
    fn transforms_and_colours() {
        let m = transform("translate(10 20) scale(2)").unwrap();
        assert_eq!(m, [2.0, 0.0, 0.0, 2.0, 10_000.0, 20_000.0]);
        assert!(transform("matrix(1 2)").is_none());
        assert_eq!(
            colour("#c33").map(|c| (c.r, c.g, c.b)),
            Some((0xcc, 0x33, 0x33))
        );
        assert_eq!(colour("#11223380").map(|c| c.a), Some(0x80));
        assert_eq!(colour("rgb(255, 0, 0)").map(|c| c.r), Some(255));
        assert_eq!(colour("Red").map(|c| c.r), Some(255));
        assert!(colour("#12").is_none());
        assert!(NAMED.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!(base64("AAEC", 10), Some(vec![0, 1, 2]));
        assert_eq!(
            base64(&crate::svg::xml::base64(b"hello!?"), 100),
            Some(b"hello!?".to_vec())
        );
    }
}
