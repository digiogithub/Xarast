//! Shaping: one paragraph of styled text in, clusters of positioned glyphs
//! with millipoint advances out, in logical order.
//!
//! # The millipoint rule
//!
//! `parley` works in `f32`. Every run is shaped at the same fixed
//! [`NOMINAL_SIZE`], whatever its real size, so a shaped advance is in
//! thousandths of an em. It is converted to millipoints once per glyph by
//! `round_half_away_from_zero(v * em / 1000)`, and positions along a line are
//! then accumulated in millipoints, never in `f32`. Shaping at the real size
//! and accumulating floats drifts visibly by the end of a long line (phase 9,
//! W9.3). Shaping at one size also makes shaped output size-independent.

use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use icu_segmenter::options::LineBreakOptions;
use icu_segmenter::{LineSegmenter, LineSegmenterBorrowed};
use parley::{FontFamily, FontFamilyName, FontFeatures, FontVariations, StyleProperty};
use xarast_geom::Mp;

use crate::font::{DbInner, FaceId, FontDb, FontSubstitution};
use crate::metrics::{CharMetrics, FaceMetrics, FontMetrics};
use crate::style::{Direction, FontQuery, FontStyle, ManualKern, StyleRange, sanitize_aspect};

/// The size every run is shaped at: 1000 units per em.
pub const NOMINAL_SIZE: f32 = 1000.0;

/// What a cluster is, for line breaking and justification.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum ClusterKind {
    /// A visible character (or a ligature, or a base plus its marks).
    Char,
    /// A space: stretched by full justification, never causes overflow.
    Space,
    /// A horizontal tab; its advance is decided by layout.
    Tab,
}

/// One glyph inside a cluster, relative to the cluster's pen position.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) struct ShapedGlyph {
    pub id: u32,
    /// Offset from the pen, x right.
    pub x: Mp,
    /// Offset from the baseline, y **up**.
    pub y: Mp,
    pub advance: Mp,
}

/// A run of clusters sharing a face and a style.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct ShapedRun {
    pub face: FaceId,
    /// Index into the normalised style runs.
    pub style: u32,
    /// Normalised variation coordinates (F2Dot14 bits).
    pub coords: Arc<[i16]>,
    /// The face's `'M'` width over its em: scales the style's em width
    /// into the unit of tracking and manual kerns.
    pub em_ratio: f64,
}

/// The smallest unit layout moves: a grapheme-ish cluster (a character, a
/// ligature, or a base with its combining marks), never split.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct ShapedCluster {
    /// Byte range in the story text.
    pub range: Range<usize>,
    pub run: u32,
    /// Range into [`ShapedParagraph::glyphs`], in visual order.
    pub glyphs: Range<u32>,
    /// Shaped advance at the run's size and aspect, without tracking.
    pub advance: Mp,
    /// Tracking added after it.
    pub tracking: Mp,
    /// Manual kern inserted before it.
    pub kern_before: Mp,
    pub kind: ClusterKind,
    /// A line may break after this cluster.
    pub break_after: bool,
    /// Resolved bidi level is odd.
    pub rtl: bool,
}

/// One shaped paragraph.
#[derive(Clone, Debug, Default)]
pub(crate) struct ShapedParagraph {
    /// Byte range of the paragraph's text, without its `'\n'`.
    pub range: Range<usize>,
    pub runs: Vec<ShapedRun>,
    pub clusters: Vec<ShapedCluster>,
    pub glyphs: Vec<ShapedGlyph>,
}

pub(crate) struct ShaperState {
    pub(crate) lcx: parley::LayoutContext<u32>,
    pub(crate) segmenter: LineSegmenterBorrowed<'static>,
    pub(crate) metrics: HashMap<(FaceId, Arc<[i16]>), Option<FaceMetrics>>,
}

/// The layout engine. Pure: text and styles in, geometry out.
///
/// Holds a `parley` layout context and shares the [`FontDb`]. Methods take
/// `&self`; concurrent calls on one `Shaper` serialise on an internal lock
/// (use one `Shaper` per worker thread to lay out in parallel).
pub struct Shaper {
    pub(crate) fonts: Arc<FontDb>,
    pub(crate) state: Mutex<ShaperState>,
}

