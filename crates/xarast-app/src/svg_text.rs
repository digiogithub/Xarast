//! Where the `.xarast` writer places text for browsers (XARA-T-0172,
//! `research/06 §6.7`).
//!
//! The format crate knows no fonts; it asks a
//! [`TextPlacer`](xarast_format::svg::TextPlacer) for the position of
//! every character so that the SVG base shows a story where Xarast draws
//! it. This one lays the story out exactly as the walker does (the same
//! attribute bridge, the same shaper, the same fit onto a path) and
//! reports, per character item, the left edge of its cluster box on its
//! baseline. A cluster of several characters (a ligature, a base with
//! marks) shares its box evenly: a browser draws each character on its
//! own, so that is the nearest it can get.
//!
//! A story on a path whose fit is a rotation per character
//! ([`PathFit::is_plain`]) is placed along the path instead (T9.5.6):
//! each character's glyph origin carried onto the path by
//! [`PathFit::cluster_transform`], and the angle it turns there.

use std::collections::HashMap;
use std::sync::Arc;

use xarast_doc::{AttrStack, Document, NodeId, NodeKind, StoryText};
use xarast_format::svg::{Placer, StoryPlacement, TextPlacer};
use xarast_geom::Mp;
use xarast_text::{PathFit, StoryInput};

use crate::fonts::{self, FontService};
use crate::text::{layout_text, path_fit, story_input};

/// Lays stories out with a [`FontService`] for the SVG writer.
pub struct SvgTextPlacer {
    fonts: Arc<FontService>,
}

impl std::fmt::Debug for SvgTextPlacer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SvgTextPlacer")
    }
}

impl SvgTextPlacer {
    /// A placer over `fonts`.
    #[must_use]
    pub fn new(fonts: Arc<FontService>) -> SvgTextPlacer {
        SvgTextPlacer { fonts }
    }
}

/// The placer over the process's font service ([`fonts::shared`]), ready
/// for [`xarast_format::svg::SvgOptions::text`].
#[must_use]
pub fn placer() -> Placer {
    Placer(Arc::new(SvgTextPlacer::new(fonts::shared())))
}

/// A millipoint value, clamped to what an `Mp` holds.
fn mp_of(v: f64) -> Mp {
    let v = v.round();
    if v.is_nan() {
        return Mp::ZERO;
    }
    Mp::new(v.clamp(f64::from(i32::MIN + 1), f64::from(i32::MAX)) as i32)
}

impl TextPlacer for SvgTextPlacer {
    fn place(
        &self,
        doc: &Document,
        story: NodeId,
        attrs: &mut AttrStack,
    ) -> Option<StoryPlacement> {
        let Some(NodeKind::TextStory(node)) = doc.tree.kind(story) else {
            return None;
        };
        let st = StoryText::collect(&doc.tree, story, attrs, &mut |_, a| {
            Arc::new(a.value.clone())
        })?;
        let mut input = story_input(&doc.tree, &st, node);
        // Along a path the story is laid out as the walker lays it out: a
        // column as long as the path, then carried onto it. A fit SVG
        // cannot say per character (reflected or sheared) stays straight.
        let fit = path_fit(&doc.tree, story, node).filter(PathFit::is_plain);
        if let Some(f) = &fit {
            input.mode = f.story_mode();
        }
        let layout = self.fonts.ready().layout(&StoryInput {
            text: layout_text(&st),
            runs: &input.runs,
            paragraphs: &input.paragraphs,
            kerns: &input.kerns,
            mode: input.mode,
        });
        // Byte offset of each character item.
        let mut item_at: HashMap<usize, NodeId> = HashMap::new();
        for e in &st.items {
            if e.len > 0 {
                item_at.insert(e.byte as usize, e.node);
            }
        }
        let mut placement = StoryPlacement {
            along_path: fit.is_some(),
            ..StoryPlacement::default()
        };
        for line in &layout.lines {
            // The baseline of each cluster, shifts included: its first
            // glyph's y.
            let mut ys: HashMap<usize, Mp> = HashMap::new();
            for run in &line.runs {
                for g in &run.glyphs {
                    ys.entry(g.cluster).or_insert(g.y);
                }
            }
            for c in &line.clusters {
                let Some(text) = st.text.get(c.range.clone()) else {
                    continue;
                };
                let n = text.chars().count().max(1) as f64;
                let y = ys.get(&c.range.start).copied().unwrap_or(line.baseline_y);
                // Along a path: where the cluster's glyphs start, carried
                // onto the path, and the angle they turn by there.
                let onto = fit.as_ref().map(|f| {
                    let xf = f.cluster_transform(line, c);
                    let [a, b, ..] = xf.as_coeffs();
                    (xf, b.atan2(a).to_degrees())
                });
                for (k, (off, _)) in text.char_indices().enumerate() {
                    let Some(node) = item_at.get(&(c.range.start + off)) else {
                        continue;
                    };
                    let (x, y) = match onto {
                        Some((xf, turn)) => {
                            let x = c.pen.to_f64() + c.advance.to_f64() * k as f64 / n;
                            let p = xf * kurbo::Point::new(x, y.to_f64());
                            if turn != 0.0 {
                                placement.rotations.insert(*node, turn);
                            }
                            (mp_of(p.x), mp_of(p.y))
                        }
                        None => {
                            let dx = i64::from(c.width.raw()) * k as i64 / n as i64;
                            let x = i64::from(c.x.raw()) + dx;
                            let x = x.clamp(i64::from(i32::MIN + 1), i64::from(i32::MAX));
                            (Mp::new(x as i32), y)
                        }
                    };
                    placement.chars.insert(*node, (x, y));
                }
            }
        }
        placement.substitutions = layout
            .substitutions
            .iter()
            .map(|s| (Arc::clone(&s.requested), Arc::clone(&s.used)))
            .collect();
        Some(placement)
    }
}
