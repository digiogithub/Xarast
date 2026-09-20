//! The phase's performance budgets, as `criterion` groups.
//!
//! Every group corresponds to one row of the budget table in
//! `docs/phases/phase-04-render-engine.md`. The numbers this machine
//! produces are recorded in `docs/memory/perf.md` together with the
//! reference machine's specification, because a budget without a machine
//! attached to it is not a budget.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use xarast_color::Rgba8;
use xarast_geom::{FillRule, Mp, Path, Point, Rect};
use xarast_render::backend::cpu::Resolver;
use xarast_render::{
    BlendLuts, CpuBackend, CpuConfig, DeviceRect, DirtyRect, DisplayList, EffectSpace, Paint,
    PathRef, Profile, RampCache, RampLength, RenderQuality, Scene, SceneBuilder, SceneNodeId, Stop,
    Surface, Transform2D, ViewParams,
};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// The `bulk` spike scene: filled paths averaging twelve segments each.
fn bulk(w: u32, h: u32, count: u64) -> Scene {
    let mut rng = Rng(0xdead_beef);
    let mut scene = Scene::new();
    let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
    for i in 0..count {
        let cx = rng.next() * f64::from(w);
        let cy = rng.next() * f64::from(h);
        let r = 2.0 + rng.next() * 6.0;
        let mut p = Path::builder();
        for k in 0..12 {
            let a = std::f64::consts::TAU * f64::from(k) / 12.0;
            let rr = r * (0.6 + rng.next() * 0.4);
            let pt = Point::new(
                Mp::from_pt(cx + rr * a.cos()),
                Mp::from_pt(cy + rr * a.sin()),
            );
            if k == 0 {
                p.move_to(pt);
            } else {
                p.line_to(pt);
            }
        }
        p.close();
        b.fill(
            SceneNodeId(i),
            &PathRef::new(p.build()),
            FillRule::NonZero,
            Paint::Solid(Rgba8 {
                r: (i % 251) as u8,
                g: (i % 241) as u8,
                b: 180,
                a: 255,
            }),
        );
    }
    b.finish().expect("balanced");
    scene
}

fn view(w: u32, h: u32) -> ViewParams {
    ViewParams::new(w, h, Transform2D::scale(1.0 / 1000.0), RenderQuality::Final)
}

fn rect_path(x0: f64, y0: f64, x1: f64, y1: f64) -> PathRef {
    let mut b = Path::builder();
    b.rect(Rect::new(
        Point::new(Mp::from_pt(x0), Mp::from_pt(y0)),
        Point::new(Mp::from_pt(x1), Mp::from_pt(y1)),
    ));
    PathRef::new(b.build())
}

fn full_frame(c: &mut Criterion) {
    let mut g = c.benchmark_group("full_frame");
    g.sample_size(10);
    for (w, h, n) in [(1920u32, 1080u32, 20_000u64), (960, 540, 20_000)] {
        let scene = bulk(w, h, n);
        let view = view(w, h);
        let res = Resolver::new();
        let dl = DisplayList::build(&scene, &view, &DirtyRect::NONE);
        let mut target = Surface::new(w, h);
        let mut backend = CpuBackend::new(CpuConfig::interactive());
        g.bench_function(format!("cpu_{w}x{h}_{n}_objects"), |b| {
            b.iter(|| {
                backend
                    .render(black_box(&dl), &res, &mut target)
                    .expect("renders")
            });
        });
    }
    g.finish();
}

fn incremental(c: &mut Criterion) {
    let mut g = c.benchmark_group("incremental");
    g.sample_size(20);
    let (w, h) = (1920u32, 1080u32);
    let scene = bulk(w, h, 20_000);
    let view = view(w, h);
    let res = Resolver::new();
    let dirty = DirtyRect::of(DeviceRect::new(900, 500, 964, 564));
    let dl = DisplayList::build(&scene, &view, &dirty);
    let mut target = Surface::new(w, h);
    let mut backend = CpuBackend::new(CpuConfig::interactive());
    g.bench_function("dirty_rect_64x64", |b| {
        b.iter(|| {
            backend
                .render(black_box(&dl), &res, &mut target)
                .expect("renders")
        });
    });
    g.bench_function("display_list_build", |b| {
        b.iter(|| black_box(DisplayList::build(&scene, &view, &DirtyRect::NONE)));
    });
    g.finish();
}

fn paints(c: &mut Criterion) {
    let mut g = c.benchmark_group("paints");
    let stops: Vec<Stop> = (0..8)
        .map(|i| {
            Stop::new(
                f32::from(i) / 7.0,
                Rgba8 {
                    r: i * 31,
                    g: 255 - i * 31,
                    b: i * 17,
                    a: 255,
                },
            )
        })
        .collect();
    g.bench_function("ramp_2048_8_stops_with_profile", |b| {
        b.iter(|| {
            black_box(xarast_render::build_ramp(
                black_box(&stops),
                Profile::new(0.3, -0.4),
                EffectSpace::Rgb,
                RampLength::Long,
            ))
        });
    });
    g.bench_function("ramp_cache_hit", |b| {
        let mut cache = RampCache::new();
        let id = cache.intern(
            &stops,
            Profile::IDENTITY,
            EffectSpace::Rgb,
            RampLength::Long,
        );
        b.iter(|| black_box(cache.get(black_box(id)).len()));
    });
    g.bench_function("blend_lut_set_12_families", |b| {
        b.iter(|| black_box(BlendLuts::build(xarast_render::LumaWeights::BT601).bytes()));
    });
    g.finish();
}

/// The cache-admission sweep the phase leaves open: the threshold above
/// which a plain group earns a cache slot is chosen from the knee of this
/// curve, not from intuition.
fn cache_threshold(c: &mut Criterion) {
    let mut g = c.benchmark_group("cache_threshold");
    g.sample_size(10);
    let res = Resolver::new();
    let view = view(512, 512);
    for group_size in [8u64, 32, 64, 128, 512] {
        let mut scene = Scene::new();
        {
            let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
            b.fill(
                SceneNodeId(0),
                &rect_path(0.0, 0.0, 512.0, 512.0),
                FillRule::NonZero,
                Paint::Solid(Rgba8::WHITE),
            );
            let mut rng = Rng(0x5eed);
            for i in 0..group_size {
                let x = rng.next() * 480.0;
                let y = rng.next() * 480.0;
                b.fill(
                    SceneNodeId(i + 1),
                    &rect_path(x, y, x + 24.0, y + 24.0),
                    FillRule::NonZero,
                    Paint::Solid(Rgba8 {
                        r: 40,
                        g: 90,
                        b: 200,
                        a: 255,
                    }),
                );
            }
            b.finish().expect("balanced");
        }
        let dl = DisplayList::build(&scene, &view, &DirtyRect::NONE);
        let mut target = Surface::new(512, 512);
        let mut backend = CpuBackend::new(CpuConfig::interactive());
        g.bench_function(format!("group_of_{group_size}"), |b| {
            b.iter(|| {
                backend
                    .render(black_box(&dl), &res, &mut target)
                    .expect("renders")
            });
        });
    }
    g.finish();
}

criterion_group!(benches, full_frame, incremental, paints, cache_threshold);
criterion_main!(benches);
