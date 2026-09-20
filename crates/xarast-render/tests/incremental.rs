//! Incremental redraw: dirty rects and pan reprojection.
//!
//! The phase asks for these to be asserted by **counting rasterised
//! pixels**, not by timing, so that the test says something true on a
//! loaded CI machine. `FrameTimings::rasterised_pixels` is that counter.

use xarast_color::Rgba8;
use xarast_geom::{FillRule, Mp, Path, Point, Rect};
use xarast_render::backend::cpu::Resolver;
use xarast_render::{
    CpuBackend, CpuConfig, DeviceRect, DirtyRect, DisplayList, Paint, PathRef, RenderQuality,
    Scene, SceneBuilder, SceneNodeId, Surface, Transform2D, ViewParams, scroll_surface,
};

const W: u32 = 512;
const OBJECTS: u64 = 4_000;

fn busy_scene() -> Scene {
    let mut scene = Scene::new();
    let mut rng = 0x1234_5678_9abc_def0u64;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng >> 11) as f64 / (1u64 << 53) as f64
    };
    let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
    b.fill(
        SceneNodeId(0),
        &rect(0.0, 0.0, f64::from(W), f64::from(W)),
        FillRule::NonZero,
        Paint::Solid(Rgba8::WHITE),
    );
    for i in 0..OBJECTS {
        let x = next() * f64::from(W);
        let y = next() * f64::from(W);
        let s = 3.0 + next() * 9.0;
        b.fill(
            SceneNodeId(i + 1),
            &rect(x, y, x + s, y + s),
            FillRule::NonZero,
            Paint::Solid(Rgba8 {
                r: (x as u8).wrapping_mul(7),
                g: (y as u8).wrapping_mul(13),
                b: 200,
                a: 255,
            }),
        );
    }
    b.finish().unwrap();
    scene
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> PathRef {
    let mut b = Path::builder();
    b.rect(Rect::new(
        Point::new(Mp::from_pt(x0), Mp::from_pt(y0)),
        Point::new(Mp::from_pt(x1), Mp::from_pt(y1)),
    ));
    PathRef::new(b.build())
}

fn view() -> ViewParams {
    ViewParams::new(W, W, Transform2D::scale(1.0 / 1000.0), RenderQuality::Final)
}

#[test]
fn a_sixty_four_pixel_dirty_rect_costs_a_fraction_of_a_full_frame() {
    let scene = busy_scene();
    let mut backend = CpuBackend::new(CpuConfig::deterministic());
    let res = Resolver::new();

    let mut full_target = Surface::new(W, W);
    let full = DisplayList::build(&scene, &view(), &DirtyRect::NONE);
    let full_timings = backend.render(&full, &res, &mut full_target).unwrap();

    let dirty = DirtyRect::of(DeviceRect::new(200, 200, 264, 264));
    let partial = DisplayList::build(&scene, &view(), &dirty);
    let mut partial_target = Surface::new(W, W);
    let partial_timings = backend.render(&partial, &res, &mut partial_target).unwrap();

    let ratio = partial_timings.rasterised_pixels as f64 / full_timings.rasterised_pixels as f64;
    assert!(
        ratio < 0.02,
        "a 64x64 dirty rect rasterised {:.2} % of a full frame ({} of {} pixels)",
        ratio * 100.0,
        partial_timings.rasterised_pixels,
        full_timings.rasterised_pixels
    );
    assert!(
        partial.len() < full.len() / 10,
        "the display list should be culled too: {} of {} commands",
        partial.len(),
        full.len()
    );
}

#[test]
fn the_dirty_rect_result_matches_the_full_frame_inside_it() {
    // An incremental redraw that produces different pixels is worse than a
    // slow one.
    let scene = busy_scene();
    let mut backend = CpuBackend::new(CpuConfig::deterministic());
    let res = Resolver::new();
    let region = DeviceRect::new(200, 200, 264, 264);

    let mut full_target = Surface::filled(W, W, [255, 255, 255, 255]);
    backend
        .render(
            &DisplayList::build(&scene, &view(), &DirtyRect::NONE),
            &res,
            &mut full_target,
        )
        .unwrap();

    let mut partial_target = Surface::filled(W, W, [255, 255, 255, 255]);
    backend
        .render(
            &DisplayList::build(&scene, &view(), &DirtyRect::of(region)),
            &res,
            &mut partial_target,
        )
        .unwrap();

    for y in region.y0..region.y1 {
        for x in region.x0..region.x1 {
            assert_eq!(
                full_target.pixel(x, y),
                partial_target.pixel(x, y),
                "pixel ({x}, {y}) differs between a full and an incremental frame"
            );
        }
    }
}

#[test]
fn a_two_hundred_pixel_pan_rasterises_only_the_new_strips() {
    let scene = busy_scene();
    let mut backend = CpuBackend::new(CpuConfig::deterministic());
    let res = Resolver::new();
    let mut target = Surface::new(W, W);
    let full = backend
        .render(
            &DisplayList::build(&scene, &view(), &DirtyRect::NONE),
            &res,
            &mut target,
        )
        .unwrap();

    // Pan right by 200 pixels: the valid pixels move, and only the newly
    // exposed column needs rasterising.
    let [horizontal, vertical] = scroll_surface(&mut target, 200, 0);
    assert!(horizontal.is_empty(), "a horizontal pan exposes no band");
    let exposed = vertical.rect();
    assert_eq!(exposed.width(), 200);
    assert_eq!(exposed.height(), W);

    let panned_view = ViewParams {
        transform: Transform2D::scale(1.0 / 1000.0).then(Transform2D::translate(200.0, 0.0)),
        ..view()
    };
    let strips = DisplayList::build(&scene, &panned_view, &vertical);
    let strip_timings = backend.render(&strips, &res, &mut target).unwrap();

    let ratio = strip_timings.rasterised_pixels as f64 / full.rasterised_pixels as f64;
    assert!(
        ratio < 0.5,
        "a 200 px pan of a {W} px wide view rasterised {:.1} % of a full frame",
        ratio * 100.0
    );
}

#[test]
fn scrolling_by_nothing_invalidates_nothing() {
    let mut s = Surface::filled(32, 32, [1, 2, 3, 255]);
    let before = s.clone();
    let [a, b] = scroll_surface(&mut s, 0, 0);
    assert!(a.is_empty() && b.is_empty());
    assert_eq!(s, before);
}
