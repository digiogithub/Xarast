//! What a pan or a `Draft` zoom costs when the pixels stay on the GPU
//! (`GpuTileCache`), against what the shell does today (scroll the kept
//! frame on the CPU, then upload all of it). XARA-US-0011.
//!
//! ```text
//! cargo bench -p xarast-render --features gpu --bench tiles
//! WGPU_ADAPTER_NAME=intel cargo bench -p xarast-render --features gpu --bench tiles
//! TILES_FRAMES=100 ...
//! ```
//!
//! GPU rows are wall-clock from the first `wgpu` call to `poll(wait)`
//! returning, like the W0 spike: an upper bound on GPU time that includes
//! submission and the driver's own overhead, which is what a frame pays.
//! Every row prints median, min and max over the frames.

use std::sync::Arc;
use std::time::{Duration, Instant};

use xarast_color::Rgba8;
use xarast_geom::{FillRule, Mp, Path, Point};
use xarast_render::backend::cpu::Resolver;
use xarast_render::backend::gpu_tiles::create_target;
use xarast_render::{
    CpuBackend, CpuConfig, DeviceRect, DirtyRect, DisplayList, GpuTileCache, GpuTileCacheConfig,
    Paint, PathRef, RenderQuality, Scene, SceneBuilder, SceneNodeId, Surface, TileGrid,
    TilePlacement, Transform2D, ViewParams, compose_cpu, scroll_surface, whole_tile,
};

const W: u32 = 1920;
const H: u32 = 1080;
const TILE: u32 = 256;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// The light `bulk` scene of `benches/render.rs`, over a level area of
/// `w × h` pixels.
fn bulk(w: u32, h: u32, count: u64) -> Scene {
    let mut rng = Rng(0xdead_beef);
    let mut scene = Scene::new();
    let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
    for i in 0..count {
        let cx = rng.next() * f64::from(w);
        let cy = rng.next() * f64::from(h);
        let r = 2.0 + rng.next() * 6.0;
        let mut p = Path::builder();
        for k in 0..12 {
            let a = std::f64::consts::TAU * f64::from(k) / 12.0;
            let rr = r * (0.6 + rng.next() * 0.4);
            let pt = Point::new(
                Mp::from_pt(cx + rr * a.cos()),
                Mp::from_pt(cy + rr * a.sin()),
            );
            if k == 0 {
                p.move_to(pt);
            } else {
                p.line_to(pt);
            }
        }
        p.close();
        b.fill(
            SceneNodeId(i),
            &PathRef::new(p.build()),
            FillRule::NonZero,
            Paint::Solid(Rgba8 {
                r: (i % 251) as u8,
                g: (i % 241) as u8,
                b: 180,
                a: 255,
            }),
        );
    }
    b.finish().expect("balanced");
    scene
}

#[derive(Clone, Copy)]
struct Stats {
    median: f64,
    min: f64,
    max: f64,
}

fn stats(mut xs: Vec<Duration>) -> Stats {
    xs.sort();
    let ms = |d: Duration| d.as_secs_f64() * 1e3;
    Stats {
        median: ms(xs[xs.len() / 2]),
        min: ms(xs[0]),
        max: ms(xs[xs.len() - 1]),
    }
}

fn frames() -> usize {
    std::env::var("TILES_FRAMES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(50)
        .max(1)
}

fn row(what: &str, s: Stats) {
    println!("| {what} | {:.3} | {:.3} | {:.3} |", s.median, s.min, s.max);
}

fn time(mut f: impl FnMut()) -> Stats {
    f(); // warm-up: pipelines, staging belts, page faults
    let xs = (0..frames())
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed()
        })
        .collect();
    stats(xs)
}

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

fn wait(device: &wgpu::Device) {
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("device poll");
}

/// Placements for every tile of `level_rect` seen through `m` (level
/// device space → view device space).
fn placements(grid: TileGrid, level_rect: DeviceRect, m: Transform2D) -> Vec<TilePlacement> {
    placements_in(grid, level_rect, m, (W, H))
}

fn placements_in(
    grid: TileGrid,
    level_rect: DeviceRect,
    m: Transform2D,
    (w, h): (u32, u32),
) -> Vec<TilePlacement> {
    grid.covering(0, level_rect)
        .into_iter()
        .filter_map(|k| {
            grid.placement(k, Transform2D::IDENTITY, m, whole_tile(TILE))
                .filter(|p| p.target_rect().intersects(DeviceRect::from_size(w, h)))
        })
        .collect()
}

