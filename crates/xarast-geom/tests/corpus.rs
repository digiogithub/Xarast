//! The pathological path corpus.
//!
//! These cases are **our own**, authored from geometric categories rather
//! than extracted from any existing program: the clean-room rule applies to
//! test data as much as to code. The categories are the ones that break
//! boolean engines in practice — self-intersection, tangency, coincident
//! edges, zero-area subpaths, cusps, and extreme scale ratios — plus the
//! sizes that stress the algorithms rather than the geometry.

use xarast_geom::{Mp, Path, Point, Rect};

/// One named case.
#[derive(Debug)]
pub struct Case {
    /// A short name, used in assertion messages and snapshot keys.
    pub name: &'static str,
    /// The path itself.
    pub path: Path,
}

/// Builds a closed polygon from raw millipoint coordinates.
fn poly(pts: &[(i32, i32)]) -> Path {
    let mut b = Path::builder();
    for (i, &(x, y)) in pts.iter().enumerate() {
        let p = Point::raw(x, y);
        if i == 0 {
            b.move_to(p);
        } else {
            b.line_to(p);
        }
    }
    b.close();
    b.build()
}

/// A regular star polygon, which self-intersects whenever `step > 1`.
fn star(n: usize, step: usize, r: f64) -> Path {
    let mut b = Path::builder();
    for i in 0..n {
        let a = std::f64::consts::TAU * (i * step % n) as f64 / n as f64;
        let p = Point::from_f64_round(r * a.cos(), r * a.sin());
        if i == 0 {
            b.move_to(p);
        } else {
            b.line_to(p);
        }
    }
    b.close();
    b.build()
}

/// A circle approximated by `n` cubics, for the large-input cases.
fn many_segment_blob(n: usize, r: f64) -> Path {
    let mut b = Path::builder();
    let step = std::f64::consts::TAU / n as f64;
    // A radius that wobbles keeps the segments from being collinear, which
    // would let the engine collapse them and defeat the point of the case.
    let rad = |i: usize| r * (1.0 + 0.05 * ((i as f64) * 0.7).sin());
    let at = |i: usize| {
        let a = step * i as f64;
        Point::from_f64_round(rad(i) * a.cos(), rad(i) * a.sin())
    };
    b.move_to(at(0));
    for i in 1..n {
        b.line_to(at(i));
    }
    b.close();
    b.build()
}

