//! Arbitrary scenes, through `DisplayList::build` and the CPU backend.
//!
//! A scene is what the walker in `xarast-app` records from a document, so
//! every geometry, transform, paint and transparency in it traces back to a
//! file. The target drives `SceneBuilder` with an arbitrary sequence —
//! pops with nothing to pop and pushes never closed included — and checks:
//!
//! 1. The builder either rejects the sequence or accepts a balanced one.
//! 2. The display list of an accepted scene is balanced: every `PushClip`
//!    and `PushLayer` is closed, in order.
//! 3. Its bounds lie inside the viewport.
//! 4. The deterministic CPU backend renders it without panicking, and a
//!    second render of the same list gives the same bytes.
//!
//! Transforms are bounded to what a document can hold — a 16.16 linear
//! part and a translation inside `i32` millipoints — and the view scale to
//! a little past the maximum zoom, so that a slow case means a real
//! drawing would be slow too, not that the fuzzer asked for a 10^300 zoom.
//!
//! Geometry spans the whole document extent and dash patterns are not
//! limited. Both used to be bounded, because the CPU backend dashed and
//! expanded a whole path before clipping it: an extent-sized, finely
//! dashed, round-capped stroke at deep zoom exhausted memory. The backend
//! now cuts a stroke to the band plus its reach and caps the dash count
//! (`xarast_render::stroke_cull`, gintrack XARA-T-0022).

#![no_main]

mod common;

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use xarast_color::Rgba8;
use xarast_geom::{Cap, DashPattern, FillRule, Join, Mp, StrokeStyle};
use xarast_render::{
    ALL_FAMILIES, BlendFamily, CacheHint, CpuBackend, CpuConfig, DeviceRect, DirtyRect,
    DisplayList, DrawCmd, EffectSpace, Filter, GradMapping, GradRamp, GradShape, ImageId,
    ImageRef, LayerKind, Paint, PathRef, Point64, Profile, RampId, RenderQuality, Repeat,
    Resolver, Scene, SceneBuilder, SceneNodeId, Stop, Surface, Transform2D, TranspSource,
    Transparency, build_transparency_ramp,
};

const MAX_STEPS: usize = 32;
const MAX_DASH: usize = 8;

#[derive(Arbitrary, Debug)]
enum PaintSel {
    Solid([u8; 4]),
    Gradient {
        shape: u8,
        handles: [(f32, f32); 4],
        perspective: bool,
        repeat: u8,
        ramp: u8,
        corners: [[u8; 4]; 4],
    },
    Image {
        handles: [(f32, f32); 4],
        perspective: bool,
        repeat: u8,
        filter: u8,
    },
}

#[derive(Arbitrary, Debug)]
enum TranspSel {
    Flat(u8, u8),
    Gradient {
        family: u8,
        shape: u8,
        handles: [(f32, f32); 3],
        repeat: u8,
        ramp: u8,
    },
    Image {
        family: u8,
        handles: [(f32, f32); 3],
        repeat: u8,
    },
}

#[derive(Arbitrary, Debug)]
enum Step {
    Fill {
        path: Vec<common::PathOp>,
        rule: u8,
        paint: PaintSel,
    },
    Stroke {
        path: Vec<common::PathOp>,
        width: common::Coord,
        caps: (u8, u8),
        join: u8,
        mitre: f32,
        dash: Option<(Vec<common::Coord>, common::Coord)>,
        paint: PaintSel,
    },
    Image {
        handles: [(f32, f32); 4],
        perspective: bool,
        paint: PaintSel,
    },
    PushGroup([f32; 6], u8),
    PopGroup,
    PushClip(Vec<common::PathOp>, u8),
    PopClip,
    PushTransparency(TranspSel),
    PopTransparency,
    PushLayer(u8, TranspSel),
    PopLayer,
    /// A feather: size in points, bias, gain.
    PushFeather(f32, f32, f32),
    PopFeather,
}

#[derive(Arbitrary, Debug)]
struct Input {
    width: u8,
    height: u8,
    view: [f32; 6],
    draft: bool,
    dirty: Option<(i16, i16, i16, i16)>,
    ramps: Vec<Vec<(f32, [u8; 4])>>,
    image: (u8, u8, u8),
    steps: Vec<Step>,
}

fn rgba(c: [u8; 4]) -> Rgba8 {
    Rgba8 {
        r: c[0],
        g: c[1],
        b: c[2],
        a: c[3],
    }
}

