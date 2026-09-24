//! The text tool on a story that sits on a path (XARA-T-0250): the caret,
//! hit testing and the selection follow the fitted text, not the straight
//! layout it was fitted from. On `Designs/TextCurve.xar` with the pinned
//! fonts.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use kurbo::{Affine, Point, Vec2};
use xarast_app::fonts::FontService;
use xarast_app::text_edit::{Caret, CaretMap, CaretMotion};
use xarast_app::text_tool::{TextEditing, caret_map};
use xarast_app::{
    DevicePoint, DocumentId, Intent, OverlayShape, PointerButton, PointerSample, Session, TextKey,
    TextNav, ToolId,
};
use xarast_doc::{Document, NodeId, NodeKind, TextLayout};
use xarast_geom::Mp;

/// The pinned fonts, installed as the process's shared service before any
/// test here opens a session. Opening one fits the view to the page, which
/// measures the text with [`xarast_app::fonts::shared`]; that service is
/// chosen once per process, so a test that opened a session before another
/// installed the pinned fonts would pin the system's fonts for all of them,
/// and a session tool laying the story out with those would disagree with a
/// caret map built from these (XARA-T-0294). Every test calls this first.
fn fonts() -> Arc<FontService> {
    static PINNED: OnceLock<Arc<FontService>> = OnceLock::new();
    let fonts = PINNED.get_or_init(|| {
        FontService::from_dir(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../xarast-text/tests/fonts"),
        )
    });
    xarast_app::fonts::set_shared(Arc::clone(fonts));
    assert!(
        Arc::ptr_eq(&xarast_app::fonts::shared(), fonts),
        "the process's font service is not the pinned one"
    );
    Arc::clone(fonts)
}

fn corpus() -> Option<PathBuf> {
    let root = PathBuf::from(
        std::env::var("XARAST_XAR_CORPUS").unwrap_or_else(|_| "/home/user/xara-xtreme".into()),
    );
    if root.join("Designs/TextCurve.xar").is_file() {
        Some(root)
    } else {
        assert!(
            std::env::var("XARAST_CORPUS_REQUIRED").as_deref() != Ok("1"),
            "XARAST_CORPUS_REQUIRED=1 but no corpus"
        );
        eprintln!("skipping: no .xar corpus (set XARAST_XAR_CORPUS)");
        None
    }
}

fn open(root: &Path) -> Session {
    let path = root.join("Designs/TextCurve.xar");
    let bytes = std::fs::read(&path).unwrap();
    Session::open_bytes(DocumentId(1), &path, &bytes).unwrap()
}

fn on_path_stories(doc: &Document) -> Vec<NodeId> {
    doc.tree
        .preorder(doc.tree.root())
        .filter(|&n| {
            matches!(doc.tree.kind(n), Some(NodeKind::TextStory(t))
                if matches!(t.layout, TextLayout::OnPath { .. }))
        })
        .collect()
}

fn p(x: Mp, y: Mp) -> Point {
    Point::new(x.to_f64(), y.to_f64())
}

/// Distance from `q` to the line through `a` and `b`.
fn off_line(q: Point, a: Point, b: Point) -> f64 {
    let d = b - a;
    ((q - a).cross(d) / d.hypot()).abs()
}

/// Whether `x` is on a cluster's glyphs (within half its advance of
/// them), rather than out in a long manual kern.
fn near_glyph(c: &xarast_text::LaidCluster, x: Mp) -> bool {
    let g0 = c.pen.to_f64();
    let g1 = g0 + c.advance.to_f64();
    let slack = (c.advance.to_f64() / 2.0).max(500.0);
    x.to_f64() >= g0 - slack && x.to_f64() <= g1 + slack
}

