//! The canvas path of a pan or zoom, offscreen, at any size (XARA-T-0050).
//!
//! The live probe (`xarast --probe pan|zoom`) measures the real window,
//! but a tiling compositor decides the window's size, so it cannot show a
//! 4K canvas on a smaller monitor. This drives the same pieces without a
//! window: the session, the Draft → Final scheduler and render thread,
//! the shell's tile planner and a tile store, composited into a canvas-sized
//! texture and waited for on the GPU. What it leaves out is the interface
//! frame and the swapchain present (together ~0.3 ms in the live probe).
//!
//! ```text
//! WGPU_ADAPTER_NAME=intel cargo run --release -p xarast-shell --example canvas_probe -- \
//!     --size 3840x2160 --tier gpu --kind pan --synthetic 250000
//! ```
//!
//! One step per 8 ms (input at 120 Hz): apply the intent, pump the
//! scheduler, keep whatever frame the render thread delivered, composite
//! the current view, submit and wait. The sample is the step's time.

use std::sync::Arc;
use std::time::{Duration, Instant};

use xarast_app::schedule::{Backdrop, Canvas};
use xarast_app::{DevicePoint, DeviceSize, DocumentId, Intent, Session, ZoomTarget};
use xarast_render::{GpuTileCache, GpuTileCacheConfig, Surface};
use xarast_shell::tiles::{
    CanvasView, CpuTileStore, GpuTileStore, TilePlanner, TileStore, TiledFrame, capacity_for,
};

const BACKDROP: Backdrop = Backdrop {
    pasteboard: [0x80, 0x80, 0x84, 0xff],
    page: [0xff; 4],
};

struct Args {
    size: (u32, u32),
    gpu: bool,
    zoom: bool,
    synthetic: Option<usize>,
    file: Option<std::path::PathBuf>,
    samples: usize,
}

fn args() -> Args {
    let mut a = Args {
        size: (1920, 1080),
        gpu: true,
        zoom: false,
        synthetic: None,
        file: None,
        samples: 300,
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut v = || it.next().unwrap_or_default();
        match k.as_str() {
            "--size" => {
                let s = v();
                let (w, h) = s.split_once('x').expect("--size WxH");
                a.size = (w.parse().expect("width"), h.parse().expect("height"));
            }
            "--tier" => a.gpu = v() != "cpu",
            "--kind" => a.zoom = v() == "zoom",
            "--synthetic" => a.synthetic = v().parse().ok(),
            "--samples" => a.samples = v().parse().expect("samples"),
            f => a.file = Some(f.into()),
        }
    }
    a
}

fn device() -> (wgpu::Adapter, Arc<wgpu::Device>, Arc<wgpu::Queue>) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let want = std::env::var("WGPU_ADAPTER_NAME")
        .unwrap_or_default()
        .to_lowercase();
    let adapter = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::VULKAN))
        .into_iter()
        .find(|a| a.get_info().name.to_lowercase().contains(&want))
        .expect("an adapter matching WGPU_ADAPTER_NAME");
    let (d, q) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("a device");
    (adapter, Arc::new(d), Arc::new(q))
}

fn percentile(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let i = ((v.len() - 1) as f64 * p).round() as usize;
    v[i]
}