impl std::fmt::Debug for Shaper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shaper")
            .field("fonts", &self.fonts)
            .finish()
    }
}

fn segmenter() -> LineSegmenterBorrowed<'static> {
    #[cfg(feature = "complex-scripts")]
    {
        LineSegmenter::new_dictionary(LineBreakOptions::default())
    }
    #[cfg(not(feature = "complex-scripts"))]
    {
        LineSegmenter::new_for_non_complex_scripts(LineBreakOptions::default())
    }
}

impl Shaper {
    /// A shaper over `fonts`.
    #[must_use]
    pub fn new(fonts: Arc<FontDb>) -> Shaper {
        Shaper {
            fonts,
            state: Mutex::new(ShaperState {
                lcx: parley::LayoutContext::new(),
                segmenter: segmenter(),
                metrics: HashMap::new(),
            }),
        }
    }

    /// The font database this shaper resolves faces in.
    #[must_use]
    pub fn fonts(&self) -> &Arc<FontDb> {
        &self.fonts
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, ShaperState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Normalises style runs so they cover `0..len` exactly, in order, without
/// overlap: sorted by start, clipped, gaps filled with the previous run's
/// style (the first run's for a leading gap), and a default run when there
/// are none. A malformed document still lays out.
pub(crate) fn normalise_runs(runs: &[StyleRange], len: usize) -> Vec<StyleRange> {
    let mut sorted: Vec<&StyleRange> = runs
        .iter()
        .filter(|r| r.range.start < r.range.end && r.range.start < len)
        .collect();
    sorted.sort_by_key(|r| r.range.start);
    let mut out: Vec<StyleRange> = Vec::with_capacity(sorted.len() + 1);
    let mut pos = 0usize;
    for r in sorted {
        let end = r.range.end.min(len);
        if end <= pos {
            continue;
        }
        let start = r.range.start.max(pos);
        if start > pos {
            // A gap: extend the previous run, or pull this one back.
            match out.last_mut() {
                Some(prev) => prev.range.end = start,
                None => {
                    let mut s = r.clone();
                    s.range = 0..start;
                    out.push(s);
                }
            }
        }
        let mut s = r.clone();
        s.range = start..end;
        out.push(s);
        pos = end;
    }
    match out.last_mut() {
        Some(last) if pos < len => last.range.end = len,
        None => out.push(StyleRange::new(0..len, FontQuery::new(""), Mp::new(12_000))),
        _ => {}
    }
    out
}

fn to_mp(v: f32, em: Mp) -> Mp {
    Mp::from_f64_round(f64::from(v) * em.to_f64() / f64::from(NOMINAL_SIZE))
}

fn classify(s: &str) -> ClusterKind {
    if s == "\t" {
        ClusterKind::Tab
    } else if !s.is_empty()
        && s.chars()
            .all(|c| matches!(c, ' ' | '\u{00A0}' | '\u{3000}'))
    {
        ClusterKind::Space
    } else {
        ClusterKind::Char
    }
}

/// The most characters one grapheme cluster keeps. Longer clusters (a base
/// with thousands of combining marks, an endless Hangul jamo run) are a
/// denial-of-service input, and `parley` 0.11.1 counts a cluster's
/// characters in a `u8` (`src/shape/mod.rs:270`), so more than 255 overflows
/// it. Real text never comes close: Unicode's stream-safe format allows 30
/// non-starters in a row.
pub(crate) const MAX_GRAPHEME_CHARS: usize = 64;

/// Replaces every character past [`MAX_GRAPHEME_CHARS`] in a grapheme cluster
/// with U+0001 filler of the same byte length, so every offset stays valid,
/// and returns the filled byte ranges. `None` when nothing needed changing.
pub(crate) fn guard_long_graphemes(s: &str) -> Option<(String, Vec<Range<usize>>)> {
    // Fast path: a cluster this long needs at least that many bytes.
    if s.len() <= MAX_GRAPHEME_CHARS {
        return None;
    }
    let bounds: Vec<usize> = icu_segmenter::GraphemeClusterSegmenter::new()
        .segment_str(s)
        .collect();
    let mut out: Option<(Vec<u8>, Vec<Range<usize>>)> = None;
    for w in bounds.windows(2) {
        let (a, b) = (w[0], w[1]);
        let Some(g) = s.get(a..b) else { continue };
        if let Some((cut, _)) = g.char_indices().nth(MAX_GRAPHEME_CHARS) {
            let (bytes, ranges) = out.get_or_insert_with(|| (s.as_bytes().to_vec(), Vec::new()));
            if let Some(tail) = bytes.get_mut(a + cut..b) {
                tail.fill(0x01);
            }
            ranges.push(a + cut..b);
        }
    }
    let (bytes, ranges) = out?;
    // Only whole characters were overwritten with ASCII, so this is UTF-8.
    String::from_utf8(bytes).ok().map(|t| (t, ranges))
}

/// Everything per-story that shaping one paragraph needs.
pub(crate) struct ParagraphRequest<'a> {
    pub text: &'a str,
    pub range: Range<usize>,
    pub runs: &'a [StyleRange],
    /// The family each run resolved to.
    pub families: &'a [Arc<str>],
    pub kerns: &'a [ManualKern],
    pub auto_kern: bool,
    pub direction: Direction,
}

/// Resolves every run's family through the substitution ladder.
pub(crate) fn resolve_families(
    db: &mut DbInner,
    runs: &[StyleRange],
    subs: &mut Vec<FontSubstitution>,
) -> Vec<Arc<str>> {
    let mut cache: HashMap<(FontQuery, Option<[u8; 10]>), Arc<str>> = HashMap::new();
    runs.iter()
        .map(|r| {
            if let Some(f) = cache.get(&(r.font.clone(), r.panose)) {
                return f.clone();
            }
            let fam = match db.query(&r.font, r.panose) {
                Some(m) => {
                    // Warm the blob map, so the faces parley loads through the
                    // same source cache are recognised as ours.
                    let _ = db.face_data(m.face);
                    if let Some(s) = m.substitution
                        && !subs.contains(&s)
                    {
                        subs.push(s);
                    }
                    m.family
                }
                None => r.font.family.clone(),
            };
            cache.insert((r.font.clone(), r.panose), fam.clone());
            fam
        })
        .collect()
}

/// Shapes one paragraph.
pub(crate) fn shape_paragraph(
    st: &mut ShaperState,
    db: &mut DbInner,
    req: &ParagraphRequest<'_>,
) -> ShapedParagraph {
    let para = req.range.clone();
    let Some(slice) = req.text.get(para.clone()) else {
        return ShapedParagraph {
            range: para,
            ..ShapedParagraph::default()
        };
    };
    // parley always detects the base direction itself; a leading mark forces
    // it when the paragraph style names one.
    let prefix = match req.direction {
        Direction::Auto => "",
        Direction::Ltr => "\u{200E}",
        Direction::Rtl => "\u{200F}",
    };
    let (guarded, suppressed) = match guard_long_graphemes(slice) {
        Some((g, s)) => (Cow::Owned(g), s),
        None => (Cow::Borrowed(slice), Vec::new()),
    };
    // Suppressed ranges, in story offsets.
    let suppressed: Vec<Range<usize>> = suppressed
        .into_iter()
        .map(|r| r.start + para.start..r.end + para.start)
        .collect();
    let shaped_text: Cow<'_, str> = if prefix.is_empty() {
        guarded
    } else {
        Cow::Owned(format!("{prefix}{guarded}"))
    };
    let shift = prefix.len();
    // Byte offset in `shaped_text` → story offset.
    let to_story = |i: usize| (i + para.start).saturating_sub(shift);

