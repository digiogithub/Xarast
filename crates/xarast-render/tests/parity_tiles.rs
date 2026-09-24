//! The GPU tile cache against the CPU reference composite, on every
//! adapter the machine has (or the one `WGPU_ADAPTER_NAME` names).
//!
//! The tiles are real CPU renders of the 120-scene feature corpus, cut
//! into 32 × 32 tiles through [`TileGrid::tile_view`]. Each case is then
//! composited eight ways (identity, whole-pixel pan, fractional pan, zoom
//! in ×1.5 and ×3.7, zoom out ×0.6, a missing tile, and a partial tile at
//! a texel offset next to a tile uploaded in two halves) by
//! [`compose_cpu`] and by [`GpuTileCache`], and the two must agree **byte
//! for byte**: the texel rule is one subtraction and one multiplication in
//! `f32` on both sides and the shader reads with `textureLoad`, so there
//! is nothing for a driver to round differently. A device that breaks
//! this breaks it for a reason worth knowing; the failure message prints
//! the parity-band figures as well.
//!
//! Skips, never fails, with no adapter: the crate must test headless.

#![cfg(feature = "gpu")]

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use xarast_render::backend::gpu_tiles::{create_target, read_back};
use xarast_render::corpus::all_cases;
use xarast_render::golden::compare;
use xarast_render::gpu_test_lock;
use xarast_render::{
    CpuBackend, CpuConfig, DeviceRect, DirtyRect, DisplayList, GpuTileCache, GpuTileCacheConfig,
    Surface, TileGrid, TileKey, TilePlacement, Transform2D, compose_cpu, whole_tile,
};

const TILE: u32 = 32;
const BACKDROP: [u8; 4] = [40, 44, 52, 255];

fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    use std::task::{Context, Poll, Waker};
    let mut cx = Context::from_waker(Waker::noop());
    let mut fut = Box::pin(fut);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

/// Every adapter, or those whose name contains `WGPU_ADAPTER_NAME`
/// (case-insensitive), with a device each.
///
/// The caller must hold the machine-wide GPU lock
/// ([`gpu_test_lock::acquire`]) for as long as the devices live.
fn devices() -> Vec<(String, Arc<wgpu::Device>, Arc<wgpu::Queue>)> {
    if wgpu::Instance::enabled_backend_features().is_empty() {
        return Vec::new();
    }
    let want = std::env::var("WGPU_ADAPTER_NAME")
        .ok()
        .map(|s| s.to_lowercase());
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let mut out = Vec::new();
    for adapter in block_on(instance.enumerate_adapters(wgpu::Backends::all())) {
        let info = adapter.get_info();
        let label = format!("{} ({:?}, {:?})", info.name, info.backend, info.device_type);
        if want
            .as_ref()
            .is_some_and(|w| !info.name.to_lowercase().contains(w))
        {
            continue;
        }
        match block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())) {
            Ok((d, q)) => out.push((label, Arc::new(d), Arc::new(q))),
            Err(e) => eprintln!("{label}: no device: {e}"),
        }
    }
    out
}

struct Tiled {
    tiles: HashMap<TileKey, Surface>,
    level: Transform2D,
    full: Surface,
}

/// Renders a case whole and as tiles, both on the deterministic backend.
fn tiled(case: &xarast_render::corpus::Case, grid: TileGrid) -> Tiled {
    let full = common::render_case(case);
    let mut backend = CpuBackend::new(CpuConfig::deterministic());
    let mut tiles = HashMap::new();
    for key in grid.covering(0, case.view.viewport) {
        let view = grid.tile_view(&case.view, key);
        let dl = DisplayList::build(&case.scene, &view, &DirtyRect::NONE);
        let mut s = Surface::new(TILE, TILE);
        backend
            .render(&dl, &case.resolver, &mut s)
            .expect("a tile fits");
        tiles.insert(key, s);
    }
    Tiled {
        tiles,
        level: case.view.transform,
        full,
    }
}

/// The view changes under test, as transforms applied after the level's.
fn mappings() -> Vec<(&'static str, Transform2D)> {
    vec![
        ("identity", Transform2D::IDENTITY),
        ("pan_whole", Transform2D::translate(13.0, -7.0)),
        ("pan_fractional", Transform2D::translate(10.4, 3.7)),
        (
            "zoom_in_1_5",
            Transform2D::scale(1.5).then(Transform2D::translate(-20.25, -11.5)),
        ),
        (
            "zoom_out_0_6",
            Transform2D::scale(0.6).then(Transform2D::translate(17.3, 9.9)),
        ),
        (
            "zoom_in_3_7",
            Transform2D::scale(3.7).then(Transform2D::translate(-101.0, -77.3)),
        ),
    ]
}

