//! Where the `.xarast` writer places text for browsers (XARA-T-0172,
//! `research/06 §6.7`).
//!
//! The format crate knows no fonts; it asks a
//! [`TextPlacer`](xarast_format::svg::TextPlacer) for the position of
//! every character so that the SVG base shows a story where Xarast draws
//! it. This one lays the story out exactly as the walker does (the same
//! attribute bridge, the same shaper) and reports, per character item, the
//! left edge of its cluster box on its baseline. A cluster of several
//! characters (a ligature, a base with marks) shares its box evenly: a
//! browser draws each character on its own, so that is the nearest it can
//! get.

use std::collections::HashMap;
use std::sync::Arc;

use xarast_doc::{AttrStack, Document, NodeId, NodeKind, StoryText};
use xarast_format::svg::{Placer, StoryPlacement, TextPlacer};
use xarast_geom::Mp;
use xarast_text::StoryInput;

use crate::fonts::{self, FontService};
use crate::text::{layout_text, story_input};

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
        let input = story_input(&doc.tree, &st, node);
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
        let mut chars = HashMap::new();
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
                let n = text.chars().count().max(1) as i64;
                let y = ys.get(&c.range.start).copied().unwrap_or(line.baseline_y);
                for (k, (off, _)) in text.char_indices().enumerate() {
                    let Some(node) = item_at.get(&(c.range.start + off)) else {
                        continue;
                    };
                    let dx = i64::from(c.width.raw()) * k as i64 / n;
                    let x = i64::from(c.x.raw()) + dx;
                    let x = Mp::new(x.clamp(i64::from(i32::MIN + 1), i64::from(i32::MAX)) as i32);
                    chars.insert(*node, (x, y));
                }
            }
        }
        let substitutions = layout
            .substitutions
            .iter()
            .map(|s| (Arc::clone(&s.requested), Arc::clone(&s.used)))
            .collect();
        Some(StoryPlacement {
            chars,
            substitutions,
        })
    }
}
