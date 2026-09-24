//! Three- and four-colour transparencies are evaluated per pixel, on the
//! same frame, corner order and tiling as a colour mesh (XARA-T-0256).
//!
//! A white fill mixed over black at transparency `t` comes out at
//! `255 − t`, so a mesh *transparency* over black must draw the same
//! picture as an opaque *colour* mesh whose corners are `255 − level`,
//! in every repeat mode. The two differ only where a channel lands on an
//! exact half and rounds the other way, hence the tolerance of one.

use xarast_color::Rgba8;
use xarast_geom::{FillRule, Mp, Path, Point, Rect};
use xarast_render::backend::cpu::Resolver;
use xarast_render::blend::{BlendFamily, TranspSource, Transparency};
use xarast_render::paint::ALL_REPEATS;
use xarast_render::{
    CpuBackend, CpuConfig, DirtyRect, DisplayList, GradMapping, GradRamp, GradShape, MeshLevels,
    Paint, PathRef, Point64, RenderQuality, Repeat, Scene, SceneBuilder, SceneNodeId, Surface,
    Transform2D, ViewParams,
};

const SIZE: u32 = 64;

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> PathRef {
    let mut b = Path::builder();
    b.rect(Rect::new(
        Point::new(Mp::from_pt(x0), Mp::from_pt(y0)),
        Point::new(Mp::from_pt(x1), Mp::from_pt(y1)),
    ));
    PathRef::new(b.build())
}

fn dpt(x: f64, y: f64) -> Point64 {
    Point64::new(x * f64::from(Mp::PER_PT), y * f64::from(Mp::PER_PT))
}

/// A frame much smaller than the view, so every repeat mode shows tiles.
fn mapping(four: bool) -> GradMapping {
    if four {
        GradMapping::Perspective {
            a: dpt(20.0, 20.0),
            b: dpt(20.0, 38.0),
            c: dpt(40.0, 22.0),
            d: dpt(42.0, 40.0),
        }
    } else {
        GradMapping::Affine {
            a: dpt(20.0, 20.0),
            b: dpt(20.0, 36.0),
            c: dpt(38.0, 20.0),
        }
    }
}

fn render(top: impl FnOnce(&mut SceneBuilder<'_>)) -> Surface {
    let mut scene = Scene::new();
    {
        let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
        b.fill(
            SceneNodeId(0),
            &rect(0.0, 0.0, 64.0, 64.0),
            FillRule::NonZero,
            Paint::Solid(Rgba8::BLACK),
        );
        top(&mut b);
        b.finish().expect("balanced");
    }
    let view = ViewParams::new(
        SIZE,
        SIZE,
        Transform2D::scale(1.0 / f64::from(Mp::PER_PT)),
        RenderQuality::Final,
    );
    let dl = DisplayList::build(&scene, &view, &DirtyRect::NONE);
    let mut target = Surface::new(SIZE, SIZE);
    CpuBackend::new(CpuConfig::deterministic())
        .render(&dl, &Resolver::new(), &mut target)
        .expect("fits");
    target
}

fn grey(l: u8) -> Rgba8 {
    Rgba8 {
        r: 255 - l,
        g: 255 - l,
        b: 255 - l,
        a: 255,
    }
}

#[test]
fn a_mesh_transparency_draws_its_colour_mesh_in_every_repeat_mode() {
    let meshes = [
        (false, MeshLevels::Three([0, 250, 120])),
        (true, MeshLevels::Four([0, 250, 120, 60])),
    ];
    for (four, levels) in meshes {
        for repeat in ALL_REPEATS {
            let through_transparency = render(|b| {
                b.push_transparency(Transparency {
                    family: BlendFamily::Mix,
                    source: TranspSource::Mesh {
                        mapping: mapping(four),
                        repeat,
                        levels,
                    },
                });
                b.fill(
                    SceneNodeId(1),
                    &rect(0.0, 0.0, 64.0, 64.0),
                    FillRule::NonZero,
                    Paint::Solid(Rgba8::WHITE),
                );
                b.pop_transparency();
            });
            let (shape, ramp) = match levels {
                MeshLevels::Three(l) => (
                    GradShape::Mesh3,
                    GradRamp::Mesh3([grey(l[0]), grey(l[1]), grey(l[2])]),
                ),
                MeshLevels::Four(l) => (
                    GradShape::Mesh4,
                    GradRamp::Mesh4([grey(l[0]), grey(l[1]), grey(l[2]), grey(l[3])]),
                ),
            };
            let through_colour = render(|b| {
                b.fill(
                    SceneNodeId(1),
                    &rect(0.0, 0.0, 64.0, 64.0),
                    FillRule::NonZero,
                    Paint::Gradient {
                        shape,
                        mapping: mapping(four),
                        repeat,
                        ramp,
                    },
                );
            });
            let (a, b) = (through_transparency.data(), through_colour.data());
            let worst = a
                .iter()
                .zip(b)
                .map(|(x, y)| x.abs_diff(*y))
                .max()
                .unwrap_or(0);
            assert!(worst <= 1, "{levels:?} {repeat:?}: off by {worst}");
            // The transparency is not flat: it spans most of its levels.
            let reds: Vec<u8> = a.as_chunks::<4>().0.iter().map(|p| p[0]).collect();
            let (lo, hi) = (
                reds.iter().min().copied().unwrap_or(0),
                reds.iter().max().copied().unwrap_or(0),
            );
            assert!(hi - lo > 150, "{levels:?} {repeat:?}: {lo}..{hi}");
        }
    }
}

#[test]
fn tiled_mesh_levels_mirror_at_the_tile_edge() {
    // One tile past the frame's u = 1 edge, a mirrored mesh reads back the
    // level it had just inside it; a clamped one keeps the edge's value.
    let levels = MeshLevels::Four([0, 200, 0, 200]);
    let at = |u: f64, repeat: Repeat| {
        let (u, v) = xarast_render::paint::mesh_uv((u, 0.0), repeat);
        levels.at(u, v)
    };
    assert_eq!(at(0.75, Repeat::Mirror), at(1.25, Repeat::Mirror));
    assert_eq!(at(1.25, Repeat::Simple), 200);
    assert_eq!(at(1.25, Repeat::Repeat), at(0.25, Repeat::Repeat));
}
