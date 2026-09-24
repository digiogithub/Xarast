//! The live-effect layer cache (phase 13 B9, XARA-T-0314).
//!
//! The CPU backend renders every live effect offscreen before a frame's
//! bands (`crate::effect`): what it wraps, twice when the effect needs a
//! silhouette, then the effect's own filter. Without a cache that happens
//! on every frame and every repaint that reaches the effect, whether or
//! not anything under it changed. This cache keeps the offscreen results
//! and hands them back, **byte for byte what a recomputation gives**.
//!
//! # What a result depends on, and so what the key holds
//!
//! A kept pixel of an effect is a function of (`docs/memory/render.md`,
//! invariant 23):
//!
//! * **the wrapped content**: the scene ops from the effect's push to its
//!   pop, the effect's own parameters among them, compared by op equality,
//!   plus every ramp and image they name, compared by content with the
//!   rule `damage::scene_damage` uses (`damage::ramp_tables`,
//!   `damage::same_image`) — an interned ramp slot can be reused, so an
//!   equal id is not enough (render.md invariant 16);
//! * **the device transform** the effect sits under (its ancestors'
//!   groups and the view), bit for bit, and the rest of the view: the
//!   viewport (the coverage origin is fixed by it), quality and dpi
//!   (flatness, hairlines);
//! * **the pass**: the coverage origin's left edge, the band grid and the
//!   pinned SIMD level and luminance weights of the configuration.
//!
//! It does **not** depend on the draw area: that is exactly what invariant
//! 23 guarantees. So a result computed for one rectangle is valid, pixel for
//! pixel, over the part of it that was kept (`valid`), for any later draw
//! area. One key holds several results (layers), each with its own valid
//! rectangle; a lookup assembles the draw area's pixels from as many of
//! them as cover it and renders only what none covers, as one more layer.
//!
//! Nothing here consults the walker's `ContentHash`, and nothing needs
//! explicit invalidation: a change under an effect changes the key (the
//! ops, a resource or a transform differ), which is exactly the case in
//! which `scene_damage` reports the effect's pixels as damage; a change
//! elsewhere leaves the key and the cached pixels alone.
//!
//! # Pans are not served from here
//!
//! The key holds the exact transform, so a translated view misses. That is
//! deliberate: the coverage origin of an effect's region is fixed by the
//! view (`left = min(parent, viewport.x0) − reach`) and its rows by the
//! frame's band grid, not by the content, so the same content rendered
//! after a pan is rasterised from a different origin relative to it, and
//! the `f64 → f32` geometry can round differently. A translated cached
//! layer is therefore not guaranteed to be the recomputed one. And there is
//! nothing to gain: a whole-pixel pan already moves the pixels on screen
//! (`scroll_surface`), and the strips it exposes lay outside the previous
//! view, where no kept pixel was ever computed.
//!
//! # Memory
//!
//! The cache has its own byte ceiling ([`DEFAULT_EFFECT_CACHE_BYTES`],
//! [`EffectCache::set_limit`]), separate from the image pixel budget
//! (`crate::pixel_budget`), which covers decoded image levels. It counts
//! each layer's pixels and each key's copy of its ops and resource tables
//! (not the path geometry the ops share with the scene by `Arc`). Least
//! recently used layers go first, except that a layer used in the current
//! frame is never evicted to make room: a frame whose effects do not fit
//! stores what fits and recomputes the rest, rather than evicting its own
//! results in a cycle and hitting nothing.

use std::sync::Arc;

use xarast_color::Rgba8;

use crate::backend::cpu::{CpuConfig, Resolver};
use crate::damage::{Ref, key, ramp_tables, refs, same_image};
use crate::display_list::DisplayList;
use crate::paint::{ImageId, ImageRef};
use crate::precision::Transform2D;
use crate::ramp::RampId;
use crate::scene::{RenderQuality, SceneOp};
use crate::surface::DeviceRect;

/// The default ceiling of an [`EffectCache`]: 128 MiB, about 32 million
/// premultiplied pixels, fifteen 1080p frames' worth of effect layers.
pub const DEFAULT_EFFECT_CACHE_BYTES: usize = 128 << 20;

