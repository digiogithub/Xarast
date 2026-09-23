//! Pan and zoom through the Draft → Final scheduler over 100 000 objects.
//!
//! `cargo bench -p xarast-app -- viewport`
//!
//! Every figure is the latency of one canvas frame as the window sees it:
//! the main-thread side (`Session::apply`, `Canvas::note`, `Canvas::pump`)
//! plus the render thread producing the frame, measured from the intent to
//! the frame being collected. The budget is 16 ms per pan or zoom frame
//! (`docs/memory/perf.md`).
//!
//! * `pan_draft` — a scripted drag, 1920 × 1080: the render thread
//!   scrolls the last frame and rasterises the exposed strips.
//! * `zoom_draft` — a scripted wheel zoom, alternately in and out by one
//!   notch: the render thread resamples the last frame and paints the
//!   backdrop into the border a zoom-out uncovers.
//! * `final_after_idle` — the upgrade 120 ms after the gesture: a full
//!   `Final` frame, off the interactive path.
//! * `full_draft` — a full `Draft` frame with no reuse, for reference: what
//!   a pan frame would cost without pixel reuse.

use std::hint::black_box;
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use xarast_app::schedule::{Backdrop, Canvas, QualityScheduler};
use xarast_app::{DevicePoint, DeviceSize, DocumentId, Intent, RenderThread, Session, ZoomTarget};
use xarast_doc::{SynthSpec, synthetic_document};
use xarast_render::RenderQuality;

const W: u32 = 1920;
const H: u32 = 1080;
const BACKDROP: Backdrop = Backdrop {
    pasteboard: [0x80, 0x80, 0x84, 0xff],
    page: [0xff; 4],
};
const T: Duration = Duration::from_secs(60);

/// The synthetic document at about 100 000 objects: 250 000 nodes give
/// 105 852 paths, each filled and stroked, so 224 218 primitives. Framed
/// on its page, then zoomed by `zoom` about the middle.
///
/// Two views are measured. `fit` is the whole page: the drawing fills
/// the height, and strips at the sides fall on the pasteboard and are
/// skipped. `zoomed` is 3× that: every pixel of the viewport is over the
/// drawing, so every exposed strip has ink in it — the worst case for
/// reuse.
fn session(zoom: f64) -> Session {
    let doc = synthetic_document(SynthSpec {
        nodes: 250_000,
        ..SynthSpec::default()
    });
    let mut s = Session::adopt(DocumentId(1), doc, None);
    s.apply(Intent::Resize(DeviceSize::new(W, H))).unwrap();
    s.apply(Intent::ZoomTo(ZoomTarget::Page)).unwrap();
    s.apply(Intent::Zoom {
        factor: zoom,
        anchor: DevicePoint::new(f64::from(W) / 2.0, f64::from(H) / 2.0),
    })
    .unwrap();
    let stats = s.rebuild_scene(None).unwrap();
    eprintln!(
        "viewport bench: {} primitives ({} fills, {} strokes), {W}x{H}",
        stats.primitives(),
        stats.fills,
        stats.strokes
    );
    s
}

fn canvas() -> (Canvas, mpsc::Receiver<()>) {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let rt = RenderThread::spawn(Box::new(move || {
        let _ = tx.lock().map(|t| t.send(()));
    }))
    .unwrap();
    (
        Canvas::new(rt, BACKDROP).with_scheduler(QualityScheduler::default()),
        rx,
    )
}

/// Applies `intent` at `now`, pumps, and waits for the frame.
fn step(
    s: &mut Session,
    c: &mut Canvas,
    woken: &mpsc::Receiver<()>,
    now: Instant,
    intent: Option<Intent>,
) -> xarast_app::RenderedFrame {
    if let Some(i) = intent {
        let changed = s.apply(i).unwrap();
        c.note(now, changed);
    }
    c.pump(now, s).unwrap();
    woken.recv_timeout(T).expect("a frame");
    c.take_latest().expect("a frame after the wake")
}

/// Where a kind of frame spends its time on the render thread.
#[derive(Default)]
struct Tally {
    frames: u64,
    build_us: u64,
    raster_us: u64,
    with_raster: u64,
}

impl Tally {
    fn add(&mut self, f: &xarast_app::RenderedFrame) {
        self.frames += 1;
        self.build_us += u64::from(f.timings.build_us);
        self.raster_us += u64::from(f.timings.raster_us);
        self.with_raster += u64::from(f.timings.rasterised_pixels > 0);
    }