#[test]
fn every_caret_stop_sits_on_its_fitted_cluster_and_turns_with_it() {
    let Some(root) = corpus() else {
        return;
    };
    let fonts = fonts();
    let s = open(&root);
    let stories = on_path_stories(&s.doc);
    assert!(stories.len() >= 3, "{stories:?}");
    let (mut checked, mut turned) = (0usize, 0usize);
    for story in stories {
        let map = caret_map(&s.doc, story, &fonts).unwrap();
        let fit = map.path_fit().expect("a story on a path keeps its fit");
        for (i, line) in map.layout().lines.iter().enumerate() {
            for stop in map.stops(i) {
                if map.line_of(stop.caret) != i {
                    continue;
                }
                let Some(k) = stop.cluster else {
                    continue;
                };
                let c = &line.clusters[k];
                let (seg, _) = map.caret_segments(stop.caret);
                if !near_glyph(c, stop.x) {
                    continue;
                }
                let xf: Affine = fit.cluster_transform(line, c);
                let (bottom, top) = map.line_band(i);
                // The fitted cluster's edge, within 1 mp.
                let want_bottom = xf * p(stop.x, bottom);
                let want_top = xf * p(stop.x, top);
                assert!(
                    (seg.bottom - want_bottom).hypot() < 1.0,
                    "story {story:?} line {i} stop {:?}: {:?} vs {want_bottom:?}",
                    stop.caret,
                    seg.bottom
                );
                assert!((seg.top - want_top).hypot() < 1.0);
                // On the glyph's baseline: the fitted baseline point is on
                // the caret, and the caret leans as the glyph's own
                // vertical does (square to the baseline unless sheared).
                let base = xf * p(stop.x, line.baseline_y);
                assert!(off_line(base, seg.bottom, seg.top) < 1.0);
                let vertical: Vec2 = xf * Point::new(0.0, 1.0) - xf * Point::ORIGIN;
                let up = seg.top - seg.bottom;
                assert!(
                    vertical.cross(up).abs() / (vertical.hypot() * up.hypot()) < 1e-6,
                    "the caret does not turn with its glyph"
                );
                if up.x.abs() > 0.05 * up.hypot() {
                    turned += 1;
                }
                checked += 1;
            }
        }
    }
    assert!(checked > 1000, "{checked} stops checked");
    // The text curves: most carets are not upright.
    assert!(turned * 2 > checked, "{turned} of {checked} turned");
}

/// A click on the left or right part of every drawn glyph puts the caret
/// at that glyph's nearer edge.
#[test]
fn a_click_on_a_fitted_glyph_puts_the_caret_at_that_glyph() {
    let Some(root) = corpus() else {
        return;
    };
    let fonts = fonts();
    let s = open(&root);
    let mut clicks = 0usize;
    for story in on_path_stories(&s.doc) {
        let map = caret_map(&s.doc, story, &fonts).unwrap();
        let fit = map.path_fit().unwrap();
        for line in &map.layout().lines {
            let lift = line.baseline_y.to_f64() + line.ascent.to_f64() * 0.25;
            for c in &line.clusters {
                if c.advance.raw() < 1000 || c.range.is_empty() {
                    continue;
                }
                let xf = fit.cluster_transform(line, c);
                // The glyph's part of the cluster's box (tight tracking can
                // make a glyph stick out of its box into the next one's).
                let g0 = c.pen.max(c.x).to_f64();
                let g1 = c
                    .pen
                    .saturating_add(c.advance)
                    .min(c.x.saturating_add(c.width))
                    .to_f64();
                if g1 - g0 < 500.0 {
                    continue;
                }
                for frac in [0.2, 0.8] {
                    let x = g0 + (g1 - g0) * frac;
                    let at = xf * Point::new(x, lift);
                    let (caret, dist) = map.hit_point(at);
                    assert!(dist < 1e-6, "the glyph's own point is off the text");
                    let left = (x - c.x.to_f64()).abs() <= (x - (c.x + c.width).to_f64()).abs();
                    let want = if left {
                        Caret::at(c.range.start)
                    } else {
                        Caret::after(c.range.end)
                    };
                    assert_eq!(caret, want, "cluster {:?} at {frac}", c.range);
                    clicks += 1;
                }
            }
        }
    }
    assert!(clicks > 1000, "{clicks}");
}

/// Hit testing is the inverse of drawing: the fitted place of every stop
/// hits that stop.
#[test]
fn every_stop_round_trips_through_a_click() {
    let Some(root) = corpus() else {
        return;
    };
    let fonts = fonts();
    let s = open(&root);
    let mut n = 0;
    for story in on_path_stories(&s.doc) {
        let map = caret_map(&s.doc, story, &fonts).unwrap();
        for (i, line) in map.layout().lines.iter().enumerate() {
            let mid = line.baseline_y.saturating_add(line.ascent.mul_ratio(1, 4));
            for stop in map.stops(i) {
                let Some(k) = stop.cluster else {
                    continue;
                };
                let c = &line.clusters[k];
                if c.width.raw() < 1000 {
                    continue;
                }
                // A quarter of the cluster in from its edge. (Nearer the
                // edge, neighbouring fitted boxes overlap where the path
                // bends hard and either may win.)
                let quarter = c.width.mul_ratio(1, 4);
                let inward = if stop.x == c.x {
                    quarter
                } else {
                    Mp::ZERO.saturating_sub(quarter)
                };
                let at = map.place(i, Some(k), stop.x.saturating_add(inward), mid);
                assert_eq!(map.hit_point(at).0, stop.caret, "line {i}");
                n += 1;
            }
        }
    }
    assert!(n > 1000, "{n}");
}