/// A key keeps at most this many layers (valid rectangles); the least
/// recently used goes when another arrives. Four frame columns and a few
/// repaints fit.
const MAX_LAYERS_PER_KEY: usize = 8;

/// What the cache has done since it was made.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EffectCacheStats {
    /// Effects drawn entirely from cached layers.
    pub hits: u64,
    /// Effects drawn partly from cached layers, the rest rendered.
    pub partial: u64,
    /// Effects rendered with nothing reusable.
    pub misses: u64,
    /// Layers stored.
    pub stored: u64,
    /// Layers evicted to make room or to respect the per-key limit.
    pub evicted: u64,
    /// Results not stored: bigger than the ceiling, no room without
    /// evicting the frame's own layers, or drawn from a substituted image
    /// (`MissingLevels::Substitute`), which is not the exact picture.
    pub refused: u64,
    /// Keys held now.
    pub keys: usize,
    /// Layers held now.
    pub layers: usize,
    /// Bytes held now.
    pub bytes: usize,
}

/// Part of an effect's result to composite: the pixels of a layer that
/// covers `rect` (premultiplied RGBA8, `rect.width()` to a row), drawn
/// inside `clip` only.
#[derive(Debug, Clone)]
pub(crate) struct Piece {
    pub(crate) clip: DeviceRect,
    pub(crate) rect: DeviceRect,
    pub(crate) pixels: Arc<Vec<u8>>,
}

/// Everything of a key but the ops, compared bit for bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Setting {
    /// The effect's document-to-device transform.
    xf: [u64; 6],
    /// The view's transform, viewport, quality and dpi.
    view_xf: [u64; 6],
    viewport: DeviceRect,
    quality: RenderQuality,
    dpi: u64,
    /// The pass: coverage origin, band grid, SIMD pin, luminance weights.
    left: i32,
    rows_per_band: usize,
    pin_simd: bool,
    weights: [u32; 3],
}

fn bits(t: Transform2D) -> [u64; 6] {
    t.to_affine().as_coeffs().map(f64::to_bits)
}

fn mix(h: u64, v: u64) -> u64 {
    let mut z = h.rotate_left(5) ^ v.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// One effect as a frame asks for it: the key, borrowed.
pub(crate) struct Probe<'a> {
    hash: u64,
    setting: Setting,
    /// The effect's push through its pop.
    ops: &'a [SceneOp],
    res: &'a Resolver,
}

impl<'a> Probe<'a> {
    /// The key of the effect whose push is scene op `op` of `dl`, drawn
    /// under `xf` from coverage origin `left` on a grid of
    /// `rows_per_band`. `None` when the push has no matching pop (the
    /// result is then not cached).
    pub(crate) fn new(
        dl: &'a DisplayList,
        op: u32,
        xf: Transform2D,
        left: i32,
        rows_per_band: usize,
        cfg: &CpuConfig,
        res: &'a Resolver,
    ) -> Option<Probe<'a>> {
        let all = dl.scene_ops();
        let start = op as usize;
        if !matches!(all.get(start), Some(SceneOp::PushEffect(_))) {
            return None;
        }
        let mut depth = 0usize;
        let mut end = None;
        for (i, o) in all.iter().enumerate().skip(start) {
            match o {
                SceneOp::PushEffect(_) => depth += 1,
                SceneOp::PopEffect => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let ops = &all[start..=end?];
        let view = dl.view();
        let setting = Setting {
            xf: bits(xf),
            view_xf: bits(view.transform),
            viewport: view.viewport,
            quality: view.quality,
            dpi: view.dpi.to_bits(),
            left,
            rows_per_band,
            pin_simd: cfg.pin_simd,
            weights: [
                cfg.weights.r.to_bits(),
                cfg.weights.g.to_bits(),
                cfg.weights.b.to_bits(),
            ],
        };
        let mut h = setting.xf.iter().fold(0x5eed, |h, v| mix(h, *v));
        h = mix(h, ops.len() as u64);
        for o in ops {
            h = mix(h, key(o));
        }
        Some(Probe {
            hash: h,
            setting,
            ops,
            res,
        })
    }
}

/// A ramp id and its colour and transparency tables.
type RampTables = (RampId, Option<Vec<Rgba8>>, Option<Vec<u8>>);

/// The resources a key's ops name, as they resolved when it was stored.
#[derive(Debug, Default)]
struct Snapshot {
    ramps: Vec<RampTables>,
    images: Vec<(ImageId, Option<ImageRef>)>,
}

impl Snapshot {
    fn capture(ops: &[SceneOp], res: &Resolver) -> Snapshot {
        let mut s = Snapshot::default();
        for op in ops {
            refs(op, &mut |r| match r {
                Ref::Ramp(id) => {
                    if !s.ramps.iter().any(|(i, ..)| *i == id) {
                        let (c, t) = ramp_tables(res, id);
                        s.ramps
                            .push((id, c.map(<[Rgba8]>::to_vec), t.map(<[u8]>::to_vec)));
                    }
                }
                Ref::Image(id) => {
                    if !s.images.iter().any(|(i, _)| *i == id) {
                        s.images.push((id, res.images.get(id).cloned()));
                    }
                }
            });
        }
        s
    }