#[test]
fn the_gpu_tile_composite_equals_the_cpu_reference_byte_for_byte() {
    // Bound first so that it drops last, after every device.
    let Some(_gpu) = gpu_test_lock::acquire("parity_tiles::byte_for_byte") else {
        return;
    };
    let devices = devices();
    if devices.is_empty() {
        eprintln!("skipping tile parity: no adapter. The gate is unmeasured, not passed.");
        return;
    }
    let grid = TileGrid { tile_size: TILE };
    let cases: Vec<_> = all_cases()
        .iter()
        .map(|c| (c.name.clone(), tiled(c, grid)))
        .collect();
    // A target larger than the case, so that every mapping leaves some
    // backdrop showing.
    let (tw, th) = (112, 104);
    for (label, device, queue) in devices {
        let mut cache = GpuTileCache::new(
            device.clone(),
            queue.clone(),
            GpuTileCacheConfig {
                tile_size: TILE,
                capacity: 16,
            },
        )
        .expect("a tile cache");
        let target = create_target(&device, tw, th);
        let mut checked = 0;
        let mut failures = Vec::new();
        for (name, t) in &cases {
            cache.clear();
            for (key, s) in &t.tiles {
                cache
                    .upload(*key, s, s.bounds(), [0, 0])
                    .expect("a tile uploads");
            }
            let mut maps = mappings();
            maps.push(("missing_tile", Transform2D::IDENTITY));
            maps.push((
                "partial_and_split",
                Transform2D::scale(1.5).then(Transform2D::translate(-20.25, -11.5)),
            ));
            let hole = TileKey {
                level: 0,
                tx: 1,
                ty: 1,
            };
            let corner = TileKey {
                level: 0,
                tx: 0,
                ty: 0,
            };
            // A partial tile, valid on no edge of the tile.
            let piece = [5, 3, 25, 20];
            for (what, m) in maps {
                let view = t.level.then(m);
                let mut keys: Vec<TileKey> = t.tiles.keys().copied().collect();
                keys.sort();
                let placements: Vec<TilePlacement> = keys
                    .iter()
                    .map(|k| {
                        let valid = if what == "partial_and_split" && *k == hole {
                            piece
                        } else {
                            whole_tile(TILE)
                        };
                        grid.placement(*k, t.level, view, valid)
                            .expect("axis-aligned")
                    })
                    .collect();
                if what == "missing_tile" {
                    cache.invalidate(&hole);
                }
                if what == "partial_and_split" {
                    // The hole comes back as a piece at a texel offset; the
                    // corner tile is uploaded afresh in two halves.
                    let src = &t.tiles[&hole];
                    let r = DeviceRect::new(5, 3, 25, 20);
                    cache.upload(hole, src, r, [5, 3]).expect("a piece");
                    let src = &t.tiles[&corner];
                    let half = (TILE / 2).cast_signed();
                    let full = TILE.cast_signed();
                    cache.invalidate(&corner);
                    cache
                        .upload(corner, src, DeviceRect::new(0, 0, full, half), [0, 0])
                        .expect("top half");
                    cache
                        .upload(
                            corner,
                            src,
                            DeviceRect::new(0, half, full, full),
                            [0, TILE / 2],
                        )
                        .expect("bottom half");
                }
                let mut cpu = Surface::new(tw, th);
                compose_cpu(&mut cpu, BACKDROP, &placements, |k| {
                    (what != "missing_tile" || *k != hole)
                        .then(|| t.tiles.get(k))
                        .flatten()
                });
                cache
                    .compose(&target, &placements, BACKDROP)
                    .expect("composes");
                let gpu = read_back(&device, &queue, &target).expect("reads back");
                checked += 1;
                if gpu != cpu {
                    let c = compare(&gpu, &cpu);
                    failures.push(format!(
                        "{name}/{what}: {} px differ, rms {:.5}, max {}/255",
                        c.differing_pixels, c.rms, c.max_channel_delta
                    ));
                }
            }
        }
        eprintln!(
            "{label}: {checked} composites, {} differ from the CPU reference",
            failures.len()
        );
        assert!(
            failures.is_empty(),
            "{label}: {} of {checked} composites differ: {:?}",
            failures.len(),
            &failures[..failures.len().min(8)]
        );
    }
}