    let mut layout = {
        let mut b = st.lcx.ranged_builder(&mut db.fcx, &shaped_text, 1.0, false);
        b.push_default(StyleProperty::FontSize(NOMINAL_SIZE));
        b.push_default(StyleProperty::Brush(0u32));
        for (i, r) in req.runs.iter().enumerate() {
            let s = r.range.start.max(para.start);
            let e = r.range.end.min(para.end);
            if s >= e {
                continue;
            }
            let range = (s - para.start + shift)..(e - para.start + shift);
            let range = if s == para.start { 0..range.end } else { range };
            let family = req
                .families
                .get(i)
                .cloned()
                .unwrap_or_else(|| r.font.family.clone());
            b.push(
                StyleProperty::FontFamily(FontFamily::List(Cow::Owned(vec![
                    FontFamilyName::Named(Cow::Owned(family.to_string())),
                    FontFamilyName::Generic(parley::GenericFamily::SansSerif),
                ]))),
                range.clone(),
            );
            b.push(
                StyleProperty::FontWeight(parley::FontWeight::new(f32::from(
                    r.font.weight.clamp(1, 1000),
                ))),
                range.clone(),
            );
            b.push(
                StyleProperty::FontStyle(match r.font.style {
                    FontStyle::Normal => parley::FontStyle::Normal,
                    FontStyle::Italic => parley::FontStyle::Italic,
                    FontStyle::Oblique(a) => parley::FontStyle::Oblique(a),
                }),
                range.clone(),
            );
            b.push(
                StyleProperty::FontWidth(parley::FontWidth::from_percentage(f32::from(
                    r.font.stretch.clamp(50, 200),
                ))),
                range.clone(),
            );
            let mut feats: Vec<parley::FontFeature> = r
                .features
                .iter()
                .map(|f| parley::FontFeature::new(parley::setting::Tag::new(&f.tag), f.value))
                .collect();
            if !req.auto_kern && !r.features.iter().any(|f| &f.tag == b"kern") {
                feats.push(parley::FontFeature::new(
                    parley::setting::Tag::new(b"kern"),
                    0,
                ));
            }
            if !feats.is_empty() {
                b.push(
                    StyleProperty::FontFeatures(FontFeatures::List(Cow::Owned(feats))),
                    range.clone(),
                );
            }
            if !r.variations.is_empty() {
                let vars: Vec<parley::FontVariation> = r
                    .variations
                    .iter()
                    .filter(|v| v.value.is_finite())
                    .map(|v| parley::FontVariation::new(parley::setting::Tag::new(&v.tag), v.value))
                    .collect();
                b.push(
                    StyleProperty::FontVariations(FontVariations::List(Cow::Owned(vars))),
                    range.clone(),
                );
            }
            b.push(
                StyleProperty::Brush(u32::try_from(i).unwrap_or(u32::MAX)),
                range,
            );
        }
        b.build(&shaped_text)
    };
    layout.break_all_lines(None);

