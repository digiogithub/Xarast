//! A smoke test: the renderer must actually put the right colours down.

use xarast_color::Rgba8;
use xarast_geom::{FillRule, Mp, Path, Point, Rect};
use xarast_render::backend::cpu::Resolver;
use xarast_render::{
    CpuBackend, CpuConfig, DirtyRect, DisplayList, Paint, PathRef, RenderQuality, Scene,
    SceneBuilder, SceneNodeId, Surface, Transform2D, ViewParams,
};

fn rect_path(x0: f64, y0: f64, x1: f64, y1: f64) -> PathRef {
    let mut b = Path::builder();
    b.rect(Rect::new(
        Point::new(Mp::from_pt(x0), Mp::from_pt(y0)),
        Point::new(Mp::from_pt(x1), Mp::from_pt(y1)),
    ));
    PathRef::new(b.build())
}

#[test]
fn a_red_square_on_white_lands_where_it_should() {
    let mut scene = Scene::new();
    {
        let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
        b.fill(
            SceneNodeId(0),
            &rect_path(0.0, 0.0, 64.0, 64.0),
            FillRule::NonZero,
            Paint::Solid(Rgba8::WHITE),
        );
        b.fill(
            SceneNodeId(1),
            &rect_path(16.0, 16.0, 48.0, 48.0),
            FillRule::NonZero,
            Paint::Solid(Rgba8 { r: 255, g: 0, b: 0, a: 255 }),
        );
        b.finish().unwrap();
    }
    let view = ViewParams::new(64, 64, Transform2D::scale(1.0 / 1000.0), RenderQuality::Final);
    let dl = DisplayList::build(&scene, &view, &DirtyRect::NONE);
    let mut target = Surface::new(64, 64);
    let mut backend = CpuBackend::new(CpuConfig::deterministic());
    let t = backend.render(&dl, &Resolver::new(), &mut target).unwrap();
    eprintln!("timings: {t:?}");
    assert_eq!(target.pixel(2, 2), Some([255, 255, 255, 255]), "background");
    assert_eq!(target.pixel(32, 32), Some([255, 0, 0, 255]), "the square");
    assert_eq!(target.pixel(60, 60), Some([255, 255, 255, 255]), "outside");
    std::fs::write(
        std::env::temp_dir().join("xarast_smoke.png"),
        xarast_render::golden::encode_png(&target).unwrap(),
    )
    .unwrap();
}