    /// Whether `res` resolves every id to the same data (the rule of
    /// `scene_damage`).
    fn matches(&self, res: &Resolver) -> bool {
        self.ramps
            .iter()
            .all(|(id, c, t)| ramp_tables(res, *id) == (c.as_deref(), t.as_deref()))
            && self
                .images
                .iter()
                .all(|(id, img)| same_image(res.images.get(*id), img.as_ref()))
    }

    fn bytes(&self) -> usize {
        let ramps: usize = self
            .ramps
            .iter()
            .map(|(_, c, t)| {
                c.as_ref().map_or(0, |c| c.len() * size_of::<Rgba8>())
                    + t.as_ref().map_or(0, Vec::len)
            })
            .sum();
        ramps + self.images.len() * size_of::<(ImageId, Option<ImageRef>)>()
    }
}

/// One cached result: its pixels over `rect`, exact over `valid`.
#[derive(Debug)]
struct Layer {
    valid: DeviceRect,
    rect: DeviceRect,
    pixels: Arc<Vec<u8>>,
    /// The cache clock when it was last used.
    used: u64,
    /// The frame it was last used in.
    frame: u64,
}

impl Layer {
    fn bytes(&self) -> usize {
        self.pixels.len() + size_of::<Layer>()
    }
}

/// One key and its layers.
#[derive(Debug)]
struct Entry {
    hash: u64,
    setting: Setting,
    ops: Vec<SceneOp>,
    snapshot: Snapshot,
    /// Bytes of the key itself: the ops and the resource tables.
    fixed: usize,
    layers: Vec<Layer>,
}

impl Entry {
    fn matches(&self, p: &Probe<'_>) -> bool {
        self.hash == p.hash
            && self.setting == p.setting
            && self.ops.as_slice() == p.ops
            && self.snapshot.matches(p.res)
    }
}

/// What a lookup found for a draw area: pieces to composite and the
/// rectangles no cached layer covers.
#[derive(Debug, Default)]
pub(crate) struct Cover {
    pub(crate) pieces: Vec<Piece>,
    pub(crate) missing: Vec<DeviceRect>,
}

/// The cache of offscreen effect results. One per [`crate::CpuBackend`];
/// see the module documentation.
#[derive(Debug)]
pub struct EffectCache {
    entries: Vec<Entry>,
    limit: usize,
    bytes: usize,
    clock: u64,
    frame: u64,
    stats: EffectCacheStats,
}

impl Default for EffectCache {
    fn default() -> EffectCache {
        EffectCache::new(DEFAULT_EFFECT_CACHE_BYTES)
    }
}

fn contains(outer: DeviceRect, inner: DeviceRect) -> bool {
    outer.x0 <= inner.x0 && inner.x1 <= outer.x1 && outer.y0 <= inner.y0 && inner.y1 <= outer.y1
}

/// The sorted, distinct cut positions of `lo..hi` by the given edges.
fn cuts(lo: i32, hi: i32, edges: impl Iterator<Item = i32>) -> Vec<i32> {
    let mut v: Vec<i32> = [lo, hi]
        .into_iter()
        .chain(edges.map(|e| e.clamp(lo, hi)))
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

impl EffectCache {
    /// An empty cache holding at most `limit` bytes. Zero disables it:
    /// nothing is ever stored.
    #[must_use]
    pub fn new(limit: usize) -> EffectCache {
        EffectCache {
            entries: Vec::new(),
            limit,
            bytes: 0,
            clock: 0,
            frame: 0,
            stats: EffectCacheStats::default(),
        }
    }

    /// The ceiling in bytes.
    #[must_use]
    pub const fn limit(&self) -> usize {
        self.limit
    }

    /// Changes the ceiling, evicting least recently used layers down to it.
    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit;
        while self.bytes > self.limit {
            if !self.evict_one(true) {
                break;
            }
        }
    }

    /// Drops every layer; the counters are kept.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }

    /// The counters, and what is held now.
    #[must_use]
    pub fn stats(&self) -> EffectCacheStats {
        EffectCacheStats {
            keys: self.entries.len(),
            layers: self.entries.iter().map(|e| e.layers.len()).sum(),
            bytes: self.bytes,
            ..self.stats
        }
    }

    /// Starts a frame (one backend render call): layers used from here on
    /// are protected from eviction until the next.
    pub(crate) fn begin_frame(&mut self) {
        self.frame += 1;
    }

    /// The entry holding `p`'s key.
    pub(crate) fn find(&self, p: &Probe<'_>) -> Option<usize> {
        self.entries.iter().position(|e| e.matches(p))
    }

    /// Covers `keep` from entry `at`'s layers: each part of `keep` inside
    /// a layer's valid rectangle becomes a piece of that layer, and what no
    /// layer covers is returned as missing. The layers used are touched.
    pub(crate) fn cover(&mut self, at: usize, keep: DeviceRect) -> Cover {
        let mut out = Cover::default();
        let Some(entry) = self.entries.get_mut(at) else {
            out.missing.push(keep);
            return out;
        };
        let near: Vec<usize> = (0..entry.layers.len())
            .filter(|&i| entry.layers[i].valid.intersects(keep))
            .collect();
        let xs = cuts(
            keep.x0,
            keep.x1,
            near.iter()
                .flat_map(|&i| [entry.layers[i].valid.x0, entry.layers[i].valid.x1]),
        );
        let ys = cuts(
            keep.y0,
            keep.y1,
            near.iter()
                .flat_map(|&i| [entry.layers[i].valid.y0, entry.layers[i].valid.y1]),
        );
        // A cell lies wholly inside or wholly outside every valid
        // rectangle, because the cuts include all their edges; a run of
        // cells from one source along a row becomes one piece.
        let mut runs: Vec<(Option<usize>, DeviceRect)> = Vec::new();
        for y in ys.windows(2) {
            let mut run: Option<(Option<usize>, i32, i32)> = None;
            for x in xs.windows(2) {
                let cell = DeviceRect::new(x[0], y[0], x[1], y[1]);
                let src = near
                    .iter()
                    .copied()
                    .find(|&i| contains(entry.layers[i].valid, cell));
                match run {
                    Some((s, x0, _)) if s == src => run = Some((s, x0, x[1])),
                    _ => {
                        if let Some((s, x0, x1)) = run {
                            runs.push((s, DeviceRect::new(x0, y[0], x1, y[1])));
                        }
                        run = Some((src, x[0], x[1]));
                    }
                }
            }
            if let Some((s, x0, x1)) = run {
                runs.push((s, DeviceRect::new(x0, y[0], x1, y[1])));
            }
        }
        self.clock += 1;
        for (src, r) in runs {
            match src {
                Some(i) => {
                    let l = &mut entry.layers[i];
                    l.used = self.clock;
                    l.frame = self.frame;
                    out.pieces.push(Piece {
                        clip: r,
                        rect: l.rect,
                        pixels: Arc::clone(&l.pixels),
                    });
                }
                None => out.missing.push(r),
            }
        }
        out
    }

    /// Stores a result for `p`'s key: `pixels` over `rect`, exact over
    /// `valid`. Returns whether it was stored.
    pub(crate) fn store(
        &mut self,
        p: &Probe<'_>,
        valid: DeviceRect,
        rect: DeviceRect,
        pixels: Arc<Vec<u8>>,
    ) -> bool {
        let layer = Layer {
            valid,
            rect,
            pixels,
            used: 0,
            frame: self.frame,
        };
        let at = self.find(p);
        // A new key's snapshot is counted once it exists; the ops and the
        // layer are what can be judged before.
        let fixed = if at.is_none() {
            size_of_val(p.ops) + size_of::<Entry>()
        } else {
            0
        };
        let need = layer.bytes() + fixed;
        if need > self.limit {
            self.stats.refused += 1;
            return false;
        }
        if let Some(i) = at
            && self.entries[i].layers.len() >= MAX_LAYERS_PER_KEY
        {
            self.drop_oldest_layer_of(i);
        }
        while self.bytes + need > self.limit {
            if !self.evict_one(false) {
                self.stats.refused += 1;
                return false;
            }
        }
        // Eviction may have removed the key, or moved it.
        let at = match self.find(p) {
            Some(i) => i,
            None => {
                let snapshot = Snapshot::capture(p.ops, p.res);
                let fixed = size_of_val(p.ops) + size_of::<Entry>() + snapshot.bytes();
                self.bytes += fixed;
                self.entries.push(Entry {
                    hash: p.hash,
                    setting: p.setting,
                    ops: p.ops.to_vec(),
                    snapshot,
                    fixed,
                    layers: Vec::new(),
                });
                self.entries.len() - 1
            }
        };
        self.clock += 1;
        let layer = Layer {
            used: self.clock,
            ..layer
        };
        self.bytes += layer.bytes();
        self.entries[at].layers.push(layer);
        self.stats.stored += 1;
        // The new key's resource tables were not in `need`; they are
        // small, and older layers make room for them.
        while self.bytes > self.limit && self.evict_one(false) {}
        true
    }

    /// Counts a lookup: `reused` pieces came from the cache and `rendered`
    /// rectangles were not there.
    pub(crate) fn count(&mut self, reused: bool, rendered: bool) {
        match (reused, rendered) {
            (true, false) => self.stats.hits += 1,
            (true, true) => self.stats.partial += 1,
            (false, _) => self.stats.misses += 1,
        }
    }

    /// Counts a result that is not stored because it is not exact.
    pub(crate) fn refuse(&mut self) {
        self.stats.refused += 1;
    }

    fn remove_layer(&mut self, e: usize, l: usize) {
        let layer = self.entries[e].layers.swap_remove(l);
        self.bytes -= layer.bytes();
        self.stats.evicted += 1;
        if self.entries[e].layers.is_empty() {
            let entry = self.entries.swap_remove(e);
            self.bytes -= entry.fixed;
        }
    }

    fn drop_oldest_layer_of(&mut self, e: usize) {
        if let Some((l, _)) = self.entries[e]
            .layers
            .iter()
            .enumerate()
            .min_by_key(|(_, l)| l.used)
        {
            self.remove_layer(e, l);
        }
    }

    /// Evicts the least recently used layer; `any` allows one used in the
    /// current frame. Returns whether one went.
    fn evict_one(&mut self, any: bool) -> bool {
        let frame = self.frame;
        let victim = self
            .entries
            .iter()
            .enumerate()
            .flat_map(|(e, entry)| {
                entry
                    .layers
                    .iter()
                    .enumerate()
                    .map(move |(l, layer)| (e, l, layer))
            })
            .filter(|(_, _, layer)| any || layer.frame != frame)
            .min_by_key(|(_, _, layer)| layer.used)
            .map(|(e, l, _)| (e, l));
        match victim {
            Some((e, l)) => {
                self.remove_layer(e, l);
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuts_are_sorted_distinct_and_clamped() {
        assert_eq!(cuts(0, 10, [5, -3, 12, 5].into_iter()), vec![0, 5, 10]);
    }

    #[test]
    fn a_rectangle_contains_itself_and_not_a_larger_one() {
        let r = DeviceRect::new(1, 2, 5, 6);
        assert!(contains(r, r));
        assert!(!contains(r, r.inflated(1)));
        assert!(contains(r.inflated(1), r));
    }
}
