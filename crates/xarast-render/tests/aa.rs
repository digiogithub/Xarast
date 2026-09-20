//! Antialiasing: the level count and the fidelity bound.
//!
//! The phase's criterion 10 asks for at least 132 distinct coverage levels
//! — the original's high-quality figure — and mean ΔE₀₀ below 1.0 with p99
//! below 3.0 against a supersampled reference. Both are checked here.

mod common;

use xarast_color::Rgba8;
use xarast_geom::{FillRule, Mp, Path, Point};
use xarast_render::backend::cpu::Resolver;
use xarast_render::corpus::all_cases;
use xarast_render::golden::{compare, coverage_levels, downsample};
use xarast_render::{
    CpuBackend, CpuConfig, DirtyRect, DisplayList, Paint, PathRef, RenderQuality, Scene,
    SceneBuilder, SceneNodeId, Surface, Transform2D, ViewParams,
};

/// The original scores 85 levels in its normal mode.
const CDRAW_NORMAL_LEVELS: usize = 85;
/// …and 132 in high quality. That is the bar.
const CDRAW_HIGH_QUALITY_LEVELS: usize = 132;

fn ramp_scene(width: f64, height: f64, degrees: f64) -> Scene {
    let mut scene = Scene::new();
    let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
    let mut bg = Path::builder();
    bg.move_to(Point::new(Mp::ZERO, Mp::ZERO));
    bg.line_to(Point::new(Mp::from_pt(width), Mp::ZERO));
    bg.line_to(Point::new(Mp::from_pt(width), Mp::from_pt(height)));
    bg.line_to(Point::new(Mp::ZERO, Mp::from_pt(height)));
    bg.close();
    b.fill(
        SceneNodeId(0),
        &PathRef::new(bg.build()),
        FillRule::NonZero,
        Paint::Solid(Rgba8::WHITE),
    );
    let t = degrees.to_radians().tan();
    let mut p = Path::builder();
    p.move_to(Point::new(Mp::from_pt(-8.0), Mp::from_pt(height / 2.0)));
    p.line_to(Point::new(
        Mp::from_pt(width + 8.0),
        Mp::from_pt(height / 2.0 - (width + 16.0) * t),
    ));
    p.line_to(Point::new(
        Mp::from_pt(width + 8.0),
        Mp::from_pt(height + 8.0),
    ));
    p.line_to(Point::new(Mp::from_pt(-8.0), Mp::from_pt(height + 8.0)));
    p.close();
    b.fill(
        SceneNodeId(1),
        &PathRef::new(p.build()),
        FillRule::NonZero,
        Paint::Solid(Rgba8::BLACK),
    );
    b.finish().unwrap();
    scene
}

fn render(scene: &Scene, w: u32, h: u32, scale: f64) -> Surface {
    render_at(scene, w, h, scale, RenderQuality::Final)
}

fn render_at(scene: &Scene, w: u32, h: u32, scale: f64, quality: RenderQuality) -> Surface {
    let view = ViewParams::new(
        w,
        h,
        Transform2D::scale(scale / f64::from(Mp::PER_PT)),
        quality,
    );
    let dl = DisplayList::build(scene, &view, &DirtyRect::NONE);
    let mut target = Surface::new(w, h);
    CpuBackend::new(CpuConfig::deterministic())
        .render(&dl, &Resolver::new(), &mut target)
        .unwrap();
    target
}

#[test]
fn a_half_degree_edge_resolves_more_levels_than_the_original() {
    let scene = ramp_scene(1024.0, 256.0, 0.5);
    let s = render(&scene, 1024, 256, 1.0);
    let levels = coverage_levels(&s);
    assert!(
        levels >= CDRAW_HIGH_QUALITY_LEVELS,
        "{levels} coverage levels, the bar is {CDRAW_HIGH_QUALITY_LEVELS} \
         (the original's high-quality mode; its normal mode gives {CDRAW_NORMAL_LEVELS})"
    );
    eprintln!("coverage levels on a 0.5 degree edge: {levels}");
}

#[test]
fn antialiasing_matches_a_sixteen_times_supersampled_reference() {
    for degrees in [0.5f64, 2.0, 17.0, 45.0] {
        let scene = ramp_scene(64.0, 64.0, degrees);
        let direct = render(&scene, 64, 64, 1.0);
        let reference = downsample(&render(&scene, 1024, 1024, 16.0), 16);
        let c = compare(&direct, &reference);
        assert!(
            c.passes_perceptual(),
            "{degrees} degrees: mean dE {:.4}, p99 {:.4}, max channel {}",
            c.mean_delta_e,
            c.p99_delta_e,
            c.max_channel_delta
        );
        eprintln!(
            "{degrees:>5} deg: mean dE00 {:.4}, p99 {:.4}, max channel {}/255",
            c.mean_delta_e, c.p99_delta_e, c.max_channel_delta
        );
    }
}

