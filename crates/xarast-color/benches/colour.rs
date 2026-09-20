//! Benchmarks for the colour operations the phase document gives budgets to:
//! `ColourTable::resolve` at 100 ns for a depth-3 tint chain, and
//! `interpolate` at 20 ns for any effect.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use xarast_color::{
    ColourDef, ColourKind, ColourModel, ColourTable, ColourValue, FillEffect, interpolate,
};

fn bench_resolve(c: &mut Criterion) {
    let mut t = ColourTable::new();
    let mut id = t.insert(ColourDef::normal(ColourValue::rgb(0.1, 0.2, 0.3)));
    for _ in 0..3 {
        id = t.insert(ColourDef {
            model: ColourModel::Rgbt,
            kind: ColourKind::Tint { factor: 0.9 },
            parent: Some(id),
            ..ColourDef::default()
        });
    }
    c.bench_function("resolve/tint_depth3", |b| {
        b.iter(|| black_box(t.resolve(id)))
    });
    c.bench_function("resolve_rgba8/tint_depth3", |b| {
        b.iter(|| black_box(t.resolve_rgba8(id)))
    });
}

fn bench_interpolate(c: &mut Criterion) {
    let a = ColourValue::rgb(0.1, 0.8, 0.3);
    let b = ColourValue::rgb(0.9, 0.2, 0.7);
    let mut g = c.benchmark_group("interpolate");
    for (name, effect) in [
        ("fade", FillEffect::Fade),
        ("rainbow", FillEffect::Rainbow),
        ("alt_rainbow", FillEffect::AltRainbow),
    ] {
        g.bench_function(name, |bn| b_iter(bn, a, b, effect));
    }
    g.finish();
}

fn b_iter(bn: &mut criterion::Bencher<'_>, a: ColourValue, b: ColourValue, effect: FillEffect) {
    bn.iter(|| black_box(interpolate(a, b, 0.5, effect)));
}

fn bench_convert(c: &mut Criterion) {
    let v = ColourValue::rgb(0.3, 0.6, 0.9);
    let mut g = c.benchmark_group("convert");
    g.bench_function("to_cmyk", |b| b.iter(|| black_box(v.to_cmyk())));
    g.bench_function("to_hsvt", |b| b.iter(|| black_box(v.to_hsvt())));
    g.bench_function("to_rgba8", |b| b.iter(|| black_box(v.to_rgba8())));
    g.finish();
}

criterion_group!(benches, bench_resolve, bench_interpolate, bench_convert);
criterion_main!(benches);
