//! The `.xar` predefined colours, addressed by negative reference.

use crate::ColourValue;

/// A colour the format refers to by a negative reference rather than by a
/// record number.
///
/// In `.xar`, a colour reference of `0` means "none / error", a positive
/// value is a record number, and a negative value indexes this table.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
#[repr(i32)]
pub enum BuiltinColour {
    /// No colour at all — not black, and not transparent black either: the
    /// object simply has no fill or no line.
    None = -1,
    /// Black.
    Black = -2,
    /// White.
    White = -3,
    /// Red.
    Red = -4,
    /// Green.
    Green = -5,
    /// Blue.
    Blue = -6,
    /// Cyan.
    Cyan = -7,
    /// Magenta.
    Magenta = -8,
    /// Yellow.
    Yellow = -9,
    /// The CMYK key plate's black, which is a CMYK colour rather than an RGB
    /// one and therefore separates differently from [`BuiltinColour::Black`].
    CmykKey = -10,
}

impl BuiltinColour {
    /// Decodes a negative colour reference, or `None` when the value is not
    /// one of the ten defined ones.
    ///
    /// A positive value is a record number and a zero means none, so both
    /// return `None` here; the caller distinguishes them.
    #[must_use]
    pub const fn from_ref(v: i32) -> Option<BuiltinColour> {
        Some(match v {
            -1 => BuiltinColour::None,
            -2 => BuiltinColour::Black,
            -3 => BuiltinColour::White,
            -4 => BuiltinColour::Red,
            -5 => BuiltinColour::Green,
            -6 => BuiltinColour::Blue,
            -7 => BuiltinColour::Cyan,
            -8 => BuiltinColour::Magenta,
            -9 => BuiltinColour::Yellow,
            -10 => BuiltinColour::CmykKey,
            _ => return None,
        })
    }

    /// The reference value this colour is addressed by.
    #[inline]
    #[must_use]
    pub const fn to_ref(self) -> i32 {
        self as i32
    }

    /// The colour's value, or `None` for [`BuiltinColour::None`], which means
    /// "no colour" and must not be confused with black or with transparent.
    #[must_use]
    pub fn value(self) -> Option<ColourValue> {
        Some(match self {
            BuiltinColour::None => return None,
            BuiltinColour::Black => ColourValue::rgb(0.0, 0.0, 0.0),
            BuiltinColour::White => ColourValue::rgb(1.0, 1.0, 1.0),
            BuiltinColour::Red => ColourValue::rgb(1.0, 0.0, 0.0),
            BuiltinColour::Green => ColourValue::rgb(0.0, 1.0, 0.0),
            BuiltinColour::Blue => ColourValue::rgb(0.0, 0.0, 1.0),
            BuiltinColour::Cyan => ColourValue::rgb(0.0, 1.0, 1.0),
            BuiltinColour::Magenta => ColourValue::rgb(1.0, 0.0, 1.0),
            BuiltinColour::Yellow => ColourValue::rgb(1.0, 1.0, 0.0),
            // Key is defined in CMYK, not RGB: it is the black separation
            // plate, and converting it to RGB here would lose the fact that
            // it must print on one plate rather than four.
            BuiltinColour::CmykKey => ColourValue::cmyk(0.0, 0.0, 0.0, 1.0),
        })
    }

    /// Every built-in colour, in reference order.
    pub const ALL: [BuiltinColour; 10] = [
        BuiltinColour::None,
        BuiltinColour::Black,
        BuiltinColour::White,
        BuiltinColour::Red,
        BuiltinColour::Green,
        BuiltinColour::Blue,
        BuiltinColour::Cyan,
        BuiltinColour::Magenta,
        BuiltinColour::Yellow,
        BuiltinColour::CmykKey,
    ];
}