    // Break opportunities (UAX #14) over the paragraph text itself.
    let breaks: Vec<usize> = st.segmenter.segment_str(slice).collect();

    let mut out = ShapedParagraph {
        range: para.clone(),
        ..ShapedParagraph::default()
    };
    let mut run_index: HashMap<(FaceId, u32, Arc<[i16]>), u32> = HashMap::new();
    // The unconverted advance of each cluster, parallel to `out.clusters`.
    let mut raw_adv: Vec<f32> = Vec::new();
    for line in layout.lines() {
        for run in line.runs() {
            let face = db.face_for_parley(run.font());
            let coords: Arc<[i16]> = Arc::from(run.normalized_coords());
            let em_ratio = face_metrics(st, db, face, &coords).map_or(1.0, |m| m.em_char_ratio());
            let rtl = run.is_rtl();
            let mut last_char_cluster: Option<usize> = None;
            let mut pending: Option<(Range<usize>, f32)> = None;
            for cluster in run.clusters() {
                let tr = cluster.text_range();
                if tr.end <= shift {
                    continue; // the direction mark
                }
                let style = cluster.first_style().brush;
                let key = (face, style, coords.clone());
                let run_id = *run_index.entry(key).or_insert_with(|| {
                    out.runs.push(ShapedRun {
                        face,
                        style,
                        coords: coords.clone(),
                        em_ratio,
                    });
                    u32::try_from(out.runs.len() - 1).unwrap_or(u32::MAX)
                });
                let style_ref = req.runs.get(style as usize);
                let size = style_ref.map_or(Mp::new(12_000), StyleRange::effective_size);
                let em = style_ref.map_or(size, StyleRange::em_width);
                let range = to_story(tr.start)..to_story(tr.end);

                // Components of a ligature, or combining marks shaped into
                // their base, are separate parley clusters with no glyphs and
                // a share of the advance. `clusters()` walks text order, so in
                // a left-to-right run they follow the cluster holding the
                // glyphs and in a right-to-left run they precede it.
                if cluster.is_ligature_continuation() {
                    let adv = cluster.advance();
                    if !rtl {
                        if let Some((prev, raw)) = last_char_cluster
                            .and_then(|i| out.clusters.get_mut(i).zip(raw_adv.get_mut(i)))
                        {
                            prev.range.start = prev.range.start.min(range.start);
                            prev.range.end = prev.range.end.max(range.end);
                            // Summed in f32 and converted once, like any cluster.
                            *raw += adv;
                            prev.advance = to_mp(*raw, em);
                            continue;
                        }
                    } else {
                        pending = Some(match pending.take() {
                            Some((r, a)) => {
                                (r.start.min(range.start)..r.end.max(range.end), a + adv)
                            }
                            None => (range.clone(), adv),
                        });
                        continue;
                    }
                }
                let (range, lig_extra) = match pending.take() {
                    Some((r, a)) => (r.start.min(range.start)..r.end.max(range.end), a),
                    None => (range, 0.0),
                };
                let g0 = u32::try_from(out.glyphs.len()).unwrap_or(u32::MAX);
                let mut pen = 0.0f32;
                for g in cluster.glyphs() {
                    out.glyphs.push(ShapedGlyph {
                        id: g.id,
                        x: to_mp(pen + g.x, em),
                        y: to_mp(-g.y, size),
                        advance: to_mp(g.advance, em),
                    });
                    pen += g.advance;
                }
                let g1 = u32::try_from(out.glyphs.len()).unwrap_or(u32::MAX);
                let kind = classify(
                    slice
                        .get(range.start - para.start..range.end - para.start)
                        .unwrap_or(""),
                );
                out.clusters.push(ShapedCluster {
                    range,
                    run: run_id,
                    glyphs: g0..g1,
                    advance: to_mp(cluster.advance() + lig_extra, em),

                    tracking: style_ref.map_or(Mp::ZERO, |s| {
                        s.em_width().scale(em_ratio).mul_ratio(s.tracking, 1000)
                    }),
                    kern_before: Mp::ZERO,
                    kind,
                    break_after: false,
                    rtl,
                });
                raw_adv.push(cluster.advance() + lig_extra);
                last_char_cluster = Some(out.clusters.len() - 1);
            }
        }
    }
    out.clusters.sort_by_key(|c| c.range.start);
    // A ligature component that preceded its start in iteration order (RTL)
    // may leave clusters with overlapping ranges; merge any such pair.
    let mut merged: Vec<ShapedCluster> = Vec::with_capacity(out.clusters.len());
    for c in out.clusters.drain(..) {
        let hidden = suppressed
            .iter()
            .any(|r| r.start <= c.range.start && c.range.end <= r.end);
        match merged.last_mut() {
            // Filler standing in for the tail of an over-long grapheme: its
            // bytes join the grapheme's head, its glyphs are dropped.
            Some(prev) if hidden => {
                prev.range.end = prev.range.end.max(c.range.end);
            }
            Some(prev) if c.range.start < prev.range.end => {
                prev.range.end = prev.range.end.max(c.range.end);
                prev.advance += c.advance;
                if prev.glyphs.is_empty() {
                    prev.glyphs = c.glyphs;
                }
            }
            _ => merged.push(c),
        }
    }
    out.clusters = merged;

