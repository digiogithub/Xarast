//! Glyph outlines: `skrifa`'s outline pen into `kurbo::BezPath`, cached.
//!
//! Outlines are extracted **once per glyph at unit scale** — in font units, y
//! up, unhinted — and cached by `(face, glyph, variation coordinates)`.
//! Drawing a glyph at a size is a transform
//! (`GlyphRun::glyph_transform`), never a
//! re-extraction, so converting a story at several sizes costs one
//! extraction per distinct glyph (phase 9, W9.6).

use std::collections::HashMap;
use std::sync::Arc;

use kurbo::BezPath;
use skrifa::instance::{LocationRef, NormalizedCoord, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{GlyphId, MetadataProvider, Tag};

use crate::font::{FaceData, FaceId, FontDb};
use crate::style::FontVariation;

/// Entries kept before the cache is cleared wholesale. A story of a few
/// thousand distinct glyphs fits many times over; clearing rather than
/// evicting keeps lookups a plain hash probe.
const MAX_ENTRIES: usize = 1 << 16;

type Entry = (Box<[i16]>, Option<Arc<BezPath>>);

/// Cache of extracted outlines.
#[derive(Default)]
pub(crate) struct OutlineCache {
    map: HashMap<(FaceId, u32), Vec<Entry>>,
    len: usize,
}

impl OutlineCache {
    fn get(&self, face: FaceId, glyph: u32, coords: &[i16]) -> Option<Option<Arc<BezPath>>> {
        self.map
            .get(&(face, glyph))?
            .iter()
            .find(|(c, _)| &c[..] == coords)
            .map(|(_, p)| p.clone())
    }

    fn insert(&mut self, face: FaceId, glyph: u32, coords: &[i16], path: Option<Arc<BezPath>>) {
        if self.len >= MAX_ENTRIES {
            self.map.clear();
            self.len = 0;
        }
        self.map
            .entry((face, glyph))
            .or_default()
            .push((Box::from(coords), path));
        self.len += 1;
    }
}

struct BezPen(BezPath);

impl OutlinePen for BezPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to((f64::from(x), f64::from(y)));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to((f64::from(x), f64::from(y)));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0.quad_to(
            (f64::from(cx0), f64::from(cy0)),
            (f64::from(x), f64::from(y)),
        );
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.curve_to(
            (f64::from(cx0), f64::from(cy0)),
            (f64::from(cx1), f64::from(cy1)),
            (f64::from(x), f64::from(y)),
        );
    }
    fn close(&mut self) {
        self.0.close_path();
    }
}

/// Extracts one outline, uncached. `None` when the face does not parse or
/// has no such glyph; an empty path for a glyph with no contours (a space).
fn extract(data: &FaceData, glyph: u32, coords: &[i16]) -> Option<BezPath> {
    let font = data.font_ref()?;
    let outlines = font.outline_glyphs();
    let g = outlines.get(GlyphId::new(glyph))?;
    let norm: Vec<NormalizedCoord> = coords
        .iter()
        .map(|&c| NormalizedCoord::from_bits(c))
        .collect();
    let mut pen = BezPen(BezPath::new());
    g.draw(
        DrawSettings::unhinted(Size::unscaled(), LocationRef::new(&norm)),
        &mut pen,
    )
    .ok()?;
    Some(pen.0)
}

impl FontDb {
    /// Normalised variation coordinates (F2Dot14 bits) of `face` for user-space
    /// axis settings. Axes not mentioned sit at their default; a face with no
    /// variations yields an empty vector.
    #[must_use]
    pub fn normalized_coords(&self, face: FaceId, variations: &[FontVariation]) -> Vec<i16> {
        let Some(data) = self.face_data(face) else {
            return Vec::new();
        };
        let Some(font) = data.font_ref() else {
            return Vec::new();
        };
        let axes = font.axes();
        if axes.is_empty() {
            return Vec::new();
        }
        let loc = axes.location(
            variations
                .iter()
                .filter(|v| v.value.is_finite())
                .map(|v| (Tag::new(&v.tag), v.value)),
        );
        loc.coords().iter().map(|c| c.to_bits()).collect()
    }

    /// A glyph's outline in font units (y up, unhinted), with the variation
    /// applied. Scale by `size / units_per_em`. Cached.
    #[must_use]
    pub fn glyph_outline(
        &self,
        face: FaceId,
        glyph: u32,
        variations: &[FontVariation],
    ) -> Option<Arc<BezPath>> {
        let coords = if variations.is_empty() {
            Vec::new()
        } else {
            self.normalized_coords(face, variations)
        };
        self.glyph_outline_normalized(face, glyph, &coords)
    }

    /// [`FontDb::glyph_outline`] for normalised coordinates, as a
    /// `GlyphRun` carries them. Trailing zero coordinates
    /// are insignificant and are ignored for caching.
    #[must_use]
    pub fn glyph_outline_normalized(
        &self,
        face: FaceId,
        glyph: u32,
        coords: &[i16],
    ) -> Option<Arc<BezPath>> {
        let end = coords.iter().rposition(|&c| c != 0).map_or(0, |i| i + 1);
        let coords = &coords[..end];
        let mut g = self.lock();
        if let Some(hit) = g.outlines.get(face, glyph, coords) {
            return hit;
        }
        let path = g
            .face_data(face)
            .and_then(|d| extract(&d, glyph, coords))
            .map(Arc::new);
        g.outlines.insert(face, glyph, coords, path.clone());
        path
    }

    /// Units per em of `face` (1000 when unreadable, never 0).
    #[must_use]
    pub fn units_per_em(&self, face: FaceId) -> u16 {
        self.face_data(face)
            .and_then(|d| {
                use skrifa::raw::TableProvider;
                d.font_ref()
                    .and_then(|f| f.head().ok().map(|h| h.units_per_em()))
            })
            .filter(|&u| u > 0)
            .unwrap_or(1000)
    }
}
