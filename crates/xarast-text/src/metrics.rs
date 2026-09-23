//! Font metrics: per-face vertical metrics in font units, and the
//! `FontMetrics` measuring interface the document layer formats with
//! (`research/02 §10.11`, the replacement for Xara's `FormatRegion`).

use xarast_geom::Mp;

use crate::font::{FaceData, FaceId};
use crate::style::FontQuery;

/// A face's vertical metrics in **font units**, at one variation location.
/// Scale by `size / units_per_em`; see [`FaceMetrics::scaled`].
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct FaceMetrics {
    /// Units per em.
    pub units_per_em: u16,
    /// Distance from the baseline to the top of the line box; positive.
    pub ascent: f32,
    /// Distance from the baseline to the bottom of the line box; positive
    /// (the font tables store it negative).
    pub descent: f32,
    /// Recommended extra gap between lines.
    pub leading: f32,
    /// Height of capitals, when the font says.
    pub cap_height: Option<f32>,
    /// Height of lower-case x, when the font says.
    pub x_height: Option<f32>,
    /// Underline position (negative is below the baseline) and thickness.
    pub underline: Option<(f32, f32)>,
}

/// [`FaceMetrics`] at a size, in millipoints.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct ScaledMetrics {
    /// Ascent, positive.
    pub ascent: Mp,
    /// Descent, positive below the baseline.
    pub descent: Mp,
    /// Line gap.
    pub leading: Mp,
}

impl FaceMetrics {
    /// Reads the metrics of `data` at the normalised variation location
    /// `coords` (F2Dot14 bits, empty for the default instance).
    #[must_use]
    pub fn read(data: &FaceData, coords: &[i16]) -> Option<FaceMetrics> {
        use skrifa::MetadataProvider;
        use skrifa::instance::{LocationRef, NormalizedCoord, Size};
        let font = data.font_ref()?;
        let norm: Vec<NormalizedCoord> = coords
            .iter()
            .map(|&c| NormalizedCoord::from_bits(c))
            .collect();
        let m = font.metrics(Size::unscaled(), LocationRef::new(&norm));
        Some(FaceMetrics {
            units_per_em: m.units_per_em.max(1),
            ascent: m.ascent,
            descent: -m.descent,
            leading: m.leading,
            cap_height: m.cap_height,
            x_height: m.x_height,
            underline: m.underline.map(|d| (d.offset, d.thickness)),
        })
    }

    /// The metrics at font size `size`, rounded half away from zero.
    #[must_use]
    pub fn scaled(&self, size: Mp) -> ScaledMetrics {
        let k = size.to_f64() / f64::from(self.units_per_em.max(1));
        ScaledMetrics {
            ascent: Mp::from_f64_round(f64::from(self.ascent) * k),
            descent: Mp::from_f64_round(f64::from(self.descent) * k),
            leading: Mp::from_f64_round(f64::from(self.leading) * k),
        }
    }
}

/// Measurements of one character in context-free isolation.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct CharMetrics {
    /// The face that renders it (after fallback).
    pub face: FaceId,
    /// Its glyph in that face (0 when nothing covers it).
    pub glyph: u32,
    /// Advance without tracking or kerning.
    pub advance: Mp,
    /// Ascent of the face at this size.
    pub ascent: Mp,
    /// Descent of the face at this size, positive.
    pub descent: Mp,
    /// The em width tracking and manual kerns are measured in.
    pub em_width: Mp,
}

/// Measuring without drawing: what the document layer needs to format a
/// story without seeing a shaping engine. Implemented by
/// [`Shaper`](crate::Shaper).
pub trait FontMetrics {
    /// Metrics of `c` in `font` at `size` and horizontal `aspect`.
    fn char_metrics(&self, font: &FontQuery, size: Mp, aspect: f32, c: char)
    -> Option<CharMetrics>;

    /// The font's pair kerning between `left` and `right` at `size`: the
    /// change in `left`'s advance when kerning is on.
    fn kern_pair(&self, font: &FontQuery, size: Mp, left: char, right: char) -> Mp;
}