    // Break opportunities and manual kerns.
    let mut bi = breaks.iter().peekable();
    for c in &mut out.clusters {
        let rel_end = c.range.end - para.start;
        while bi.peek().is_some_and(|&&b| b < rel_end) {
            bi.next();
        }
        c.break_after = bi.peek().is_some_and(|&&b| b == rel_end) && rel_end < slice.len();
    }
    for k in req.kerns {
        if k.at < para.start || k.at > para.end {
            continue;
        }
        // The kern belongs to the cluster that starts at (or contains) `at`.
        let idx = out.clusters.partition_point(|c| c.range.end <= k.at);
        if let Some(c) = out.clusters.get_mut(idx) {
            let em = out
                .runs
                .get(c.run as usize)
                .and_then(|r| {
                    req.runs
                        .get(r.style as usize)
                        .map(|s| s.em_width().scale(r.em_ratio))
                })
                .unwrap_or(Mp::new(12_000));
            c.kern_before += em.mul_ratio(k.amount, 1000);
        }
    }
    out
}

pub(crate) fn face_metrics(
    st: &mut ShaperState,
    db: &mut DbInner,
    face: FaceId,
    coords: &Arc<[i16]>,
) -> Option<FaceMetrics> {
    if let Some(m) = st.metrics.get(&(face, coords.clone())) {
        return *m;
    }
    let m = db
        .face_data(face)
        .and_then(|d| FaceMetrics::read(&d, coords));
    st.metrics.insert((face, coords.clone()), m);
    m
}

