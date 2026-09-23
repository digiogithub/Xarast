//! Phase 11 raster-export budgets (`docs/phases/phase-11-export-filters.md`,
//! "Performance budgets"): A4 at 300 dpi (2480 × 3508) of a 10 000-object
//! document to PNG, JPEG q90 and lossless WebP, and the size/DPI
//! recomputation the dialog runs on every keystroke. Measured numbers are
//! in `docs/memory/perf.md`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use xarast_color::Rgba8;
use xarast_geom::{FillRule, Mp, Path, Point, Rect};
use xarast_io::{
    Background, ExportArea, ExportRequest, ExportSizing, FormatId, FormatOptions, NoProgress,
    Registry, SceneSource, SizingEdit,
};
use xarast_render::{Paint, PathRef, RenderQuality, Resolver, Scene, SceneBuilder, SceneNodeId};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// A4 in points.
const A4: (f64, f64) = (595.276, 841.89);

/// `count` twelve-sided blobs, 5–25 pt across, over an A4 page; one in
/// four translucent.
fn document(count: u64) -> Scene {
    let mut rng = Rng(0x5eed_cafe);
    let mut scene = Scene::new();
    let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
    for i in 0..count {
        let (cx, cy) = (rng.next() * A4.0, rng.next() * A4.1);
        let r = 5.0 + rng.next() * 20.0;
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
        #[allow(clippy::cast_possible_truncation)]
        let colour = Rgba8 {
            r: (i % 251) as u8,
            g: (i * 7 % 241) as u8,
            b: 180,
            a: if i % 4 == 0 { 140 } else { 255 },
        };
        b.fill(
            SceneNodeId(i),
            &PathRef::new(p.build()),
            FillRule::NonZero,
            Paint::Solid(colour),
        );
    }
    b.finish().expect("balanced");
    scene
}

fn a4() -> Rect {
    Rect::new(
        Point::new(Mp::ZERO, Mp::ZERO),
        Point::new(Mp::from_pt(A4.0), Mp::from_pt(A4.1)),
    )
}

fn a4_300dpi(c: &mut Criterion) {
    let scene = document(10_000);
    let resolver = Resolver::new();
    let src = SceneSource {
        scene: &scene,
        resolver: &resolver,
        area: a4(),
        paper: Rgba8::WHITE,
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let registry = Registry::with_builtin();
    let mut g = c.benchmark_group("export_a4_300dpi_10k");
    g.sample_size(10);
    for id in [FormatId::Png, FormatId::Jpeg, FormatId::WebP] {
        let mut req = ExportRequest::new(
            FormatOptions::default_for(id),
            dir.path().join(format!("a4.{}", id.extension())),
        );
        req.area = ExportArea::Page(None);
        req.sizing = ExportSizing::at_dpi(300.0);
        req.background = Background::Paper;
        g.bench_function(id.extension(), |b| {
            b.iter(|| {
                registry
                    .export(black_box(&src), &req, &NoProgress)
                    .expect("exports")
            });
        });
    }
    g.finish();
}

fn sizing(c: &mut Criterion) {
    let area = a4();
    let mut s = ExportSizing::at_dpi(300.0);
    s.resolve(area).expect("resolves");
    c.bench_function("sizing_edit_keystroke", |b| {
        let mut w = 1000u32;
        b.iter(|| {
            w = if w == 3000 { 1000 } else { w + 1 };
            s.edit(black_box(SizingEdit::PixelWidth(w)), area)
                .expect("edits");
        });
    });
}

criterion_group!(benches, a4_300dpi, sizing);
criterion_main!(benches);
