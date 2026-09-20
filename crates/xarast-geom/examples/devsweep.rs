//! Measures the worst flattening deviation over random cubics, to set the
//! tolerance slack from data rather than from a guess.
use xarast_geom::{Path, Point, Tolerance, flatten_traced, max_deviation};

fn main() {
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut rng = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for tol in [1.0, 10.0, 50.0, 100.0, 1000.0] {
        let mut worst: f64 = 0.0;
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
            let (polys, _) = flatten_traced(&p, Tolerance(tol));
            let seg = p.segments().next().unwrap();
            worst = worst.max(max_deviation(seg, &polys[0].points, 300));
        }
        println!(
            "tol {tol:>8} worst {worst:>12.4}  over {:>8.4}  ratio {:.4}",
            worst - tol,
            worst / tol
        );
    }
}
