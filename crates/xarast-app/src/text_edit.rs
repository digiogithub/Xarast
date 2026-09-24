//! Carets, selections and caret movement over a laid-out story (phase 9,
//! W9.4: T9.4.2–T9.4.4, and the hit test T9.4.5 needs).
//!
//! Everything here is pure: it reads a [`Layout`] and the text it was laid
//! out from, both in **story space** (millipoints, y up, the first baseline
//! at 0), and knows nothing about the document, the view or the tool. The
//! text tool ([`crate::text_tool`]) maps story space to the document with
//! the story's matrix.
//!
//! # Positions
//!
//! A caret sits at a byte offset of the text, always on a cluster boundary
//! (a cluster is never split: a ligature or a base with its marks is one
//! caret stop), and carries an **affinity**: [`Caret::upstream`] says it
//! belongs with the text before it. Affinity matters in two places:
//!
//! * a soft line break, where the end of one line and the start of the
//!   next are the same offset — upstream shows the caret at the end of the
//!   upper line, downstream at the start of the lower one;
//! * a direction boundary in bidirectional text, where one offset is drawn
//!   at two places on the line (the split caret).
//!
//! # Visual order
//!
//! A line's **stops** are the edges of its clusters from left to right.
//! Each cluster gives two: for a left-to-right cluster its left edge is its
//! logical start and its right edge its logical end; for a right-to-left
//! cluster the other way round. Where two neighbouring clusters meet, their
//! stops share an x: the same offset in plain text, two different offsets
//! at a direction boundary. Arrow keys walk the stops by x (visual order,
//! the default, as the phase asks); logical movement walks offsets.
//!
//! # Text on a path
//!
//! A story on a path is laid out straight and each cluster is then carried
//! onto the path by its own transform ([`PathFit::cluster_transform`]).
//! Movement still happens in the straight layout (bending the line does
//! not change logical or visual order), but everything drawn or hit tested
//! goes through the fitted clusters: a caret sits on the edge of the
//! fitted cluster it belongs to and turns with it
//! ([`CaretMap::caret_segments`]), a selection is one quadrilateral per
//! cluster ([`CaretMap::selection_quads`]), and a point hits the nearest
//! fitted cluster box ([`CaretMap::hit_point`]). A long manual kern in a
//! cluster's box (how the original lets text on a path start further
//! along) is cut into pieces that follow the path on their own, so the box
//! does not stick out straight along one tangent.

use std::ops::Range;

use kurbo::{Affine, Point};
use xarast_geom::Mp;
use xarast_text::{LaidCluster, LaidLine, Layout, PathFit, WordSegment};

/// A caret position: a byte offset in the story text, and which side it
/// belongs to.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct Caret {
    /// Byte offset in the laid-out text, on a cluster boundary.
    pub byte: usize,
    /// The caret belongs with the text before it: the end of a soft-wrapped
    /// line rather than the start of the next, the trailing edge of the
    /// cluster just passed rather than the leading edge of the next one.
    pub upstream: bool,
}

impl Caret {
    /// A caret belonging with the text after it.
    #[must_use]
    pub const fn at(byte: usize) -> Caret {
        Caret {
            byte,
            upstream: false,
        }
    }

    /// A caret belonging with the text before it.
    #[must_use]
    pub const fn after(byte: usize) -> Caret {
        Caret {
            byte,
            upstream: true,
        }
    }
}

/// What a caret movement moves by (`phase-09 §W9.4`).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum CaretMotion {
    /// One cluster. `visual`: in the direction the arrow points on screen,
    /// through bidi runs; otherwise in logical (typing) order.
    Character {
        /// Visual order.
        visual: bool,
    },
    /// To the start of the next or previous word, in logical order.
    Word,
    /// One line up or down, keeping the goal x.
    Line,
    /// The logical start of the caret's line.
    LineStart,
    /// The logical end of the caret's line.
    LineEnd,
    /// The start of the story.
    StoryStart,
    /// The end of the story.
    StoryEnd,
    /// As many lines as fit in `height`, up or down.
    Page {
        /// The height of the view, in story units.
        height: Mp,
    },
}