fn rule(v: u8) -> FillRule {
    match v % 4 {
        0 => FillRule::NonZero,
        1 => FillRule::EvenOdd,
        2 => FillRule::Positive,
        _ => FillRule::Negative,
    }
}

fn repeat(v: u8) -> Repeat {
    xarast_render::ALL_REPEATS[usize::from(v) % xarast_render::ALL_REPEATS.len()]
}

fn family(v: u8) -> BlendFamily {
    ALL_FAMILIES[usize::from(v) % ALL_FAMILIES.len()]
}

/// A device-space point from a fuzzed pair: finite, and a few viewports
/// either side of the surface.
fn dev(p: (f32, f32)) -> Point64 {
    let c = |v: f32| f64::from(if v.is_finite() { v.clamp(-1e4, 1e4) } else { 0.0 });
    Point64::new(c(p.0), c(p.1))
}

fn mapping(h: &[(f32, f32)], perspective: bool) -> GradMapping {
    let p = |i: usize| dev(h.get(i).copied().unwrap_or((0.0, 0.0)));
    if perspective {
        GradMapping::Perspective {
            a: p(0),
            b: p(1),
            c: p(2),
            d: p(3),
        }
    } else {
        GradMapping::Affine {
            a: p(0),
            b: p(1),
            c: p(2),
        }
    }
}

/// A document transform: a 16.16-sized linear part and an `i32`-sized
/// translation, as a `.xar` matrix can express.
fn doc_transform(c: [f32; 6]) -> Transform2D {
    let lin = |v: f32| f64::from(if v.is_finite() { v.clamp(-32_768.0, 32_768.0) } else { 0.0 });
    let tr = |v: f32| f64::from(if v.is_finite() { v.clamp(-2.1e9, 2.1e9) } else { 0.0 });
    Transform2D::new([lin(c[0]), lin(c[1]), lin(c[2]), lin(c[3]), tr(c[4]), tr(c[5])])
}

/// Document to device: at most about 0.35 px per millipoint, which is
/// 25 600 % zoom at 96 dpi, with a translation that keeps the drawing
/// somewhere near the surface.
fn view_transform(c: [f32; 6]) -> Transform2D {
    let lin = |v: f32| f64::from(if v.is_finite() { v.clamp(-0.35, 0.35) } else { 0.001 });
    let tr = |v: f32| f64::from(if v.is_finite() { v.clamp(-1e6, 1e6) } else { 0.0 });
    Transform2D::new([lin(c[0]), lin(c[1]), lin(c[2]), lin(c[3]), tr(c[4]), tr(c[5])])
}

struct Ctx {
    ramps: Vec<RampId>,
    image: ImageId,
}

impl Ctx {
    fn ramp(&self, i: u8) -> Option<RampId> {
        if self.ramps.is_empty() {
            None
        } else {
            self.ramps.get(usize::from(i) % self.ramps.len()).copied()
        }
    }

    fn paint(&self, p: &PaintSel) -> Paint {
        match p {
            PaintSel::Solid(c) => Paint::Solid(rgba(*c)),
            PaintSel::Gradient {
                shape,
                handles,
                perspective,
                repeat: r,
                ramp,
                corners,
            } => {
                let shape = xarast_render::ALL_SHAPES
                    [usize::from(*shape) % xarast_render::ALL_SHAPES.len()];
                let ramp = match shape {
                    GradShape::Mesh3 => {
                        GradRamp::Mesh3([rgba(corners[0]), rgba(corners[1]), rgba(corners[2])])
                    }
                    GradShape::Mesh4 => GradRamp::Mesh4(corners.map(rgba)),
                    _ => match self.ramp(*ramp) {
                        Some(id) => GradRamp::Table(id),
                        None => return Paint::Solid(rgba(corners[0])),
                    },
                };
                Paint::Gradient {
                    shape,
                    mapping: mapping(handles, *perspective),
                    repeat: repeat(*r),
                    ramp,
                }
            }
            PaintSel::Image {
                handles,
                perspective,
                repeat: r,
                filter,
            } => Paint::Image {
                image: self.image,
                mapping: mapping(handles, *perspective),
                repeat: repeat(*r),
                filter: match filter % 3 {
                    0 => Filter::Nearest,
                    1 => Filter::Bilinear,
                    _ => Filter::HighQuality,
                },
                contone: None,
                adjust: Default::default(),
            },
        }
    }

