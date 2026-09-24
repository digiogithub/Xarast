//! Releasing a photo panel slider on a large photograph (XARA-T-0304):
//! the walk registers the committed chain's image **deferred** — it
//! evaluates nothing at full resolution — and the render thread draws a
//! stand-in, has its helper make the image and its pyramid, and repaints
//! the picture's damage. The settled frame is the full-resolution
//! evaluation, byte for byte, and an undo or redo while the helper works
//! settles on the right picture.
//!
//! No wall clock is asserted here: the release frame's time is the perf
//! gate `photo-release-24mpx` (`xarast-cli bench photo`). The tests print
//! what they measured (`-- --nocapture`).

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use xarast_app::photo_panel::PhotoPanelOp;
use xarast_app::render_thread::{FrameJob, FrameReuse, RenderThread, RenderedFrame};
use xarast_app::viewport::ZoomTarget;
use xarast_app::walker::SceneWalker;
use xarast_app::{AppState, DeviceSize, Intent, SelectMode, Session};
use xarast_doc::photo::{PhotoOp, PhotoOps};
use xarast_doc::resources::{BitmapData, BitmapInfo, BitmapResource};
use xarast_doc::{Command, EditError, NodeId, NodeKind, Tx};
use xarast_geom::{Matrix, Vector};
use xarast_render::{DeviceRect, RenderQuality};

const BACKGROUND: [u8; 4] = [40, 40, 40, 255];
const PAGE: [u8; 4] = [255, 255, 255, 255];

fn gradient(w: u32, h: u32, seed: u8) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            [(x * 255 / w) as u8, (y * 255 / h) as u8, seed, 255]
        })
        .collect()
}

fn native(w: u32, h: u32, rgba: Vec<u8>) -> BitmapResource {
    BitmapResource {
        name: Arc::from("photo"),
        info: BitmapInfo {
            width: w,
            height: h,
            bpp: 32,
            dpi_x: 96,
            dpi_y: 96,
        },
        pixels: Arc::new(BitmapData {
            pixels: Arc::from(rgba),
            palette: Arc::from(Vec::new()),
        }),
        original: None,
        procedural: None,
        transparent_index: None,
    }
}

#[derive(Debug)]
struct MoveBy(NodeId, Vector);

impl Command for MoveBy {
    fn label(&self) -> &'static str {
        "Move"
    }
    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        tx.transform(self.0, Matrix::translate(self.1))
    }
}

/// Replaces a bitmap object's node data (its image, its placement).
#[derive(Debug)]
struct Swap(NodeId, xarast_doc::BitmapNode);

impl Command for Swap {
    fn label(&self) -> &'static str {
        "Swap"
    }
    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        tx.set_kind(self.0, NodeKind::Bitmap(Box::new(self.1.clone())))
    }
}

fn bitmaps(s: &Session) -> Vec<NodeId> {
    s.doc
        .tree
        .preorder(s.doc.tree.root())
        .filter(|n| matches!(s.doc.tree.kind(*n), Some(NodeKind::Bitmap(_))))
        .collect()
}

fn bitmap(s: &Session, n: NodeId) -> xarast_doc::BitmapNode {
    let Some(NodeKind::Bitmap(b)) = s.doc.tree.kind(n) else {
        panic!("not a bitmap")
    };
    (**b).clone()
}

/// The chain a tone slider shows at step `i`.
fn slider(i: usize) -> PhotoOps {
    PhotoOps {
        ops: vec![
            PhotoOp::Brightness(-0.3 + 0.01 * i as f32),
            PhotoOp::Contrast(0.2),
            PhotoOp::Gamma(1.3),
            PhotoOp::Saturation(-0.4),
        ],
    }
}