    fn report(&self, name: &str) {
        let n = self.frames.max(1);
        eprintln!(
            "viewport bench: {name}: {} frames, mean list build {:.2} ms, mean raster {:.2} ms, {} frames rasterised anything",
            self.frames,
            self.build_us as f64 / n as f64 / 1000.0,
            self.raster_us as f64 / n as f64 / 1000.0,
            self.with_raster
        );
    }
}

fn bench(cr: &mut Criterion) {
    scenario(cr, "fit", 1.0);
    scenario(cr, "zoomed", 3.0);
}

fn scenario(cr: &mut Criterion, name: &str, zoom_by: f64) {
    let mut s = session(zoom_by);
    let (mut c, woken) = canvas();
    let t0 = Instant::now();
    // The first frame of a document is a full Final.
    step(&mut s, &mut c, &woken, t0, None);

    let mut g = cr.benchmark_group(format!("viewport/{name}"));
    g.sample_size(20);

    // A drag: 9 px right and 5 px up per frame, back and forth so the view
    // stays on the drawing.
    let mut clock = t0;
    let mut i = 0u64;
    let mut pan = Tally::default();
    g.bench_function("pan_draft", |b| {
        let tally = &mut pan;
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                i += 1;
                clock += Duration::from_millis(16);
                let sign = if (i / 20).is_multiple_of(2) {
                    1.0
                } else {
                    -1.0
                };
                let start = Instant::now();
                let f = step(
                    &mut s,
                    &mut c,
                    &woken,
                    clock,
                    Some(Intent::Pan {
                        dx: 9.0 * sign,
                        dy: -5.0 * sign,
                    }),
                );
                total += start.elapsed();
                assert_eq!(f.view.quality, RenderQuality::Draft);
                tally.add(&f);
                black_box(f);
            }
            total
        });
    });

    // A wheel: one notch in, one notch out, about the middle.
    let mid = DevicePoint::new(f64::from(W) / 2.0, f64::from(H) / 2.0);
    let mut zoom = Tally::default();
    g.bench_function("zoom_draft", |b| {
        let tally = &mut zoom;
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                i += 1;
                clock += Duration::from_millis(16);
                let factor = if i.is_multiple_of(2) { 1.25 } else { 0.8 };
                let start = Instant::now();
                let f = step(
                    &mut s,
                    &mut c,
                    &woken,
                    clock,
                    Some(Intent::Zoom {
                        factor,
                        anchor: mid,
                    }),
                );
                total += start.elapsed();
                assert_eq!(f.view.quality, RenderQuality::Draft);
                tally.add(&f);
                black_box(f);
            }
            total
        });
    });

    // The upgrade: one Draft pan, then the Final 120 ms later.
    g.sample_size(10);
    let mut fin = Tally::default();
    g.bench_function("final_after_idle", |b| {
        let tally = &mut fin;
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                i += 1;
                clock += Duration::from_millis(16);
                let sign = if i.is_multiple_of(2) { 1.0 } else { -1.0 };
                step(
                    &mut s,
                    &mut c,
                    &woken,
                    clock,
                    Some(Intent::Pan {
                        dx: 3.0 * sign,
                        dy: 0.0,
                    }),
                );
                clock += Duration::from_millis(120);
                let start = Instant::now();
                let f = step(&mut s, &mut c, &woken, clock, None);
                total += start.elapsed();
                assert_eq!(f.view.quality, RenderQuality::Final);
                assert!(f.exact);
                tally.add(&f);
                black_box(f);
            }
            total
        });
    });

    // For reference: a Draft frame with nothing to reuse. A fresh scene
    // epoch on every job defeats reuse without changing the pixels.
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let mut rt = RenderThread::spawn(Box::new(move || {
        let _ = tx.lock().map(|t| t.send(()));
    }))
    .unwrap();
    let mut epoch = 1_000_000;
    g.bench_function("full_draft", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let mut job = s.frame_job(BACKDROP.pasteboard, BACKDROP.page);
                job.view.quality = RenderQuality::Draft;
                epoch += 1;
                job.scene_epoch = epoch;
                let start = Instant::now();
                rt.submit(job);
                rx.recv_timeout(T).expect("a frame");
                total += start.elapsed();
                black_box(rt.take_latest());
            }
            total
        });
    });
    g.finish();

    pan.report(&format!("{name}/pan_draft"));
    zoom.report(&format!("{name}/zoom_draft"));
    fin.report(&format!("{name}/final_after_idle"));
    let st = c.render_thread().stats();
    eprintln!(
        "viewport bench: {name}: scheduler frames {} rendered, {} scrolled, {} rescaled, {} aborted",
        st.rendered, st.scrolled, st.rescaled, st.aborted
    );
}

criterion_group!(benches, bench);
criterion_main!(benches);