    fn transparency(&self, t: &TranspSel) -> Transparency {
        match t {
            TranspSel::Flat(f, level) => Transparency::flat(family(*f), *level),
            TranspSel::Gradient {
                family: f,
                shape,
                handles,
                repeat: r,
                ramp,
            } => {
                let Some(ramp) = self.ramp(*ramp) else {
                    return Transparency::flat(family(*f), 0);
                };
                // Only the scalar shapes drive a transparency.
                let shape = [
                    GradShape::Linear,
                    GradShape::Radial,
                    GradShape::Conical,
                    GradShape::Diamond,
                ][usize::from(*shape) % 4];
                Transparency {
                    family: family(*f),
                    source: TranspSource::Gradient {
                        shape,
                        mapping: mapping(handles, false),
                        repeat: repeat(*r),
                        ramp,
                    },
                }
            }
            TranspSel::Image {
                family: f,
                handles,
                repeat: r,
            } => Transparency {
                family: family(*f),
                source: TranspSource::Image {
                    image: self.image,
                    mapping: mapping(handles, false),
                    repeat: repeat(*r),
                    filter: match (r / 4) % 3 {
                        0 => Filter::Nearest,
                        1 => Filter::Bilinear,
                        _ => Filter::HighQuality,
                    },
                    // Luminance read straight as the level (XARA-T-0307
                    // added the level ramp; the fuzzer never built one).
                    ramp: None,
                },
            },
        }
    }
}

fn stroke_style(
    width: common::Coord,
    caps: (u8, u8),
    join: u8,
    mitre: f32,
    dash: &Option<(Vec<common::Coord>, common::Coord)>,
) -> StrokeStyle {
    let cap = |v: u8| match v % 3 {
        0 => Cap::Butt,
        1 => Cap::Round,
        _ => Cap::Square,
    };
    // Line widths in a document are non-negative and at most a few metres.
    let width = Mp(width.mp().raw().rem_euclid(1_000_000));
    StrokeStyle {
        width,
        cap_start: cap(caps.0),
        cap_end: cap(caps.1),
        join: match join % 3 {
            0 => Join::Mitre,
            1 => Join::Round,
            _ => Join::Bevel,
        },
        mitre_limit: if mitre.is_finite() {
            f64::from(mitre.clamp(0.0, 100.0))
        } else {
            4.0
        },
        dash: dash.as_ref().map(|(elements, offset)| DashPattern {
            elements: elements
                .iter()
                .take(MAX_DASH)
                .map(|c| Mp(c.mp().raw().rem_euclid(1_000_000)))
                .collect(),
            offset: offset.mp(),
            reference_width: None,
        }),
    }
}

/// Checks that the structural commands nest properly.
fn assert_balanced(dl: &DisplayList) {
    #[derive(PartialEq, Debug)]
    enum Open {
        Clip,
        Layer,
        Effect,
    }
    let mut stack = Vec::new();
    for c in dl.commands() {
        match c {
            DrawCmd::PushClip { .. } => stack.push(Open::Clip),
            DrawCmd::PushLayer { .. } => stack.push(Open::Layer),
            DrawCmd::PopClip => assert_eq!(stack.pop(), Some(Open::Clip), "unmatched PopClip"),
            DrawCmd::PopLayer { .. } => {
                assert_eq!(stack.pop(), Some(Open::Layer), "unmatched PopLayer");
            }
            DrawCmd::PushEffect { .. } => stack.push(Open::Effect),
            DrawCmd::PopEffect => {
                assert_eq!(stack.pop(), Some(Open::Effect), "unmatched PopEffect");
            }
            _ => {}
        }
    }
    assert!(stack.is_empty(), "unclosed {stack:?}");
}

