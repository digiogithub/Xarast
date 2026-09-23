//! T9.5.3: the fidelity measurement that settled architecture open
//! question 4 ("does text on a path need our own layout pass over
//! parley?"). Kept as a test so the numbers in `docs/memory/text.md` can
//! be reproduced:
//!
//! ```text
//! XARAST_XAR_CORPUS=… cargo test -p xarast-app --lib text_path_fidelity -- --nocapture
//! ```
//!
//! * **Reference**: the original's placement rule (`docs/memory/text.md`,
//!   "Text on a path"), evaluated independently of the production code:
//!   the path is flattened to a fine polyline (0.25 mp) and walked, where
//!   production inverts kurbo's exact arc length per segment.
//! * **Spike A** (production, [`xarast_text::PathFit`]): lay out straight,
//!   then carry each cluster onto the path.
//! * **Spike B** (this file only; the loser, never shipped): layout-aware
//!   spacing. Each cluster's step along the path is its box width scaled
//!   by `1 / (1 − κ·h)`, κ the path's signed curvature there and h half a
//!   line's x-height, so glyphs neither crowd on the inside of a curve nor
//!   spread on the outside. This is what feeding "the advance available
//!   at each position" back into layout amounts to for a single line.
//! * **Metric**: per glyph, the distance between its origin (its left end
//!   on the baseline) and the reference's, over its advance. Pass: 95th
//!   percentile ≤ 0.25, maximum ≤ 0.5.

use std::collections::HashMap;
use std::path::Path as FsPath;
use std::sync::Arc;

use kurbo::{BezPath, Point, Vec2};
use xarast_doc::builder::{BuildLimits, skeleton};
use xarast_doc::{
    AttrValue, CharsTransform, Document, Justification, NodeId, NodeKind, StoryText, TextItem,
    TextLayout, TextStoryNode, TypefaceRef,
};
use xarast_geom::{Matrix, Mp};
use xarast_text::{LaidCluster, LaidLine};

use crate::fonts::FontService;

fn fonts() -> Arc<FontService> {
    FontService::from_dir(
        &FsPath::new(env!("CARGO_MANIFEST_DIR")).join("../xarast-text/tests/fonts"),
    )
}

/// A path flattened finely and walked by distance: the reference.
struct Polyline {
    pts: Vec<Point>,
    /// Arc length at each point.
    at: Vec<f64>,
    closed: bool,
    /// The directions of the path's first and last two control points: the
    /// original extends an open path along them and offsets later lines
    /// by the normal of the first.
    start_dir: Vec2,
    end_dir: Vec2,
}

impl Polyline {
    fn new(path: &BezPath, closed: bool) -> Polyline {
        let mut pts: Vec<Point> = Vec::new();
        kurbo::flatten(path, 0.25, |el| match el {
            kurbo::PathEl::MoveTo(p) | kurbo::PathEl::LineTo(p) => {
                if pts.last() != Some(&p) {
                    pts.push(p);
                }
            }
            kurbo::PathEl::ClosePath => {
                if let Some(&first) = pts.first()
                    && pts.last() != Some(&first)
                {
                    pts.push(first);
                }
            }
            _ => {}
        });
        let mut at = vec![0.0];
        for w in pts.windows(2) {
            at.push(at[at.len() - 1] + (w[1] - w[0]).hypot());
        }
        let coords: Vec<Point> = path
            .elements()
            .iter()
            .flat_map(|el| match *el {
                kurbo::PathEl::MoveTo(p) | kurbo::PathEl::LineTo(p) => vec![p],
                kurbo::PathEl::QuadTo(a, b) => vec![a, b],
                kurbo::PathEl::CurveTo(a, b, c) => vec![a, b, c],
                kurbo::PathEl::ClosePath => vec![],
            })
            .collect();
        let unit = |v: Vec2| v / v.hypot();
        let start_dir = coords
            .iter()
            .skip(1)
            .map(|p| *p - coords[0])
            .find(|v| v.hypot() > 1e-9)
            .map_or(Vec2::new(1.0, 0.0), unit);
        let last = coords[coords.len() - 1];
        let end_dir = coords
            .iter()
            .rev()
            .skip(1)
            .map(|p| last - *p)
            .find(|v| v.hypot() > 1e-9)
            .map_or(Vec2::new(1.0, 0.0), unit);
        Polyline {
            pts,
            at,
            closed,
            start_dir,
            end_dir,
        }
    }