/// One stop of a line, in visual order.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Stop {
    /// Where, in story space.
    pub x: Mp,
    /// The caret this stop is.
    pub caret: Caret,
    /// The direction of the cluster the stop is an edge of.
    pub rtl: bool,
    /// The index, in the line's clusters, of the cluster the stop is an
    /// edge of; `None` on an empty line.
    pub cluster: Option<usize>,
}

/// A caret as drawn: a segment from the bottom of its line to the top, in
/// story space (`f64` millipoints, y up). Upright for straight text; on a
/// path it turns with the fitted cluster.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct CaretSegment {
    /// On the descent line.
    pub bottom: Point,
    /// On the ascent line.
    pub top: Point,
}

/// One rigid piece of a line on a path: a span of straight-layout x
/// carried onto the path by one transform.
#[derive(Clone, Debug)]
struct Piece {
    /// The cluster it belongs to; `None` for an empty line's caret place.
    cluster: Option<usize>,
    x0: f64,
    x1: f64,
    /// Straight layout to story space.
    xf: Affine,
    /// Story space to straight layout.
    inv: Affine,
}

/// The fit of a story on a path, with every line cut into pieces.
#[derive(Clone, Debug)]
struct OnPath {
    fit: PathFit,
    /// Per line, its pieces in cluster order.
    lines: Vec<Vec<Piece>>,
}

/// The transform that carries the straight span `x0..x1` of `line` onto
/// the path as if it were one character with that advance.
fn span_transform(fit: &PathFit, line: &LaidLine, x0: Mp, x1: Mp) -> Affine {
    let w = x1.saturating_sub(x0).max(Mp::ZERO);
    fit.cluster_transform(
        line,
        &LaidCluster {
            range: 0..0,
            x: x0,
            width: w,
            pen: x0,
            advance: w,
            rtl: false,
        },
    )
}

fn piece(cluster: Option<usize>, x0: Mp, x1: Mp, xf: Affine) -> Piece {
    let d = xf.determinant();
    let inv = if d.is_finite() && d.abs() > 1e-12 {
        xf.inverse()
    } else {
        Affine::IDENTITY
    };
    Piece {
        cluster,
        x0: x0.to_f64(),
        x1: x1.to_f64(),
        xf,
        inv,
    }
}

/// Cuts `line` into pieces: each cluster's glyphs as the fit places them,
/// and any gap in its box longer than both its glyphs and half the line's
/// size (a manual kern) into pieces no longer than that, each following
/// the path on its own.
fn line_pieces(fit: &PathFit, line: &LaidLine) -> Vec<Piece> {
    if line.clusters.is_empty() {
        return vec![piece(
            None,
            line.x,
            line.x,
            span_transform(fit, line, line.x, line.x),
        )];
    }
    let half_size = i64::from(line.size.raw() / 2).max(1_000);
    let mut out = Vec::new();
    for (k, c) in line.clusters.iter().enumerate() {
        let b0 = c.x;
        let b1 = c.x.saturating_add(c.width).max(b0);
        let g0 = c.pen.clamp(b0, b1);
        let g1 = c.pen.saturating_add(c.advance).clamp(g0, b1);
        let limit = half_size.max(i64::from(c.advance.raw()));
        let len = |a: Mp, b: Mp| i64::from(b.raw()) - i64::from(a.raw());
        let chunks = |a: Mp, b: Mp, out: &mut Vec<Piece>| {
            let n = (len(a, b) + limit - 1) / limit;
            let at = |j: i64| {
                let v = i64::from(a.raw()) + len(a, b) * j / n;
                Mp::new(i32::try_from(v).unwrap_or(a.raw()))
            };
            for i in 0..n {
                let (p0, p1) = (at(i), at(i + 1));
                out.push(piece(Some(k), p0, p1, span_transform(fit, line, p0, p1)));
            }
        };
        let start = if len(b0, g0) > limit {
            chunks(b0, g0, &mut out);
            g0
        } else {
            b0
        };
        let end = if len(g1, b1) > limit { g1 } else { b1 };
        out.push(piece(Some(k), start, end, fit.cluster_transform(line, c)));
        if end < b1 {
            chunks(g1, b1, &mut out);
        }
    }
    out
}

