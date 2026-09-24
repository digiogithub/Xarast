//! The Phase 4 W0 rasteriser spike, throughput half (gates G1 and G2).
//!
//! `docs/phases/phase-04-render-engine.md` §W0 defines the `bulk` scene and
//! the two throughput gates:
//!
//! - **G1** — `bulk` at 1920 × 1080 in ≤ 120 ms on one thread and ≤ 25 ms
//!   across eight cores, on `vello_cpu`.
//! - **G2** — `bulk` at 1920 × 1080 in ≤ 8 ms on the reference GPU, on
//!   `vello`.
//!
//! The original harness never entered the repository (see
//! `docs/memory/render.md`); this is its throughput half, rebuilt so the
//! numbers can be reproduced on the reference machine. It talks to the
//! rasterisers directly rather than through our display list and
//! compositor, because the gates price the *rasteriser*: the production
//! path is priced by `benches/render.rs`.
//!
//! This is a `harness = false` bench with its own protocol instead of
//! `criterion`, because what it has to report is a table over thread counts
//! and adapters, and a GPU frame is only meaningful once the queue has
//! drained. Protocol: three warm-up frames, then `SPIKE_FRAMES` (default
//! 30) timed frames; median, minimum and maximum are printed. A CPU frame
//! includes scene recording (paths are pre-built, as a display list would
//! hold them); a GPU frame is timed from submission to the completion of
//! `device.poll(wait)`, which is an upper bound on the GPU time a timestamp
//! query would report.
//!
//! ```text
//! cargo bench -p xarast-render --bench spike                        # CPU (G1)
//! cargo bench -p xarast-render --bench spike --features spike-gpu   # + GPU (G2)
//! SPIKE_THREADS=0,7,23 SPIKE_FRAMES=50 cargo bench ...              # worker counts
//! ```
//!
//! `SPIKE_THREADS` lists `vello_cpu` *worker* counts: `0` is the
//! single-threaded dispatcher, `n > 0` is `n` workers plus the recording
//! thread. Pin the process with `taskset` to price a given set of cores.

use std::time::{Duration, Instant};

use vello_cpu::color::{AlphaColor, Srgb};
use vello_cpu::kurbo::{BezPath, Point};
use vello_cpu::{Level, Pixmap, RenderContext, RenderSettings, Resources};

/// Frame size the gates are written for.
const W: u16 = 1920;
const H: u16 = 1080;
/// Object count the gates are written for.
const COUNT: usize = 100_000;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// One filled object of the `bulk` scene: a twelve-segment closed polygon
/// with a slightly irregular radius, and its colour.
struct Obj {
    path: BezPath,
    rgba: [u8; 4],
}

/// The two `bulk` variants the original spike measured. The phase does not
/// fix the object size and the overdraw factor dominates the result, so
/// both are kept: `heavy` (radius 3..28 px, ≈ ×27 overdraw) is the one the
/// gate verdict was recorded against, `light` (2..8 px, ≈ ×2.6) is the one
/// `benches/render.rs` uses.
#[derive(Clone, Copy)]
enum Variant {
    Heavy,
    Light,
}

impl Variant {
    fn name(self) -> &'static str {
        match self {
            Variant::Heavy => "bulk r3..28 px",
            Variant::Light => "bulk r2..8 px",
        }
    }

    fn radius(self, u: f64) -> f64 {
        match self {
            Variant::Heavy => 3.0 + u * 25.0,
            Variant::Light => 2.0 + u * 6.0,
        }
    }
}

fn bulk(v: Variant) -> Vec<Obj> {
    let mut rng = Rng(0xdead_beef);
    (0..COUNT)
        .map(|i| {
            let cx = rng.next() * f64::from(W);
            let cy = rng.next() * f64::from(H);
            let r = v.radius(rng.next());
            let mut path = BezPath::new();
            for k in 0..12 {
                let a = std::f64::consts::TAU * f64::from(k) / 12.0;
                let rr = r * (0.6 + rng.next() * 0.4);
                let p = Point::new(cx + rr * a.cos(), cy + rr * a.sin());
                if k == 0 {
                    path.move_to(p);
                } else {
                    path.line_to(p);
                }
            }
            path.close_path();
            let rgba = [(i % 251) as u8, (i % 241) as u8, 180, 255];
            Obj { path, rgba }
        })
        .collect()
}

fn colour(rgba: [u8; 4]) -> AlphaColor<Srgb> {
    AlphaColor::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3])
}