fn main() {
    let a = args();
    let (w, h) = a.size;
    let mut session = if let Some(path) = &a.file {
        Session::open(DocumentId(1), path).expect("open")
    } else {
        let doc = xarast_doc::synthetic_document(xarast_doc::SynthSpec {
            nodes: a.synthetic.unwrap_or(250_000),
            ..xarast_doc::SynthSpec::default()
        });
        Session::adopt(DocumentId(1), doc, None)
    };
    session
        .apply(Intent::Resize(DeviceSize::new(w, h)))
        .unwrap();
    session.apply(Intent::ZoomTo(ZoomTarget::Page)).unwrap();

    let (adapter, device, queue) = device();
    let ts = xarast_render::GPU_TILE_SIZE;
    let capacity = capacity_for(w, h, ts);
    let mut gpu = a.gpu.then(|| GpuTileStore {
        cache: GpuTileCache::new(
            device.clone(),
            queue.clone(),
            GpuTileCacheConfig {
                tile_size: ts,
                capacity,
            },
        )
        .expect("tile cache"),
    });
    let mut cpu = CpuTileStore::new(ts, capacity);
    let mut planner = TilePlanner::new(ts);
    let target = xarast_render::backend::gpu_tiles::create_target(&device, w, h);
    let mut scratch = Surface::new(w, h);

    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let tx = std::sync::Mutex::new(tx);
    let mut canvas = Canvas::spawn(
        Box::new(move || {
            let _ = tx.lock().map(|t| t.send(()));
        }),
        BACKDROP,
    )
    .expect("render thread");
    canvas.set_cpu_rescale(false);

    // The first frame, Final, before measuring.
    canvas.pump(Instant::now(), &mut session).unwrap();
    let first = loop {
        rx.recv_timeout(Duration::from_secs(120))
            .expect("first frame");
        if let Some(f) = canvas.take_latest() {
            break f;
        }
    };
    let keep = |planner: &mut TilePlanner,
                gpu: &mut Option<GpuTileStore>,
                cpu: &mut CpuTileStore,
                f: xarast_app::RenderedFrame|
     -> u64 {
        let tf = TiledFrame {
            surface: f.surface,
            transform: f.view.transform,
            content: (f.doc.0, f.scene_epoch),
            generation: f.generation,
            base: f.base,
            covered: f.covered,
            fresh: f.fresh,
        };
        let store: &mut dyn TileStore = match gpu.as_mut() {
            Some(g) => g,
            None => cpu,
        };
        planner.accept(&tf, store).pixels
    };
    let full = keep(&mut planner, &mut gpu, &mut cpu, first);
    eprintln!("first frame: {full} pixels uploaded");

    let mut samples = Vec::new();
    let mut uploaded = 0_u64;
    let mut delivered = 0_u32;
    let period = Duration::from_millis(8);
    for step in 0..(a.samples + 10) {
        let tick = Instant::now();
        let intent = if a.zoom {
            let f = if (step / 10) % 2 == 0 {
                1.05
            } else {
                1.0 / 1.05
            };
            Intent::Zoom {
                factor: f,
                anchor: DevicePoint::new(f64::from(w) / 2.0, f64::from(h) / 2.0),
            }
        } else {
            let dir = if (step / 60) % 2 == 0 { 1.0 } else { -1.0 };
            Intent::Pan {
                dx: 23.4 * dir,
                dy: 7.7 * dir,
            }
        };
        let changed = session.apply(intent).unwrap();
        canvas.note(tick, changed);
        canvas.pump(tick, &mut session).unwrap();
        if let Some(f) = canvas.take_latest() {
            delivered += 1;
            uploaded += keep(&mut planner, &mut gpu, &mut cpu, f);
        }
        let view = CanvasView {
            origin: (0, 0),
            width: w,
            height: h,
            transform: session.viewport.transform(),
            backdrop: BACKDROP.pasteboard,
        };
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        if let Some(g) = gpu.as_mut() {
            let placements = planner.placements(&view, &*g);
            g.cache
                .encode(&mut enc, &target, &placements, view.backdrop)
                .unwrap();
        } else {
            let placements = planner.placements(&view, &cpu);
            cpu.compose(&mut scratch, &placements, view.backdrop);
            queue.write_texture(
                target.as_image_copy(),
                scratch.data(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w * 4),
                    rows_per_image: Some(h),
                },
                target.size(),
            );
        }
        queue.submit([enc.finish()]);
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll");
        let took = tick.elapsed();
        if step >= 10 {
            samples.push(took.as_secs_f64() * 1e3);
        }
        if let Some(rest) = period.checked_sub(took) {
            std::thread::sleep(rest);
        }
    }
    let n = samples.len();
    println!(
        "canvas_probe {} {} {}x{} on {}: {n} steps, {delivered} render-thread frames, {:.1} MB uploaded after the first frame",
        if a.gpu { "GPU tiles" } else { "CPU" },
        if a.zoom { "zoom" } else { "pan" },
        w,
        h,
        adapter.get_info().name,
        uploaded as f64 * 4.0 / 1e6,
    );
    println!(
        "  input -> composited, GPU idle: p50 {:.2} ms, p95 {:.2} ms, p99 {:.2} ms, max {:.2} ms",
        percentile(&mut samples, 0.5),
        percentile(&mut samples, 0.95),
        percentile(&mut samples, 0.99),
        percentile(&mut samples, 1.0),
    );
    println!("  render thread: {:?}", canvas.render_thread().stats());
    canvas.shutdown();
}