/// Arrow keys walk the text along the path in logical order, visiting
/// every boundary, and the drawn caret moves along the curve.
#[test]
fn arrows_walk_the_path_in_logical_order() {
    let Some(root) = corpus() else {
        return;
    };
    let fonts = fonts();
    let s = open(&root);
    for story in on_path_stories(&s.doc) {
        let map: CaretMap = caret_map(&s.doc, story, &fonts).unwrap();
        let mut c = Caret::at(0);
        let mut seen = vec![0usize];
        for _ in 0..10_000 {
            let n = map.move_caret(c, CaretMotion::Character { visual: true }, true, None);
            if n == c {
                break;
            }
            assert!(n.byte >= c.byte, "{c:?} -> {n:?}");
            if n.byte == c.byte {
                // A soft line end: the same offset on the next line.
                assert_ne!(map.line_of(n), map.line_of(c));
            } else {
                let (a, b) = (map.caret_segments(c).0, map.caret_segments(n).0);
                // Never more than a few em from the previous stop along the
                // path, unless the move crossed a line.
                if map.line_of(n) == map.line_of(c) {
                    let size = map.layout().lines[map.line_of(c)].size.to_f64();
                    let w = map.layout().lines[map.line_of(c)]
                        .clusters
                        .iter()
                        .map(|cl| cl.width.to_f64())
                        .fold(0.0, f64::max);
                    assert!((b.bottom - a.bottom).hypot() <= w.max(size) * 1.01);
                }
            }
            seen.push(n.byte);
            c = n;
        }
        let mut want: Vec<usize> = map
            .layout()
            .lines
            .iter()
            .flat_map(|l| l.clusters.iter().flat_map(|c| [c.range.start, c.range.end]))
            .collect();
        want.sort_unstable();
        want.dedup();
        seen.dedup();
        for b in want {
            assert!(seen.contains(&b), "offset {b} never visited");
        }
    }
}

fn sample(at: DevicePoint, t: u64) -> PointerSample {
    PointerSample {
        at,
        pressure: None,
        time_ms: t,
    }
}

/// Through the session, as the shell drives it: a click on a glyph of the
/// curve, an arrow, a selection of three characters drawn as three quads.
#[test]
fn the_tool_follows_the_path_through_the_session() {
    let Some(root) = corpus() else {
        return;
    };
    let fonts = fonts();
    let mut s = open(&root);
    s.apply(Intent::ChooseTool(ToolId::Text)).unwrap();
    let story = on_path_stories(&s.doc)[0];
    let map = caret_map(&s.doc, story, &fonts).unwrap();
    let Some(NodeKind::TextStory(node)) = s.doc.tree.kind(story) else {
        unreachable!()
    };
    let story_xf = node.transform.to_affine();
    // A letter in the middle of the first line, with letters after it.
    let line = &map.layout().lines[0];
    let k = line.clusters.len() / 2;
    let c = line.clusters[k].clone();
    let fit = map.path_fit().unwrap();
    let lift = line.baseline_y.to_f64() + line.ascent.to_f64() * 0.25;
    let x = c.pen.to_f64() + c.advance.to_f64() * 0.2;
    let doc_at = story_xf * (fit.cluster_transform(line, &c) * Point::new(x, lift));
    let dev = s
        .viewport
        .doc_to_device(xarast_geom::Point::from_f64_round(doc_at.x, doc_at.y));
    s.apply(Intent::PointerMove(sample(dev, 0))).unwrap();
    s.apply(Intent::PointerDown {
        button: PointerButton::Primary,
        sample: sample(dev, 0),
    })
    .unwrap();
    s.apply(Intent::PointerUp {
        button: PointerButton::Primary,
        sample: sample(dev, 0),
    })
    .unwrap();
    let sel = match s.text_state() {
        Some(TextEditing::Story(sel)) => sel,
        other => panic!("not editing: {other:?}"),
    };
    assert_eq!(sel.story, story);
    assert_eq!(sel.head.byte, c.range.start);

    // The drawn caret is the fitted edge, in document space, turned.
    let caret = |s: &Session| {
        s.overlay()
            .into_iter()
            .find_map(|o| match o {
                OverlayShape::Caret {
                    from,
                    to,
                    primary: true,
                    ..
                } => Some((from, to)),
                _ => None,
            })
            .unwrap()
    };
    let (from, _) = caret(&s);
    let want = story_xf * map.caret_segments(sel.head).0.bottom;
    let got = Point::new(from.x.to_f64(), from.y.to_f64());
    assert!((got - want).hypot() <= 1.0, "{got:?} vs {want:?}");

    // Right moves to the next boundary in logical order.
    let nav = |s: &mut Session, extend: bool| {
        s.apply(Intent::TextNav(TextNav {
            key: TextKey::Right,
            word: false,
            extend,
        }))
        .unwrap();
    };
    nav(&mut s, false);
    let head = match s.text_state() {
        Some(TextEditing::Story(sel)) => sel.head,
        _ => unreachable!(),
    };
    assert_eq!(head.byte, c.range.end);

    // Three characters selected: three quads along the curve.
    for _ in 0..3 {
        nav(&mut s, true);
    }
    let quads: Vec<_> = s
        .overlay()
        .into_iter()
        .filter_map(|o| match o {
            OverlayShape::Highlight { corners } => Some(corners),
            _ => None,
        })
        .collect();
    assert_eq!(quads.len(), 3);
    // Each is its cluster's fitted box, not a piece of one straight band.
    let angle = |q: &[xarast_geom::Point; 4]| {
        let (a, b) = (q[0].to_f64(), q[1].to_f64());
        (b.1 - a.1).atan2(b.0 - a.0)
    };
    let turned = (angle(&quads[0]) - angle(&quads[2])).abs();
    assert!(turned > 1e-3, "the quads do not turn with the path");
}

