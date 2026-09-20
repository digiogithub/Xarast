//! Property tests over randomly generated scenes.

mod common;

use proptest::prelude::*;
use xarast_color::Rgba8;
use xarast_geom::{FillRule, Mp, Path, Point, Rect};
use xarast_render::backend::cpu::Resolver;
use xarast_render::blend::{ALL_FAMILIES, Transparency};
use xarast_render::golden::digest;
use xarast_render::{
    CpuBackend, CpuConfig, DirtyRect, DisplayList, LayerKind, Paint, PathRef, RenderQuality, Scene,
    SceneBuilder, SceneError, SceneNodeId, Surface, Transform2D, ViewParams,
};

/// One randomly generated drawing step.
#[derive(Debug, Clone)]
enum Step {
    Fill(f64, f64, f64, u8),
    PushGroup(f64, f64),
    PopGroup,
    PushLayer(usize, u8),
    PopLayer,
    PushClip(f64, f64),
    PopClip,
    PushTransparency(usize, u8),
    PopTransparency,
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        (0.0f64..60.0, 0.0f64..60.0, 4.0f64..30.0, any::<u8>())
            .prop_map(|(x, y, s, c)| Step::Fill(x, y, s, c)),
        (-20.0f64..20.0, -20.0f64..20.0).prop_map(|(x, y)| Step::PushGroup(x, y)),
        Just(Step::PopGroup),
        (0usize..12, any::<u8>()).prop_map(|(f, t)| Step::PushLayer(f, t)),
        Just(Step::PopLayer),
        (0.0f64..40.0, 20.0f64..60.0).prop_map(|(a, b)| Step::PushClip(a, b)),
        Just(Step::PopClip),
        (0usize..12, any::<u8>()).prop_map(|(f, t)| Step::PushTransparency(f, t)),
        Just(Step::PopTransparency),
    ]
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> PathRef {
    let mut b = Path::builder();
    b.rect(Rect::new(
        Point::new(Mp::from_pt(x0), Mp::from_pt(y0)),
        Point::new(Mp::from_pt(x1), Mp::from_pt(y1)),
    ));
    PathRef::new(b.build())
}

/// Records the steps, closing whatever is left open and dropping any pop
/// that has nothing to pop, so that the scene handed to the renderer is
/// always balanced. Returns the scene and the builder's verdict.
fn record(steps: &[Step]) -> (Scene, Result<(), SceneError>) {
    let mut scene = Scene::new();
    let mut depth: Vec<Step> = Vec::new();
    let mut raw_ok = true;
    {
        let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
        for (i, s) in steps.iter().enumerate() {
            match s {
                Step::Fill(x, y, sz, c) => b.fill(
                    SceneNodeId(i as u64),
                    &rect(*x, *y, x + sz, y + sz),
                    FillRule::NonZero,
                    Paint::Solid(Rgba8 {
                        r: *c,
                        g: c.wrapping_mul(3),
                        b: c.wrapping_mul(7),
                        a: 255,
                    }),
                ),
                Step::PushGroup(dx, dy) => {
                    b.push_group(
                        SceneNodeId(1_000 + i as u64),
                        Transform2D::translate(dx * 1000.0, dy * 1000.0),
                        xarast_render::CacheHint::Auto,
                    );
                    depth.push(s.clone());
                }
                Step::PopGroup => match depth.last() {
                    Some(Step::PushGroup(..)) => {
                        depth.pop();
                        b.pop_group();
                    }
                    _ => raw_ok = false,
                },
                Step::PushLayer(f, t) => {
                    b.push_layer(
                        LayerKind::Isolated,
                        Transparency::flat(ALL_FAMILIES[*f], *t),
                    );
                    depth.push(s.clone());
                }
                Step::PopLayer => match depth.last() {
                    Some(Step::PushLayer(..)) => {
                        depth.pop();
                        b.pop_layer();
                    }
                    _ => raw_ok = false,
                },
                Step::PushClip(a, c) => {
                    b.push_clip(&rect(*a, *a, *c, *c), FillRule::NonZero);
                    depth.push(s.clone());
                }
                Step::PopClip => match depth.last() {
                    Some(Step::PushClip(..)) => {
                        depth.pop();
                        b.pop_clip();
                    }
                    _ => raw_ok = false,
                },
                Step::PushTransparency(f, t) => {
                    b.push_transparency(Transparency::flat(ALL_FAMILIES[*f], *t));
                    depth.push(s.clone());
                }
                Step::PopTransparency => match depth.last() {
                    Some(Step::PushTransparency(..)) => {
                        depth.pop();
                        b.pop_transparency();
                    }
                    _ => raw_ok = false,
                },
            }
        }
        if !depth.is_empty() {
            raw_ok = false;
        }
        while let Some(open) = depth.pop() {
            match open {
                Step::PushGroup(..) => b.pop_group(),
                Step::PushLayer(..) => b.pop_layer(),
                Step::PushClip(..) => b.pop_clip(),
                Step::PushTransparency(..) => b.pop_transparency(),
                _ => unreachable!("only pushes are recorded as open"),
            }
        }
        let _ = raw_ok;
        let closed = b.finish().map(|_| ());
        (scene, closed)
    }
}

