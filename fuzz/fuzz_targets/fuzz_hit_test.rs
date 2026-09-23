//! Precise hit testing and the object hit index.
//!
//! Two halves per input:
//!
//! 1. An arbitrary path, transform, stroke style, pick point and tolerance
//!    through `hit_fill_transformed` and `hit_stroke_transformed`. The
//!    property: they return — no panic, no hang, whatever the geometry
//!    (extent-sized, degenerate, self-intersecting), the matrix (singular,
//!    mirroring, huge) or the style (negative width, NaN mitre, a dash
//!    pattern of one millipoint along fourteen kilometres) — and that
//!    `HitShape::hit` is exactly "stroke or fill".
//!
//!    "A larger radius never loses a hit" is deliberately *not* asserted:
//!    it holds for real regions, but the flattening tolerance follows the
//!    radius, and on a zero-area sliver (a singular transform, cancelling
//!    curves) two tolerances legitimately disagree about a point lying on
//!    it. The first run found exactly that with a matrix of determinant 0.
//! 2. An arbitrary sequence of edits and queries on a `HitIndex`, checked
//!    against a brute-force list after every step.

#![no_main]

mod common;

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use std::collections::HashMap;
use xarast_geom::{
    Cap, DashPattern, FillRule, HitIndex, HitShape, HitTolerance, Join, Matrix, Mp, Point, Rect,
    RectMode, StrokeStyle, hit_fill_transformed, hit_stroke_transformed,
};

#[derive(Arbitrary, Debug)]
struct Style {
    width: i32,
    caps: (u8, u8),
    join: u8,
    mitre: f64,
    dash: Option<(Vec<i32>, i32, Option<i32>)>,
}

#[derive(Arbitrary, Debug)]
enum IndexOp {
    Insert(u8, common::Pt, common::Pt, u64),
    Move(u8, common::Pt, common::Pt),
    SetZ(u8, u64),
    Remove(u8),
    Rebuild,
    Pick(common::Pt, u16),
    Marquee(common::Pt, common::Pt, bool),
}

#[derive(Arbitrary, Debug)]
struct Input {
    path: Vec<common::PathOp>,
    matrix: [f64; 4],
    translate: common::Pt,
    style: Style,
    rule: u8,
    at: common::Pt,
    radius: f64,
    min_width: f64,
    index: Vec<IndexOp>,
}

const RULES: [FillRule; 4] = [
    FillRule::NonZero,
    FillRule::EvenOdd,
    FillRule::Positive,
    FillRule::Negative,
];
const CAPS: [Cap; 3] = [Cap::Butt, Cap::Round, Cap::Square];
const JOINS: [Join; 3] = [Join::Mitre, Join::Round, Join::Bevel];

fn rect_distance_sq(p: Point, r: Rect) -> f64 {
    let (px, py) = p.to_f64();
    let dx = (r.lo.x.to_f64() - px).max(px - r.hi.x.to_f64()).max(0.0);
    let dy = (r.lo.y.to_f64() - py).max(py - r.hi.y.to_f64()).max(0.0);
    dx * dx + dy * dy
}

fuzz_target!(|input: Input| {
    // ── Precise tests.
    let path = common::build_path(&input.path);
    let t = input.translate.point();
    // The matrix is kept to what a document can hold: finite, and scaled
    // at most 1000x, since a larger factor turns an extent-sized path into
    // coordinates no stroker should be asked about.
    let c = |v: f64| common::finite(v).clamp(-1_000.0, 1_000.0);
    let m = Matrix {
        a: c(input.matrix[0]),
        b: c(input.matrix[1]),
        c: c(input.matrix[2]),
        d: c(input.matrix[3]),
        e: t.x,
        f: t.y,
    };
    let s = &input.style;
    let style = StrokeStyle {
        width: Mp(s.width.clamp(-1_000_000, 1_000_000)),
        cap_start: CAPS[usize::from(s.caps.0) % 3],
        cap_end: CAPS[usize::from(s.caps.1) % 3],
        join: JOINS[usize::from(s.join) % 3],
        mitre_limit: s.mitre,
        dash: s.dash.as_ref().map(|(els, off, rw)| DashPattern {
            elements: els.iter().take(8).map(|&e| Mp(e)).collect(),
            offset: Mp(*off),
            reference_width: rw.map(Mp),
        }),
    };
    let rule = RULES[usize::from(input.rule) % 4];
    let p = input.at.point();
    let tol = HitTolerance::new(input.radius, input.min_width);
    let fill = hit_fill_transformed(&path, m, rule, p, tol);
    let stroke = hit_stroke_transformed(&path, m, &style, p, tol);
    let shape = HitShape {
        path: &path,
        transform: m,
        fill: Some(rule),
        stroke: Some(&style),
    };
    assert_eq!(shape.hit(p, tol).is_some(), fill || stroke);

    // ── The index against a brute-force list.
    let mut idx: HitIndex<u8> = HitIndex::new();
    let mut model: HashMap<u8, (Rect, u64)> = HashMap::new();
    for op in input.index.iter().take(256) {
        match op {
            IndexOp::Insert(k, a, b, z) => {
                let r = Rect::new(a.point(), b.point());
                assert_eq!(idx.insert(*k, r, *z), model.insert(*k, (r, *z)));
            }
            IndexOp::Move(k, a, b) => {
                let r = Rect::new(a.point(), b.point());
                let had = model.get_mut(k).map(|e| e.0 = r).is_some();
                assert_eq!(idx.set_bounds(*k, r), had);
            }
            IndexOp::SetZ(k, z) => {
                let had = model.get_mut(k).map(|e| e.1 = *z).is_some();
                assert_eq!(idx.set_z(*k, *z), had);
            }
            IndexOp::Remove(k) => {
                assert_eq!(idx.remove(*k), model.remove(k));
            }
            IndexOp::Rebuild => idx.rebuild(),
            IndexOp::Pick(at, r) => {
                let at = at.point();
                let r = i32::from(*r) * 16;
                let got: Vec<(u8, u64)> = idx.candidates_at(at, Mp(r)).collect();
                assert!(got.windows(2).all(|w| w[0].1 >= w[1].1), "not in z order");
                let mut got_sorted = got;
                got_sorted.sort_unstable();
                let r2 = f64::from(r) * f64::from(r);
                let mut want: Vec<(u8, u64)> = model
                    .iter()
                    .filter(|(_, (b, _))| !b.is_empty() && rect_distance_sq(at, *b) <= r2)
                    .map(|(&k, &(_, z))| (k, z))
                    .collect();
                want.sort_unstable();
                assert_eq!(got_sorted, want);
            }
            IndexOp::Marquee(a, b, enclose) => {
                let q = Rect::new(a.point(), b.point());
                let mode = if *enclose {
                    RectMode::Enclose
                } else {
                    RectMode::Touch
                };
                let mut got = Vec::new();
                idx.query_rect(q, mode, &mut got);
                got.sort_unstable();
                let mut want: Vec<u8> = model
                    .iter()
                    .filter(|(_, (b, _))| {
                        !b.is_empty()
                            && if *enclose {
                                q.contains_rect(*b)
                            } else {
                                q.intersects(*b)
                            }
                    })
                    .map(|(&k, _)| k)
                    .collect();
                want.sort_unstable();
                assert_eq!(got, want);
            }
        }
        assert_eq!(idx.len(), model.len());
    }
});
