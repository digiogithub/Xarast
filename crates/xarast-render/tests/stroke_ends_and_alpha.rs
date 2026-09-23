//! Regressions for gintrack XARA-T-0231, at the pixel level.
//!
//! * A partly transparent Mix over a transparent destination keeps its
//!   colour: the result is source-over, not the colour mixed towards the
//!   black of an empty pixel and premultiplied a second time.
//! * The CPU stroker gives each end of a stroke its own cap, and starts the
//!   dash pattern at the pattern's offset, as `xarast_geom::stroke_to_path`
//!   defines both — on the whole-path route and on the route that cuts a
//!   stroke to the band first.

use xarast_color::Rgba8;
use xarast_geom::{
    Cap, DashPattern, FillRule, Join, Mp, Path, Point, Rect, StrokeStyle, Tolerance, stroke_to_path,
};
use xarast_render::backend::cpu::Resolver;
use xarast_render::{
    CpuBackend, CpuConfig, DirtyRect, DisplayList, Paint, PathRef, RenderQuality, Scene,
    SceneBuilder, SceneNodeId, Surface, Transform2D, Transparency, ViewParams,
};

/// Ten device pixels per point: one pixel is 100 mp.
const SCALE: f64 = 0.01;

fn render_onto(scene: &Scene, mut target: Surface) -> Surface {
    let view = ViewParams::new(
        target.width(),
        target.height(),
        Transform2D::scale(SCALE),
        RenderQuality::Final,
    );
    let dl = DisplayList::build(scene, &view, &DirtyRect::NONE);
    CpuBackend::new(CpuConfig::deterministic())
        .render(&dl, &Resolver::new(), &mut target)
        .expect("renders");
    target
}

fn rect_path(x0: i32, y0: i32, x1: i32, y1: i32) -> PathRef {
    let mut b = Path::builder();
    b.rect(Rect::raw(x0, y0, x1, y1));
    PathRef::new(b.build())
}

fn line(x0: i32, x1: i32, y: i32) -> PathRef {
    let mut b = Path::builder();
    b.move_to(Point::new(Mp::new(x0), Mp::new(y)));
    b.line_to(Point::new(Mp::new(x1), Mp::new(y)));
    PathRef::new(b.build())
}

fn solid(r: u8, g: u8, b: u8) -> Paint {
    Paint::Solid(Rgba8 { r, g, b, a: 255 })
}

/// One filled rectangle with a flat Mix transparency, over `target`.
fn mix_fill(target: Surface, paint: Paint, t: u8) -> Surface {
    let mut scene = Scene::new();
    let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
    b.push_transparency(Transparency::mix(t));
    // Whole pixels from x = 2 to 8, and a left edge half way through
    // pixel 1, so that one column is antialiased.
    b.fill(
        SceneNodeId(1),
        &rect_path(150, 0, 800, 1_000),
        FillRule::NonZero,
        paint,
    );
    b.pop_transparency();
    b.finish().expect("balanced");
    render_onto(&scene, target)
}

fn px(s: &Surface, x: i32, y: i32) -> [u8; 4] {
    s.pixel(x, y).expect("inside")
}

fn straight(p: [u8; 4]) -> [u8; 4] {
    let mut v = p.to_vec();
    xarast_render::unpremultiply_rgba_in_place(&mut v);
    [v[0], v[1], v[2], v[3]]
}

#[test]
fn half_transparent_white_over_nothing_stays_white() {
    // Transparency 127 is an alpha of 128: the acceptance case, a 50 %
    // white square exporting as (255, 255, 255, 128).
    let s = mix_fill(Surface::new(10, 10), solid(255, 255, 255), 127);
    assert_eq!(px(&s, 4, 5), [128, 128, 128, 128], "premultiplied white");
    assert_eq!(straight(px(&s, 4, 5)), [255, 255, 255, 128]);
    // The antialiased column: white at whatever alpha it has.
    let edge = px(&s, 1, 5);
    assert!(edge[3] > 0 && edge[3] < 128, "{edge:?}");
    assert_eq!(edge[0], edge[3], "{edge:?}");
    assert_eq!(straight(edge)[..3], [255, 255, 255], "{edge:?}");
    // Nothing drawn outside it.
    assert_eq!(px(&s, 9, 5), [0, 0, 0, 0]);
}

#[test]
fn a_translucent_colour_over_nothing_keeps_its_hue() {
    let s = mix_fill(Surface::new(10, 10), solid(200, 100, 50), 127);
    let got = straight(px(&s, 4, 5));
    for (g, want) in got.iter().zip([200u8, 100, 50, 128]) {
        assert!(g.abs_diff(want) <= 1, "{got:?}");
    }
}

#[test]
fn a_translucent_colour_over_a_translucent_destination_is_source_over() {
    // Destination: blue at alpha 128, premultiplied. Source: white at
    // alpha 128. Source-over: alpha 128 + 128·127/255 = 192; red and green
    // are the source's 128; blue adds the destination's 128·127/255 = 64.
    let s = mix_fill(
        Surface::filled(10, 10, [0, 0, 128, 128]),
        solid(255, 255, 255),
        127,
    );
    let got = px(&s, 4, 5);
    for (g, want) in got.iter().zip([128u8, 128, 192, 192]) {
        assert!(g.abs_diff(want) <= 1, "{got:?}");
    }
}

#[test]
fn an_opaque_destination_is_unchanged_by_the_fix() {
    let s = mix_fill(
        Surface::filled(10, 10, [0, 0, 0, 255]),
        solid(255, 255, 255),
        127,
    );
    assert_eq!(px(&s, 4, 5), [128, 128, 128, 255]);
}