/// How far `v` is outside `lo..=hi`; 0 inside.
fn outside(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo - v
    } else if v > hi {
        v - hi
    } else {
        0.0
    }
}

/// Where a caret is drawn.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct CaretGeometry {
    /// The line index.
    pub line: usize,
    /// The primary caret: the stop the caret is on (its offset and its
    /// side), so a run of arrow presses moves it steadily across the line.
    pub x: Mp,
    /// The secondary caret, at a direction boundary: the other place the
    /// same offset is drawn. `None` when the two coincide.
    pub split: Option<Mp>,
    /// The bottom of the caret (y up).
    pub bottom: Mp,
    /// The top of the caret.
    pub top: Mp,
}

/// One highlighted span of a line.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct SelectionRect {
    /// The line index.
    pub line: usize,
    /// Left edge.
    pub x0: Mp,
    /// Right edge.
    pub x1: Mp,
    /// Bottom edge (y up).
    pub bottom: Mp,
    /// Top edge.
    pub top: Mp,
}

/// A laid-out story and what caret logic needs of its text.
#[derive(Debug, Clone)]
pub struct CaretMap {
    text: String,
    layout: Layout,
    words: Vec<WordSegment>,
    path: Option<OnPath>,
}

impl CaretMap {
    /// The map of `layout`, laid out from `text`.
    #[must_use]
    pub fn new(text: String, layout: Layout) -> CaretMap {
        let words = xarast_text::word_segments(&text);
        CaretMap {
            text,
            layout,
            words,
            path: None,
        }
    }

    /// The map of a story on a path: `layout` was made with
    /// [`PathFit::story_mode`] and `fit` carries it onto the path.
    #[must_use]
    pub fn on_path(text: String, layout: Layout, fit: PathFit) -> CaretMap {
        let lines = layout.lines.iter().map(|l| line_pieces(&fit, l)).collect();
        CaretMap {
            path: Some(OnPath { fit, lines }),
            ..CaretMap::new(text, layout)
        }
    }

    /// The fit onto a path, when the story is on one.
    #[must_use]
    pub fn path_fit(&self) -> Option<&PathFit> {
        self.path.as_ref().map(|p| &p.fit)
    }

    /// A straight-layout point of `line` at `x` on the edge of `cluster`
    /// (an index in the line's clusters), in story space: carried onto the
    /// path by the piece of that cluster nearest `x` when the story is on
    /// a path, unchanged otherwise.
    #[must_use]
    pub fn place(&self, line: usize, cluster: Option<usize>, x: Mp, y: Mp) -> Point {
        let p = Point::new(x.to_f64(), y.to_f64());
        let Some(path) = &self.path else {
            return p;
        };
        let (Some(pieces), Some(l)) = (path.lines.get(line), self.line(line)) else {
            return p;
        };
        let xf = cluster
            .and_then(|k| {
                pieces
                    .iter()
                    .filter(|q| q.cluster == Some(k))
                    .min_by(|a, b| outside(p.x, a.x0, a.x1).total_cmp(&outside(p.x, b.x0, b.x1)))
            })
            .map_or_else(|| span_transform(&path.fit, l, x, x), |q| q.xf);
        xf * p
    }

    /// The text the layout was made from.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The layout.
    #[must_use]
    pub const fn layout(&self) -> &Layout {
        &self.layout
    }

