//! `criterion` on `build_ui_frame()`: the P1 axis of the density spike.
//!
//! `cargo bench -p xarast-ui`
//!
//! The example `density` reports all nine axes once; this benchmark is the
//! one that has to keep reporting, so that a panel added in a later phase
//! that quietly costs two milliseconds is caught by a regression rather
//! than by a user.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use xarast_ui::density::{DensityProbe, ProbeConfig};
use xarast_ui::model::{DocumentView, LayerInfo, LayerKey, PaletteEntry, UiModel};
use xarast_ui::theme::{ResolvedTheme, apply};
use xarast_ui::{Scale, Workspace};

fn probe_context() -> egui::Context {
    let ctx = egui::Context::default();
    apply(&ctx, ResolvedTheme::Dark);
    ctx
}

fn input(size: egui::Vec2) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        ..Default::default()
    }
}

fn realistic_model() -> UiModel {
    let layers: Vec<_> = (0..64)
        .map(|i| LayerInfo {
            object_count: i * 7,
            ..LayerInfo::new(i as u64, format!("Layer {i}"))
        })
        .collect();
    UiModel {
        document: Some(DocumentView {
            layers,
            active_layer: Some(LayerKey(0)),
            grid: xarast_ui::GridSettings {
                visible: true,
                ..Default::default()
            },
            ..Default::default()
        }),
        palette: (0..512)
            .map(|i| {
                let t = i as f32 / 512.0;
                PaletteEntry::colour(
                    format!("Swatch {i}"),
                    xarast_color::ColourValue::rgb(t, 1.0 - t, 0.5),
                )
            })
            .collect(),
        ..Default::default()
    }
}

fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("build_ui_frame");

    // The spike's probe: professional density, 400+ controls.
    let ctx = probe_context();
    let mut probe = DensityProbe::new(ProbeConfig::default(), ResolvedTheme::Dark);
    // Two warm-up frames so the font atlas is not in the measurement.
    for _ in 0..2 {
        let _ = ctx.run(input(egui::vec2(2560.0, 1440.0)), |c| probe.ui(c));
    }
    group.bench_function("density_probe_2560x1440", |b| {
        b.iter(|| {
            let out = ctx.run(input(egui::vec2(2560.0, 1440.0)), |c| probe.ui(c));
            black_box(out.shapes.len())
        });
    });

    // The real default layout, which is what ships.
    let ctx = probe_context();
    let mut workspace = Workspace::new();
    let model = realistic_model();
    for _ in 0..2 {
        let _ = ctx.run(input(egui::vec2(1920.0, 1080.0)), |c| {
            workspace.ui(c, &model, Scale::new(1.0), &[]);
        });
    }
    group.bench_function("default_layout_1920x1080", |b| {
        b.iter(|| {
            let out = ctx.run(input(egui::vec2(1920.0, 1080.0)), |c| {
                black_box(workspace.ui(c, &model, Scale::new(1.0), &[]));
            });
            black_box(out.shapes.len())
        });
    });

    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
