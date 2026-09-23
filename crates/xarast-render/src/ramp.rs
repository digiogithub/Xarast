//! Gradient ramps: the bias/gain profile, multi-stop tables and their cache.
//!
//! The **application** builds the ramp and hands the rasteriser a flat
//! array, exactly as the original does (`gradtbl.cpp`); nothing about
//! profiles, HSV paths or spot-colour fallbacks belongs in a shader.
//!
//! Tables are 256 entries in [`RenderQuality::Draft`] and 2048 in
//! [`RenderQuality::Final`] (`LargeGradTables`): the long ones exist to kill
//! banding across a large gradient.
//!
//! [`RenderQuality::Draft`]: crate::RenderQuality::Draft
//! [`RenderQuality::Final`]: crate::RenderQuality::Final

use std::collections::HashMap;

use xarast_color::{ColourValue, FillEffect, Rgba8};

/// The Schlick bias/gain profile Xara attaches to gradients, blends,
/// contours and shadows.
///
/// This is `xarast-geom`'s [`BiasGain`](xarast_geom::BiasGain) under another
/// name, not a second implementation: the profile is a document-model
/// concept that the renderer consumes. The two differ only in the epsilon
/// that keeps Schlick's parameter off its poles — `xarast-geom` uses 1e-6
/// where `research/03 §2.6.3` records 1e-5 — which moves the curve by at
/// most a few times 1e-5, a hundredth of an 8-bit step. `profile::
/// agrees_with_the_research_formula` measures it.
pub type Profile = xarast_geom::BiasGain;

/// One stop of a colour ramp.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stop {
    /// Position along the ramp, `0.0..=1.0`.
    pub offset: f32,
    /// The colour at that position.
    pub color: Rgba8,
}

impl Stop {
    /// Builds a stop, clamping the offset into range.
    #[must_use]
    pub fn new(offset: f32, color: Rgba8) -> Stop {
        Stop {
            offset: if offset.is_nan() {
                0.0
            } else {
                offset.clamp(0.0, 1.0)
            },
            color,
        }
    }
}

/// How a ramp interpolates between its stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EffectSpace {
    /// Componentwise in RGB. Forced when spot inks are involved, as
    /// `gradtbl.cpp:453` does.
    #[default]
    Rgb,
    /// In HSV, the short way round the hue circle.
    HsvShort,
    /// In HSV, the long way round.
    HsvLong,
}

impl EffectSpace {
    fn to_effect(self) -> FillEffect {
        match self {
            EffectSpace::Rgb => FillEffect::Fade,
            EffectSpace::HsvShort => FillEffect::Rainbow,
            EffectSpace::HsvLong => FillEffect::AltRainbow,
        }
    }
}

/// How many entries a ramp table has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RampLength {
    /// 256 entries: the draft-quality table.
    #[default]
    Short,
    /// 2048 entries: the final-quality table, which is what stops a large
    /// gradient from banding.
    Long,
}

impl RampLength {
    /// The entry count.
    #[must_use]
    pub const fn len(self) -> usize {
        match self {
            RampLength::Short => 256,
            RampLength::Long => 2048,
        }
    }

    /// Always false; present so that `len` does not trip `clippy::len_without_is_empty`.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        false
    }
}

/// Samples a stop list at `f`, which is already profiled.
fn sample_stops(stops: &[Stop], f: f32, space: EffectSpace) -> Rgba8 {
    debug_assert!(!stops.is_empty());
    let f = if f.is_nan() { 0.0 } else { f.clamp(0.0, 1.0) };
    if stops.len() == 1 {
        return stops[0].color;
    }
    if f <= stops[0].offset {
        return stops[0].color;
    }
    let last = stops[stops.len() - 1];
    if f >= last.offset {
        return last.color;
    }
    let mut i = 0;
    while i + 2 < stops.len() && stops[i + 1].offset < f {
        i += 1;
    }
    let (a, b) = (stops[i], stops[i + 1]);
    let span = b.offset - a.offset;
    let t = if span <= f32::EPSILON {
        0.0
    } else {
        (f - a.offset) / span
    };
    if space == EffectSpace::Rgb {
        // The overwhelmingly common case, and the one a 2048-entry table
        // is built 2048 times for: interpolate the bytes directly instead
        // of going through two colour-model conversions per entry. The
        // result is identical to the general path for RGB stops, which
        // `the_fast_rgb_path_matches_the_general_one` checks.
        let lerp = |x: u8, y: u8| -> u8 {
            let v = f32::from(x) + (f32::from(y) - f32::from(x)) * t;
            v.round().clamp(0.0, 255.0) as u8
        };
        return Rgba8 {
            r: lerp(a.color.r, b.color.r),
            g: lerp(a.color.g, b.color.g),
            b: lerp(a.color.b, b.color.b),
            a: lerp(a.color.a, b.color.a),
        };
    }
    let mixed = xarast_color::interpolate(
        ColourValue::from_rgba8(a.color),
        ColourValue::from_rgba8(b.color),
        t,
        space.to_effect(),
    );
    mixed.to_rgba8()
}

