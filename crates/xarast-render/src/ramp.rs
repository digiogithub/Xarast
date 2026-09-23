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

/// A stop offset fit for sorting: a NaN becomes 0, as [`Stop::new`] makes
/// it. The fields are public, so a stop can arrive without going through
/// the constructor, and sorting by `partial_cmp` with a NaN in the list is
/// not a total order — which the standard library's sort may detect and
/// panic on.
fn sane_offset(o: f32) -> f32 {
    if o.is_nan() { 0.0 } else { o }
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
    let mut sorted: Vec<Stop> = stops
        .iter()
        .map(|s| Stop {
            offset: sane_offset(s.offset),
            color: s.color,
        })
        .collect();
    sorted.sort_by(|a, b| a.offset.total_cmp(&b.offset));
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
    let mut sorted: Vec<TranspStop> = stops
        .iter()
        .map(|s| TranspStop {
            offset: sane_offset(s.offset),
            level: s.level,
        })
        .collect();
    sorted.sort_by(|a, b| a.offset.total_cmp(&b.offset));

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
///
/// # Eviction (least recently used)
///
/// A cache that only grows is fine for a document at rest, but a fill drag
/// makes a new ramp every frame (a stop moved, a profile slid). The owner of
/// the cache marks each scene build with [`RampCache::begin_frame`] and
/// calls [`RampCache::evict`] after it; eviction drops the least recently
/// used tables until the cache fits its budget, **never** one the frame
/// just built uses, so every [`RampId`] of the current scene stays valid.
/// An evicted id's slot is reused by a later intern: a snapshot taken
/// before the eviction (the render thread's) keeps its own copy.
#[derive(Debug, Clone, Default)]
pub struct RampCache {
    keys: HashMap<RampKey, RampId>,
    tables: Vec<Vec<Rgba8>>,
    /// The key of each live slot, `None` for a free one.
    slot_keys: Vec<Option<RampKey>>,
    /// The frame each slot was last interned in.
    last_used: Vec<u64>,
    /// Slots freed by eviction, reused first.
    free: Vec<u32>,
    /// The current frame.
    frame: u64,
    /// Bytes held by live tables.
    bytes: usize,
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
            self.last_used[id.0 as usize] = self.frame;
            return *id;
        }
        let table = build_ramp(stops, profile, space, len);
        self.bytes += table.len() * 4;
        let id = match self.free.pop() {
            Some(slot) => {
                let i = slot as usize;
                self.tables[i] = table;
                self.slot_keys[i] = Some(key.clone());
                self.last_used[i] = self.frame;
                RampId(slot)
            }
            None => {
                let id = RampId(u32::try_from(self.tables.len()).expect("ramp cache overflow"));
                self.tables.push(table);
                self.slot_keys.push(Some(key.clone()));
                self.last_used.push(self.frame);
                id
            }
        };
        self.keys.insert(key, id);
        id
    }

    /// Starts a new frame: what is interned from now on counts as used by
    /// it, and [`RampCache::evict`] will not drop it.
    pub fn begin_frame(&mut self) {
        self.frame += 1;
    }

    /// Drops least recently used tables not used in the current frame until
    /// the live tables hold at most `budget` bytes (or only current-frame
    /// tables remain). Returns the evicted ids, whose slots will be reused;
    /// anything indexed by ramp id alongside the cache must forget them.
    pub fn evict(&mut self, budget: usize) -> Vec<RampId> {
        if self.bytes <= budget {
            return Vec::new();
        }
        let mut old: Vec<(u64, u32)> = self
            .last_used
            .iter()
            .enumerate()
            .filter(|(i, t)| **t < self.frame && self.slot_keys[*i].is_some())
            .map(|(i, t)| (*t, u32::try_from(i).unwrap_or(u32::MAX)))
            .collect();
        old.sort_unstable();
        let mut out = Vec::new();
        for (_, slot) in old {
            if self.bytes <= budget {
                break;
            }
            let i = slot as usize;
            if let Some(key) = self.slot_keys[i].take() {
                self.keys.remove(&key);
            }
            self.bytes -= self.tables[i].len() * 4;
            self.tables[i] = Vec::new();
            self.free.push(slot);
            out.push(RampId(slot));
        }
        out
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

    /// The table behind an id, or `None` for a foreign or evicted id.
    #[must_use]
    pub fn try_get(&self, id: RampId) -> Option<&[Rgba8]> {
        self.tables
            .get(id.0 as usize)
            .filter(|t| !t.is_empty())
            .map(Vec::as_slice)
    }

    /// How many distinct ramps are interned.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tables.len() - self.free.len()
    }

    /// Whether nothing is interned.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Total bytes held by the interned tables.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(r: u8, g: u8, b: u8) -> Rgba8 {
        Rgba8 { r, g, b, a: 255 }
    }

    /// `fuzz_ramp`: NaN offsets among twenty-odd stops made the sort's
    /// comparator inconsistent, and the standard library's sort panicked
    /// on detecting it. Both ramp builders take stops with public fields.
    #[test]
    fn nan_offsets_do_not_break_the_sort() {
        let mut offsets = vec![f32::NAN, 0.002_868_652];
        offsets.extend(std::iter::repeat_n(0.002_856_924, 12));
        offsets.extend([f32::NAN, -2.363_192_1e-27, 0.0]);
        offsets.extend(std::iter::repeat_n(0.002_856_924, 5));
        let transp: Vec<TranspStop> = offsets
            .iter()
            .map(|&offset| TranspStop { offset, level: 59 })
            .collect();
        let colour: Vec<Stop> = offsets
            .iter()
            .map(|&offset| Stop {
                offset,
                color: rgb(1, 2, 3),
            })
            .collect();
        for len in [RampLength::Short, RampLength::Long] {
            assert_eq!(
                build_transparency_ramp(&transp, Profile::IDENTITY, len).len(),
                len.len()
            );
            assert_eq!(
                build_ramp(&colour, Profile::IDENTITY, EffectSpace::Rgb, len).len(),
                len.len()
            );
        }
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
    fn eviction_drops_the_least_recently_used_and_spares_the_current_frame() {
        let mut c = RampCache::new();
        let ramp = |c: &mut RampCache, k: u8| {
            c.intern(
                &[Stop::new(0.0, rgb(k, 0, 0)), Stop::new(1.0, rgb(0, k, 0))],
                Profile::IDENTITY,
                EffectSpace::Rgb,
                RampLength::Short,
            )
        };
        c.begin_frame();
        let a = ramp(&mut c, 1);
        c.begin_frame();
        let b = ramp(&mut c, 2);
        c.begin_frame();
        let _ = ramp(&mut c, 1); // a is used again: b is now the oldest
        let d = ramp(&mut c, 3);
        assert_eq!(c.len(), 3);
        // Room for two tables: b goes, a and d (this frame) stay.
        let gone = c.evict(2 * 256 * 4);
        assert_eq!(gone, vec![b]);
        assert_eq!(c.bytes(), 2 * 256 * 4);
        assert!(c.try_get(b).is_none());
        assert_eq!(c.get(a)[0], rgb(1, 0, 0));
        assert_eq!(c.get(d)[0], rgb(3, 0, 0));
        // A budget of nothing cannot evict this frame's tables.
        assert!(c.evict(0).is_empty());
        // The freed slot is reused, and the old key is really forgotten.
        c.begin_frame();
        let e = ramp(&mut c, 4);
        assert_eq!(e, b, "the slot is reused");
        assert_eq!(c.get(e)[0], rgb(4, 0, 0));
        let b2 = ramp(&mut c, 2);
        assert_ne!(b2, e);
        assert_eq!(c.get(b2)[0], rgb(2, 0, 0));
    }

    #[test]
    fn a_cache_that_never_starts_a_frame_never_evicts_what_it_holds() {
        let mut c = RampCache::new();
        let a = c.intern(
            &[Stop::new(0.0, rgb(9, 9, 9))],
            Profile::IDENTITY,
            EffectSpace::Rgb,
            RampLength::Long,
        );
        assert!(c.evict(0).is_empty());
        assert_eq!(c.get(a).len(), 2048);
    }

    #[test]
    fn the_identity_profile_is_exactly_linear_over_2048_entries() {
        let t = build_ramp(
            &[
                Stop::new(0.0, rgb(0, 0, 0)),
                Stop::new(1.0, rgb(255, 255, 255)),
            ],
            Profile::IDENTITY,
            EffectSpace::Rgb,
            RampLength::Long,
        );
        for (i, e) in t.iter().enumerate() {
            let want = (i as f64 / 2047.0 * 255.0).round() as u8;
            assert_eq!(e.r, want, "entry {i}");
        }
        assert_eq!(t[0].r, 0);
        assert_eq!(t[2047].r, 255);
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