impl FontMetrics for Shaper {
    fn char_metrics(
        &self,
        font: &FontQuery,
        size: Mp,
        aspect: f32,
        c: char,
    ) -> Option<CharMetrics> {
        let mut st = self.lock();
        let mut db = self.fonts.lock();
        let mut text = String::new();
        text.push(c);
        let mut run = StyleRange::new(0..text.len(), font.clone(), size);
        run.aspect = sanitize_aspect(aspect);
        let runs = [run];
        let mut subs = Vec::new();
        let families = resolve_families(&mut db, &runs, &mut subs);
        let para = shape_paragraph(
            &mut st,
            &mut db,
            &ParagraphRequest {
                text: &text,
                range: 0..text.len(),
                runs: &runs,
                families: &families,
                kerns: &[],
                auto_kern: true,
                direction: Direction::Auto,
            },
        );
        let cl = para.clusters.first()?;
        let r = para.runs.get(cl.run as usize)?.clone();
        let glyph = para
            .glyphs
            .get(cl.glyphs.start as usize)
            .map_or(0, |g| g.id);
        let fm = face_metrics(&mut st, &mut db, r.face, &r.coords)?;
        let m = fm.scaled(size);
        Some(CharMetrics {
            face: r.face,
            glyph,
            advance: cl.advance,
            ascent: m.ascent,
            descent: m.descent,
            em_width: runs[0].em_width().scale(fm.em_char_ratio()),
        })
    }

    fn kern_pair(&self, font: &FontQuery, size: Mp, left: char, right: char) -> Mp {
        let mut st = self.lock();
        let mut db = self.fonts.lock();
        let mut text = String::new();
        text.push(left);
        text.push(right);
        let runs = [StyleRange::new(0..text.len(), font.clone(), size)];
        let mut subs = Vec::new();
        let families = resolve_families(&mut db, &runs, &mut subs);
        let mut adv = [Mp::ZERO; 2];
        for (slot, kern) in adv.iter_mut().zip([true, false]) {
            let p = shape_paragraph(
                &mut st,
                &mut db,
                &ParagraphRequest {
                    text: &text,
                    range: 0..text.len(),
                    runs: &runs,
                    families: &families,
                    kerns: &[],
                    auto_kern: kern,
                    direction: Direction::Auto,
                },
            );
            *slot = p.clusters.first().map_or(Mp::ZERO, |c| c.advance);
        }
        adv[0] - adv[1]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_graphemes_are_capped_without_moving_offsets() {
        assert!(guard_long_graphemes("short text").is_none());
        assert!(guard_long_graphemes(&"ab".repeat(100)).is_none());
        let mut s = String::from("x");
        s.push('a');
        s.extend(std::iter::repeat_n('\u{0301}', 300));
        s.push('y');
        let (g, ranges) = guard_long_graphemes(&s).unwrap();
        assert_eq!(g.len(), s.len());
        // The head keeps 64 characters: the base and 63 marks (2 bytes each).
        let cut = 2 + 63 * 2;
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0], cut..s.len() - 1);
        assert!(g[cut..s.len() - 1].bytes().all(|b| b == 1));
        assert!(g.ends_with('y'));
        // Hangul jamo sequences are one grapheme too.
        let jamo: String = std::iter::repeat_n('\u{1100}', 400).collect();
        let (_, r) = guard_long_graphemes(&jamo).unwrap();
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn style_runs_are_normalised_to_cover_the_text() {
        let q = FontQuery::new("F");
        let a = StyleRange::new(2..4, q.clone(), Mp::new(1));
        let b = StyleRange::new(3..9, q.clone(), Mp::new(2));
        let n = normalise_runs(&[b, a], 10);
        let spans: Vec<_> = n.iter().map(|r| (r.range.clone(), r.size.raw())).collect();
        assert_eq!(spans, [(0..2, 1), (2..4, 1), (4..10, 2)]);
        let n = normalise_runs(&[], 5);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].range, 0..5);
    }
}