/// Builds a ramp table.
///
/// The profile is applied to the ramp *parameter*, not to the colours: entry
/// `i` is the stop list sampled at `profile(i / (len - 1))`, which is what
/// `gradtbl.cpp:1206` does.
///
/// An empty stop list yields an all-transparent table rather than a panic,
/// because the stop list can come from a corrupt file.
#[must_use]
pub fn build_ramp(
    stops: &[Stop],
    profile: Profile,
    space: EffectSpace,
    len: RampLength,
) -> Vec<Rgba8> {
    let n = len.len();
    if stops.is_empty() {
        return vec![Rgba8::TRANSPARENT; n];
    }
    let mut sorted: Vec<Stop> = stops.to_vec();
    sorted.sort_by(|a, b| {
        a.offset
            .partial_cmp(&b.offset)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let d = (n - 1) as f64;
    let identity = profile.map(0.25) == 0.25 && profile.map(0.75) == 0.75;
    (0..n)
        .map(|i| {
            let x = i as f64 / d;
            // The identity short-circuit of `biasgain.cpp:341`.
            let f = if identity { x } else { profile.map(x) };
            // f32-ok: a ramp parameter in 0..=1, never a coordinate.
            sample_stops(&sorted, f as f32, space)
        })
        .collect()
}

/// One stop of a transparency ramp. Xara's convention: 0 is opaque, 255 is
/// fully transparent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TranspStop {
    /// Position along the ramp, `0.0..=1.0`.
    pub offset: f32,
    /// 0 opaque, 255 fully transparent.
    pub level: u8,
}

/// Builds a 256- or 2048-entry transparency ramp using the original's
/// fixed-point interpolation with 22 fractional bits (`gradtbl.cpp:1562`).
///
/// The `+ 2^21` in the seed is a round-to-nearest, not a fudge factor.
#[must_use]
pub fn build_transparency_ramp(stops: &[TranspStop], profile: Profile, len: RampLength) -> Vec<u8> {
    let n = len.len();
    if stops.is_empty() {
        return vec![0; n];
    }
    let mut sorted: Vec<TranspStop> = stops.to_vec();
    sorted.sort_by(|a, b| {
        a.offset
            .partial_cmp(&b.offset)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // The profile is applied to the parameter, so the fixed-point stretch
    // walk happens over the profiled index.
    let d = (n - 1) as f64;
    let identity = profile.map(0.25) == 0.25 && profile.map(0.75) == 0.75;
    let mut out = vec![0u8; n];
    for (i, slot) in out.iter_mut().enumerate() {
        let x = i as f64 / d;
        // f32-ok: a ramp parameter in 0..=1, never a coordinate.
        let f = if identity { x } else { profile.map(x) } as f32;
        *slot = sample_transparency(&sorted, f);
    }
    out
}

/// The fixed-point stretch interpolation of `gradtbl.cpp:1562`, evaluated at
/// one point rather than swept, so that it composes with the profile.
fn sample_transparency(stops: &[TranspStop], f: f32) -> u8 {
    let f = if f.is_nan() { 0.0 } else { f.clamp(0.0, 1.0) };
    if stops.len() == 1 || f <= stops[0].offset {
        return stops[0].level;
    }
    let last = stops[stops.len() - 1];
    if f >= last.offset {
        return last.level;
    }
    let mut i = 0;
    while i + 2 < stops.len() && stops[i + 1].offset < f {
        i += 1;
    }
    let (a, b) = (stops[i], stops[i + 1]);
    let span = f64::from(b.offset - a.offset);
    if span <= f64::EPSILON {
        return a.level;
    }
    let t = (f64::from(f) - f64::from(a.offset)) / span;
    // (start << 22) + 2^21, stepped by ((end - start) << 22) / steps.
    let start = i64::from(a.level) << 22;
    let delta = (i64::from(b.level) - i64::from(a.level)) << 22;
    let acc = start + (1 << 21) + (delta as f64 * t) as i64;
    u8::try_from((acc >> 22).clamp(0, 255)).unwrap_or(255)
}

/// An interned ramp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RampId(u32);

impl RampId {
    /// The index into the cache's table, for debugging and for the GPU's
    /// ramp atlas.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}

/// A key that identifies a ramp exactly: stops, profile, space and length.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RampKey {
    stops: Vec<(u32, [u8; 4])>,
    profile: (u64, u64),
    space: EffectSpace,
    len: RampLength,
}

/// Interns built ramps so that a gradient drawn a thousand times builds its
/// table once.
#[derive(Debug, Clone, Default)]
pub struct RampCache {
    keys: HashMap<RampKey, RampId>,
    tables: Vec<Vec<Rgba8>>,
}

impl RampCache {
    /// An empty cache.
    #[must_use]
    pub fn new() -> RampCache {
        RampCache::default()
    }

    /// Returns the id of the ramp with these parameters, building it if this
    /// is the first time it has been asked for.
    pub fn intern(
        &mut self,
        stops: &[Stop],
        profile: Profile,
        space: EffectSpace,
        len: RampLength,
    ) -> RampId {
        let key = RampKey {
            stops: stops
                .iter()
                .map(|s| {
                    (
                        s.offset.to_bits(),
                        [s.color.r, s.color.g, s.color.b, s.color.a],
                    )
                })
                .collect(),
            profile: (profile.bias.to_bits(), profile.gain.to_bits()),
            space,
            len,
        };
        if let Some(id) = self.keys.get(&key) {
            return *id;
        }
        let table = build_ramp(stops, profile, space, len);
        let id = RampId(u32::try_from(self.tables.len()).expect("ramp cache overflow"));
        self.tables.push(table);
        self.keys.insert(key, id);
        id
    }

    /// The table behind an id.
    ///
    /// # Panics
    ///
    /// Panics if the id came from a different cache.
    #[must_use]
    pub fn get(&self, id: RampId) -> &[Rgba8] {
        &self.tables[id.0 as usize]
    }

    /// The table behind an id, or `None` for a foreign id.
    #[must_use]
    pub fn try_get(&self, id: RampId) -> Option<&[Rgba8]> {
        self.tables.get(id.0 as usize).map(Vec::as_slice)
    }

    /// How many distinct ramps are interned.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tables.len()
    }

    /// Whether nothing is interned.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }

    /// Total bytes held by the interned tables.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.tables.iter().map(|t| t.len() * 4).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(r: u8, g: u8, b: u8) -> Rgba8 {
        Rgba8 { r, g, b, a: 255 }
    }

    #[test]
    fn a_two_stop_linear_ramp_ends_at_its_stops() {
        let stops = [
            Stop::new(0.0, rgb(0, 0, 0)),
            Stop::new(1.0, rgb(255, 255, 255)),
        ];
        let t = build_ramp(
            &stops,
            Profile::IDENTITY,
            EffectSpace::Rgb,
            RampLength::Short,
        );
        assert_eq!(t.len(), 256);
        assert_eq!(t[0], rgb(0, 0, 0));
        assert_eq!(t[255], rgb(255, 255, 255));
        assert!(t[128].r > 120 && t[128].r < 135);
    }

    #[test]
    fn a_long_ramp_is_monotone_where_the_stops_are() {
        let stops = [Stop::new(0.0, rgb(0, 0, 0)), Stop::new(1.0, rgb(255, 0, 0))];
        let t = build_ramp(
            &stops,
            Profile::IDENTITY,
            EffectSpace::Rgb,
            RampLength::Long,
        );
        assert_eq!(t.len(), 2048);
        for w in t.windows(2) {
            assert!(w[1].r >= w[0].r);
        }
    }

    #[test]
    fn multi_stop_ramps_hit_every_stop() {
        let stops = [
            Stop::new(0.0, rgb(255, 0, 0)),
            Stop::new(0.5, rgb(0, 255, 0)),
            Stop::new(1.0, rgb(0, 0, 255)),
        ];
        let t = build_ramp(
            &stops,
            Profile::IDENTITY,
            EffectSpace::Rgb,
            RampLength::Short,
        );
        assert_eq!(t[0], rgb(255, 0, 0));
        assert_eq!(t[255], rgb(0, 0, 255));
        let mid = t[127];
        assert!(mid.g > 245, "midpoint should be green, got {mid:?}");
    }

    #[test]
    fn the_profile_reshapes_the_ramp_without_moving_its_ends() {
        let stops = [
            Stop::new(0.0, rgb(0, 0, 0)),
            Stop::new(1.0, rgb(255, 255, 255)),
        ];
        let flat = build_ramp(
            &stops,
            Profile::IDENTITY,
            EffectSpace::Rgb,
            RampLength::Short,
        );
        let biased = build_ramp(
            &stops,
            Profile::new(0.6, 0.0),
            EffectSpace::Rgb,
            RampLength::Short,
        );
        assert_eq!(flat[0], biased[0]);
        assert_eq!(flat[255], biased[255]);
        assert!(
            biased[128].r > flat[128].r,
            "a positive bias lifts the middle"
        );
    }

    #[test]
    fn hsv_short_and_long_take_opposite_ways_round() {
        let stops = [
            Stop::new(0.0, rgb(255, 0, 0)),
            Stop::new(1.0, rgb(0, 255, 0)),
        ];
        let short = build_ramp(
            &stops,
            Profile::IDENTITY,
            EffectSpace::HsvShort,
            RampLength::Short,
        );
        let long = build_ramp(
            &stops,
            Profile::IDENTITY,
            EffectSpace::HsvLong,
            RampLength::Short,
        );
        // Halfway, the short way is yellow and the long way is cyan-ish.
        assert!(short[128].r > 128 && short[128].g > 128 && short[128].b < 64);
        assert!(long[128].b > 128);
    }

    #[test]
    fn the_fixed_point_transparency_path_matches_the_float_one() {
        let stops = [
            TranspStop {
                offset: 0.0,
                level: 0,
            },
            TranspStop {
                offset: 1.0,
                level: 255,
            },
        ];
        let table = build_transparency_ramp(&stops, Profile::IDENTITY, RampLength::Long);
        assert_eq!(table.len(), 2048);
        for (i, v) in table.iter().enumerate() {
            let exact = (i as f64 / 2047.0 * 255.0).round() as i32;
            assert!(
                (i32::from(*v) - exact).abs() <= 1,
                "entry {i}: fixed point {v} vs float {exact}"
            );
        }
    }

    #[test]
    fn the_fast_rgb_path_matches_the_general_one() {
        // The fast path exists for speed, not for a different answer.
        for (a, b) in [
            (rgb(0, 0, 0), rgb(255, 255, 255)),
            (rgb(13, 200, 7), rgb(240, 3, 199)),
            (rgb(128, 128, 128), rgb(129, 127, 130)),
        ] {
            for i in 0..=64 {
                // f32-ok: a ramp parameter in 0..=1, never a coordinate.
                let f = i as f32 / 64.0;
                let fast =
                    sample_stops(&[Stop::new(0.0, a), Stop::new(1.0, b)], f, EffectSpace::Rgb);
                let general = xarast_color::interpolate(
                    ColourValue::from_rgba8(a),
                    ColourValue::from_rgba8(b),
                    f,
                    FillEffect::Fade,
                )
                .to_rgba8();
                let d = i32::from(fast.r) - i32::from(general.r);
                assert!(d.abs() <= 1, "fast {fast:?} vs general {general:?} at {f}");
            }
        }
    }

    #[test]
    fn the_cache_interns_and_reuses() {
        let mut c = RampCache::new();
        let stops = [Stop::new(0.0, rgb(1, 2, 3)), Stop::new(1.0, rgb(4, 5, 6))];
        let a = c.intern(
            &stops,
            Profile::IDENTITY,
            EffectSpace::Rgb,
            RampLength::Short,
        );
        let b = c.intern(
            &stops,
            Profile::IDENTITY,
            EffectSpace::Rgb,
            RampLength::Short,
        );
        let d = c.intern(
            &stops,
            Profile::IDENTITY,
            EffectSpace::Rgb,
            RampLength::Long,
        );
        assert_eq!(a, b);
        assert_ne!(a, d);
        assert_eq!(c.len(), 2);
        assert_eq!(c.get(a).len(), 256);
        assert_eq!(c.bytes(), (256 + 2048) * 4);
    }

    #[test]
    fn an_empty_stop_list_does_not_panic() {
        let t = build_ramp(&[], Profile::IDENTITY, EffectSpace::Rgb, RampLength::Short);
        assert_eq!(t.len(), 256);
        assert_eq!(
            build_transparency_ramp(&[], Profile::IDENTITY, RampLength::Short).len(),
            256
        );
    }
}
