//! The render cache: admission, invalidation and the byte budget.

use xarast_render::cache::{AdmissionPolicy, within_rescale_tolerance};
use xarast_render::scene::{CacheHint, ContentHash};
use xarast_render::{
    CacheKey, CachedSurface, DeviceRect, RenderCache, RenderQuality, SceneNodeId, Surface,
    scale_step, step_scale,
};

fn surface(px: u32) -> CachedSurface {
    CachedSurface {
        surface: Surface::new(px, px),
        bounds: DeviceRect::from_size(px, px),
    }
}

fn key(n: u64, scale: f64) -> CacheKey {
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&n.to_le_bytes());
    CacheKey::new(ContentHash(bytes), scale, RenderQuality::Final)
}

#[test]
fn a_repeated_frame_with_no_changes_does_no_work() {
    let mut c = RenderCache::with_budget(8 << 20);
    for i in 0..32u64 {
        c.insert(key(i, 1.0), SceneNodeId(i), surface(32), 500);
    }
    let before = c.stats();
    for i in 0..32u64 {
        assert!(c.get(key(i, 1.0)).is_some(), "node {i} should still be cached");
    }
    let after = c.stats();
    assert_eq!(after.misses, before.misses, "a warm frame must not miss");
    assert_eq!(after.hits, before.hits + 32);
}

#[test]
fn editing_one_node_invalidates_exactly_that_node() {
    let mut c = RenderCache::with_budget(8 << 20);
    for i in 0..16u64 {
        c.insert(key(i, 1.0), SceneNodeId(i), surface(16), 100);
    }
    c.invalidate_node(SceneNodeId(7));
    for i in 0..16u64 {
        assert_eq!(
            c.contains(key(i, 1.0)),
            i != 7,
            "node {i} has the wrong cache state"
        );
    }
}

#[test]
fn the_budget_holds_under_ten_thousand_inserts() {
    let budget = 512 * 1024;
    let mut c = RenderCache::with_budget(budget);
    for i in 0..10_000u64 {
        c.insert(
            key(i, 1.0),
            SceneNodeId(i % 23),
            surface(32),
            u32::try_from(i % 997).unwrap_or(0),
        );
        assert!(c.bytes() <= budget, "over budget at insert {i}");
    }
    assert!(c.stats().evictions > 9_000);
    assert_eq!(c.stats().budget, budget);
}

#[test]
fn a_zoom_inside_the_tolerance_reuses_the_same_key() {
    // Powers of root two, with a 41 % band, is what stops a zoom gesture
    // from missing the cache on every frame.
    // A step spans a factor of root two, so anything inside 2^0.25 = 1.19
    // of a step centre shares its key.
    assert_eq!(scale_step(1.0), scale_step(1.18));
    assert_ne!(scale_step(1.0), scale_step(1.25));
    assert_ne!(scale_step(1.0), scale_step(2.0));
    assert!(within_rescale_tolerance(step_scale(0), 1.4));
    assert!(!within_rescale_tolerance(step_scale(0), 1.5));
}

#[test]
fn admission_keeps_the_cheap_out_and_the_expensive_in() {
    let p = AdmissionPolicy::default();
    assert!(!p.admits(CacheHint::Auto, 1, false, false), "a single path");
    assert!(p.admits(CacheHint::Auto, 1, true, false), "a transparent group");
    assert!(p.admits(CacheHint::Auto, 1, false, true), "a live effect");
    assert!(
        p.admits(CacheHint::Auto, p.group_primitive_threshold, false, false),
        "a group at the threshold"
    );
}