/// Median, minimum and maximum of a sample, in milliseconds.
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
    std::env::var("SPIKE_FRAMES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30)
        .max(1)
}

fn row(what: &str, s: Stats) {
    println!("| {what} | {:.2} | {:.2} | {:.2} |", s.median, s.min, s.max);
}

/// Renders the scene once with `vello_cpu`, recording included.
fn cpu_frame(ctx: &mut RenderContext, objs: &[Obj], px: &mut Pixmap, res: &mut Resources) {
    ctx.reset();
    for o in objs {
        ctx.set_paint(colour(o.rgba));
        ctx.fill_path(&o.path);
    }
    ctx.flush();
    ctx.render(&mut *px, res);
}

/// Times `vello_cpu` over `bulk` with the given worker count. Returns the
/// last frame so that the GPU candidates can be checked against it.
fn cpu(objs: &[Obj], workers: u16, split: bool) -> (Stats, Option<(Stats, Stats)>, Pixmap) {
    let settings = RenderSettings {
        level: Level::try_detect().unwrap_or(Level::baseline()),
        num_threads: workers,
    };
    let mut ctx = RenderContext::new_with(W, H, settings);
    let mut px = Pixmap::new(W, H);
    let mut res = Resources::new();
    for _ in 0..3 {
        cpu_frame(&mut ctx, objs, &mut px, &mut res);
    }
    let n = frames();
    let mut total = Vec::with_capacity(n);
    let mut rec = Vec::with_capacity(n);
    let mut ras = Vec::with_capacity(n);
    for _ in 0..n {
        let t0 = Instant::now();
        ctx.reset();
        for o in objs {
            ctx.set_paint(colour(o.rgba));
            ctx.fill_path(&o.path);
        }
        ctx.flush();
        let t1 = Instant::now();
        ctx.render(&mut px, &mut res);
        let t2 = Instant::now();
        total.push(t2 - t0);
        rec.push(t1 - t0);
        ras.push(t2 - t1);
    }
    // With workers, "record" includes the strip generation the workers do
    // concurrently up to `flush`; the split is only honest single-threaded.
    let split = split.then(|| (stats(rec), stats(ras)));
    (stats(total), split, px)
}

fn threads() -> Vec<u16> {
    std::env::var("SPIKE_THREADS")
        .ok()
        .map(|s| s.split(',').filter_map(|t| t.trim().parse().ok()).collect())
        .unwrap_or_else(|| vec![0, 7, 23])
}

fn main() {
    // `cargo bench` passes `--bench`; a filter argument, if any, is ignored.
    let cores = std::thread::available_parallelism().map_or(0, std::num::NonZero::get);
    println!(
        "# W0 spike, throughput — {W} × {H}, {COUNT} objects, {} frames",
        frames()
    );
    println!("# available parallelism: {cores}");
    for v in [Variant::Heavy, Variant::Light] {
        let objs = bulk(v);
        println!();
        println!(
            "## {} — vello_cpu 0.2, u8 pipeline, detected SIMD",
            v.name()
        );
        println!();
        println!("| configuration | median ms | min ms | max ms |");
        println!("|---|---|---|---|");
        let mut reference = None;
        for w in threads() {
            let (s, split, px) = cpu(&objs, w, w == 0);
            let label = if w == 0 {
                "1 thread".to_owned()
            } else {
                format!("{w} workers + 1")
            };
            row(&label, s);
            if let Some((r, f)) = split {
                row("  of which: record + flush", r);
                row("  of which: rasterise", f);
            }
            reference.get_or_insert(px);
        }
        #[cfg(feature = "spike-gpu")]
        if let Some(px) = reference {
            gpu::run(&objs, &px);
        }
        #[cfg(not(feature = "spike-gpu"))]
        drop(reference);
    }
}

#[cfg(feature = "spike-gpu")]
mod gpu {
    //! Candidates B (`vello` 0.10) and C (`vello_hybrid` 0.2), on every
    //! hardware adapter the machine exposes.

    use super::{H, Obj, Stats, W, colour, frames, row, stats};
    use std::time::Instant;
    use vello::wgpu;
    use vello_cpu::Pixmap;

    fn block<F: std::future::Future>(f: F) -> F::Output {
        pollster::block_on(f)
    }

    struct Target {
        texture: wgpu::Texture,
        view: wgpu::TextureView,
        readback: wgpu::Buffer,
        bytes_per_row: u32,
    }