    fn length(&self) -> f64 {
        self.at[self.at.len() - 1]
    }

    fn dir(&self, i: usize) -> Vec2 {
        let d = self.pts[i + 1] - self.pts[i];
        d / d.hypot()
    }

    /// Point and unit tangent at `s`, extended straight past the ends.
    fn at(&self, s: f64) -> (Point, Vec2) {
        let len = self.length();
        let s = if self.closed { s.rem_euclid(len) } else { s };
        if s < 0.0 {
            return (self.pts[0] + self.start_dir * s, self.start_dir);
        }
        if s > len {
            let end = self.pts[self.pts.len() - 1];
            return (end + self.end_dir * (s - len), self.end_dir);
        }
        let n = self.pts.len() - 1;
        let i = match self.at.binary_search_by(|v| v.total_cmp(&s)) {
            Ok(i) => i.min(n - 1),
            Err(i) => i.saturating_sub(1).min(n - 1),
        };
        let d = self.dir(i);
        (self.pts[i] + d * (s - self.at[i]), d)
    }

    fn angle(&self, s: f64) -> f64 {
        let (_, d) = self.at(s);
        d.y.atan2(d.x)
    }

    /// Signed curvature at `s`, the turning rate over a window `w`.
    fn curvature(&self, s: f64, w: f64) -> f64 {
        let mut d = self.angle(s + w) - self.angle(s - w);
        while d > std::f64::consts::PI {
            d -= std::f64::consts::TAU;
        }
        while d < -std::f64::consts::PI {
            d += std::f64::consts::TAU;
        }
        d / (2.0 * w)
    }
}

/// Where the reference rule puts a cluster's origin whose advance centre
/// is `dist` along the path, on a line at `baseline` (story space).
fn place(poly: &Polyline, normal: Vec2, dist: f64, half: f64, baseline: f64) -> Point {
    let (p, d) = poly.at(dist);
    p + normal * baseline - d * half
}

#[derive(Default)]
struct Tally {
    a: Vec<f64>,
    b: Vec<f64>,
}

fn p95_max(v: &mut [f64]) -> Score {
    if v.is_empty() {
        return (0.0, 0.0);
    }
    v.sort_by(f64::total_cmp);
    let i = ((v.len() as f64) * 0.95).ceil() as usize;
    (v[i.clamp(1, v.len()) - 1], v[v.len() - 1])
}