fn render(scene: &Scene) -> Surface {
    let view = ViewParams::new(
        64,
        64,
        Transform2D::scale(1.0 / 1000.0),
        RenderQuality::Final,
    );
    let dl = DisplayList::build(scene, &view, &DirtyRect::NONE);
    let mut target = Surface::new(64, 64);
    CpuBackend::new(CpuConfig::deterministic())
        .render(&dl, &Resolver::new(), &mut target)
        .expect("renders");
    target
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// Any display list rendered twice yields identical bytes.
    #[test]
    fn rendering_is_a_function_of_its_input(steps in prop::collection::vec(step(), 0..24)) {
        let (scene, _) = record(&steps);
        prop_assert_eq!(digest(&render(&scene)), digest(&render(&scene)));
    }

    /// A balanced sequence always closes; the builder never panics.
    #[test]
    fn a_balanced_sequence_always_closes(steps in prop::collection::vec(step(), 0..24)) {
        let (_, closed) = record(&steps);
        prop_assert!(closed.is_ok(), "{closed:?}");
    }

    /// Nothing the builder can emit makes the renderer panic or write
    /// outside its surface.
    #[test]
    fn no_sequence_escapes_the_surface(steps in prop::collection::vec(step(), 0..24)) {
        let (scene, _) = record(&steps);
        let s = render(&scene);
        prop_assert_eq!(s.data().len(), 64 * 64 * 4);
    }
}

/// An unbalanced sequence is rejected, not rendered.
#[test]
fn an_unbalanced_sequence_is_rejected_at_build_time() {
    let mut scene = Scene::new();
    let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
    b.push_layer(LayerKind::Isolated, Transparency::OPAQUE);
    b.push_clip(&rect(0.0, 0.0, 10.0, 10.0), FillRule::NonZero);
    assert!(b.finish().is_err());
}

/// A flat interior is scale-independent: doubling the zoom must not change
/// the colour away from any edge.
#[test]
fn flat_interiors_do_not_depend_on_the_scale() {
    let mut scene = Scene::new();
    {
        let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
        b.fill(
            SceneNodeId(0),
            &rect(0.0, 0.0, 200.0, 200.0),
            FillRule::NonZero,
            Paint::Solid(Rgba8::WHITE),
        );
        b.fill(
            SceneNodeId(1),
            &rect(4.0, 4.0, 60.0, 60.0),
            FillRule::NonZero,
            Paint::Solid(Rgba8 {
                r: 37,
                g: 149,
                b: 220,
                a: 255,
            }),
        );
        b.finish().unwrap();
    }
    for scale in [1.0f64, 2.0, 4.0] {
        let view = ViewParams::new(
            64,
            64,
            Transform2D::scale(scale / 1000.0),
            RenderQuality::Final,
        );
        let dl = DisplayList::build(&scene, &view, &DirtyRect::NONE);
        let mut target = Surface::new(64, 64);
        CpuBackend::new(CpuConfig::deterministic())
            .render(&dl, &Resolver::new(), &mut target)
            .unwrap();
        assert_eq!(
            target.pixel(32, 32),
            Some([37, 149, 220, 255]),
            "the interior moved at scale {scale}"
        );
    }
}