fuzz_target!(|input: Input| {
    let width = u32::from(input.width % 64) + 1;
    let height = u32::from(input.height % 64) + 1;

    let mut res = Resolver::new();
    let mut ramps = Vec::new();
    for stops in input.ramps.iter().take(4) {
        let stops: Vec<Stop> = stops
            .iter()
            .take(16)
            .map(|&(o, c)| Stop::new(o, rgba(c)))
            .collect();
        let id = res.ramps.intern(
            &stops,
            Profile::IDENTITY,
            EffectSpace::Rgb,
            xarast_render::RampLength::Short,
        );
        // The transparency ramps are indexed like the colour ramps.
        while res.transparency_ramps.len() <= id.index() as usize {
            let n = res.transparency_ramps.len();
            let level = u8::try_from(n * 60).unwrap_or(255);
            res.transparency_ramps.push(build_transparency_ramp(
                &[
                    xarast_render::TranspStop { offset: 0.0, level: 0 },
                    xarast_render::TranspStop { offset: 1.0, level },
                ],
                Profile::IDENTITY,
                xarast_render::RampLength::Short,
            ));
        }
        ramps.push(id);
    }
    let (iw, ih, seed) = input.image;
    let (iw, ih) = (u32::from(iw % 8) + 1, u32::from(ih % 8) + 1);
    let pixels: Vec<u8> = (0..iw * ih * 4)
        .map(|i| u8::try_from((i * 37 + u32::from(seed)) % 256).unwrap_or(0))
        .collect();
    let image = res.images.insert(ImageRef::new(iw, ih, pixels));
    let ctx = Ctx { ramps, image };

    let quality = if input.draft {
        RenderQuality::Draft
    } else {
        RenderQuality::Final
    };
    let mut scene = Scene::new();
    let closed = {
        let mut b = SceneBuilder::begin(&mut scene, quality);
        for (i, step) in input.steps.iter().take(MAX_STEPS).enumerate() {
            let id = SceneNodeId(i as u64);
            match step {
                Step::Fill { path, rule: r, paint } => {
                    let p = PathRef::new(common::build_path(path));
                    b.fill(id, &p, rule(*r), ctx.paint(paint));
                }
                Step::Stroke {
                    path,
                    width,
                    caps,
                    join,
                    mitre,
                    dash,
                    paint,
                } => {
                    let geometry = common::build_path(path);
                    let style = stroke_style(*width, *caps, *join, *mitre, dash);
                    b.stroke(id, &PathRef::new(geometry), style, ctx.paint(paint));
                }
                Step::Image {
                    handles,
                    perspective,
                    paint,
                } => {
                    b.image(id, ctx.image, mapping(handles, *perspective), ctx.paint(paint));
                }
                Step::PushGroup(m, hint) => {
                    let hint = match hint % 3 {
                        0 => CacheHint::Never,
                        1 => CacheHint::Auto,
                        _ => CacheHint::Always,
                    };
                    b.push_group(id, doc_transform(*m), hint);
                }
                Step::PopGroup => b.pop_group(),
                Step::PushClip(path, r) => {
                    b.push_clip(&PathRef::new(common::build_path(path)), rule(*r));
                }
                Step::PopClip => b.pop_clip(),
                Step::PushTransparency(t) => b.push_transparency(ctx.transparency(t)),
                Step::PopTransparency => b.pop_transparency(),
                Step::PushLayer(kind, t) => {
                    let kind = match kind % 3 {
                        0 => LayerKind::Plain,
                        1 => LayerKind::Isolated,
                        _ => LayerKind::DestinationReading,
                    };
                    b.push_layer(kind, ctx.transparency(t));
                }
                Step::PopLayer => b.pop_layer(),
                Step::PushFeather(size, bias, gain) => {
                    b.push_effect(xarast_render::LayerEffect::Feather {
                        // Up to 100 pt, which at the fuzzed scales spans
                        // the whole range up to the 100 px ceiling.
                        size: f64::from(size.abs() % 100.0) * 1000.0,
                        profile: Profile::new(f64::from(*bias), f64::from(*gain)),
                    });
                }
                Step::PopFeather => b.pop_effect(),
            }
        }
        b.finish()
    };
    if closed.is_err() {
        return;
    }

    let mut view = xarast_render::ViewParams::new(
        width,
        height,
        view_transform(input.view),
        quality,
    );
    view.viewport = DeviceRect::from_size(width, height);
    let dirty = match input.dirty {
        Some((x0, y0, x1, y1)) => DirtyRect::of(DeviceRect::new(
            i32::from(x0),
            i32::from(y0),
            i32::from(x1),
            i32::from(y1),
        )),
        None => DirtyRect::NONE,
    };
    let dl = DisplayList::build(&scene, &view, &dirty);
    assert_balanced(&dl);
    let b = dl.bounds();
    assert!(
        b.is_empty() || b.intersection(view.viewport) == b,
        "display list bounds {b:?} escape the viewport {:?}",
        view.viewport
    );

    let mut backend = CpuBackend::new(CpuConfig::deterministic());
    let mut first = Surface::new(width, height);
    backend
        .render(&dl, &res, &mut first)
        .expect("a 64 x 64 surface is always accepted");
    let mut second = Surface::new(width, height);
    backend
        .render(&dl, &res, &mut second)
        .expect("a 64 x 64 surface is always accepted");
    assert!(first.data() == second.data(), "rendering is a function of its input");
});