/// Measures one story: A and B against the reference, per glyph cluster.
fn measure_story(fonts: &FontService, doc: &Document, story: NodeId, tally: &mut Tally) {
    let Some(NodeKind::TextStory(node)) = doc.tree.kind(story) else {
        return;
    };
    let TextLayout::OnPath {
        reversed,
        left_indent,
        chars,
        ..
    } = node.layout
    else {
        return;
    };
    // The measurement covers what the corpus has: tangential, unreflected.
    assert!(!chars.reflected);
    let mut stack = xarast_doc::attr::resolve_inherited(&doc.tree, story, &doc.defaults);
    let Some(st) = StoryText::collect(&doc.tree, story, &mut stack, &mut |_, a| {
        Arc::new(a.value.clone())
    }) else {
        return;
    };
    let (_, layout, fit) = crate::text::lay_story(fonts, &doc.tree, &st, node);
    let fit = fit.expect("a story on a path with its path");
    let data = doc
        .tree
        .children(story)
        .find_map(|c| match doc.tree.kind(c) {
            Some(NodeKind::Path(p)) => Some(Arc::clone(&p.data)),
            _ => None,
        })
        .unwrap();
    let local = node.transform.to_affine().inverse() * data.to_bez_path();
    let local = if reversed {
        local.reverse_subpaths()
    } else {
        local
    };
    let poly = Polyline::new(&local, fit.path().is_closed());
    let normal = Vec2::new(-poly.start_dir.y, poly.start_dir.x);
    let li = left_indent.to_f64();

    for line in &layout.lines {
        let base = line.baseline_y.to_f64();
        let h = 0.25 * line.size.to_f64();
        let inked: Vec<&LaidCluster> = line
            .clusters
            .iter()
            .filter(|c| c.advance > Mp::ZERO && has_glyphs(line, c))
            .collect();
        // Spike B: re-space along the path from the line's first cluster.
        let mut visual: Vec<&LaidCluster> = line.clusters.iter().collect();
        visual.sort_by_key(|c| c.x);
        let mut b_pen: HashMap<usize, f64> = HashMap::new();
        if let Some(first) = visual.first() {
            let mut s = first.x.to_f64() + li;
            for c in &visual {
                let lead = (c.pen - c.x).to_f64();
                b_pen.insert(c.range.start, s + lead);
                let k = poly.curvature(s + c.width.to_f64() / 2.0, h.max(1.0));
                let factor = (1.0 / (1.0 - k * h)).clamp(0.25, 4.0);
                s += c.width.to_f64() * factor;
            }
        }
        for c in inked {
            let adv = c.advance.to_f64();
            let half = adv / 2.0;
            let pen = c.pen.to_f64();
            let reference = place(&poly, normal, pen + li + half, half, base);
            let a = fit.cluster_transform(line, c) * Point::new(pen, base);
            let b_start = b_pen.get(&c.range.start).copied().unwrap_or(pen + li);
            let b = place(&poly, normal, b_start + half, half, base);
            tally.a.push((a - reference).hypot() / adv);
            tally.b.push((b - reference).hypot() / adv);
        }
    }
}

fn has_glyphs(line: &LaidLine, c: &LaidCluster) -> bool {
    line.runs
        .iter()
        .any(|r| r.glyphs.iter().any(|g| g.cluster == c.range.start))
}

/// A synthetic story on a path: `text` at `size_pt`, `justification`,
/// following `path` (document space; the story sits at the origin).
fn synthetic(text: &str, size_pt: i32, justification: Justification, path: BezPath) -> Document {
    let mut b = skeleton(BuildLimits::default()).unwrap();
    b.node(NodeKind::TextStory(Box::new(TextStoryNode {
        transform: Matrix::IDENTITY,
        layout: TextLayout::OnPath {
            reversed: false,
            tangential: true,
            left_indent: Mp::ZERO,
            right_indent: Mp::ZERO,
            chars: CharsTransform::default(),
        },
        ..TextStoryNode::default()
    })))
    .unwrap();
    b.push_scope().unwrap();
    b.attribute(AttrValue::FontTypeface(Arc::new(TypefaceRef {
        full_name: Arc::from("Noto Sans"),
        family: Arc::from("Noto Sans"),
        panose: None,
    })))
    .unwrap();
    b.attribute(AttrValue::FontSize(Mp::new(size_pt * 1000)))
        .unwrap();
    let mut p = xarast_doc::PathNode::new(xarast_geom::Path::from_bez_path(&path).0);
    p.filled = false;
    b.node(NodeKind::Path(Box::new(p))).unwrap();
    b.node(NodeKind::TextLine(Box::default())).unwrap();
    b.push_scope().unwrap();
    b.attribute(AttrValue::Justification(justification))
        .unwrap();
    for c in text.chars() {
        b.node(NodeKind::TextItem(TextItem::Char(c))).unwrap();
    }
    b.node(NodeKind::TextItem(TextItem::LineBreak(true)))
        .unwrap();
    b.pop_scope();
    b.pop_scope();
    b.finish().unwrap().0
}

/// A circle of `r` points about the origin, counter-clockwise from its
/// lowest point, so glyph tops point inwards: whole and closed, or a 300°
/// open arc.
fn circle(r: f64, closed: bool) -> BezPath {
    let sweep = if closed {
        std::f64::consts::TAU
    } else {
        300f64.to_radians()
    };
    let start = -std::f64::consts::FRAC_PI_2;
    let arc = kurbo::Arc::new((0.0, 0.0), (r * 1000.0, r * 1000.0), start, sweep, 0.0);
    let mut p = BezPath::new();
    p.move_to(arc.center + Vec2::from_angle(start) * arc.radii.x);
    for el in arc.append_iter(0.1) {
        p.push(el);
    }
    if closed {
        p.close_path();
    }
    p
}