    fn target(device: &wgpu::Device, storage: bool) -> Target {
        let usage = if storage {
            wgpu::TextureUsages::STORAGE_BINDING
        } else {
            wgpu::TextureUsages::RENDER_ATTACHMENT
        } | wgpu::TextureUsages::COPY_SRC;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("spike target"),
            size: wgpu::Extent3d {
                width: u32::from(W),
                height: u32::from(H),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bytes_per_row = (u32::from(W) * 4).next_multiple_of(256);
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("spike readback"),
            size: u64::from(bytes_per_row) * u64::from(H),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Target {
            texture,
            view,
            readback,
            bytes_per_row,
        }
    }

    /// The reference frame as straight-alpha RGBA bytes.
    fn straight(px: &Pixmap) -> Vec<u8> {
        px.clone()
            .take_unpremultiplied()
            .into_iter()
            .flat_map(|c| [c.r, c.g, c.b, c.a])
            .collect()
    }

    fn wait(device: &wgpu::Device) {
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("device poll");
    }

    /// Copies the target back and returns the mean absolute channel
    /// difference against the `vello_cpu` frame. `vello` writes straight
    /// (unpremultiplied) colour and `vello_hybrid` premultiplied, so the
    /// caller passes the reference in the matching form; comparing the wrong
    /// pair reads as a large error on every antialiased edge.
    fn diff(device: &wgpu::Device, queue: &wgpu::Queue, t: &Target, reference: &[u8]) -> f64 {
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &t.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &t.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(t.bytes_per_row),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width: u32::from(W),
                height: u32::from(H),
                depth_or_array_layers: 1,
            },
        );
        queue.submit([enc.finish()]);
        t.readback.slice(..).map_async(wgpu::MapMode::Read, |r| {
            r.expect("map readback");
        });
        wait(device);
        let row_bytes = usize::from(W) * 4;
        let mut sum = 0u64;
        {
            let data = t.readback.slice(..).get_mapped_range();
            for (y, row) in data.chunks_exact(t.bytes_per_row as usize).enumerate() {
                let want = &reference[y * row_bytes..(y + 1) * row_bytes];
                for (a, b) in row[..row_bytes].iter().zip(want) {
                    sum += u64::from(a.abs_diff(*b));
                }
            }
        }
        t.readback.unmap();
        sum as f64 / (row_bytes * usize::from(H)) as f64
    }

    fn vello_full(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        objs: &[Obj],
        reference: &Pixmap,
    ) -> Result<(), String> {
        let mut renderer = vello::Renderer::new(
            device,
            vello::RendererOptions {
                antialiasing_support: vello::AaSupport::area_only(),
                ..Default::default()
            },
        )
        .map_err(|e| e.to_string())?;
        let t = target(device, true);
        // Converted outside the timer: the conversion is an artefact of the
        // two `kurbo` releases, not a cost the renderer imposes.
        let paths: Vec<vello::kurbo::BezPath> = objs.iter().map(|o| kpath(&o.path)).collect();
        let t_enc = Instant::now();
        let mut scene = vello::Scene::new();
        for (o, p) in objs.iter().zip(&paths) {
            let c = vello::peniko::Color::from_rgba8(o.rgba[0], o.rgba[1], o.rgba[2], o.rgba[3]);
            scene.fill(
                vello::peniko::Fill::NonZero,
                vello::kurbo::Affine::IDENTITY,
                c,
                None,
                p,
            );
        }
        let encode = t_enc.elapsed();
        let params = vello::RenderParams {
            base_color: vello::peniko::Color::TRANSPARENT,
            width: u32::from(W),
            height: u32::from(H),
            antialiasing_method: vello::AaConfig::Area,
        };
        let mut frame = || -> Result<(), String> {
            renderer
                .render_to_texture(device, queue, &scene, &t.view, &params)
                .map_err(|e| e.to_string())?;
            wait(device);
            Ok(())
        };
        for _ in 0..3 {
            frame()?;
        }
        let mut xs = Vec::with_capacity(frames());
        for _ in 0..frames() {
            let t0 = Instant::now();
            frame()?;
            xs.push(t0.elapsed());
        }
        let s: Stats = stats(xs);
        row("vello 0.10, area AA, retained scene: render + wait", s);
        println!(
            "| vello 0.10: scene encoding (once, CPU) | {:.2} | | |",
            encode.as_secs_f64() * 1e3
        );
        println!(
            "| vello 0.10: mean abs channel diff vs vello_cpu | {:.3} | | |",
            diff(device, queue, &t, &straight(reference))
        );
        Ok(())
    }

    /// `vello` and `vello_cpu` link different `kurbo` releases; a path is
    /// rebuilt element by element rather than assumed to be the same type.
    fn kpath(p: &vello_cpu::kurbo::BezPath) -> vello::kurbo::BezPath {
        use vello::kurbo::{PathEl as B, Point};
        use vello_cpu::kurbo::PathEl as A;
        let pt = |p: vello_cpu::kurbo::Point| Point::new(p.x, p.y);
        p.elements()
            .iter()
            .map(|el| match *el {
                A::MoveTo(a) => B::MoveTo(pt(a)),
                A::LineTo(a) => B::LineTo(pt(a)),
                A::QuadTo(a, b) => B::QuadTo(pt(a), pt(b)),
                A::CurveTo(a, b, c) => B::CurveTo(pt(a), pt(b), pt(c)),
                A::ClosePath => B::ClosePath,
            })
            .collect()
    }

    fn hybrid(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        objs: &[Obj],
        reference: &Pixmap,
    ) -> Result<(), String> {
        let t = target(device, false);
        let (mut renderer, mut resources) = vello_hybrid::Renderer::new(
            device,
            &vello_hybrid::RenderTargetConfig {
                format: wgpu::TextureFormat::Rgba8Unorm,
                width: u32::from(W),
                height: u32::from(H),
            },
        );
        let size = vello_hybrid::RenderSize {
            width: u32::from(W),
            height: u32::from(H),
        };
        let bindings = vello_hybrid::TextureBindings::new();
        let mut scene = vello_hybrid::Scene::new(W, H);
        let record = |scene: &mut vello_hybrid::Scene| {
            scene.reset();
            for o in objs {
                scene.set_paint(colour(o.rgba));
                scene.fill_path(&o.path);
            }
        };
        let gpu = |scene: &vello_hybrid::Scene,
                   renderer: &mut vello_hybrid::Renderer,
                   resources: &mut vello_hybrid::Resources|
         -> Result<(), String> {
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            renderer
                .render(
                    scene, resources, device, queue, &mut enc, &size, &t.view, &bindings,
                )
                .map_err(|e| format!("{e:?}"))?;
            queue.submit([enc.finish()]);
            wait(device);
            Ok(())
        };
        for _ in 0..3 {
            record(&mut scene);
            gpu(&scene, &mut renderer, &mut resources)?;
        }
        let n = frames();
        let (mut tot, mut rec, mut ras) = (Vec::new(), Vec::new(), Vec::new());
        for _ in 0..n {
            let t0 = Instant::now();
            record(&mut scene);
            let t1 = Instant::now();
            gpu(&scene, &mut renderer, &mut resources)?;
            let t2 = Instant::now();
            tot.push(t2 - t0);
            rec.push(t1 - t0);
            ras.push(t2 - t1);
        }
        row("vello_hybrid 0.2: record + render + wait", stats(tot));
        row("  of which: record (CPU strips)", stats(rec));
        row("  of which: render + wait", stats(ras));
        println!(
            "| vello_hybrid 0.2: mean abs channel diff vs vello_cpu | {:.3} | | |",
            diff(device, queue, &t, reference.data_as_u8_slice())
        );
        Ok(())
    }

    pub fn run(objs: &[Obj], reference: &Pixmap) {
        // The machine-wide GPU lock, held until every device is gone.
        let Some(_gpu) = xarast_render::gpu_test_lock::acquire("bench spike") else {
            return;
        };
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..wgpu::InstanceDescriptor::new_without_display_handle_from_env()
        });
        let adapters = block(instance.enumerate_adapters(wgpu::Backends::VULKAN));
        for adapter in adapters {
            let info = adapter.get_info();
            if info.device_type == wgpu::DeviceType::Cpu {
                continue;
            }
            println!();
            println!(
                "### {} ({:?}, {} {})",
                info.name, info.device_type, info.driver, info.driver_info
            );
            println!();
            println!("| configuration | median ms | min ms | max ms |");
            println!("|---|---|---|---|");
            let (device, queue) = match block(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("spike"),
                required_limits: adapter.limits(),
                ..Default::default()
            })) {
                Ok(dq) => dq,
                Err(e) => {
                    println!("| device request failed: {e} | | | |");
                    continue;
                }
            };
            if let Err(e) = vello_full(&device, &queue, objs, reference) {
                println!("| vello 0.10 failed: {e} | | | |");
            }
            if let Err(e) = hybrid(&device, &queue, objs, reference) {
                println!("| vello_hybrid 0.2 failed: {e} | | | |");
            }
        }
    }
}
