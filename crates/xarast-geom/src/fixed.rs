//! Fixed-point codecs for the `.xar` boundary.

use core::fmt;

/// A signed fixed-point number with 16 fractional bits.
///
/// This is how `.xar` stores matrix coefficients and angles in radians. It is
/// a **codec**, not a working type: a value read through it is converted to
/// `f64` immediately and everything downstream stays in `f64`. Composing
/// quantised matrices accumulates error — a rotation carries up to about 4.5
/// arcseconds of it — which is why [`Matrix`](crate::Matrix) keeps its linear
/// part in `f64` and quantises exactly once, on write.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct Fixed16(pub i32);

impl Fixed16 {
    /// The value 1.0.
    pub const ONE: Fixed16 = Fixed16(1 << 16);
    /// The value 0.0.
    pub const ZERO: Fixed16 = Fixed16(0);
    /// The size of one representable step, `2^-16`.
    pub const EPSILON: f64 = 1.0 / 65536.0;

    /// Exact: a 32-bit integer divided by a power of two is exact in `f64`.
    #[inline]
    #[must_use]
    pub const fn to_f64(self) -> f64 {
        self.0 as f64 / 65536.0
    }

    /// Rounds half away from zero and saturates. A NaN becomes zero, for the
    /// same reason as in [`Mp::from_f64_round`](crate::Mp::from_f64_round).
    #[inline]
    #[must_use]
    pub fn from_f64_round(v: f64) -> Fixed16 {
        if v.is_nan() {
            return Fixed16::ZERO;
        }
        let r = (v * 65536.0).round();
        if r <= i32::MIN as f64 {
            Fixed16(i32::MIN)
        } else if r >= i32::MAX as f64 {
            Fixed16(i32::MAX)
        } else {
            Fixed16(r as i32)
        }
    }

    /// The `f64` value rounded to the nearest representable `Fixed16`, without
    /// leaving `f64`. This is the quantisation `Matrix::quantise_fixed16` and
    /// the deviation tests both need.
    #[inline]
    #[must_use]
    pub fn quantise(v: f64) -> f64 {
        Fixed16::from_f64_round(v).to_f64()
    }
}

impl fmt::Debug for Fixed16 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fixed16({} = {})", self.0, self.to_f64())
    }
}