fn report(name: &str, t: &mut Tally) -> (Score, Score) {
    let n = t.a.len();
    let a = p95_max(&mut t.a);
    let b = p95_max(&mut t.b);
    eprintln!(
        "{name}: {n} glyphs; A p95 {:.4} max {:.4}; B p95 {:.4} max {:.4} (advances)",
        a.0, a.1, b.0, b.1
    );
    (a, b)
}

fn stories_on_a_path(doc: &Document) -> Vec<NodeId> {
    // Stories under an opaque record (a mould) are not drawn: skip them.
    doc.tree
        .preorder(doc.tree.root())
        .filter(|n| {
            matches!(doc.tree.kind(*n), Some(NodeKind::TextStory(s))
                if matches!(s.layout, TextLayout::OnPath { .. }))
                && !doc
                    .tree
                    .ancestors(*n)
                    .any(|a| matches!(doc.tree.kind(a), Some(NodeKind::Opaque(_))))
        })
        .collect()
}

/// `(95th percentile, maximum)` displacement, in advances.
type Score = (f64, f64);

const PASS_P95: f64 = 0.25;
const PASS_MAX: f64 = 0.5;

fn passes((p95, max): Score) -> bool {
    p95 <= PASS_P95 && max <= PASS_MAX
}

#[test]
fn spike_a_matches_the_reference_on_every_fixture_and_spike_b_does_not() {
    let fonts = fonts();
    let mut results: Vec<(String, Score, Score)> = Vec::new();

    // Synthetic 1: a tight 300° arc, radius twice the size, text inside.
    // 18 characters: about the arc's 125 pt, so the curve is measured,
    // not the straight run past its end.
    let sample = "Tight curves crowd";
    let doc = synthetic(sample, 12, Justification::Left, circle(24.0, false));
    let mut t = Tally::default();
    for s in stories_on_a_path(&doc) {
        measure_story(&fonts, &doc, s, &mut t);
    }
    let (a, b) = report("high curvature (r = 2 em, open)", &mut t);
    results.push(("high curvature".into(), a, b));

    // Synthetic 2: a closed circle with full justification.
    let doc = synthetic(
        "Round and round the text goes, justified all the way",
        14,
        Justification::Full,
        circle(60.0, true),
    );
    let mut t = Tally::default();
    for s in stories_on_a_path(&doc) {
        measure_story(&fonts, &doc, s, &mut t);
    }
    let (a, b) = report("closed path, full justification", &mut t);
    results.push(("closed + justified".into(), a, b));

    // The corpus fixtures, when the corpus is here.
    let root = std::env::var("XARAST_XAR_CORPUS").map(std::path::PathBuf::from);
    if let Ok(root) = root
        && root.join("Designs/TextCurve.xar").is_file()
    {
        for rel in [
            "Designs/TextCurve.xar",
            "TextDesigns/AngledText.xar",
            "TextDesigns/Rotated.xar",
        ] {
            let path = root.join(rel);
            let bytes = std::fs::read(&path).unwrap();
            let s = crate::Session::open_bytes(crate::DocumentId(1), &path, &bytes).unwrap();
            let stories = stories_on_a_path(&s.doc);
            if rel == "Designs/TextCurve.xar" {
                assert_eq!(stories.len(), 3, "{rel}");
            } else {
                // No story on a path: the fit never runs, so A and B are
                // both the straight layout, displacement 0.
                assert!(stories.is_empty(), "{rel}");
            }
            let mut t = Tally::default();
            for st in stories {
                measure_story(&fonts, &s.doc, st, &mut t);
            }
            let (a, b) = report(rel, &mut t);
            results.push((rel.into(), a, b));
        }
    } else {
        eprintln!("corpus fixtures skipped (set XARAST_XAR_CORPUS)");
    }

    for (name, a, _) in &results {
        assert!(passes(*a), "spike A fails on {name}: {a:?}");
    }
    let (_, _, b) = &results[0];
    assert!(
        !passes(*b),
        "spike B would stay within the tolerance on the tight curve: {b:?}"
    );
}
