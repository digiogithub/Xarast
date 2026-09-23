//! Benchmarks for the colour operations the phase document gives budgets to:
//! `ColourTable::resolve` at 100 ns for a depth-3 tint chain, and
//! `interpolate` at 20 ns for any effect; and phase 8's
//! `ColourContext::convert` at 20 ns and `ColourTable::redefine` at 20 µs on
//! a 256-entry palette with a 4-deep derivation chain.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use xarast_color::{
    ColourContext, ColourDef, ColourKind, ColourModel, ColourTable, ColourValue, FillEffect,
    interpolate,
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
    g.bench_function("to_rgba8_packed", |b| {
        b.iter(|| black_box(v.to_rgba8_packed()))
    });
    let ctx = ColourContext::uncalibrated();
    g.bench_function("context_rgb_to_cmyk", |b| {
        b.iter(|| black_box(ctx.convert(black_box(v), ColourModel::Cmyk)))
    });
    g.bench_function("context_rgb_to_hsvt", |b| {
        b.iter(|| black_box(ctx.convert(black_box(v), ColourModel::Hsvt)))
    });
    g.finish();
}

fn bench_redefine(c: &mut Criterion) {
    // 256 entries: 251 independent colours plus a base with a 4-deep chain
    // of tints, shades and a link below it.
    let mut t = ColourTable::new();
    for i in 0..251 {
        t.insert(ColourDef::normal(ColourValue::rgb(
            i as f32 / 251.0,
            0.5,
            0.5,
        )));
    }
    let base = t.insert(ColourDef::normal(ColourValue::hsvt(0.3, 0.8, 0.7, 0.0)));
    let mut at = base;
    for k in [
        ColourKind::Tint { factor: 0.8 },
        ColourKind::Shade { x: -0.2, y: 0.1 },
        ColourKind::Linked,
        ColourKind::Tint { factor: 0.5 },
    ] {
        let id = t.insert(ColourDef::default());
        t.reparent(id, k, Some(at)).unwrap();
        at = id;
    }
    assert_eq!(t.len(), 256);
    let mut flip = false;
    c.bench_function("redefine/256_entries_chain4", |b| {
        b.iter(|| {
            flip = !flip;
            let v = if flip { 0.25 } else { 0.75 };
            black_box(
                t.redefine(
                    base,
                    [Some(v), Some(0.8), Some(0.7), Some(0.0)],
                    ColourModel::Hsvt,
                )
                .unwrap(),
            )
        })
    });
}

criterion_group!(
    benches,
    bench_resolve,
    bench_interpolate,
    bench_convert,
    bench_redefine
);
criterion_main!(benches);