fn main() {
    // The level space is a 3 × 3 screen area, so that pans and zoom-outs
    // always have tiles to show.
    let (lw, lh) = (W * 3, H * 3);
    let scene = bulk(lw, lh, 900_000);
    let level_view = ViewParams::new(
        lw,
        lh,
        Transform2D::scale(1.0 / 1000.0),
        RenderQuality::Draft,
    );
    let res = Resolver::new();
    let grid = TileGrid { tile_size: TILE };
    let mut cpu = CpuBackend::new(CpuConfig::interactive());

    println!("## CPU side (this machine, `CpuConfig::interactive()`)");
    println!();
    println!("| operation | median ms | min ms | max ms |");
    println!("|---|---|---|---|");
    // What the render thread does today on a pan: move the kept frame.
    let mut kept = Surface::filled(W, H, [200, 200, 200, 255]);
    row(
        "scroll_surface 1920 × 1080 by (13, -7)",
        time(|| {
            let _ = scroll_surface(&mut kept, 13, -7);
        }),
    );
    // What a tile scheduler rasterises when a pan crosses a tile row: one
    // 1920 × 256 dirty band of the 100k-per-screen scene.
    let mut level = Surface::new(lw, lh);
    let t = TILE as i32;
    let band = DeviceRect::new(W as i32, 4 * t, 2 * W as i32, 5 * t);
    row(
        "rasterise a 1920 × 256 tile row (100k objects per screen)",
        time(|| {
            let dl = DisplayList::build(&scene, &level_view, &DirtyRect::of(band));
            cpu.render(&dl, &res, &mut level).expect("renders");
        }),
    );
    row(
        "  of which: DisplayList::build for that row",
        time(|| {
            let dl = DisplayList::build(&scene, &level_view, &DirtyRect::of(band));
            std::hint::black_box(&dl);
        }),
    );
    let one = DeviceRect::new(8 * t, 4 * t, 9 * t, 5 * t);
    row(
        "rasterise one 256 × 256 tile",
        time(|| {
            let dl = DisplayList::build(&scene, &level_view, &DirtyRect::of(one));
            cpu.render(&dl, &res, &mut level).expect("renders");
        }),
    );
    // Fill the level surface once, for the tiles below.
    let dl = DisplayList::build(&scene, &level_view, &DirtyRect::NONE);
    cpu.render(&dl, &res, &mut level).expect("renders");
    let tiles: std::collections::HashMap<_, _> = grid
        .covering(0, level.bounds())
        .into_iter()
        .map(|k| {
            (
                k,
                level.sub_surface(grid.rect(k).intersection(level.bounds())),
            )
        })
        .collect();
    let screen = DeviceRect::new(W as i32, H as i32, 2 * W as i32, 2 * H as i32);
    let pan = Transform2D::translate(-f64::from(W) + 13.0, -f64::from(H) - 7.0);
    let pan_p = placements(grid, screen.inflated(TILE as i32), pan);
    let mut out = Surface::new(W, H);
    row(
        &format!("compose_cpu, pan, {} tiles (software tier)", pan_p.len()),
        time(|| {
            compose_cpu(&mut out, [0; 4], &pan_p, |k| tiles.get(k));
        }),
    );
    let zoom = Transform2D::translate(-f64::from(W) * 1.5, -f64::from(H) * 1.5)
        .then(Transform2D::scale(1.3))
        .then(Transform2D::translate(
            f64::from(W) / 2.0,
            f64::from(H) / 2.0,
        ));
    let zoom_p = placements(grid, level.bounds(), zoom);
    row(
        &format!("compose_cpu, Draft zoom 1.3×, {} tiles", zoom_p.len()),
        time(|| {
            compose_cpu(&mut out, [0; 4], &zoom_p, |k| tiles.get(k));
        }),
    );

    // The machine-wide GPU lock, held until every device is gone.
    let Some(_gpu) = xarast_render::gpu_test_lock::acquire("bench tiles") else {
        return;
    };
    let want = std::env::var("WGPU_ADAPTER_NAME")
        .ok()
        .map(|s| s.to_lowercase());
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    for adapter in block_on(instance.enumerate_adapters(wgpu::Backends::VULKAN)) {
        let info = adapter.get_info();
        let named = want.as_ref().map(|w| info.name.to_lowercase().contains(w));
        if named == Some(false) || (named.is_none() && info.device_type == wgpu::DeviceType::Cpu) {
            continue;
        }
        println!();
        println!(
            "## {} ({:?}, {} {})",
            info.name, info.device_type, info.driver, info.driver_info
        );
        println!();
        println!("| operation | median ms | min ms | max ms |");
        println!("|---|---|---|---|");
        let (device, queue) = match block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("tiles bench"),
            required_limits: adapter.limits(),
            ..Default::default()
        })) {
            Ok((d, q)) => (Arc::new(d), Arc::new(q)),
            Err(e) => {
                println!("| no device: {e} | | | |");
                continue;
            }
        };
        let target = create_target(&device, W, H);
        let mut cache = GpuTileCache::new(
            device.clone(),
            queue.clone(),
            GpuTileCacheConfig {
                tile_size: TILE,
                capacity: 512,
            },
        )
        .expect("a tile cache");

        row(
            "floor: an empty submit + poll(wait)",
            time(|| {
                queue.submit([]);
                wait(&device);
            }),
        );
        // Today's presentation of a CPU frame: the whole of it, uploaded.
        let full = out.clone();
        row(
            "today: upload the whole 1920 × 1080 frame",
            time(|| {
                queue.write_texture(
                    target.as_image_copy(),
                    full.data(),
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(W * 4),
                        rows_per_image: Some(H),
                    },
                    target.size(),
                );
                queue.submit([]);
                wait(&device);
            }),
        );

        let out_zoom = Transform2D::translate(-f64::from(W) * 1.5, -f64::from(H) * 1.5)
            .then(Transform2D::scale(0.5))
            .then(Transform2D::translate(
                f64::from(W) / 2.0,
                f64::from(H) / 2.0,
            ));
        let out_p = placements(grid, level.bounds(), out_zoom);
        // The 4K view: the middle 3840 x 2160 of the level space.
        let (w4, h4) = (2 * W, 2 * H);
        let target4 = create_target(&device, w4, h4);
        let screen4 = DeviceRect::new(
            W as i32 / 2,
            H as i32 / 2,
            5 * W as i32 / 2,
            5 * H as i32 / 2,
        );
        let pan4 = Transform2D::translate(-f64::from(W) / 2.0 + 13.0, -f64::from(H) / 2.0 - 7.0);
        let pan4_p = placements_in(grid, screen4.inflated(TILE as i32), pan4, (w4, h4));
        let full4 = level.sub_surface(screen4);
        let mut needed: Vec<_> = [&pan_p, &zoom_p, &out_p, &pan4_p]
            .iter()
            .flat_map(|ps| ps.iter().map(|p| p.key))
            .collect();
        needed.sort();
        needed.dedup();
        assert!(needed.len() <= 512, "{} tiles needed", needed.len());
        for k in &needed {
            let s = &tiles[k];
            cache.upload(*k, s, s.bounds(), [0, 0]).expect("uploads");
        }
        wait(&device);
        row(
            &format!("pan, all tiles resident ({} placed)", pan_p.len()),
            time(|| {
                cache.compose(&target, &pan_p, [0; 4]).expect("composes");
                wait(&device);
            }),
        );
        let row_keys = grid.covering(0, band);
        row(
            &format!(
                "pan crossing a tile row: upload {} tiles + compose",
                row_keys.len()
            ),
            time(|| {
                for k in &row_keys {
                    cache
                        .upload(
                            *k,
                            &level,
                            grid.rect(*k).intersection(level.bounds()),
                            [0, 0],
                        )
                        .expect("uploads");
                }
                cache.compose(&target, &pan_p, [0; 4]).expect("composes");
                wait(&device);
            }),
        );
        row(
            &format!("Draft zoom 1.3×, all resident ({} placed)", zoom_p.len()),
            time(|| {
                cache.compose(&target, &zoom_p, [0; 4]).expect("composes");
                wait(&device);
            }),
        );
        row(
            &format!("Draft zoom 0.5×, all resident ({} placed)", out_p.len()),
            time(|| {
                cache.compose(&target, &out_p, [0; 4]).expect("composes");
                wait(&device);
            }),
        );
        row(
            "4K today: upload the whole 3840 × 2160 frame",
            time(|| {
                queue.write_texture(
                    target4.as_image_copy(),
                    full4.data(),
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(w4 * 4),
                        rows_per_image: Some(h4),
                    },
                    target4.size(),
                );
                queue.submit([]);
                wait(&device);
            }),
        );
        row(
            &format!("4K pan, all tiles resident ({} placed)", pan4_p.len()),
            time(|| {
                cache.compose(&target4, &pan4_p, [0; 4]).expect("composes");
                wait(&device);
            }),
        );
        // Option (b)'s floor: a GPU compositor fed CPU coverage has to
        // upload the coverage. 64 MiB of A8 is about half of what a
        // gradient-heavy corpus frame composites (1.4 x 10^8 px).
        let side = 8192;
        let masks = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("coverage upload probe"),
            size: wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let bytes = vec![0x80u8; (side * side) as usize];
        row(
            "option (b) floor: upload 64 MiB of A8 coverage",
            time(|| {
                queue.write_texture(
                    masks.as_image_copy(),
                    &bytes,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(side),
                        rows_per_image: Some(side),
                    },
                    masks.size(),
                );
                queue.submit([]);
                wait(&device);
            }),
        );
    }
}
