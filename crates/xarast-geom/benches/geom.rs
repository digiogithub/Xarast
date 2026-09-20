//! Benchmarks for the operations the phase document gives budgets to.
//!
//! The budgets are, on the reference machine recorded in
//! `docs/memory/perf.md`: `bounds` 30 us, `tight_bounds` 400 us, `flatten`
//! 2 ms, `flatten_traced` at most 1.3x `flatten`, `stroke_to_path` 8 ms,
//! `dash` 6 ms, `boolean` union 3 ms / 40 ms / 600 ms at 1 k / 10 k / 100 k
//! segments, `HitIndex::build` 500 us, `hit_fill` via the index 15 us, and
//! `arclen` 200 us for a thousand cubics.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use xarast_geom::{
    BoolOp, DashPattern, FillRule, HitIndex, Mp, Path, Point, StrokeStyle, Tolerance, arclen,
    boolean, dash, flatten, flatten_traced, hit_fill, stroke_to_path,
};

/// A wobbly closed blob of `n` cubic segments, which is the shape of real
/// artwork: many segments, all curved, none collinear.
fn blob(n: usize, r: f64) -> Path {
    let mut b = Path::builder();
    let step = std::f64::consts::TAU / n as f64;
    let at = |i: usize, o: f64| {
        let a = step * (i as f64 + o);
        let rad = r * (1.0 + 0.1 * (a * 3.0).sin());
        Point::from_f64_round(rad * a.cos(), rad * a.sin())
    };
    b.move_to(at(0, 0.0));
    for i in 0..n {
        b.cubic_to(at(i, 0.33), at(i, 0.66), at(i + 1, 0.0));
    }
    b.close();
    b.build()
}

/// A path of `n` straight segments.
fn polygon(n: usize, r: f64) -> Path {
    let mut b = Path::builder();
    let step = std::f64::consts::TAU / n as f64;
    let at = |i: usize| {
        let a = step * i as f64;
        let rad = r * (1.0 + 0.1 * (a * 3.0).sin());
        Point::from_f64_round(rad * a.cos(), rad * a.sin())
    };
    b.move_to(at(0));
    for i in 1..n {
        b.line_to(at(i));
    }
    b.close();
    b.build()
}

fn bench_bounds(c: &mut Criterion) {
    let p = blob(10_000, 500_000.0);
    let mut g = c.benchmark_group("bounds");
    g.bench_function("bounds/10k", |b| b.iter(|| black_box(p.bounds())));
    g.bench_function("tight_bounds/10k", |b| {
        b.iter(|| black_box(p.tight_bounds()))
    });
    g.finish();
}

fn bench_flatten(c: &mut Criterion) {
    let p = blob(10_000, 500_000.0);
    // A quarter of a device pixel at 100 % zoom and 96 dpi.
    let tol = Tolerance::from_device_px(Tolerance::RENDER_DEVICE_PX, 750.0);
    let mut g = c.benchmark_group("flatten");
    g.bench_function("flatten/10k", |b| b.iter(|| black_box(flatten(&p, tol))));
    g.bench_function("flatten_traced/10k", |b| {
        b.iter(|| black_box(flatten_traced(&p, tol)))
    });
    g.finish();
}

fn bench_stroke(c: &mut Criterion) {
    let p = blob(10_000, 500_000.0);
    let style = StrokeStyle {
        width: Mp::new(2_000),
        join: xarast_geom::Join::Round,
        ..StrokeStyle::default()
    };
    let pattern = DashPattern {
        elements: vec![Mp::new(3_000), Mp::new(1_000), Mp::new(500), Mp::new(1_000)],
        ..DashPattern::default()
    };
    let tol = Tolerance::from_device_px(Tolerance::RENDER_DEVICE_PX, 750.0);
    let mut g = c.benchmark_group("stroke");
    g.sample_size(20);
    g.bench_function("stroke_to_path/10k", |b| {
        b.iter(|| black_box(stroke_to_path(&p, &style, tol).unwrap()))
    });
    g.bench_function("dash/10k", |b| {
        b.iter(|| black_box(dash(&p, &pattern, Mp::new(2_000), tol)))
    });
    g.finish();
}

fn bench_boolean(c: &mut Criterion) {
    let mut g = c.benchmark_group("boolean");
    g.sample_size(10);
    for n in [1_000usize, 10_000, 100_000] {
        let a = polygon(n, 500_000.0);
        let b_path = a.transformed(xarast_geom::Matrix::translate(xarast_geom::Vector::raw(
            200_000, 0,
        )));
        g.bench_function(format!("union/{n}"), |bn| {
            bn.iter(|| {
                black_box(boolean(
                    &a,
                    &b_path,
                    BoolOp::Union,
                    FillRule::NonZero,
                    Tolerance::BOOLEAN,
                ))
            })
        });
    }
    g.finish();
}

fn bench_hit(c: &mut Criterion) {
    let p = blob(10_000, 500_000.0);
    let idx = HitIndex::build(&p);
    let q = Point::raw(1_000, 2_000);
    let mut g = c.benchmark_group("hit");
    g.bench_function("HitIndex::build/10k", |b| {
        b.iter(|| black_box(HitIndex::build(&p)))
    });
    g.bench_function("hit_fill_indexed/10k", |b| {
        b.iter(|| black_box(idx.hit_fill(&p, q, FillRule::NonZero)))
    });
    g.bench_function("hit_fill_exact/10k", |b| {
        b.iter(|| black_box(hit_fill(&p, q, FillRule::NonZero)))
    });
    g.finish();
}

fn bench_arclen(c: &mut Criterion) {
    let p = blob(1_000, 500_000.0);
    c.bench_function("arclen/1k_cubics", |b| {
        b.iter(|| black_box(arclen(&p, 1e-6)))
    });
}

fn bench_mp(c: &mut Criterion) {
    // The budget says `Mp` arithmetic must cost what raw `i32` saturating
    // arithmetic costs: the newtype and the contract must be free.
    let v: Vec<i32> = (0i32..4_096)
        .map(|i| i.wrapping_mul(2_654_435_761u32 as i32))
        .collect();
    let mut g = c.benchmark_group("mp");
    g.bench_function("operator_add", |b| {
        b.iter(|| {
            let mut acc = Mp::ZERO;
            for &x in &v {
                // The operator, not `+=`: this is what is being measured
                // against the raw `i32` loop below.
                acc = std::ops::Add::add(acc, Mp::new(x));
            }
            black_box(acc)
        })
    });
    g.bench_function("raw_saturating_add", |b| {
        b.iter(|| {
            let mut acc = 0i32;
            for &x in &v {
                acc = acc.saturating_add(x).max(i32::MIN + 1);
            }
            black_box(acc)
        })
    });
    g.finish();
}

criterion_group!(
    benches,
    bench_mp,
    bench_bounds,
    bench_flatten,
    bench_stroke,
    bench_boolean,
    bench_hit,
    bench_arclen
);
criterion_main!(benches);
