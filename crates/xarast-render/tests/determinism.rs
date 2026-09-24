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

/// A rectangle drawn over a frame is exactly that frame's pixels, however
/// the rectangle cuts the bands, the columns and the primitives: the
/// render thread repaints an edit's damage over the frame on screen and
/// the result must be the frame a full render gives (XARA-T-0221). Before
/// the rasteriser was given a fixed top left, a rectangle starting inside
/// a primitive moved `aa_edge_45` by 1/255 in up to 53 pixels.
#[test]
fn coverage_does_not_depend_on_the_draw_area() {
    use xarast_render::{CpuBackend, DeviceRect, DirtyRect, DisplayList};
    for cfg in [CpuConfig::deterministic(), CpuConfig::interactive()] {
        for case in all_cases() {
            let full = render_case_with(&case, cfg);
            let (w, h) = (
                i32::try_from(case.view.viewport.width()).unwrap(),
                i32::try_from(case.view.viewport.height()).unwrap(),
            );
            for r in [
                DeviceRect::new(w / 3, h / 5, w - 7, h - 3),
                DeviceRect::new(5, 13, w / 2 + 3, h / 2 + 1),
                DeviceRect::new(w / 4 + 1, 0, w, h),
                DeviceRect::new(0, h / 3 + 1, w, h),
                DeviceRect::new(w / 2 - 1, h / 2 - 2, w / 2 + 3, h / 2 + 1),
            ] {
                let mut t = full.clone();
                let stride = case.view.viewport.width() as usize * 4;
                for y in r.y0..r.y1 {
                    let row = usize::try_from(y).unwrap() * stride;
                    let (x0, x1) = (
                        usize::try_from(r.x0).unwrap() * 4,
                        usize::try_from(r.x1).unwrap() * 4,
                    );
                    t.data_mut()[row + x0..row + x1].fill(0);
                }
                let dl = DisplayList::build(&case.scene, &case.view, &DirtyRect::of(r));
                CpuBackend::new(cfg)
                    .render(&dl, &case.resolver, &mut t)
                    .expect("renders");
                assert!(
                    t == full,
                    "{}: drawing {r:?} over the frame changed it ({cfg:?})",
                    case.name
                );
            }
        }
    }
}
