//! The conversion context: every conversion between models, and every
//! resolution to a displayable value, goes through one of these.

use crate::{Colour, ColourModel, ColourTable, ColourValue, Rgba8};

/// Converts between colour models and resolves colours for display.
///
/// Document scoped rather than a set of free functions because it is where
/// colour management will live (ICC profiles, phase 15): today it holds
/// nothing and every conversion is the original's uncalibrated one
/// (`research/02 §5.10.1`), but a caller that goes through a context will
/// not need to change when it holds a profile.
///
/// It never assumes "everything is sRGB" in what it stores: it only
/// *produces* sRGB, as the canonical resolved value.
#[derive(Clone, Debug, Default)]
pub struct ColourContext {
    _reserved: (),
}

impl ColourContext {
    /// The uncalibrated context every `.xar` document uses.
    #[must_use]
    pub fn uncalibrated() -> ColourContext {
        ColourContext::default()
    }

    /// The 8-bit sRGB value of `v`, quantised as the original quantises
    /// ([`ColourValue::to_rgba8_packed`]), so a flat palette colour paints
    /// exactly the cached RGB its `.xar` record carries.
    #[inline]
    #[must_use]
    pub fn srgb_of(&self, v: ColourValue) -> Rgba8 {
        v.to_rgba8_packed()
    }

    /// Converts `v` into model `to`.
    #[inline]
    #[must_use]
    pub fn convert(&self, v: ColourValue, to: ColourModel) -> ColourValue {
        v.to_model(to)
    }

    /// Resolves an attribute colour through the palette to 8-bit sRGB.
    #[inline]
    #[must_use]
    pub fn resolve(&self, c: &Colour, table: &ColourTable) -> Rgba8 {
        self.srgb_of(c.resolve(table))
    }
}
