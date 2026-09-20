//! The per-node render cache and its admission and eviction policies.
//!
//! # Admission matters more than eviction
//!
//! Caching every node turns the cache into a memory leak with extra steps.
//! The original's criterion is encoded in the capture flags
//! `cfDIRECT`/`cfALLOWDIRECT` (`research/03 §2.5`): a node earns a slot when
//! regenerating it is expensive relative to compositing it. Here that is
//! [`AdmissionPolicy`]: transparent groups, live-effect subtrees, fractal
//! fills, and plain groups above a primitive threshold.
//!
//! # Why the scale is quantised
//!
//! Zoom is continuous and a cache keyed on the exact scale would miss every
//! frame of a zoom. Scales are therefore quantised to powers of √2, so a
//! cached surface stays usable while the zoom moves by up to ±41 %, and the
//! exact re-render is queued in the background.

use std::collections::HashMap;

use crate::scene::{CacheHint, ContentHash, RenderQuality, SceneNodeId};
use crate::surface::{DeviceRect, Surface};

/// Quantises a scale factor to a power of √2, returned as the exponent.
///
/// `scale_step(1.0) == 0`, `scale_step(√2) == 1`, `scale_step(2.0) == 2`.
#[must_use]
pub fn scale_step(scale: f64) -> i16 {
    if !scale.is_finite() || scale <= 0.0 {
        return 0;
    }
    let step = (scale.log2() * 2.0).round();
    step.clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

/// The scale a step stands for.
#[must_use]
pub fn step_scale(step: i16) -> f64 {
    2f64.powf(f64::from(step) / 2.0)
}

/// How far the true scale may drift from a cached one before the cached
/// surface has to be regenerated: half a √2 step in each direction, which
/// is ±41 %.
pub const RESCALE_TOLERANCE: f64 = 0.4143;

/// Whether a cached surface at `cached` is still usable at `wanted`.
#[must_use]
pub fn within_rescale_tolerance(cached: f64, wanted: f64) -> bool {
    if cached <= 0.0 || wanted <= 0.0 {
        return false;
    }
    let ratio = wanted / cached;
    (1.0 / (1.0 + RESCALE_TOLERANCE)..=1.0 + RESCALE_TOLERANCE).contains(&ratio)
}

/// What identifies a cached rendering of a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CacheKey {
    /// The node's content hash.
    pub content: ContentHash,
    /// The scale, quantised to powers of √2.
    pub scale_step: i16,
    /// Draft and Final are different pictures, so they are different keys.
    pub quality: RenderQuality,
}

impl CacheKey {
    /// Builds a key from a node's content hash and the view scale.
    #[must_use]
    pub fn new(content: ContentHash, scale: f64, quality: RenderQuality) -> CacheKey {
        CacheKey {
            content,
            scale_step: scale_step(scale),
            quality,
        }
    }
}

/// A rendered node, ready to be blitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedSurface {
    /// The pixels.
    pub surface: Surface,
    /// Where they belong, in the device space they were rendered in.
    pub bounds: DeviceRect,
}

impl CachedSurface {
    /// Bytes the surface occupies.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.surface.data().len()
    }
}

/// What the cache is doing, for the status bar and for the tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// Lookups that found a surface.
    pub hits: u64,
    /// Lookups that did not.
    pub misses: u64,
    /// Surfaces evicted to stay inside the budget.
    pub evictions: u64,
    /// Bytes currently held.
    pub bytes: usize,
    /// The budget in bytes.
    pub budget: usize,
    /// How many surfaces are held.
    pub entries: usize,
}

#[derive(Debug)]
struct Entry {
    surface: CachedSurface,
    cost_us: u32,
    last_use: u64,
    node: SceneNodeId,
}

impl Entry {
    /// Value per byte: expensive-to-make and small is worth keeping.
    fn value(&self) -> f64 {
        f64::from(self.cost_us) / (self.surface.bytes().max(1) as f64)
    }
}

/// A cost-weighted LRU cache of rendered nodes under a hard byte budget.
#[derive(Debug)]
pub struct RenderCache {
    entries: HashMap<CacheKey, Entry>,
    by_node: HashMap<SceneNodeId, Vec<CacheKey>>,
    budget: usize,
    bytes: usize,
    epoch: u64,
    stats: CacheStats,
}

impl Default for RenderCache {
    fn default() -> RenderCache {
        RenderCache::with_budget(256 * 1024 * 1024)
    }
}