/// Without the corpus: a line of text around a half circle, laid out and
/// fitted as the walker does, hit tests and draws along the arc.
#[test]
fn a_synthetic_arc_carries_carets_hits_and_quads() {
    use xarast_text::{
        FontQuery, ParagraphStyle, PathFit, PathFitStyle, StoryInput, StyleRange, TextPath,
    };
    let f = fonts();
    let text = "Around the arc";
    let mut arc = kurbo::BezPath::new();
    // A half circle of radius 60 pt, left to right over the top.
    arc.move_to((-60_000.0, 0.0));
    arc.curve_to((-60_000.0, 80_000.0), (60_000.0, 80_000.0), (60_000.0, 0.0));
    let fit = PathFit::new(TextPath::new(&arc, false).unwrap(), PathFitStyle::default());
    let runs = [StyleRange::new(
        0..text.len(),
        FontQuery::new("Noto Sans"),
        Mp::new(12_000),
    )];
    let layout = f.ready().layout(&StoryInput {
        text,
        runs: &runs,
        paragraphs: &[ParagraphStyle::default()],
        kerns: &[],
        mode: fit.story_mode(),
    });
    let map = CaretMap::on_path(text.to_owned(), layout, fit.clone());
    let line = &map.layout().lines[0];
    let (bottom, top) = map.line_band(0);
    let mut angles = Vec::new();
    for stop in map.stops(0) {
        let c = &line.clusters[stop.cluster.unwrap()];
        let xf = fit.cluster_transform(line, c);
        let (seg, _) = map.caret_segments(stop.caret);
        assert!((seg.bottom - xf * p(stop.x, bottom)).hypot() < 1.0);
        assert!((seg.top - xf * p(stop.x, top)).hypot() < 1.0);
        let up = seg.top - seg.bottom;
        angles.push(up.y.atan2(up.x));
        // A quarter in, a quarter up: the hit comes back to this stop.
        let q = c.width.mul_ratio(1, 4);
        let x = if stop.x == c.x {
            stop.x.saturating_add(q)
        } else {
            stop.x.saturating_sub(q)
        };
        let at = map.place(0, stop.cluster, x, line.ascent.mul_ratio(1, 4));
        assert_eq!(map.hit_point(at), (stop.caret, 0.0));
    }
    // Over the top of the arc the caret turns clockwise all the way.
    assert!(
        angles.first().unwrap() - angles.last().unwrap() > 1.0,
        "{angles:?}"
    );
    // Far from the text: a distance, not a hit.
    assert!(map.hit_point(Point::new(0.0, -100_000.0)).1 > 50_000.0);
    // One quad per selected cluster, each its fitted box.
    let quads = map.selection_quads(0, 6);
    assert_eq!(quads.len(), 6);
    for (q, c) in quads.iter().zip(&line.clusters) {
        let xf = fit.cluster_transform(line, c);
        assert!((q[0] - xf * p(c.x, bottom)).hypot() < 1.0);
        assert!((q[2] - xf * p(c.x.saturating_add(c.width), top)).hypot() < 1.0);
    }
}
