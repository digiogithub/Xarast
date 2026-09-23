//! The bridge from a story in the document to glyph outlines in the scene
//! (phase 9, W9.2 attribute bridge and the walker's text path).
//!
//! [`StoryText`] (in `xarast-doc`) gives the logical text and its resolved
//! attribute runs; this module turns those into the shaper's input
//! ([`story_input`], the table in `docs/memory/text.md`, "Attribute
//! bridge"), lays the story out, and turns the placed glyphs into one path
//! per attribute run in **document space** ([`build_story`]), which the
//! walker then paints with that run's fill, stroke and transparency. A
//! gradient's geometry is in document coordinates, so one gradient spans
//! every character of a story, as in the original, without any special
//! casing here.

use std::collections::HashMap;
use std::sync::Arc;

use kurbo::{Affine, BezPath, Shape};
use xarast_doc::{
    AttrSlot, AttrValue, NodeId, NodeKind, ResolvedAttrs, StoryFlow, StoryText, TextStoryNode, Tree,
};
use xarast_geom::{Mp, Path};
use xarast_render::PathRef;
use xarast_text::{
    FontQuery, FontStyle, FontSubstitution, Justification, LineSpacing, ManualKern, ParagraphStyle,
    StoryInput, StoryMode, StyleRange, TabKind, TabStop, TextScript,
};

use crate::fonts::FontService;

/// A story ready to paint: one outline per attribute run.
#[derive(Debug, Clone)]
pub(crate) struct StoryGeometry {
    /// Runs with ink, in logical order.
    pub runs: Vec<RunGeometry>,
    /// Families substituted while laying the story out.
    pub substitutions: Vec<FontSubstitution>,
    /// Text on a path, drawn along a straight baseline until W9.5.
    pub on_path: bool,
    /// Visible characters that produced no glyph at all: no font could be
    /// found, so the story could not be drawn.
    pub unrendered: bool,
    /// A fold of the version of every node in the story, for the render
    /// cache: a character edit changes it.
    pub version: u64,
    /// The union of the runs' outlines, document space.
    pub bounds: xarast_geom::Rect,
}

/// One attribute run's glyphs.
#[derive(Debug, Clone)]
pub(crate) struct RunGeometry {
    /// Glyph outlines (and underline), document space.
    pub path: PathRef,
    /// The attributes the run paints with.
    pub attrs: ResolvedAttrs,
}

/// The shaper's input for a story: style runs, paragraph styles, manual
/// kerns and the flow.
pub(crate) struct Input {
    pub runs: Vec<StyleRange>,
    pub paragraphs: Vec<ParagraphStyle>,
    pub kerns: Vec<ManualKern>,
    pub mode: StoryMode,
}

/// The attribute bridge (`docs/memory/text.md`, "Attribute bridge").
pub(crate) fn story_input(tree: &Tree, st: &StoryText, story: &TextStoryNode) -> Input {
    let runs = st
        .runs
        .iter()
        .map(|r| style_range(r.range.clone(), &r.attrs))
        .collect();
    let paragraphs = st
        .paragraph_first_lines()
        .into_iter()
        .map(|i| {
            let line = &st.lines[i];
            paragraph_style(&line.attrs, line_ruler(tree, line.node), story.auto_kern)
        })
        .collect::<Vec<_>>();
    let paragraphs = if paragraphs.is_empty() {
        vec![paragraph_style(&st.story_attrs, None, story.auto_kern)]
    } else {
        paragraphs
    };
    let kerns = st
        .kerns
        .iter()
        .map(|k| ManualKern {
            at: k.at,
            amount: k.amount,
        })
        .collect();
    let mode = match StoryFlow::of(story) {
        StoryFlow::Point | StoryFlow::OnPath => StoryMode::Point,
        StoryFlow::Column { width, wrap } => StoryMode::Column {
            width: Mp::new(width),
            wrap,
        },
    };
    Input {
        runs,
        paragraphs,
        kerns,
        mode,
    }
}

/// The ruler a `TextLine` node carries from the file, used when no ruler
/// attribute is in force.
fn line_ruler(tree: &Tree, line: NodeId) -> Option<Arc<[xarast_doc::TabStop]>> {
    match tree.kind(line) {
        Some(NodeKind::TextLine(l)) => l.ruler.clone(),
        _ => None,
    }
}