/// A `w` × `h` photograph placed five times the size of a small second
/// picture to its left, both in a `view`, the photograph selected.
/// Returns the app, the photograph and the other picture.
fn photo_app(w: u32, h: u32, view: DeviceSize, zoom: f64) -> (AppState, NodeId, NodeId) {
    let mut app = AppState::new();
    app.new_document();
    app.apply(Intent::Resize(view)).unwrap();
    for seed in [9, 200] {
        app.apply(Intent::PasteImage {
            width: 60,
            height: 40,
            rgba: Arc::from(gradient(60, 40, seed)),
        })
        .unwrap();
    }
    let s = app.active_mut().unwrap();
    let [node, other] = bitmaps(s)[..] else {
        panic!("two bitmaps")
    };
    let bm = bitmap(s, node);
    let big = s
        .doc
        .resources
        .insert_bitmap(native(w, h, gradient(w, h, 90)));
    s.dispatch(&Swap(
        node,
        xarast_doc::BitmapNode {
            image: big,
            major: Vector::raw(bm.major.dx.0 * 5, bm.major.dy.0 * 5),
            minor: Vector::raw(bm.minor.dx.0 * 5, bm.minor.dy.0 * 5),
            ..bm
        },
    ))
    .unwrap();
    s.dispatch(&MoveBy(other, Vector::raw(-120_000, 0)))
        .unwrap();
    s.apply(Intent::ZoomTo(ZoomTarget::Drawing)).unwrap();
    let z = s.viewport.zoom();
    s.viewport.set_zoom(z * zoom);
    s.apply(Intent::Select {
        nodes: vec![node],
        mode: SelectMode::Replace,
    })
    .unwrap();
    (app, node, other)
}

/// How many images of the session's last scene are deferred and not
/// made yet.
fn pending(s: &Session) -> usize {
    s.resolver()
        .images
        .iter()
        .filter(|i| i.is_pending())
        .count()
}

/// The device box of `nodes` on the session's view.
fn device_box(s: &Session, nodes: &[NodeId]) -> (f64, f64, f64, f64) {
    let r = xarast_app::viewport::nodes_rect(&s.doc, nodes.iter().copied());
    let (a, b) = (
        s.viewport.doc_to_device(r.lo),
        s.viewport.doc_to_device(r.hi),
    );
    (a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y))
}