fn stroke_style(start: Cap, end: Cap, dash: Option<DashPattern>) -> StrokeStyle {
    StrokeStyle {
        width: Mp::new(2_000),
        cap_start: start,
        cap_end: end,
        join: Join::Mitre,
        mitre_limit: 4.0,
        dash,
    }
}

fn stroke_scene(path: &PathRef, style: StrokeStyle) -> Scene {
    let mut scene = Scene::new();
    let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
    b.stroke(SceneNodeId(1), path, style, solid(0, 0, 0));
    b.finish().expect("balanced");
    scene
}

/// The same stroke, expanded by `stroke_to_path` and filled.
fn reference_scene(path: &PathRef, style: &StrokeStyle) -> Scene {
    let outline = stroke_to_path(path.path(), style, Tolerance(0.5)).expect("outline");
    let mut scene = Scene::new();
    let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
    b.fill(
        SceneNodeId(1),
        &PathRef::new(outline),
        FillRule::NonZero,
        solid(0, 0, 0),
    );
    b.finish().expect("balanced");
    scene
}

fn alpha(s: &Surface, x: i32, y: i32) -> u8 {
    px(s, x, y)[3]
}

fn max_diff(a: &Surface, b: &Surface) -> u8 {
    a.data()
        .iter()
        .zip(b.data())
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0)
}

#[test]
fn each_end_gets_its_own_cap() {
    // From x = 50 px to 150 px on row 50, 20 px wide.
    let path = line(5_000, 15_000, 5_000);
    for (start, end) in [
        (Cap::Round, Cap::Butt),
        (Cap::Butt, Cap::Square),
        (Cap::Square, Cap::Round),
    ] {
        let style = stroke_style(start, end, None);
        let s = render_onto(&stroke_scene(&path, style.clone()), Surface::new(200, 100));
        // Seven pixels past each end on the centre line: inside a round or
        // square cap (radius 10 px), outside a butt one.
        let covered = |c: Cap| c != Cap::Butt;
        assert_eq!(alpha(&s, 43, 50) == 255, covered(start), "{start:?} start");
        assert_eq!(alpha(&s, 156, 50) == 255, covered(end), "{end:?} end");
        // Nine pixels out diagonally: only a square cap reaches it.
        assert_eq!(alpha(&s, 41, 41) > 0, start == Cap::Square, "{start:?}");
        assert_eq!(alpha(&s, 158, 58) > 0, end == Cap::Square, "{end:?}");
        let want = render_onto(&reference_scene(&path, &style), Surface::new(200, 100));
        let d = max_diff(&s, &want);
        assert!(
            d <= 16,
            "{start:?}/{end:?}: differs from stroke_to_path by {d}"
        );
    }
}

fn dashes(offset: i32) -> Option<DashPattern> {
    // 10 px on, 10 px off.
    Some(DashPattern {
        elements: vec![Mp::new(1_000), Mp::new(1_000)],
        offset: Mp::new(offset),
        reference_width: None,
    })
}

fn centre_row(s: &Surface) -> Vec<u8> {
    (0..i32::try_from(s.width()).unwrap())
        .map(|x| alpha(s, x, 50))
        .collect()
}

#[test]
fn the_dash_offset_shifts_the_pattern() {
    let path = line(0, 20_000, 5_000);
    let plain = stroke_style(Cap::Butt, Cap::Butt, dashes(0));
    let shifted = stroke_style(Cap::Butt, Cap::Butt, dashes(500));
    let a = centre_row(&render_onto(
        &stroke_scene(&path, plain),
        Surface::new(200, 100),
    ));
    let s = render_onto(
        &stroke_scene(&path, shifted.clone()),
        Surface::new(200, 100),
    );
    let b = centre_row(&s);
    assert_ne!(a, b, "the offset moved nothing");
    // Five pixels into the pattern: the shifted row is the plain row read
    // five pixels further on.
    for x in 10..180 {
        assert!(
            b[x].abs_diff(a[x + 5]) <= 1,
            "x = {x}: {} vs {}",
            b[x],
            a[x + 5]
        );
    }
    // An offset of whole periods changes nothing.
    let wrapped = stroke_style(Cap::Butt, Cap::Butt, dashes(500 + 2_000 * 7));
    let c = centre_row(&render_onto(
        &stroke_scene(&path, wrapped),
        Surface::new(200, 100),
    ));
    assert_eq!(b, c);
    let want = render_onto(&reference_scene(&path, &shifted), Surface::new(200, 100));
    let d = max_diff(&s, &want);
    assert!(d <= 16, "differs from stroke_to_path by {d}");
}

#[test]
fn the_dash_offset_survives_band_culling() {
    // A line hundreds of screens long is cut to the band before it is
    // dashed (XARA-T-0022). It starts a whole number of periods (2 000 mp)
    // before the short one, so both put the same phase in the view.
    let long = line(-10_000_000, 10_000_000, 5_000);
    let short = line(-30_000, 30_000, 5_000);
    // 4 px wide, so the 2 px caps leave most of each 10 px gap open.
    let thin = |offset| StrokeStyle {
        width: Mp::new(400),
        ..stroke_style(Cap::Round, Cap::Square, dashes(offset))
    };
    let style = thin(700);
    let l = render_onto(&stroke_scene(&long, style.clone()), Surface::new(200, 100));
    let s = render_onto(&stroke_scene(&short, style), Surface::new(200, 100));
    let d = max_diff(&l, &s);
    assert!(d <= 1, "the culled stroke differs by {d}");
    let u = render_onto(&stroke_scene(&long, thin(0)), Surface::new(200, 100));
    assert_ne!(centre_row(&l), centre_row(&u), "the offset was lost");
}