    /// The last caret offset: the length of the text.
    #[must_use]
    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// Whether the text is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    fn line(&self, i: usize) -> Option<&LaidLine> {
        self.layout.lines.get(i)
    }

    /// The number of lines (at least one is assumed by callers; an empty
    /// layout behaves as one empty line at the origin).
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.layout.lines.len().max(1)
    }

    /// The byte range of a line: its text without the paragraph break.
    #[must_use]
    pub fn line_range(&self, line: usize) -> Range<usize> {
        self.line(line).map_or(0..0, |l| l.logical_range.clone())
    }

    /// The index of the line a caret is shown on.
    #[must_use]
    pub fn line_of(&self, c: Caret) -> usize {
        let lines = &self.layout.lines;
        let b = c.byte.min(self.len());
        for (i, l) in lines.iter().enumerate() {
            let r = &l.logical_range;
            if b < r.start {
                // Before this line: the break between two paragraphs. Show
                // it at the end of the one before.
                return i.saturating_sub(1);
            }
            if b < r.end {
                return i;
            }
            if b == r.end {
                let next_starts_here = lines.get(i + 1).is_some_and(|n| n.logical_range.start == b);
                if next_starts_here && !c.upstream {
                    continue;
                }
                return i;
            }
        }
        lines.len().saturating_sub(1)
    }

    /// The vertical extent of a line: its descent below the baseline to
    /// its ascent above it.
    #[must_use]
    pub fn line_band(&self, line: usize) -> (Mp, Mp) {
        self.line(line).map_or((Mp::ZERO, Mp::ZERO), |l| {
            (
                l.baseline_y.saturating_sub(l.descent),
                l.baseline_y.saturating_add(l.ascent),
            )
        })
    }

    /// A line's stops, left to right.
    #[must_use]
    pub fn stops(&self, line: usize) -> Vec<Stop> {
        let Some(l) = self.line(line) else {
            return vec![Stop {
                x: Mp::ZERO,
                caret: Caret::at(0),
                rtl: false,
                cluster: None,
            }];
        };
        if l.clusters.is_empty() {
            return vec![Stop {
                x: l.x,
                caret: Caret::at(l.logical_range.start),
                rtl: l.base_rtl,
                cluster: None,
            }];
        }
        let mut order: Vec<usize> = (0..l.clusters.len()).collect();
        order.sort_by_key(|&i| l.clusters[i].x);
        let mut out = Vec::with_capacity(order.len() * 2);
        for i in order {
            let c = &l.clusters[i];
            let (left, right) = if c.rtl {
                (Caret::after(c.range.end), Caret::at(c.range.start))
            } else {
                (Caret::at(c.range.start), Caret::after(c.range.end))
            };
            out.push(Stop {
                x: c.x,
                caret: left,
                rtl: c.rtl,
                cluster: Some(i),
            });
            out.push(Stop {
                x: c.x.saturating_add(c.width),
                caret: right,
                rtl: c.rtl,
                cluster: Some(i),
            });
        }
        out
    }

    /// The index in `stops` that stands for `c`: the same offset and
    /// affinity, else the same offset, else the nearest offset.
    fn stop_index(stops: &[Stop], c: Caret) -> usize {
        if let Some(i) = stops.iter().position(|s| s.caret == c) {
            return i;
        }
        if let Some(i) = stops.iter().position(|s| s.caret.byte == c.byte) {
            return i;
        }
        stops
            .iter()
            .enumerate()
            .min_by_key(|(_, s)| s.caret.byte.abs_diff(c.byte))
            .map_or(0, |(i, _)| i)
    }

    /// Moves a caret onto the nearest real stop: an offset inside a cluster
    /// (a ligature, a base and its marks) goes to the cluster's start.
    #[must_use]
    pub fn snap(&self, c: Caret) -> Caret {
        let c = Caret {
            byte: c.byte.min(self.len()),
            ..c
        };
        let stops = self.stops(self.line_of(c));
        if stops.iter().any(|s| s.caret.byte == c.byte) {
            return c;
        }
        if let Some(l) = self.line(self.line_of(c))
            && let Some(cl) = l
                .clusters
                .iter()
                .find(|cl| cl.range.start < c.byte && c.byte < cl.range.end)
        {
            return Caret::at(cl.range.start);
        }
        stops[Self::stop_index(&stops, c)].caret
    }

    /// Where a caret is drawn: its line, its primary and secondary x, its
    /// height.
    #[must_use]
    pub fn caret_geometry(&self, c: Caret) -> CaretGeometry {
        let (line, primary, secondary) = self.caret_stops(c);
        let (bottom, top) = self.line_band(line);
        CaretGeometry {
            line,
            x: primary.x,
            split: secondary.map(|s| s.x),
            bottom,
            top,
        }
    }

    /// The stops a caret is drawn at: its line, the primary stop, and the
    /// secondary one at a direction boundary.
    fn caret_stops(&self, c: Caret) -> (usize, Stop, Option<Stop>) {
        let line = self.line_of(c);
        let stops = self.stops(line);
        let base_rtl = self.line(line).is_some_and(|l| l.base_rtl);
        let matching: Vec<Stop> = stops
            .iter()
            .filter(|s| s.caret.byte == c.byte)
            .copied()
            .collect();
        // Primary: the caret's own side, then the edge of a cluster in the
        // paragraph's direction.
        let Some(primary) = matching
            .iter()
            .min_by_key(|s| (s.caret.upstream != c.upstream, s.rtl != base_rtl))
            .copied()
        else {
            return (line, stops[Self::stop_index(&stops, c)], None);
        };
        let secondary = matching.iter().find(|s| s.x != primary.x).copied();
        (line, primary, secondary)
    }

    /// The caret as drawn, in story space: the primary segment and, at a
    /// direction boundary, the secondary one (full height; the tool draws
    /// it shorter). On a path each is the edge of its fitted cluster and
    /// turns with it.
    #[must_use]
    pub fn caret_segments(&self, c: Caret) -> (CaretSegment, Option<CaretSegment>) {
        let (line, primary, secondary) = self.caret_stops(c);
        let (bottom, top) = self.line_band(line);
        let seg = |s: Stop| CaretSegment {
            bottom: self.place(line, s.cluster, s.x, bottom),
            top: self.place(line, s.cluster, s.x, top),
        };
        (seg(primary), secondary.map(seg))
    }

    /// The caret nearest a story-space point, and how far the point is
    /// from the text: 0 on a line box (on a path, on a fitted cluster's
    /// box), else the larger of its distances across and along the
    /// nearest box, in story units.
    ///
    /// Straight text hits as [`CaretMap::hit`]. On a path the nearest
    /// fitted box wins and the caret goes to the nearer edge of its
    /// cluster.
    #[must_use]
    pub fn hit_point(&self, p: Point) -> (Caret, f64) {
        let Some(path) = &self.path else {
            let dist = self
                .layout
                .lines
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    let (bottom, top) = self.line_band(i);
                    let (x0, x1) = l.clusters.iter().fold((l.x, l.x), |(a, b), c| {
                        (a.min(c.x), b.max(c.x.saturating_add(c.width)))
                    });
                    outside(p.x, x0.to_f64(), x1.to_f64()).max(outside(
                        p.y,
                        bottom.to_f64(),
                        top.to_f64(),
                    ))
                })
                .fold(f64::INFINITY, f64::min);
            let caret = self.hit(Mp::from_f64_round(p.x), Mp::from_f64_round(p.y));
            return (caret, dist);
        };
        // Fitted boxes overlap on the inside of a curve: among boxes the
        // point is in, the one it is deepest in along the line wins.
        let mut best: Option<(usize, &Piece, Point, (f64, f64))> = None;
        for (i, pieces) in path.lines.iter().enumerate() {
            let (bottom, top) = self.line_band(i);
            for q in pieces {
                let local = q.inv * p;
                let d = outside(local.x, q.x0, q.x1).max(outside(
                    local.y,
                    bottom.to_f64(),
                    top.to_f64(),
                ));
                let depth = (local.x - (q.x0 + q.x1) / 2.0).abs() - (q.x1 - q.x0) / 2.0;
                let score = (d, depth);
                if best.is_none_or(|b| score < b.3) {
                    best = Some((i, q, local, score));
                }
            }
        }
        let Some((line, q, local, (d, _))) = best else {
            return (Caret::at(0), f64::INFINITY);
        };
        let Some(l) = self.line(line) else {
            return (Caret::at(0), d);
        };
        let Some(c) = q.cluster.and_then(|k| l.clusters.get(k)) else {
            return (Caret::at(l.logical_range.start), d);
        };
        let (x0, x1) = (c.x.to_f64(), c.x.saturating_add(c.width).to_f64());
        let left = (local.x - x0).abs() <= (local.x - x1).abs();
        let caret = if left != c.rtl {
            Caret::at(c.range.start)
        } else {
            Caret::after(c.range.end)
        };
        (caret, d)
    }

    /// The caret nearest a story-space point: the nearest line by its band,
    /// then the nearest stop on it.
    #[must_use]
    pub fn hit(&self, x: Mp, y: Mp) -> Caret {
        let n = self.layout.lines.len();
        let line = (0..n)
            .min_by_key(|&i| {
                let (bottom, top) = self.line_band(i);
                if y > top {
                    i64::from(y.raw()) - i64::from(top.raw())
                } else if y < bottom {
                    i64::from(bottom.raw()) - i64::from(y.raw())
                } else {
                    0
                }
            })
            .unwrap_or(0);
        self.hit_on_line(line, x)
    }

    /// The caret nearest `x` on a line. Between two stops at the same x (a
    /// direction boundary) the one on the pointer's side wins.
    #[must_use]
    pub fn hit_on_line(&self, line: usize, x: Mp) -> Caret {
        let stops = self.stops(line);
        let dist = |s: &Stop| (i64::from(s.x.raw()) - i64::from(x.raw())).abs();
        let best = stops.iter().map(dist).min().unwrap_or(0);
        let mut cands = stops.iter().filter(|s| dist(s) == best);
        let pick = if stops.iter().any(|s| dist(s) == best && x > s.x) {
            cands.next_back()
        } else {
            cands.next()
        };
        pick.map_or(Caret::at(0), |s| s.caret)
    }

    /// The primary x of a caret: the goal a vertical movement keeps.
    #[must_use]
    pub fn caret_x(&self, c: Caret) -> Mp {
        self.caret_geometry(c).x
    }

    /// Moves a caret. `forward` is right for visual character motion, down
    /// for line and page motion, and later in the text otherwise. `goal_x`
    /// is the x a run of vertical moves keeps (the caret's own x when
    /// `None`).
    #[must_use]
    pub fn move_caret(
        &self,
        c: Caret,
        motion: CaretMotion,
        forward: bool,
        goal_x: Option<Mp>,
    ) -> Caret {
        let c = self.snap(c);
        match motion {
            CaretMotion::Character { visual: true } => self.move_visual(c, forward),
            CaretMotion::Character { visual: false } => self.move_logical(c, forward),
            CaretMotion::Word => self.move_word(c, forward),
            CaretMotion::Line => self.move_lines(c, 1, forward, goal_x),
            CaretMotion::Page { height } => {
                let line = self.line_of(c);
                let (bottom, top) = self.line_band(line);
                let h = (top.saturating_sub(bottom)).raw().max(1);
                let n = usize::try_from((height.raw() / h).max(1)).unwrap_or(1);
                self.move_lines(c, n, forward, goal_x)
            }
            CaretMotion::LineStart => Caret::at(self.line_range(self.line_of(c)).start),
            CaretMotion::LineEnd => Caret::after(self.line_range(self.line_of(c)).end),
            CaretMotion::StoryStart => Caret::at(0),
            CaretMotion::StoryEnd => Caret::after(self.len()),
        }
    }

    fn move_visual(&self, c: Caret, right: bool) -> Caret {
        let line = self.line_of(c);
        let stops = self.stops(line);
        let i = Self::stop_index(&stops, c);
        let xi = stops[i].x;
        let found = if right {
            // The first stop further right: the trailing edge of the
            // cluster just crossed.
            stops[i + 1..].iter().find(|s| s.x > xi)
        } else {
            // The nearest stop further left: the leading edge of the
            // cluster just crossed.
            stops[..i].iter().rev().find(|s| s.x < xi)
        };
        if let Some(s) = found {
            return s.caret;
        }
        // Off the end of the line: on to the next line in reading order.
        let base_rtl = self.line(line).is_some_and(|l| l.base_rtl);
        let logically_forward = right != base_rtl;
        self.cross_line(line, logically_forward).unwrap_or(c)
    }

    /// The logical start of the next line, or the logical end of the
    /// previous one.
    fn cross_line(&self, line: usize, forward: bool) -> Option<Caret> {
        if forward {
            let r = self.line(line + 1)?.logical_range.clone();
            Some(Caret::at(r.start))
        } else {
            let r = self.line(line.checked_sub(1)?)?.logical_range.clone();
            Some(Caret::after(r.end))
        }
    }

    /// Every caret offset of a line and its neighbours, in order: all a
    /// one-cluster logical move can reach.
    fn boundaries(&self, line: usize) -> Vec<usize> {
        let mut v = Vec::new();
        let lines = &self.layout.lines;
        let near = lines
            .get(line.saturating_sub(1)..(line + 2).min(lines.len()))
            .unwrap_or(&[]);
        for l in near {
            v.push(l.logical_range.start);
            v.push(l.logical_range.end);
            for c in &l.clusters {
                v.push(c.range.start);
                v.push(c.range.end);
            }
        }
        if v.is_empty() {
            v.push(0);
        }
        v.sort_unstable();
        v.dedup();
        v
    }

    fn move_logical(&self, c: Caret, forward: bool) -> Caret {
        let b = self.boundaries(self.line_of(c));
        if forward {
            match b.iter().find(|&&x| x > c.byte) {
                Some(&x) => Caret::after(x),
                None => c,
            }
        } else {
            match b.iter().rev().find(|&&x| x < c.byte) {
                Some(&x) => Caret::at(x),
                None => c,
            }
        }
    }

    fn move_word(&self, c: Caret, forward: bool) -> Caret {
        let mut starts = self
            .words
            .iter()
            .filter(|w| w.word_like)
            .map(|w| w.range.start);
        if forward {
            Caret::at(starts.find(|&s| s > c.byte).unwrap_or_else(|| self.len()))
        } else {
            Caret::at(starts.rfind(|&s| s < c.byte).unwrap_or(0))
        }
    }

    fn move_lines(&self, c: Caret, n: usize, down: bool, goal_x: Option<Mp>) -> Caret {
        let line = self.line_of(c);
        let last = self.layout.lines.len().saturating_sub(1);
        let target = if down {
            if line >= last {
                return Caret::after(self.len());
            }
            (line + n).min(last)
        } else {
            if line == 0 {
                return Caret::at(0);
            }
            line.saturating_sub(n)
        };
        let x = goal_x.unwrap_or_else(|| self.caret_x(c));
        self.hit_on_line(target, x)
    }

    /// The word (or run of spaces or punctuation) around an offset: what a
    /// double click selects.
    #[must_use]
    pub fn word_at(&self, byte: usize) -> Range<usize> {
        self.words
            .iter()
            .find(|w| w.range.start <= byte && byte < w.range.end)
            .or_else(|| self.words.iter().rev().find(|w| w.range.end == byte))
            .map_or(byte..byte, |w| w.range.clone())
    }

    /// The highlight of the text between two offsets: per line, the
    /// merged spans of the selected clusters, left to right (visual
    /// order), plus a block for a selected paragraph break.
    #[must_use]
    pub fn selection_rects(&self, a: usize, b: usize) -> Vec<SelectionRect> {
        let (lo, hi) = (a.min(b), a.max(b));
        let mut out = Vec::new();
        if lo == hi {
            return out;
        }
        for (i, l) in self.layout.lines.iter().enumerate() {
            let Some(newline) = self.selected_line(l, lo, hi) else {
                continue;
            };
            let (bottom, top) = self.line_band(i);
            let mut spans: Vec<(Mp, Mp)> = l
                .clusters
                .iter()
                .filter(|c| c.range.start >= lo && c.range.end <= hi && !c.range.is_empty())
                .map(|c| (c.x, c.x.saturating_add(c.width)))
                .collect();
            if newline {
                spans.push(Self::break_span(l));
            }
            spans.sort_by_key(|s| s.0);
            let mut merged: Vec<(Mp, Mp)> = Vec::new();
            for (x0, x1) in spans {
                match merged.last_mut() {
                    Some(m) if x0 <= m.1 => m.1 = m.1.max(x1),
                    _ => merged.push((x0, x1)),
                }
            }
            out.extend(merged.into_iter().map(|(x0, x1)| SelectionRect {
                line: i,
                x0,
                x1,
                bottom,
                top,
            }));
        }
        out
    }

    /// Whether a line shows part of the selection `lo..hi`: `None` when
    /// it does not, else whether its paragraph break is selected.
    fn selected_line(&self, l: &LaidLine, lo: usize, hi: usize) -> Option<bool> {
        let r = &l.logical_range;
        let newline = l.ends_paragraph && r.end < self.len() && lo <= r.end && r.end < hi;
        ((r.end >= lo && r.start < hi) || newline).then_some(newline)
    }

    /// The block standing for a selected paragraph break: a third of the
    /// line's size wide, at the line's logical end.
    fn break_span(l: &LaidLine) -> (Mp, Mp) {
        let w = Mp::new((l.size.raw() / 3).max(1));
        let (lo_x, hi_x) = l
            .clusters
            .iter()
            .fold(None, |acc: Option<(Mp, Mp)>, c| {
                let (a, b) = (c.x, c.x.saturating_add(c.width));
                Some(acc.map_or((a, b), |(x0, x1)| (x0.min(a), x1.max(b))))
            })
            .unwrap_or((l.x, l.x));
        if l.base_rtl {
            (lo_x.saturating_sub(w), lo_x)
        } else {
            (hi_x, hi_x.saturating_add(w))
        }
    }

    /// The highlight of the text between two offsets as quadrilaterals in
    /// story space, corners in order around each: the rectangles of
    /// [`CaretMap::selection_rects`] for straight text; on a path one quad
    /// per selected cluster, its fitted box (and one for a selected
    /// paragraph break), so the highlight bends with the text.
    #[must_use]
    pub fn selection_quads(&self, a: usize, b: usize) -> Vec<[Point; 4]> {
        let quad = |x0: Mp, x1: Mp, bottom: Mp, top: Mp, left: Affine, right: Affine| {
            let p = |xf: Affine, x: Mp, y: Mp| xf * Point::new(x.to_f64(), y.to_f64());
            [
                p(left, x0, bottom),
                p(right, x1, bottom),
                p(right, x1, top),
                p(left, x0, top),
            ]
        };
        let Some(path) = &self.path else {
            return self
                .selection_rects(a, b)
                .into_iter()
                .map(|r| {
                    let id = Affine::IDENTITY;
                    quad(r.x0, r.x1, r.bottom, r.top, id, id)
                })
                .collect();
        };
        let (lo, hi) = (a.min(b), a.max(b));
        let mut out = Vec::new();
        if lo == hi {
            return out;
        }
        for (i, (l, pieces)) in self.layout.lines.iter().zip(&path.lines).enumerate() {
            let Some(newline) = self.selected_line(l, lo, hi) else {
                continue;
            };
            let (bottom, top) = self.line_band(i);
            for (k, c) in l.clusters.iter().enumerate() {
                if c.range.start < lo || c.range.end > hi || c.range.is_empty() {
                    continue;
                }
                let mut own = pieces.iter().filter(|q| q.cluster == Some(k));
                let Some(first) = own.next() else {
                    continue;
                };
                let last = own.next_back().unwrap_or(first);
                let x1 = c.x.saturating_add(c.width);
                out.push(quad(c.x, x1, bottom, top, first.xf, last.xf));
            }
            if newline {
                let (x0, x1) = Self::break_span(l);
                let xf = span_transform(&path.fit, l, x0, x1);
                out.push(quad(x0, x1, bottom, top, xf, xf));
            }
        }
        out
    }
}
