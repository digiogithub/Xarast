//! The `FIXED24` colour-component codec and its inherit sentinel.

use core::fmt;

/// A signed fixed-point number with 24 fractional bits.
///
/// This is how `.xar` stores every colour component, normalised to
/// `0.0..=1.0` — including HSV's hue, which is `0..1` rather than degrees.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct Fixed24(pub i32);

impl Fixed24 {
    /// The value 1.0.
    pub const ONE: Fixed24 = Fixed24(1 << 24);
    /// The value 0.0.
    pub const ZERO: Fixed24 = Fixed24(0);

    /// `0xF800_0000`, which reads as `-8.0` and means **"inherit this
    /// component from the parent colour"**.
    ///
    /// Read literally it produces absurd colours, and it is the single most
    /// likely thing for a new `.xar` reader to get wrong. Making it a named
    /// constant with a predicate, and making [`Fixed24::to_f32`] return an
    /// [`Option`], turns forgetting the check from a rendering bug into a
    /// compile error at the call site.
    pub const INHERIT: Fixed24 = Fixed24(0xF800_0000u32 as i32);

    /// Whether this is the inherit sentinel.
    #[inline]
    #[must_use]
    pub const fn is_inherit(self) -> bool {
        self.0 == Fixed24::INHERIT.0
    }

    /// The component value clamped to `0.0..=1.0`, or `None` for the inherit
    /// sentinel.
    ///
    /// Clamping happens here because this is the only entry point that can
    /// see an out-of-range component: every
    /// [`ColourValue`](crate::ColourValue) is clamped on construction, so no
    /// colour downstream can hold a NaN or an out-of-range channel.
    #[inline]
    #[must_use]
    pub fn to_f32(self) -> Option<f32> {
        if self.is_inherit() {
            return None;
        }
        Some((self.0 as f32 / 16_777_216.0).clamp(0.0, 1.0))
    }

    /// The raw value as an `f32`, without clamping and without the sentinel
    /// check. For diagnostics such as `xar-dump`, which must show what the
    /// file actually said.
    #[inline]
    #[must_use]
    pub fn to_f32_raw(self) -> f32 {
        self.0 as f32 / 16_777_216.0
    }

    /// Encodes a component. Values outside `0.0..=1.0` are clamped, and a NaN
    /// becomes zero; the sentinel is never produced by this function, so a
    /// round trip through it always yields a concrete component.
    #[inline]
    #[must_use]
    pub fn from_f32(v: f32) -> Fixed24 {
        let v = if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
        Fixed24((v * 16_777_216.0).round() as i32)
    }
}

impl fmt::Debug for Fixed24 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_inherit() {
            write!(f, "Fixed24(INHERIT)")
        } else {
            write!(f, "Fixed24({} = {})", self.0, self.to_f32_raw())
        }
    }
}