#[test]
fn the_corpus_antialiasing_probes_are_all_antialiased() {
    // The regression this catches is someone turning antialiasing off to
    // make Draft faster: Draft keeps AA on, by design.
    //
    // The bar is per-probe. A shallow edge sweeps the whole coverage
    // range, but an exact 45 degree edge on a pixel grid legitimately
    // produces almost none: it passes through pixel corners, so every
    // boundary pixel is covered exactly half. Demanding many levels there
    // would be demanding an artefact.
    for case in all_cases().iter().filter(|c| c.name.starts_with("aa_")) {
        let s = common::render_case(case);
        let levels = coverage_levels(&s);
        let bar = match case.name.as_str() {
            "aa_edge_0p5" | "aa_edge_2" => 64,
            _ => 3,
        };
        assert!(
            levels >= bar,
            "{} has {levels} grey levels, expected at least {bar}: is antialiasing on?",
            case.name
        );
    }
}

#[test]
fn a_curve_also_matches_the_supersampled_reference() {
    // A straight edge is a weak test of area coverage: the box average of
    // sixteen exact sub-areas of a straight edge is the exact area, so a
    // correct rasteriser matches the reference to the last bit. A curve
    // does not have that property, so this is where the comparison has
    // something to say.
    let mut scene = Scene::new();
    {
        let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
        let mut bg = Path::builder();
        bg.move_to(Point::new(Mp::ZERO, Mp::ZERO));
        bg.line_to(Point::new(Mp::from_pt(64.0), Mp::ZERO));
        bg.line_to(Point::new(Mp::from_pt(64.0), Mp::from_pt(64.0)));
        bg.line_to(Point::new(Mp::ZERO, Mp::from_pt(64.0)));
        bg.close();
        b.fill(
            SceneNodeId(0),
            &PathRef::new(bg.build()),
            FillRule::NonZero,
            Paint::Solid(Rgba8::WHITE),
        );
        let mut p = Path::builder();
        p.ellipse(
            Point::new(Mp::from_pt(32.0), Mp::from_pt(32.0)),
            Mp::from_pt(26.0),
            Mp::from_pt(19.0),
        );
        b.fill(
            SceneNodeId(1),
            &PathRef::new(p.build()),
            FillRule::NonZero,
            Paint::Solid(Rgba8::BLACK),
        );
        b.finish().unwrap();
    }
    // The reference is rendered at sixteen times the scale, so its curve
    // is also flattened sixteen times more finely. What is left after the
    // comparison is therefore the *flatness tolerance*, not the coverage
    // maths: a chord error of `t` device pixels moves an edge by up to `t`
    // of a pixel, which is `t * 255` of coverage.
    let reference = downsample(&render(&scene, 1024, 1024, 16.0), 16);
    for (quality, tolerance_px) in [(RenderQuality::Final, 0.1f64), (RenderQuality::Draft, 0.5)] {
        let direct = render_at(&scene, 64, 64, 1.0, quality);
        let c = compare(&direct, &reference);
        eprintln!(
            "ellipse {quality:?} (flatness {tolerance_px} px): mean dE00 {:.4}, p99 {:.4}, max channel {}/255",
            c.mean_delta_e, c.p99_delta_e, c.max_channel_delta
        );
        assert!(
            c.mean_delta_e < 1.0,
            "{quality:?}: mean dE {:.4}",
            c.mean_delta_e
        );
        let bound = (tolerance_px * 255.0 * 1.2).ceil() as u8;
        assert!(
            c.max_channel_delta <= bound,
            "{quality:?}: max channel {} exceeds the {bound} the {tolerance_px} px \
             flatness allows",
            c.max_channel_delta
        );
    }

    // And the knob does something: Draft is measurably coarser than Final.
    let fine = compare(
        &render_at(&scene, 64, 64, 1.0, RenderQuality::Final),
        &reference,
    );
    let coarse = compare(
        &render_at(&scene, 64, 64, 1.0, RenderQuality::Draft),
        &reference,
    );
    assert!(
        coarse.mean_delta_e > fine.mean_delta_e,
        "Draft should be coarser than Final: {:.4} against {:.4}",
        coarse.mean_delta_e,
        fine.mean_delta_e
    );
}