impl RenderCache {
    /// A cache holding at most `bytes` of surfaces. The default budget is
    /// 256 MiB, and it is a user preference in Phase 5.
    #[must_use]
    pub fn with_budget(bytes: usize) -> RenderCache {
        RenderCache {
            entries: HashMap::new(),
            by_node: HashMap::new(),
            budget: bytes,
            bytes: 0,
            epoch: 0,
            stats: CacheStats {
                budget: bytes,
                ..CacheStats::default()
            },
        }
    }

    /// Looks a key up, counting the hit or miss and touching the entry.
    pub fn get(&mut self, key: CacheKey) -> Option<&CachedSurface> {
        self.epoch += 1;
        let epoch = self.epoch;
        match self.entries.get_mut(&key) {
            Some(e) => {
                e.last_use = epoch;
                self.stats.hits += 1;
                Some(&e.surface)
            }
            None => {
                self.stats.misses += 1;
                None
            }
        }
    }

    /// Whether a key is present, without counting a hit.
    #[must_use]
    pub fn contains(&self, key: CacheKey) -> bool {
        self.entries.contains_key(&key)
    }

    /// Stores a rendered node, evicting until the budget is respected.
    ///
    /// A surface larger than the whole budget is not stored at all, which
    /// is better than evicting everything for something that cannot help.
    pub fn insert(
        &mut self,
        key: CacheKey,
        node: SceneNodeId,
        surface: CachedSurface,
        cost_us: u32,
    ) {
        let size = surface.bytes();
        if size > self.budget {
            return;
        }
        self.epoch += 1;
        if let Some(old) = self.entries.remove(&key) {
            self.bytes -= old.surface.bytes();
        }
        self.entries.insert(
            key,
            Entry {
                surface,
                cost_us,
                last_use: self.epoch,
                node,
            },
        );
        self.by_node.entry(node).or_default().push(key);
        self.bytes += size;
        self.evict_to_budget();
        self.sync_stats();
    }

    /// Drops every surface belonging to a node and its recorded keys.
    pub fn invalidate_node(&mut self, node: SceneNodeId) {
        if let Some(keys) = self.by_node.remove(&node) {
            for k in keys {
                if let Some(e) = self.entries.remove(&k) {
                    self.bytes -= e.surface.bytes();
                }
            }
        }
        self.sync_stats();
    }

    /// Drops everything.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.by_node.clear();
        self.bytes = 0;
        self.sync_stats();
    }

    /// Current counters.
    #[must_use]
    pub fn stats(&self) -> CacheStats {
        self.stats
    }

    /// Bytes currently held.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// How many surfaces are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn evict_to_budget(&mut self) {
        while self.bytes > self.budget {
            // Evict the least valuable, breaking ties by least recently
            // used. Both keys are total orders, so eviction is
            // deterministic, which the determinism test depends on.
            let victim = self
                .entries
                .iter()
                .min_by(|a, b| {
                    a.1.value()
                        .partial_cmp(&b.1.value())
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then(a.1.last_use.cmp(&b.1.last_use))
                        .then(a.0.cmp(b.0))
                })
                .map(|(k, _)| *k);
            let Some(k) = victim else { break };
            if let Some(e) = self.entries.remove(&k) {
                self.bytes -= e.surface.bytes();
                if let Some(keys) = self.by_node.get_mut(&e.node) {
                    keys.retain(|x| *x != k);
                }
                self.stats.evictions += 1;
            }
        }
    }

    fn sync_stats(&mut self) {
        self.stats.bytes = self.bytes;
        self.stats.entries = self.entries.len();
    }
}

/// Decides which nodes are worth a cache slot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdmissionPolicy {
    /// A plain group earns a slot above this many primitives.
    ///
    /// The phase leaves the value to be determined by sweeping it over the
    /// corpus and taking the knee of the frame-time curve; the sweep lives
    /// in `benches/render.rs` (`cache_threshold`). 64 is the starting point
    /// and the number to revisit with corpus data.
    pub group_primitive_threshold: usize,
    /// Whether a group that composites with a destination-reading family is
    /// always cached.
    pub cache_transparent_groups: bool,
}

impl Default for AdmissionPolicy {
    fn default() -> AdmissionPolicy {
        AdmissionPolicy {
            group_primitive_threshold: 64,
            cache_transparent_groups: true,
        }
    }
}