fn style_range(range: std::ops::Range<usize>, a: &ResolvedAttrs) -> StyleRange {
    let (family, panose) = match a.get(AttrSlot::TxtFontTypeface) {
        AttrValue::FontTypeface(t) => (Arc::clone(&t.family), t.panose),
        _ => (Arc::from("Times New Roman"), None),
    };
    let bold = matches!(a.get(AttrSlot::TxtBold), AttrValue::Bold(true));
    let italic = matches!(a.get(AttrSlot::TxtItalic), AttrValue::Italic(true));
    let font = FontQuery {
        family,
        weight: if bold { 700 } else { 400 },
        style: if italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        },
        stretch: 100,
    };
    let size = match a.get(AttrSlot::TxtFontSize) {
        AttrValue::FontSize(s) if s.raw() > 0 => *s,
        _ => Mp::new(16_000),
    };
    let mut r = StyleRange::new(range, font, size);
    r.panose = panose;
    if let AttrValue::Tracking(t) = a.get(AttrSlot::TxtTracking) {
        // The raw value is em/1000: the `Mp` type is historical.
        r.tracking = t.raw();
    }
    if let AttrValue::AspectRatio(x) = a.get(AttrSlot::TxtAspectRatio) {
        r.aspect = *x;
    }
    if let AttrValue::Baseline(b) = a.get(AttrSlot::TxtBaseline) {
        r.baseline_shift = *b;
    }
    if let AttrValue::Script(s) = a.get(AttrSlot::TxtScript) {
        r.script = if s.on {
            TextScript {
                offset: s.offset,
                size: s.size,
            }
        } else {
            TextScript::NONE
        };
    }
    r.underline = matches!(a.get(AttrSlot::TxtUnderline), AttrValue::Underline(true));
    r
}

fn paragraph_style(
    a: &ResolvedAttrs,
    line_ruler: Option<Arc<[xarast_doc::TabStop]>>,
    auto_kern: bool,
) -> ParagraphStyle {
    let mut p = ParagraphStyle {
        auto_kern,
        ..ParagraphStyle::default()
    };
    if let AttrValue::Justification(j) = a.get(AttrSlot::TxtJustification) {
        p.justification = match j {
            xarast_doc::Justification::Left => Justification::Left,
            xarast_doc::Justification::Centre => Justification::Centre,
            xarast_doc::Justification::Right => Justification::Right,
            xarast_doc::Justification::Full => Justification::Full,
        };
    }
    if let AttrValue::LineSpace(l) = a.get(AttrSlot::TxtLineSpace) {
        p.line_spacing = match l {
            xarast_doc::LineSpacing::Ratio(r) => LineSpacing::Ratio(*r),
            xarast_doc::LineSpacing::Absolute(v) => LineSpacing::Absolute(*v),
        };
    }
    if let AttrValue::LeftMargin(v) = a.get(AttrSlot::TxtLeftMargin) {
        p.left_margin = *v;
    }
    if let AttrValue::RightMargin(v) = a.get(AttrSlot::TxtRightMargin) {
        p.right_margin = *v;
    }
    if let AttrValue::FirstIndent(v) = a.get(AttrSlot::TxtFirstIndent) {
        p.first_indent = *v;
    }
    let ruler = match a.get(AttrSlot::TxtRuler) {
        AttrValue::Ruler(r) if !r.is_empty() => Some(Arc::clone(r)),
        _ => line_ruler,
    };
    if let Some(r) = ruler {
        let mut tabs: Vec<TabStop> = r
            .iter()
            .map(|t| TabStop {
                pos: t.position,
                kind: match t.kind & 3 {
                    1 => TabKind::Right,
                    2 => TabKind::Centre,
                    3 => TabKind::Decimal,
                    _ => TabKind::Left,
                },
            })
            .collect();
        tabs.sort_by_key(|t| t.pos);
        p.tabs = Arc::from(tabs);
    }
    p
}

