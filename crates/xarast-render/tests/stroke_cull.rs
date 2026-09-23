//! Strokes far bigger than the view: bounded work, unchanged pixels.
//!
//! gintrack XARA-T-0022. Before the stroke was cut to the band, a thick,
//! round-capped, finely dashed line kilometres long at deep zoom was
//! dashed and expanded whole, which exhausted memory.

use xarast_color::Rgba8;
use xarast_geom::{Cap, DashPattern, Join, Mp, Path, Point, StrokeStyle};
use xarast_render::backend::cpu::Resolver;
use xarast_render::{
    CpuBackend, CpuConfig, DirtyRect, DisplayList, Paint, PathRef, RenderQuality, Scene,
    SceneBuilder, SceneNodeId, Surface, Transform2D, ViewParams,
};

fn line(x0: i32, x1: i32, y: i32) -> PathRef {
    let mut b = Path::builder();
    b.move_to(Point::new(Mp::new(x0), Mp::new(y)));
    b.line_to(Point::new(Mp::new(x1), Mp::new(y)));
    PathRef::new(b.build())
}

fn style() -> StrokeStyle {
    StrokeStyle {
        width: Mp::new(400),
        cap_start: Cap::Round,
        cap_end: Cap::Round,
        join: Join::Round,
        mitre_limit: 4.0,
        dash: Some(DashPattern {
            elements: vec![Mp::new(1_000), Mp::new(500)],
            offset: Mp::ZERO,
            reference_width: None,
        }),
    }
}

/// Renders one stroked line at ten device pixels per point, with the
/// view's origin at document (0, 0).
fn render(path: &PathRef) -> Surface {
    let mut scene = Scene::new();
    let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
    b.stroke(
        SceneNodeId(1),
        path,
        style(),
        Paint::Solid(Rgba8 {
            r: 20,
            g: 40,
            b: 200,
            a: 255,
        }),
    );
    b.finish().expect("balanced");
    let view = ViewParams::new(200, 100, Transform2D::scale(0.01), RenderQuality::Final);
    let dl = DisplayList::build(&scene, &view, &DirtyRect::NONE);
    let mut target = Surface::new(200, 100);
    CpuBackend::new(CpuConfig::deterministic())
        .render(&dl, &Resolver::new(), &mut target)
        .expect("renders");
    target
}

#[test]
fn a_kilometre_of_fine_dashes_at_deep_zoom_renders_the_window_only() {
    // Twenty million millipoints (7 m) of 1 pt dashes: over thirteen
    // thousand dashes in all, three or four in the view. The long line
    // starts a whole number of periods (1 500 mp) before the short one, so
    // the phase in the window is the same.
    let long = render(&line(-10_000_500, 10_000_000, 5_000));
    let short = render(&line(-30_000, 30_000, 5_000));
    let worst = long
        .data()
        .iter()
        .zip(short.data())
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0);
    assert!(worst <= 1, "the window differs by {worst}");
    // Along the centre row, dashes and gaps alternate.
    let row = &long.data()[50 * 200 * 4..51 * 200 * 4];
    let alphas: Vec<u8> = row.chunks(4).map(|px| px[3]).collect();
    assert!(alphas.contains(&255), "a dash was drawn");
    assert!(alphas.contains(&0), "a gap was left");
}
