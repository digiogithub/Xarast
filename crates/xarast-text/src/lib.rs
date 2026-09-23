//! Font handling, shaping and text layout for Xarast.
//!
//! * [`FontDb`] — the font database: system enumeration through `fontique`,
//!   matching by family/weight/style/stretch, the missing-font substitution
//!   ladder, script fallback with a preference override, faces embedded in a
//!   document (shadowing system faces of the same name), and shared,
//!   LRU-cached face data.
//!
//! Nothing outside this crate names a `parley`, `fontique` or `skrifa` type:
//! the whole Linebender text stack is pre-1.0 and sits behind this API.
//!
//! See `docs/phases/phase-09-text.md` and `docs/memory/text.md`.

pub mod font;
pub mod style;

pub use font::{
    FaceData, FaceId, FaceInfo, FontDb, FontDbOptions, FontError, FontMatch, FontSubstitution,
    GenericName, ScriptTag, SubstitutionReason, Synthesis,
};
pub use style::{
    Direction, FontFeature, FontQuery, FontStyle, FontVariation, Justification, LineSpacing,
    ManualKern, ParagraphStyle, StoryInput, StoryMode, StyleRange, TabKind, TabStop, TextScript,
};