/// Every case in the corpus.
pub fn corpus() -> Vec<Case> {
    let mut v: Vec<Case> = Vec::new();
    let mut add = |name: &'static str, path: Path| v.push(Case { name, path });

    add("empty", Path::new());
    add("single_move", {
        let mut b = Path::builder();
        b.move_to(Point::raw(10, 10));
        b.build()
    });
    add("single_line", {
        let mut b = Path::builder();
        b.move_to(Point::raw(0, 0));
        b.line_to(Point::raw(1000, 0));
        b.build()
    });
    add("two_point_closed", poly(&[(0, 0), (1000, 0)]));
    add("three_point_closed", poly(&[(0, 0), (1000, 0), (0, 1000)]));
    add(
        "unit_square",
        poly(&[(0, 0), (1000, 0), (1000, 1000), (0, 1000)]),
    );
    add(
        "clockwise_square",
        poly(&[(0, 0), (0, 1000), (1000, 1000), (1000, 0)]),
    );
    add("square_with_hole", {
        let mut b = Path::builder();
        b.rect(Rect::raw(0, 0, 10_000, 10_000));
        b.move_to(Point::raw(2_000, 2_000));
        b.line_to(Point::raw(2_000, 8_000));
        b.line_to(Point::raw(8_000, 8_000));
        b.line_to(Point::raw(8_000, 2_000));
        b.close();
        b.build()
    });
    add("nested_three_deep", {
        let mut b = Path::builder();
        for (i, s) in [10_000, 6_000, 3_000].iter().enumerate() {
            let r = Rect::raw(-s, -s, *s, *s);
            if i % 2 == 0 {
                b.rect(r);
            } else {
                b.move_to(Point::new(r.lo.x, r.lo.y));
                b.line_to(Point::new(r.lo.x, r.hi.y));
                b.line_to(Point::new(r.hi.x, r.hi.y));
                b.line_to(Point::new(r.hi.x, r.lo.y));
                b.close();
            }
        }
        b.build()
    });
    add(
        "figure_eight",
        poly(&[(0, 0), (1000, 1000), (0, 1000), (1000, 0)]),
    );
    add(
        "bowtie_tangent",
        poly(&[(0, 0), (1000, 500), (0, 1000), (1000, 1000), (1000, 0)]),
    );
    add("five_star", star(5, 2, 10_000.0));
    add("seven_star", star(7, 3, 10_000.0));
    add(
        "collinear_edges",
        poly(&[(0, 0), (500, 0), (1000, 0), (1000, 1000), (0, 1000)]),
    );
    add(
        "repeated_points",
        poly(&[(0, 0), (0, 0), (1000, 0), (1000, 0), (1000, 1000)]),
    );
    add("zero_length_segments", {
        let mut b = Path::builder();
        b.move_to(Point::raw(0, 0));
        b.line_to(Point::raw(0, 0));
        b.line_to(Point::raw(1000, 0));
        b.line_to(Point::raw(1000, 0));
        b.line_to(Point::raw(1000, 1000));
        b.close();
        b.build()
    });
    add("zero_area_subpath", {
        let mut b = Path::builder();
        b.rect(Rect::raw(0, 0, 1000, 1000));
        b.move_to(Point::raw(2000, 0));
        b.line_to(Point::raw(3000, 0));
        b.line_to(Point::raw(2000, 0));
        b.close();
        b.build()
    });
    add("coincident_edges", {
        // Two squares sharing a full edge: the classic tangency case.
        let mut b = Path::builder();
        b.rect(Rect::raw(0, 0, 1000, 1000));
        b.rect(Rect::raw(1000, 0, 2000, 1000));
        b.build()
    });
    add("touching_corners", {
        let mut b = Path::builder();
        b.rect(Rect::raw(0, 0, 1000, 1000));
        b.rect(Rect::raw(1000, 1000, 2000, 2000));
        b.build()
    });
    add("cusp_cubic", {
        let mut b = Path::builder();
        b.move_to(Point::raw(0, 0));
        // Control points crossing produce a cusp, where adaptive flattening
        // has to place many vertices in a tiny parameter range.
        b.cubic_to(
            Point::raw(2000, 2000),
            Point::raw(-2000, 2000),
            Point::raw(0, 0),
        );
        b.close();
        b.build()
    });
    add("self_intersecting_cubics", {
        let mut b = Path::builder();
        b.move_to(Point::raw(0, 0));
        b.cubic_to(
            Point::raw(4000, 0),
            Point::raw(4000, 4000),
            Point::raw(0, 4000),
        );
        b.cubic_to(
            Point::raw(-4000, 4000),
            Point::raw(4000, -1000),
            Point::raw(0, 0),
        );
        b.close();
        b.build()
    });
    add("circle", {
        let mut b = Path::builder();
        b.ellipse(Point::ORIGIN, Mp::new(10_000), Mp::new(10_000));
        b.build()
    });
    add("flat_ellipse", {
        let mut b = Path::builder();
        b.ellipse(Point::ORIGIN, Mp::new(100_000), Mp::new(10));
        b.build()
    });
    add("tiny_beside_huge", {
        // 1 mp beside 10^8 mp: the scale ratio that breaks fixed tolerances.
        let mut b = Path::builder();
        b.rect(Rect::raw(0, 0, 100_000_000, 100_000_000));
        b.rect(Rect::raw(0, 0, 1, 1));
        b.build()
    });
    add("extent_corners", {
        let e = Mp::EXTENT_MAX.raw();
        poly(&[(-e, -e), (e, -e), (e, e), (-e, e)])
    });
    add("open_and_closed_mixed", {
        let mut b = Path::builder();
        b.rect(Rect::raw(0, 0, 1000, 1000));
        b.move_to(Point::raw(2000, 0));
        b.line_to(Point::raw(3000, 1000));
        b.line_to(Point::raw(4000, 0));
        b.build()
    });
    add("winding_rules_disagree", {
        // Two nested squares wound the same way: non-zero fills the middle,
        // even-odd leaves it hollow.
        let mut b = Path::builder();
        b.rect(Rect::raw(0, 0, 10_000, 10_000));
        b.rect(Rect::raw(2_000, 2_000, 8_000, 8_000));
        b.build()
    });
    add("spiral", {
        let mut b = Path::builder();
        for i in 0..200 {
            let a = i as f64 * 0.2;
            let r = 50.0 * a;
            let p = Point::from_f64_round(r * a.cos(), r * a.sin());
            if i == 0 {
                b.move_to(p);
            } else {
                b.line_to(p);
            }
        }
        b.build()
    });
    add("blob_1k", many_segment_blob(1_000, 50_000.0));
    add("blob_10k", many_segment_blob(10_000, 50_000.0));
    v
}

/// The cases that are cheap enough for the per-push property suites; the
/// large ones are exercised by the benchmarks and by the explicit large-input
/// tests instead.
pub fn small_corpus() -> Vec<Case> {
    corpus()
        .into_iter()
        .filter(|c| c.path.segment_count() <= 64)
        .collect()
}