/// Lays a story out and turns it into document-space outlines.
pub(crate) fn build_story(
    fonts: &FontService,
    tree: &Tree,
    st: &StoryText,
    story: &TextStoryNode,
) -> StoryGeometry {
    let input = story_input(tree, st, story);
    let shaper = fonts.ready();
    let layout = shaper.layout(&StoryInput {
        text: layout_text(st),
        runs: &input.runs,
        paragraphs: &input.paragraphs,
        kerns: &input.kerns,
        mode: input.mode,
    });
    let db = fonts.db();
    let story_xf = story.transform.to_affine();

    // Faux italic and bold: what the matched face lacks, per style run.
    let mut synth: HashMap<usize, Option<f64>> = HashMap::new();
    let mut paths: Vec<BezPath> = vec![BezPath::new(); input.runs.len()];
    let mut glyphs = 0usize;
    // Glyphs with ink that are not the missing-glyph box: a story whose
    // visible text has none could not be drawn.
    let mut inked = 0usize;
    for line in &layout.lines {
        for run in &line.runs {
            let Some(target) = paths.get_mut(run.style) else {
                continue;
            };
            let skew = *synth.entry(run.style).or_insert_with(|| {
                let r = &input.runs[run.style];
                db.query_with_panose(&r.font, r.panose)
                    .and_then(|m| m.synthesis.skew)
                    .map(|deg| f64::from(deg).to_radians().tan())
            });
            let upem = db.units_per_em(run.face);
            for g in &run.glyphs {
                glyphs += 1;
                let Some(outline) = db.glyph_outline_normalized(run.face, g.id, &run.coords) else {
                    continue;
                };
                if g.id != 0 && !outline.elements().is_empty() {
                    inked += 1;
                }
                let mut xf = run.glyph_transform(g, upem);
                if let Some(k) = skew {
                    // Shear about the glyph's own baseline.
                    let (x, y) = (g.x.to_f64(), g.y.to_f64());
                    xf = Affine::translate((x, y))
                        * Affine::new([1.0, 0.0, k, 1.0, 0.0, 0.0])
                        * Affine::translate((-x, -y))
                        * xf;
                }
                let xf = story_xf * xf;
                for el in outline.elements() {
                    target.push(xf * *el);
                }
            }
            if input.runs[run.style].underline && !run.glyphs.is_empty() {
                let first = &run.glyphs[0];
                let last = &run.glyphs[run.glyphs.len() - 1];
                let x0 = first.x.to_f64();
                let x1 = last.x.to_f64() + last.advance.to_f64();
                let size = run.size.to_f64();
                // Position and thickness are the usual typographic defaults
                // (a tenth of the size below the baseline, a twentieth
                // thick): the original's own values come from the font.
                let y = line.baseline_y.to_f64() - size * 0.1;
                let rect = kurbo::Rect::new(x0, y - size * 0.05, x1, y);
                for el in rect.path_elements(0.1) {
                    target.push(story_xf * el);
                }
            }
        }
    }
    let visible = st.text.chars().any(|c| !c.is_whitespace());
    let mut bounds = xarast_geom::Rect::EMPTY;
    for p in &paths {
        if p.elements().is_empty() {
            continue;
        }
        let b = p.bounding_box();
        let r = xarast_geom::Rect::new(
            xarast_geom::Point::new(Mp::from_f64_round(b.x0), Mp::from_f64_round(b.y0)),
            xarast_geom::Point::new(Mp::from_f64_round(b.x1), Mp::from_f64_round(b.y1)),
        );
        bounds = if bounds.is_empty() {
            r
        } else {
            bounds.union(r)
        };
    }
    let runs = paths
        .into_iter()
        .zip(&st.runs)
        .filter(|(p, _)| !p.elements().is_empty())
        .map(|(p, r)| RunGeometry {
            path: PathRef::new(Path::from_bez_path(&p).0),
            attrs: r.attrs.clone(),
        })
        .collect();
    StoryGeometry {
        runs,
        substitutions: layout.substitutions,
        on_path: matches!(StoryFlow::of(story), StoryFlow::OnPath),
        unrendered: visible && (glyphs == 0 || inked == 0),
        version: subtree_version(tree, st.story),
        bounds,
    }
}

/// The document-space box a story's lines occupy (advance boxes, not ink),
/// or an empty rectangle when it holds no text. `attrs` is the state in
/// force at the story, as for [`StoryText::collect`].
pub(crate) fn story_rect(
    fonts: &FontService,
    tree: &Tree,
    story_node: NodeId,
    attrs: &mut xarast_doc::AttrStack,
) -> xarast_geom::Rect {
    let Some(NodeKind::TextStory(story)) = tree.kind(story_node) else {
        return xarast_geom::Rect::EMPTY;
    };
    let Some(st) = StoryText::collect(tree, story_node, attrs, &mut |_, a| {
        Arc::new(a.value.clone())
    }) else {
        return xarast_geom::Rect::EMPTY;
    };
    if st.text.trim().is_empty() {
        return xarast_geom::Rect::EMPTY;
    }
    let input = story_input(tree, &st, story);
    let layout = fonts.ready().layout(&StoryInput {
        text: layout_text(&st),
        runs: &input.runs,
        paragraphs: &input.paragraphs,
        kerns: &input.kerns,
        mode: input.mode,
    });
    if layout.bounds.is_empty() {
        return xarast_geom::Rect::EMPTY;
    }
    story.transform.transform_rect(layout.bounds)
}

/// The text layout sees: the logical text without the paragraph break
/// that ends the story. Every line of a `.xar` story ends with an EOL, so
/// the last one would otherwise open an empty paragraph after the text,
/// which the original never draws or measures.
fn layout_text(st: &StoryText) -> &str {
    st.text.strip_suffix('\n').unwrap_or(&st.text)
}

/// A fold of the tag and content revision of every node under `root`.
fn subtree_version(tree: &Tree, root: NodeId) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for n in tree.preorder(root) {
        let tag = u64::from(tree.get(n).map_or(0, |d| d.tag.0));
        let v = (tag << 32) ^ tree.content_rev(n);
        h = (h ^ v).wrapping_mul(0x0100_0000_01b3);
        h ^= h >> 29;
    }
    h
}