impl AdmissionPolicy {
    /// Whether a node should be cached.
    #[must_use]
    pub fn admits(
        &self,
        hint: CacheHint,
        primitives: usize,
        reads_destination: bool,
        is_effect: bool,
    ) -> bool {
        match hint {
            CacheHint::Never => false,
            CacheHint::Always => true,
            CacheHint::Auto => {
                is_effect
                    || (self.cache_transparent_groups && reads_destination)
                    || primitives >= self.group_primitive_threshold
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: u8) -> CacheKey {
        CacheKey {
            content: ContentHash([n; 16]),
            scale_step: 0,
            quality: RenderQuality::Final,
        }
    }

    fn surface(px: u32) -> CachedSurface {
        CachedSurface {
            surface: Surface::new(px, px),
            bounds: DeviceRect::from_size(px, px),
        }
    }

    #[test]
    fn scale_quantises_to_powers_of_root_two() {
        assert_eq!(scale_step(1.0), 0);
        assert_eq!(scale_step(std::f64::consts::SQRT_2), 1);
        assert_eq!(scale_step(2.0), 2);
        assert_eq!(scale_step(0.5), -2);
        assert_eq!(scale_step(0.0), 0);
        assert!((step_scale(2) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn adjacent_zoom_steps_share_a_key_inside_the_tolerance() {
        // Within 41 % the cached surface is rescaled rather than rebuilt.
        assert!(within_rescale_tolerance(1.0, 1.4));
        assert!(within_rescale_tolerance(1.0, 0.71));
        assert!(!within_rescale_tolerance(1.0, 1.5));
        assert!(!within_rescale_tolerance(1.0, 0.6));
    }

    #[test]
    fn a_hit_is_counted_and_a_miss_is_too() {
        let mut c = RenderCache::with_budget(1 << 20);
        assert!(c.get(key(1)).is_none());
        c.insert(key(1), SceneNodeId(1), surface(8), 100);
        assert!(c.get(key(1)).is_some());
        let s = c.stats();
        assert_eq!((s.hits, s.misses), (1, 1));
    }

    #[test]
    fn invalidating_a_node_drops_exactly_its_surfaces() {
        let mut c = RenderCache::with_budget(1 << 20);
        c.insert(key(1), SceneNodeId(1), surface(8), 10);
        c.insert(key(2), SceneNodeId(2), surface(8), 10);
        c.invalidate_node(SceneNodeId(1));
        assert!(!c.contains(key(1)));
        assert!(c.contains(key(2)));
    }

    #[test]
    fn the_budget_is_never_exceeded_under_stress() {
        let budget = 64 * 1024;
        let mut c = RenderCache::with_budget(budget);
        for i in 0..10_000u32 {
            let k = CacheKey {
                content: ContentHash(i.to_le_bytes().repeat(4).try_into().unwrap()),
                scale_step: 0,
                quality: RenderQuality::Final,
            };
            c.insert(k, SceneNodeId(u64::from(i % 17)), surface(16), i % 500);
            assert!(c.bytes() <= budget, "budget exceeded at insert {i}");
        }
        assert!(c.stats().evictions > 0);
    }

    #[test]
    fn a_surface_larger_than_the_budget_is_refused_rather_than_clearing_the_cache() {
        let mut c = RenderCache::with_budget(1024);
        c.insert(key(1), SceneNodeId(1), surface(8), 10);
        c.insert(key(2), SceneNodeId(2), surface(256), 10_000);
        assert!(c.contains(key(1)));
        assert!(!c.contains(key(2)));
    }

    #[test]
    fn eviction_prefers_the_cheap_and_the_stale() {
        let mut c = RenderCache::with_budget(8 * 8 * 4 * 2);
        c.insert(key(1), SceneNodeId(1), surface(8), 10_000); // expensive
        c.insert(key(2), SceneNodeId(2), surface(8), 1); // cheap
        c.insert(key(3), SceneNodeId(3), surface(8), 5_000);
        assert!(c.contains(key(1)), "the expensive one survives");
        assert!(!c.contains(key(2)), "the cheap one goes first");
    }

    #[test]
    fn the_admission_policy_keeps_the_cheap_out() {
        let p = AdmissionPolicy::default();
        assert!(!p.admits(CacheHint::Auto, 3, false, false));
        assert!(
            p.admits(CacheHint::Auto, 3, true, false),
            "transparent groups"
        );
        assert!(p.admits(CacheHint::Auto, 3, false, true), "live effects");
        assert!(p.admits(CacheHint::Auto, 64, false, false), "big groups");
        assert!(!p.admits(CacheHint::Never, 10_000, true, true));
        assert!(p.admits(CacheHint::Always, 0, false, false));
    }
}