/// Not a GPU test: whether a frame assembled from tiles rasterised one by
/// one equals the frame rasterised whole. This is what a tile-based
/// scheduler would show at rest, so the answer belongs next to the
/// compositor's, and it is recorded rather than assumed. It is **not**
/// exact, which is why a `Final` frame at rest is still rasterised whole.
#[test]
fn a_frame_assembled_from_tiles_differs_from_the_whole_frame_in_few_pixels() {
    let grid = TileGrid { tile_size: TILE };
    let mut exact = 0;
    let mut worst = (String::new(), 0u64, 0u8);
    let cases = all_cases();
    for case in &cases {
        let t = tiled(case, grid);
        let (w, h) = (case.view.viewport.width(), case.view.viewport.height());
        let placements: Vec<TilePlacement> = t
            .tiles
            .keys()
            .map(|k| {
                grid.placement(*k, t.level, t.level, whole_tile(TILE))
                    .expect("identity")
            })
            .collect();
        let mut assembled = Surface::new(w, h);
        compose_cpu(&mut assembled, [0; 4], &placements, |k| t.tiles.get(k));
        let c = compare(&assembled, &t.full);
        if c.is_exact() {
            exact += 1;
        } else if c.differing_pixels > worst.1 {
            worst = (case.name.clone(), c.differing_pixels, c.max_channel_delta);
        }
        // Measured 2026-09-23: 105 of 120 cases are byte-identical; twelve
        // mesh gradients and `aa_seam` move 1–17 pixels by 1/255, and the
        // two perspective linear gradients with a repeat move one pixel on
        // the wrap edge by 230/255 (a sub-ulp change flips which side of
        // the discontinuity the pixel centre falls on). Hence a pixel
        // budget rather than the parity band, which one pixel of 230 fails
        // on a 96 x 96 frame.
        assert!(
            c.differing_pixels * 500 <= c.pixels,
            "{}: {} of {} pixels differ from the whole frame (max {}/255); the budget is 0.2 %",
            case.name,
            c.differing_pixels,
            c.pixels,
            c.max_channel_delta
        );
        assert_eq!(
            assembled.bounds(),
            DeviceRect::from_size(w, h),
            "the assembled frame is the case's size"
        );
    }
    eprintln!(
        "tiles vs whole frame: {exact} of {} cases byte-identical; worst {} ({} px, max {}/255)",
        cases.len(),
        worst.0,
        worst.1,
        worst.2
    );
}

#[test]
fn a_full_cache_evicts_the_least_recently_used_tile() {
    let Some(_gpu) = gpu_test_lock::acquire("parity_tiles::lru_eviction") else {
        return;
    };
    let Some((_, device, queue)) = devices().into_iter().next() else {
        eprintln!("skipping: no adapter");
        return;
    };
    let mut cache = GpuTileCache::new(
        device.clone(),
        queue,
        GpuTileCacheConfig {
            tile_size: TILE,
            capacity: 2,
        },
    )
    .expect("a tile cache");
    let s = Surface::filled(TILE, TILE, [1, 2, 3, 255]);
    let key = |tx| TileKey {
        level: 0,
        tx,
        ty: 0,
    };
    cache.upload(key(0), &s, s.bounds(), [0, 0]).expect("0");
    cache.upload(key(1), &s, s.bounds(), [0, 0]).expect("1");
    // Drawing tile 0 makes tile 1 the least recently used.
    let target = create_target(&device, TILE, TILE);
    let p = TileGrid { tile_size: TILE }
        .placement(
            key(0),
            Transform2D::IDENTITY,
            Transform2D::IDENTITY,
            whole_tile(TILE),
        )
        .expect("identity");
    let stats = cache.compose(&target, &[p], [0; 4]).expect("composes");
    assert_eq!((stats.drawn, stats.missing), (1, 0));
    cache.upload(key(2), &s, s.bounds(), [0, 0]).expect("2");
    assert!(cache.contains(&key(0)) && cache.contains(&key(2)));
    assert!(!cache.contains(&key(1)));
    assert_eq!(cache.len(), 2);
    // A piece that does not fit at its offset is refused, not clipped.
    assert!(
        cache
            .upload(key(3), &s, DeviceRect::new(0, 0, 8, 8), [TILE - 4, 0])
            .is_err()
    );
}
