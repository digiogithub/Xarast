//! The CPU backend is the oracle, so it has to be bit-reproducible.
//!
//! The W0 spike established what that costs: `vello_cpu` is byte-identical
//! over 100 runs at a fixed SIMD level, and differs in 3 of 786,432 channel
//! samples between the AVX2 and baseline paths. The deterministic
//! configuration therefore pins the level.

mod common;

use std::collections::BTreeSet;

use common::{render_case, render_case_with};
use xarast_render::CpuConfig;
use xarast_render::corpus::all_cases;
use xarast_render::golden::digest;

#[test]
fn the_whole_corpus_hashes_the_same_on_every_run() {
    let cases = all_cases();
    let baseline: Vec<String> = cases.iter().map(|c| digest(&render_case(c))).collect();
    for run in 1..20 {
        for (case, want) in cases.iter().zip(baseline.iter()) {
            let got = digest(&render_case(case));
            assert_eq!(&got, want, "run {run} changed {}", case.name);
        }
    }
}

#[test]
fn parallel_bands_produce_the_same_bytes_as_serial_ones() {
    // Determinism survives parallelism only if the merge order is fixed;
    // this is the test that says so.
    let serial = CpuConfig {
        threads: 1,
        ..CpuConfig::deterministic()
    };
    let parallel = CpuConfig {
        threads: 0,
        ..CpuConfig::deterministic()
    };
    for case in all_cases() {
        let a = digest(&render_case_with(&case, serial));
        let b = digest(&render_case_with(&case, parallel));
        assert_eq!(a, b, "{} differs between 1 and N threads", case.name);
    }
}

#[test]
fn the_band_height_does_not_change_the_pixels() {
    // Bands are an implementation detail of memory, not of appearance. If
    // this fails, something accumulates across a band boundary.
    for case in all_cases() {
        let mut digests = BTreeSet::new();
        for budget in [1 << 10, 1 << 14, 1 << 20, 1 << 26] {
            let cfg = CpuConfig {
                band_budget_bytes: budget,
                ..CpuConfig::deterministic()
            };
            digests.insert(digest(&render_case_with(&case, cfg)));
        }
        assert_eq!(digests.len(), 1, "{} depends on the band height", case.name);
    }
}

#[test]
fn the_interactive_configuration_is_not_claimed_to_be_deterministic() {
    use xarast_render::CpuBackend;
    assert!(
        CpuBackend::new(CpuConfig::deterministic())
            .capabilities()
            .deterministic
    );
    assert!(
        !CpuBackend::new(CpuConfig::interactive())
            .capabilities()
            .deterministic
    );
}

#[test]
fn a_thin_strip_renders_the_same_in_column_tiles() {
    // A strip one band tall is split into column tiles so that it does not
    // rasterise on one core (XARA-T-0034). A tile edge must not show:
    // serial rendering never tiles, so the two must agree byte for byte.
    use xarast_color::Rgba8;
    use xarast_geom::{FillRule, Mp, Path, Point, Rect, StrokeStyle};
    use xarast_render::{
        BlendFamily, CpuBackend, DirtyRect, DisplayList, LayerKind, Paint, PathRef, RenderQuality,
        Resolver, Scene, SceneBuilder, SceneNodeId, Surface, Transform2D, Transparency, ViewParams,
    };
    let (w, h) = (1024u32, 48u32);
    let mut scene = Scene::new();
    let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
    let pt = |x: f64, y: f64| Point::new(Mp::from_pt(x), Mp::from_pt(y));
    let mut clip = Path::builder();
    clip.rect(Rect::new(pt(10.0, 4.0), pt(1000.0, 44.0)));
    b.push_clip(&PathRef::new(clip.build()), FillRule::NonZero);
    b.push_layer(
        LayerKind::Isolated,
        Transparency::flat(BlendFamily::Mix, 40),
    );
    let mut s: u64 = 0x5eed;
    let mut next = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 11) as f64 / (1u64 << 53) as f64
    };
    for i in 0..400u64 {
        let (cx, cy, r) = (next() * 1024.0, next() * 48.0, 3.0 + next() * 40.0);
        let mut p = Path::builder();
        p.move_to(pt(cx - r, cy));
        p.line_to(pt(cx + r * 0.3, cy - r * 0.7));
        p.line_to(pt(cx + r, cy + r * 0.2));
        p.close();
        let path = PathRef::new(p.build());
        let colour = Rgba8 {
            r: (i * 37 % 256) as u8,
            g: (i * 91 % 256) as u8,
            b: 200,
            a: if i % 3 == 0 { 160 } else { 255 },
        };
        if i % 5 == 0 {
            let style = StrokeStyle {
                width: Mp::from_pt(1.5),
                ..StrokeStyle::default()
            };
            b.stroke(SceneNodeId(i), &path, style, Paint::Solid(colour));
        } else {
            b.fill(
                SceneNodeId(i),
                &path,
                FillRule::NonZero,
                Paint::Solid(colour),
            );
        }
    }
    b.pop_layer();
    b.pop_clip();
    b.finish().expect("balanced");
    let view = ViewParams::new(w, h, Transform2D::scale(1.0 / 1000.0), RenderQuality::Final);
    let dl = DisplayList::build(&scene, &view, &DirtyRect::NONE);
    let render = |threads: usize| {
        let mut target = Surface::new(w, h);
        let cfg = CpuConfig {
            threads,
            ..CpuConfig::interactive()
        };
        CpuBackend::new(cfg)
            .render(&dl, &Resolver::new(), &mut target)
            .expect("renders");
        digest(&target)
    };
    assert_eq!(render(0), render(1), "a column-tile edge changed the strip");
}
