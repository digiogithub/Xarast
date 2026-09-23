//! Property tests of node editing (`phase-07` acceptance criterion 15):
//! add∘delete is the identity (to within the split's rounding), smooth ⇄
//! cusp ⇄ smooth is the identity on a smooth node, and closing then
//! reopening keeps the point count.

use kurbo::{ParamCurve, ParamCurveNearest};
use proptest::prelude::*;
use xarast_geom::{EditPath, NodeRef, Path, PathBuilder, Point, SegRef};

fn coord() -> impl Strategy<Value = i32> {
    -200_000i32..200_000
}

fn point() -> impl Strategy<Value = Point> {
    (coord(), coord()).prop_map(|(x, y)| Point::raw(x, y))
}

/// An open path of 2–6 segments, each a line or a cubic.
fn path() -> impl Strategy<Value = Path> {
    (
        point(),
        prop::collection::vec((any::<bool>(), point(), point(), point()), 1..6),
    )
        .prop_map(|(start, segs)| {
            let mut b = PathBuilder::new();
            b.move_to(start);
            for (curve, c1, c2, p) in segs {
                if curve {
                    b.cubic_to(c1, c2, p);
                } else {
                    b.line_to(p);
                }
            }
            b.build()
        })
}

/// The largest distance from a sample of `b` to the curve `a`.
fn deviation(a: &Path, b: &Path) -> f64 {
    let sa: Vec<kurbo::PathSeg> = a.segments().map(|s| s.to_kurbo()).collect();
    let mut worst = 0.0f64;
    for s in b.segments() {
        let k = s.to_kurbo();
        for i in 0..=16 {
            let q = k.eval(f64::from(i) / 16.0);
            let d = sa
                .iter()
                .map(|t| t.nearest(q, 1e-6).distance_sq.sqrt())
                .fold(f64::INFINITY, f64::min);
            worst = worst.max(d);
        }
    }
    worst
}

fn curve_at(p: &Path, seg: usize) -> bool {
    let e = EditPath::from_path(p);
    let count = e.subpaths[0].segment_count();
    e.is_curve(SegRef {
        subpath: 0,
        segment: seg % count,
    })
}

proptest! {
    #[test]
    fn adding_then_deleting_a_point_gives_the_path_back(p in path(), seg in 0usize..6, t in 0.1f64..0.9) {
        let mut e = EditPath::from_path(&p);
        let count = e.subpaths[0].segment_count();
        let s = SegRef { subpath: 0, segment: seg % count };
        let curve = e.is_curve(s);
        let n = e.split(s, t).expect("a segment");
        // The split does not move the curve.
        prop_assert!(deviation(&p, &e.to_path()) < 2.0);
        e.delete(&[n]);
        let back = e.to_path();
        prop_assert_eq!(back.verbs(), p.verbs());
        if curve {
            // The recovered curve is the original to within the rounding
            // the split introduced, which recovering the handles magnifies
            // by 1/t: a few hundredths of a point on curves 400 pt across.
            let bound = 2.0 / t.min(1.0 - t);
            prop_assert!(deviation(&p, &back) < bound, "{}", deviation(&p, &back));
            prop_assert!(deviation(&back, &p) < bound, "{}", deviation(&back, &p));
        } else {
            prop_assert_eq!(back, p);
        }
    }

    #[test]
    fn smooth_cusp_smooth_is_the_identity_on_a_smooth_node(p in path(), seg in 0usize..6, t in 0.1f64..0.9) {
        let mut e = EditPath::from_path(&p);
        let count = e.subpaths[0].segment_count();
        let n = e.split(SegRef { subpath: 0, segment: seg % count }, t).expect("a segment");
        if !e.node(n).unwrap().is_smooth() {
            // A node on a line is a corner.
            prop_assert!(!curve_at(&p, seg));
            e.make_smooth(n);
        }
        let smooth = e.clone();
        // Smoothing a smooth node changes nothing ...
        e.make_smooth(n);
        prop_assert_eq!(&e, &smooth);
        // ... and neither does making it a cusp and smooth again.
        e.make_cusp(n);
        e.make_smooth(n);
        prop_assert_eq!(e, smooth);
    }

    #[test]
    fn closing_then_reopening_keeps_the_point_count(p in path()) {
        let mut e = EditPath::from_path(&p);
        e.close(0);
        e.open(0);
        prop_assert_eq!(e.to_path().points().len(), p.points().len());
    }

    #[test]
    fn the_node_view_round_trips_exactly(p in path()) {
        let e = EditPath::from_path(&p);
        prop_assert_eq!(e.to_path(), p.clone());
        let roles = e.layout();
        prop_assert_eq!(roles.len(), p.points().len());
        for n in e.node_refs() {
            let i = e.node_point_index(n).expect("every node is written");
            prop_assert_eq!(e.node_at_point_index(i), Some(n));
            prop_assert_eq!(p.points()[i as usize], e.node(n).unwrap().at);
        }
        let _ = NodeRef::new(0, 0);
    }
}
