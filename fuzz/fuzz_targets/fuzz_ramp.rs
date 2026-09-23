//! Gradient ramps and gradient evaluation, over arbitrary stops, profiles
//! and control points.
//!
//! Every value here comes from a file in the end — stop offsets, bias and
//! gain, the three or four gradient handles — so each has to survive NaN,
//! infinities, duplicates, reversed order and collinear handles. The
//! properties: no panic, tables of exactly the requested length, interning
//! is idempotent, and evaluation at any point returns a colour.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use xarast_color::Rgba8;
use xarast_render::paint::{apply_repeat, eval_paint, grad_param};
use xarast_render::{
    EffectSpace, GradMapping, GradRamp, GradShape, ImageRegistry, Paint, Point64, Profile,
    RampCache, RampLength, Repeat, Stop, TranspStop, build_ramp, build_transparency_ramp,
};

#[derive(Arbitrary, Debug)]
struct Input {
    stops: Vec<(f32, [u8; 4])>,
    transp: Vec<(f32, u8)>,
    bias: f64,
    gain: f64,
    /// Set the profile's fields directly rather than through the
    /// clamping constructor, as a caller holding raw file values might.
    raw_profile: bool,
    space: u8,
    long: bool,
    shape: u8,
    repeat: u8,
    perspective: bool,
    handles: [(f64, f64); 4],
    corners: [[u8; 4]; 4],
    probes: Vec<(f64, f64)>,
}

const MAX_STOPS: usize = 64;
const MAX_PROBES: usize = 32;

fn space(v: u8) -> EffectSpace {
    match v % 3 {
        0 => EffectSpace::Rgb,
        1 => EffectSpace::HsvShort,
        _ => EffectSpace::HsvLong,
    }
}

fn shape(v: u8) -> GradShape {
    xarast_render::ALL_SHAPES[usize::from(v) % xarast_render::ALL_SHAPES.len()]
}

fn repeat(v: u8) -> Repeat {
    xarast_render::ALL_REPEATS[usize::from(v) % xarast_render::ALL_REPEATS.len()]
}

fn rgba(c: [u8; 4]) -> Rgba8 {
    Rgba8 {
        r: c[0],
        g: c[1],
        b: c[2],
        a: c[3],
    }
}

fuzz_target!(|input: Input| {
    let stops: Vec<Stop> = input
        .stops
        .iter()
        .take(MAX_STOPS)
        .map(|&(o, c)| Stop::new(o, rgba(c)))
        .collect();
    let transp: Vec<TranspStop> = input
        .transp
        .iter()
        .take(MAX_STOPS)
        .map(|&(offset, level)| TranspStop { offset, level })
        .collect();
    let profile = if input.raw_profile {
        Profile {
            bias: input.bias,
            gain: input.gain,
        }
    } else {
        Profile::new(input.bias, input.gain)
    };
    if !input.raw_profile {
        for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let m = profile.map(t);
            assert!((0.0..=1.0).contains(&m), "profile maps {t} to {m}");
        }
    }
    let space = space(input.space);
    let len = if input.long {
        RampLength::Long
    } else {
        RampLength::Short
    };

    let table = build_ramp(&stops, profile, space, len);
    assert_eq!(table.len(), len.len());
    let t = build_transparency_ramp(&transp, profile, len);
    assert_eq!(t.len(), len.len());

    let mut cache = RampCache::new();
    let id = cache.intern(&stops, profile, space, len);
    assert_eq!(id, cache.intern(&stops, profile, space, len), "interning is idempotent");
    assert_eq!(cache.get(id), table.as_slice(), "the cache holds what build_ramp built");

    // Gradient evaluation at arbitrary device points.
    let h = input.handles.map(|(x, y)| Point64::new(x, y));
    let mapping = if input.perspective {
        GradMapping::Perspective {
            a: h[0],
            b: h[1],
            c: h[2],
            d: h[3],
        }
    } else {
        GradMapping::Affine {
            a: h[0],
            b: h[1],
            c: h[2],
        }
    };
    let shape = shape(input.shape);
    let repeat = repeat(input.repeat);
    let ramp = match shape {
        GradShape::Mesh3 => GradRamp::Mesh3([
            rgba(input.corners[0]),
            rgba(input.corners[1]),
            rgba(input.corners[2]),
        ]),
        GradShape::Mesh4 => GradRamp::Mesh4(input.corners.map(rgba)),
        _ => GradRamp::Table(id),
    };
    let paint = Paint::Gradient {
        shape,
        mapping,
        repeat,
        ramp,
    };
    let _ = paint.validate();
    let images = ImageRegistry::new();
    for &(x, y) in input.probes.iter().take(MAX_PROBES) {
        let p = Point64::new(x, y);
        if let Some(s) = grad_param(shape, mapping, p) {
            let r = apply_repeat(s, repeat);
            assert!((0.0..=1.0).contains(&r), "apply_repeat({s}) = {r}");
        }
        let _ = eval_paint(&paint, &cache, &images, p);
    }
});
