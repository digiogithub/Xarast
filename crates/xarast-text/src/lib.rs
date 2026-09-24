//! Font handling, shaping and text layout for Xarast.
//!
//! * [`FontDb`] — the font database: system enumeration through `fontique`,
//!   matching by family/weight/style/stretch, the missing-font substitution
//!   ladder, script fallback with a preference override, faces embedded in a
//!   document (shadowing system faces of the same name), and shared,
//!   LRU-cached face data.
//! * [`Shaper`] — shaping through `parley` (HarfBuzz-class shaping by
//!   `harfrust`), UAX #14 line breaking through ICU4X, UAX #9 reordering,
//!   Xara-style justification and line spacing, in millipoints.
//! * [`path`] — text on a path: a laid-out story carried onto a curve by
//!   arc length ([`PathFit`]).
//! * [`FontDb::glyph_outline`] — cached glyph outlines as `kurbo::BezPath`,
//!   what the renderer and convert-to-shapes draw.
//!
//! Nothing outside this crate names a `parley`, `fontique` or `skrifa` type:
//! the whole Linebender text stack is pre-1.0 and sits behind this API.
//!
//! See `docs/phases/phase-09-text.md` and `docs/memory/text.md`.

pub mod font;
mod layout;
pub mod metrics;
mod outline;
pub mod path;
pub mod segment;
mod shape;
pub mod style;

pub use font::{
    FaceData, FaceId, FaceInfo, FontDb, FontDbOptions, FontError, FontMatch, FontSubstitution,
    GenericName, ScriptTag, SubstitutionReason, Synthesis,
};
pub use layout::{DEFAULT_TAB_INTERVAL, GlyphRun, LaidCluster, LaidLine, Layout, PlacedGlyph};
pub use metrics::{CharMetrics, FaceMetrics, FontMetrics, ScaledMetrics};
pub use path::{PLAIN_SHEAR, PathFit, PathFitStyle, TextPath};
pub use segment::{WordSegment, next_grapheme, prev_grapheme, word_segments};
pub use shape::{NOMINAL_SIZE, Shaper};
pub use style::{
    Direction, FontFeature, FontQuery, FontStyle, FontVariation, Justification, LineSpacing,
    ManualKern, ParagraphStyle, StoryInput, StoryMode, StyleRange, TabKind, TabStop, TextScript,
};