/// Whether `r` lies within the box, give or take the antialiasing pixel
/// and the repaint's guard.
fn inside(r: DeviceRect, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> bool {
    f64::from(r.x0) >= x0 - 3.0
        && f64::from(r.y0) >= y0 - 3.0
        && f64::from(r.x1) <= x1 + 3.0
        && f64::from(r.y1) <= y1 + 3.0
}

/// A render thread and the channel its wakes arrive on.
struct Thread {
    rt: RenderThread,
    rx: mpsc::Receiver<()>,
}

impl Thread {
    fn spawn() -> Thread {
        let (tx, rx) = mpsc::channel::<()>();
        let tx = Mutex::new(tx);
        let rt = RenderThread::spawn(Box::new(move || {
            let _ = tx.lock().map(|t| t.send(()));
        }))
        .expect("a render thread");
        Thread { rt, rx }
    }

    /// Every frame published until one of generation `≥ g` is exact.
    fn until_exact(&mut self, g: u64) -> Vec<RenderedFrame> {
        let mut out = Vec::new();
        loop {
            self.rx
                .recv_timeout(Duration::from_secs(120))
                .expect("a frame within two minutes");
            if let Some(f) = self.rt.take_latest() {
                let done = f.generation >= g && f.exact;
                out.push(f);
                if done {
                    return out;
                }
            }
        }
    }

    /// The first frame of generation `≥ g`.
    fn next(&mut self, g: u64) -> RenderedFrame {
        loop {
            self.rx
                .recv_timeout(Duration::from_secs(120))
                .expect("a frame within two minutes");
            if let Some(f) = self.rt.take_latest().filter(|f| f.generation >= g) {
                return f;
            }
        }
    }
}

fn job(s: &mut Session) -> FrameJob {
    let mut job = s.frame_job(BACKGROUND, PAGE);
    job.view.quality = RenderQuality::Final;
    job
}

/// The picture the session's document should settle on: its scene built
/// by a fresh walker — no deferral, no shared cache, every derived image
/// evaluated at full resolution on the walk — on a render thread of its
/// own.
fn reference(s: &mut Session) -> Vec<u8> {
    let mut walker = SceneWalker::new();
    let mut scene = xarast_render::Scene::new();
    walker
        .rebuild(
            &s.doc,
            &s.edit,
            &s.viewport,
            RenderQuality::Final,
            None,
            &mut scene,
        )
        .unwrap();
    assert_eq!(
        walker
            .resolver()
            .images
            .iter()
            .filter(|i| i.is_pending())
            .count(),
        0
    );
    let mut j = job(s);
    j.scene = Arc::new(scene);
    j.resolver = Arc::new(walker.into_resolver());
    let mut t = Thread::spawn();
    let g = t.rt.submit(j);
    let f = t.until_exact(g).pop().unwrap();
    t.rt.shutdown();
    f.surface.data().to_vec()
}

/// T10.6.5's release on a 24 Mpx photograph: the walk after the release
/// evaluates nothing at full resolution — the committed chain's image is
/// registered deferred, with the drag's last proxy as its stand-in — and
/// the walk after that still has nothing to make.
#[test]
fn a_release_on_a_24_mpx_photo_evaluates_nothing_on_the_walk() {
    let (mut app, node, _) = photo_app(6000, 4000, DeviceSize::new(1280, 800), 0.9);
    let s = app.active_mut().unwrap();
    let cache = s.decoded_images().clone();
    s.rebuild_scene(None).unwrap();
    let len = s.bus.history().len();
    for i in 0..10 {
        s.apply(Intent::PhotoPanel(PhotoPanelOp::Preview(slider(i))))
            .unwrap();
        s.rebuild_scene(None).unwrap();
    }
    let proxy = s.photo_proxies()[0];
    assert!(proxy.level >= 1, "{proxy:?}");

    let t = Instant::now();
    s.apply(Intent::PhotoPanel(PhotoPanelOp::Commit)).unwrap();
    s.rebuild_scene(None).unwrap();
    let release = t.elapsed();
    assert_eq!(s.bus.history().len(), len + 1);
    assert_eq!(bitmap(s, node).photo_ops, slider(9).normalised());
    assert!(s.photo_proxies().is_empty());
    // One derived image filed, and it is still to be made: the walk ran
    // no full-resolution evaluation and built no pyramid.
    assert_eq!(cache.stats().derived, 1, "{:?}", cache.stats());
    assert_eq!(pending(s), 1, "the committed image is deferred");
    // Another walk (a selection change, say) reuses it as it is.
    s.rebuild_scene(None).unwrap();
    assert_eq!(pending(s), 1);
    assert_eq!(cache.stats().derived, 1);
    println!(
        "24 Mpx release: intent + walk {:.1} ms (was ≈ 264 ms of evaluation and pyramid)",
        release.as_secs_f64() * 1e3
    );
}

/// The release through the render thread: the frame after it draws the
/// stand-in and is not exact; the helper makes the image; the repair
/// repaints the picture only; the settled frame equals a fresh walker's
/// full-resolution picture byte for byte.
#[test]
fn the_released_picture_converges_to_the_full_resolution_evaluation() {
    let (mut app, node, other) = photo_app(2400, 1600, DeviceSize::new(640, 480), 0.5);
    let s = app.active_mut().unwrap();
    let mut t = Thread::spawn();
    s.rebuild_scene(None).unwrap();
    let g = t.rt.submit(job(s));
    t.until_exact(g);
    let photo = device_box(s, &[node]);
    let beside = device_box(s, &[other]);
    assert!(beside.2 < photo.0, "the other picture is to the left");

    for i in 0..5 {
        s.apply(Intent::PhotoPanel(PhotoPanelOp::Preview(slider(i))))
            .unwrap();
        s.rebuild_scene(None).unwrap();
        let g = t.rt.submit(job(s));
        t.next(g);
    }
    let started = Instant::now();
    s.apply(Intent::PhotoPanel(PhotoPanelOp::Commit)).unwrap();
    s.rebuild_scene(None).unwrap();
    let walked = started.elapsed();
    assert_eq!(pending(s), 1);
    let g = t.rt.submit(job(s));
    let frames = t.until_exact(g);
    let settled = started.elapsed();

    // The release frame drew the stand-in and said so.
    let first = &frames[0];
    assert_eq!(first.generation, g);
    assert!(!first.exact, "the release frame drew a stand-in");
    assert_eq!(first.reuse, FrameReuse::Repainted, "{:?}", first.reuse);
    // Every rectangle any of them rasterised is on the photograph.
    for f in &frames {
        for r in &f.fresh {
            assert!(
                inside(*r, photo),
                "frame {}: {r:?} is outside the photograph {photo:?}",
                f.generation
            );
        }
    }
    let last = frames.last().unwrap();
    assert!(last.exact && last.generation > g, "a repair settled it");
    assert_eq!(pending(s), 0, "the helper made it");
    let stats = t.rt.stats();
    assert!(stats.substituted >= 1 && stats.repaired >= 1, "{stats:?}");
    t.rt.shutdown();

    let want = reference(s);
    assert!(
        last.surface.data() == want.as_slice(),
        "the settled frame is not the full-resolution picture"
    );
    println!(
        "2400 × 1600 release: walk {:.1} ms; settled after {:.1} ms in {} frames; {stats:?}",
        walked.as_secs_f64() * 1e3,
        settled.as_secs_f64() * 1e3,
        frames.len()
    );
}

/// Undo and redo while the helper is still making the committed image:
/// the helper's result belongs to the image it was asked for, the
/// render thread repairs only the frame still on screen (a repair is
/// owed to a generation), and every exact frame after the undo shows the
/// undone picture — never the stale one; likewise after the redo.
#[test]
fn undo_and_redo_during_the_background_evaluation_settle_right() {
    // 24 Mpx: the helper needs ≈ 260 ms per image, so the undo lands
    // while it works.
    let (mut app, _, _) = photo_app(6000, 4000, DeviceSize::new(640, 480), 0.5);
    let s = app.active_mut().unwrap();
    let mut t = Thread::spawn();
    // A first committed chain, settled: the state the undo returns to.
    s.apply(Intent::PhotoPanel(PhotoPanelOp::Preview(slider(0))))
        .unwrap();
    s.apply(Intent::PhotoPanel(PhotoPanelOp::Commit)).unwrap();
    s.rebuild_scene(None).unwrap();
    let g = t.rt.submit(job(s));
    t.until_exact(g);
    let before = reference(s);

    // A second chain, released; its image goes to the helper.
    for i in 20..25 {
        s.apply(Intent::PhotoPanel(PhotoPanelOp::Preview(slider(i))))
            .unwrap();
        s.rebuild_scene(None).unwrap();
        let g = t.rt.submit(job(s));
        t.next(g);
    }
    s.apply(Intent::PhotoPanel(PhotoPanelOp::Commit)).unwrap();
    s.rebuild_scene(None).unwrap();
    let released = t.rt.submit(job(s));
    let f = t.next(released);
    assert!(!f.exact, "the release frame drew a stand-in");
    let after_release = s.resolver_snapshot();

    // Undo at once, while the helper evaluates the released chain.
    let stale = after_release
        .images
        .iter()
        .find(|i| i.is_pending())
        .expect("the helper is still at work")
        .clone();
    s.apply(Intent::Undo).unwrap();
    s.rebuild_scene(None).unwrap();
    let undone = t.rt.submit(job(s));
    let frames = t.until_exact(undone);
    for f in frames.iter().filter(|f| f.generation >= undone && f.exact) {
        assert!(
            f.surface.data() == before.as_slice(),
            "frame {} after the undo is not the undone picture",
            f.generation
        );
    }
    assert!(frames.last().unwrap().surface.data() == before.as_slice());

    // Redo, and undo's picture must not come back either.
    s.apply(Intent::Redo).unwrap();
    s.rebuild_scene(None).unwrap();
    let redone = t.rt.submit(job(s));
    let frames = t.until_exact(redone);
    t.rt.shutdown();
    let want = reference(s);
    assert!(before != want);
    for f in frames.iter().filter(|f| f.generation >= redone && f.exact) {
        assert!(
            f.surface.data() == want.as_slice(),
            "frame {} after the redo is not the redone picture",
            f.generation
        );
    }
    // The image the helper was making when the undo came was finished
    // into its own store, which no frame after the undo drew.
    assert!(!stale.is_pending());
}
