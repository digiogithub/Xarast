//! Compares our flattener against `kurbo::flatten` on the same random cubics.
//!
//! Replacing a mature library's flattener is a decision that has to be paid
//! for in evidence, and this is the evidence. Both are measured the same way:
//! the same curves, the same deviation metric, the same sample count.
use kurbo::{ParamCurve, PathEl, Shape};
use xarast_geom::{Path, Point, Segment, Tolerance, flatten_traced, max_deviation};

fn main() {
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut rng = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };

    println!(
        "{:>8} {:>14} {:>8} {:>14} {:>8}",
        "tol", "ours worst", "ratio", "kurbo worst", "ratio"
    );
    for tol in [1.0, 10.0, 50.0, 100.0, 1000.0] {
        let (mut ours, mut theirs) = (0.0f64, 0.0f64);
        let (mut ours_pts, mut theirs_pts) = (0usize, 0usize);
        for _ in 0..20_000 {
            let mut c = || ((rng() % 400_000) as i32) - 200_000;
            let mut b = Path::builder();
            b.move_to(Point::raw(c(), c()));
            b.cubic_to(
                Point::raw(c(), c()),
                Point::raw(c(), c()),
                Point::raw(c(), c()),
            );
            let p = b.build();
            let seg: Segment = p.segments().next().unwrap();

            let (polys, _) = flatten_traced(&p, Tolerance(tol));
            ours = ours.max(max_deviation(seg, &polys[0].points, 300));
            ours_pts += polys[0].points.len();

            // The same segment, flattened by kurbo, measured identically.
            let k = seg.to_kurbo();
            let start = Point::from_kurbo(k.start());
            let mut pts = vec![start];
            kurbo::flatten(k.path_elements(0.0), tol, |el| {
                if let PathEl::LineTo(q) = el {
                    pts.push(Point::from_kurbo(q));
                }
            });
            theirs = theirs.max(max_deviation(seg, &pts, 300));
            theirs_pts += pts.len();
        }
        println!(
            "{tol:>8} {ours:>14.4} {:>7.2}x {theirs:>14.4} {:>7.2}x   points ours {ours_pts} kurbo {theirs_pts}",
            ours / tol,
            theirs / tol
        );
    }
}
