//! The photo panel's live preview (phase 10, T10.6.5 and T10.6.8): a
//! slider drag previews at proxy resolution without touching the
//! document, commits one undo step on release, and `Esc` leaves nothing
//! behind; the committed picture is the full-resolution evaluation.

use std::sync::Arc;
use std::time::{Duration, Instant};

use xarast_app::headless::{HeadlessFrame, HeadlessOptions, render, render_with_walker};
use xarast_app::photo_panel::PhotoPanelOp;
use xarast_app::viewport::ZoomTarget;
use xarast_app::walker::SceneWalker;
use xarast_app::{AppState, DeviceSize, Intent, SelectMode, Session};
use xarast_doc::photo::{Levels, LevelsChannel, PhotoOp, PhotoOps, PhotoOrient};
use xarast_doc::resources::{BitmapData, BitmapInfo, BitmapResource};
use xarast_doc::{Command, EditError, NodeId, NodeKind, Tx};
use xarast_geom::{Matrix, Vector};
use xarast_render::{CpuBackend, CpuConfig, DirtyRect, DisplayList, Surface};

fn gradient(w: u32, h: u32, seed: u8) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            [(x * 255 / w) as u8, (y * 255 / h) as u8, seed, 255]
        })
        .collect()
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

/// Two pasted pictures side by side on a 400 × 300 view at 100 %; the
/// first one selected.
fn app() -> (AppState, NodeId, NodeId) {
    let mut app = AppState::new();
    app.new_document();
    app.apply(Intent::Resize(DeviceSize::new(400, 300)))
        .unwrap();
    app.apply(Intent::SetZoom {
        zoom: 1.0,
        anchor: None,
    })
    .unwrap();
    for seed in [7, 200] {
        app.apply(Intent::PasteImage {
            width: 60,
            height: 40,
            rgba: Arc::from(gradient(60, 40, seed)),
        })
        .unwrap();
    }
    let s = app.active_mut().unwrap();
    let [first, second] = bitmaps(s)[..] else {
        panic!("two bitmaps")
    };
    s.dispatch(&MoveBy(second, Vector::raw(120_000, 0)))
        .unwrap();
    s.apply(Intent::Select {
        nodes: vec![first],
        mode: SelectMode::Replace,
    })
    .unwrap();
    s.rebuild_scene(None).unwrap();
    (app, first, second)
}

/// The chain the slider shows at step `i` of a drag.
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

fn opts() -> HeadlessOptions {
    HeadlessOptions {
        size: DeviceSize::new(400, 300),
        frame: HeadlessFrame::Session,
        ..HeadlessOptions::default()
    }
}

#[test]
fn a_sixty_event_slider_drag_is_one_undo_step_and_undo_is_exact() {
    let (mut app, first, second) = app();
    let s = app.active_mut().unwrap();
    let before = s.doc.canonical_digest();
    let len = s.bus.history().len();
    let cache = s.decoded_images().clone();
    let (s0, r0) = (s.scene_snapshot(), s.resolver_snapshot());
    let view = s.view_params();
    let other = xarast_app::viewport::nodes_rect(&s.doc, [second]);
    let (a, b) = (
        s.viewport.doc_to_device(other.lo),
        s.viewport.doc_to_device(other.hi),
    );
    let (ox0, ox1) = (a.x.min(b.x), a.x.max(b.x));

    for i in 0..60 {
        s.apply(Intent::PhotoPanel(PhotoPanelOp::Preview(slider(i))))
            .unwrap();
        s.rebuild_scene(None).unwrap();
        assert_eq!(
            s.doc.canonical_digest(),
            before,
            "frame {i} touched the document"
        );
        assert_eq!(s.bus.history().len(), len);
        assert!(s.photo_panel_view().dragging);
        assert_eq!(s.photo_panel_view().ops, slider(i).normalised());
        let proxies = s.photo_proxies();
        assert_eq!(proxies.len(), 1, "{proxies:?}");
        assert_eq!(proxies[0].node, first);
        // Damage stays on the object being adjusted.
        let (s1, r1) = (s.scene_snapshot(), s.resolver_snapshot());
        let damage =
            xarast_render::scene_damage((&s0, &r0), (&s1, &r1), &view, 8).expect("comparable");
        assert!(!damage.rects.is_empty(), "frame {i} shows nothing new");
        for r in &damage.rects {
            assert!(
                f64::from(r.x1) <= ox0 + 1.0 || f64::from(r.x0) >= ox1 - 1.0,
                "frame {i}: {r:?} reaches the other picture ({ox0}..{ox1})"
            );
        }
    }
    // Nothing was evaluated for the document's derived-image cache: the
    // drag drew proxies only.
    assert_eq!(cache.stats().derived, 0, "{:?}", cache.stats());
    let previewed = render(s, &opts()).unwrap();

    s.apply(Intent::PhotoPanel(PhotoPanelOp::Commit)).unwrap();
    s.rebuild_scene(None).unwrap();
    assert!(s.photo_proxies().is_empty());
    assert!(!s.photo_panel_view().dragging);
    assert_eq!(s.bus.history().len(), len + 1, "one step");
    assert_eq!(s.undo_label(), Some("Adjust Photo"));
    assert_eq!(bitmap(s, first).photo_ops, slider(59).normalised());
    let committed = render(s, &opts()).unwrap();
    // A 60 × 40 picture at 100 % is its own proxy: the preview was exact.
    assert!(committed.surface.data() == previewed.surface.data());

    s.apply(Intent::Undo).unwrap();
    assert_eq!(s.doc.canonical_digest(), before, "undo is exact");
    s.rebuild_scene(None).unwrap();
    let (s2, r2) = (s.scene_snapshot(), s.resolver_snapshot());
    let back = xarast_render::scene_damage((&s0, &r0), (&s2, &r2), &view, 8).expect("comparable");
    assert!(back.rects.is_empty(), "{back:?}");
}

#[test]
fn escape_mid_drag_restores_everything() {
    let (mut app, first, _) = app();
    let s = app.active_mut().unwrap();
    // An earlier adjustment, so the drag starts from a chain.
    s.apply(Intent::PhotoPanel(PhotoPanelOp::Set(slider(0))))
        .unwrap();
    s.rebuild_scene(None).unwrap();
    let before = s.doc.canonical_digest();
    let len = s.bus.history().len();
    let label = s.undo_label();
    let pixels = render(s, &opts()).unwrap();
    for i in 1..30 {
        s.apply(Intent::PhotoPanel(PhotoPanelOp::Preview(slider(i))))
            .unwrap();
        s.rebuild_scene(None).unwrap();
    }
    assert!(render(s, &opts()).unwrap().surface.data() != pixels.surface.data());
    // `Esc` is consumed by the drag: the selection stays.
    s.apply(Intent::Cancel).unwrap();
    assert!(s.preview().is_empty());
    assert_eq!(s.edit.selection().collect::<Vec<_>>(), [first]);
    s.rebuild_scene(None).unwrap();
    assert!(s.photo_proxies().is_empty());
    assert_eq!(s.doc.canonical_digest(), before);
    assert_eq!(s.bus.history().len(), len);
    assert_eq!(s.undo_label(), label);
    assert_eq!(bitmap(s, first).photo_ops, slider(0).normalised());
    assert!(render(s, &opts()).unwrap().surface.data() == pixels.surface.data());
    // A release after the cancel commits nothing.
    s.apply(Intent::PhotoPanel(PhotoPanelOp::Commit)).unwrap();
    assert_eq!(s.bus.history().len(), len);

    // The panel's own cancel does the same, and so does an undo.
    s.apply(Intent::PhotoPanel(PhotoPanelOp::Preview(slider(40))))
        .unwrap();
    s.apply(Intent::PhotoPanel(PhotoPanelOp::Cancel)).unwrap();
    assert!(s.preview().is_empty());
    s.apply(Intent::PhotoPanel(PhotoPanelOp::Preview(slider(40))))
        .unwrap();
    s.apply(Intent::Undo).unwrap();
    assert!(s.preview().is_empty());
    assert!(
        bitmap(s, first).photo_ops.is_empty(),
        "the undo undid the Set"
    );
}

#[test]
fn geometry_is_set_at_once_and_unknown_chains_are_read_only() {
    let (mut app, first, _) = app();
    let s = app.active_mut().unwrap();
    let len = s.bus.history().len();
    // A turn cannot be previewed in the old placement: it is one step.
    let turned = PhotoOps {
        ops: vec![PhotoOp::Orient(PhotoOrient::CW)],
    };
    s.apply(Intent::PhotoPanel(PhotoPanelOp::Preview(turned.clone())))
        .unwrap();
    assert!(s.preview().is_empty());
    assert_eq!(bitmap(s, first).photo_ops, turned);
    assert_eq!(s.bus.history().len(), len + 1);
    // Levels on top of the turn preview as usual.
    let levels = turned.with(PhotoOp::Levels(Levels {
        channel: LevelsChannel::Red,
        in_lo: 20,
        in_hi: 230,
        out_lo: 0,
        out_hi: 255,
    }));
    s.apply(Intent::PhotoPanel(PhotoPanelOp::Preview(levels.clone())))
        .unwrap();
    assert_eq!(s.preview().photo, [(first, levels.clone())]);
    s.apply(Intent::PhotoPanel(PhotoPanelOp::Commit)).unwrap();
    assert_eq!(bitmap(s, first).photo_ops, levels);
    // Reset is one step back to the master.
    s.apply(Intent::PhotoPanel(PhotoPanelOp::Reset)).unwrap();
    assert!(bitmap(s, first).photo_ops.is_empty());
    assert_eq!(s.bus.history().len(), len + 3);

    // A chain from a newer version: shown, never edited.
    let mut bm = bitmap(s, first);
    bm.photo_ops = PhotoOps {
        ops: vec![
            PhotoOp::Brightness(0.1),
            PhotoOp::Unknown {
                kind: "curves".into(),
                raw: "<xarast:photo-op xarast:kind=\"curves\"/>".into(),
            },
        ],
    };
    s.dispatch(&Swap(first, bm.clone())).unwrap();
    let v = s.photo_panel_view();
    assert!(!v.editable);
    assert_eq!(v.unknown_kinds(), ["curves"]);
    let digest = s.doc.canonical_digest();
    s.apply(Intent::PhotoPanel(PhotoPanelOp::Preview(slider(3))))
        .unwrap();
    assert!(s.preview().is_empty());
    assert!(
        s.apply(Intent::PhotoPanel(PhotoPanelOp::Set(slider(3))))
            .is_err()
    );
    assert_eq!(s.doc.canonical_digest(), digest);

    // Nothing, or two objects, selected: nothing to edit.
    s.apply(Intent::SelectNone).unwrap();
    assert!(s.photo_panel_view().node.is_none());
    s.apply(Intent::PhotoPanel(PhotoPanelOp::Preview(slider(3))))
        .unwrap();
    assert!(s.preview().is_empty());
}

#[test]
fn the_committed_render_is_the_full_resolution_evaluation() {
    let (mut app, first, _) = app();
    let s = app.active_mut().unwrap();
    for i in 0..10 {
        s.apply(Intent::PhotoPanel(PhotoPanelOp::Preview(slider(i))))
            .unwrap();
        s.rebuild_scene(None).unwrap();
    }
    s.apply(Intent::PhotoPanel(PhotoPanelOp::Commit)).unwrap();
    let committed = render(s, &opts()).unwrap();
    // A fresh walker with no shared cache evaluates at full resolution.
    let fresh = render_with_walker(s, &opts(), SceneWalker::new()).unwrap();
    assert!(committed.surface.data() == fresh.surface.data());

    // And that is the master baked by hand at full resolution, with no
    // chain left on the object.
    let bm = bitmap(s, first);
    let master = s.doc.resources.bitmap(bm.image).unwrap().clone();
    let decoded = xarast_image::decode(
        &master.original.as_ref().unwrap().bytes,
        &xarast_image::DecodeLimits::default(),
    )
    .unwrap()
    .data;
    let (w, h, rgba) = xarast_io::photo::bake(
        decoded.width,
        decoded.height,
        &decoded.to_straight_rgba8(),
        &slider(9),
    )
    .unwrap();
    let baked = s.doc.resources.insert_bitmap(native(w, h, rgba));
    s.dispatch(&Swap(
        first,
        xarast_doc::BitmapNode {
            image: baked,
            photo_ops: PhotoOps::new(),
            ..bm
        },
    ))
    .unwrap();
    let plain = render(s, &opts()).unwrap();
    assert!(committed.surface.data() == plain.surface.data());
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

/// Renders the session's own scene as the render thread would: display
/// list, then the CPU backend, into `surface`.
fn draw(s: &mut Session, surface: &mut Surface) {
    let scene = s.scene_snapshot();
    let resolver = s.resolver_snapshot();
    let params = s.view_params();
    let dl = DisplayList::build(&scene, &params, &DirtyRect::of(params.viewport));
    CpuBackend::new(CpuConfig::interactive())
        .render(&dl, &resolver, surface)
        .unwrap();
}

/// T10.6.5's budget: a slider frame on a 24 Mpx photograph — the intent,
/// the walk with its proxy evaluation, and a CPU render of the view —
/// within 33 ms. Prints the numbers (`-- --nocapture`).
#[test]
fn a_slider_frame_on_a_24_mpx_photo_stays_within_33_ms() {
    const W: u32 = 6000;
    const H: u32 = 4000;
    let mut app = AppState::new();
    app.new_document();
    app.apply(Intent::Resize(DeviceSize::new(1280, 800)))
        .unwrap();
    app.apply(Intent::PasteImage {
        width: 60,
        height: 40,
        rgba: Arc::from(gradient(60, 40, 9)),
    })
    .unwrap();
    let s = app.active_mut().unwrap();
    let node = bitmaps(s)[0];
    let bm = bitmap(s, node);
    let big = s
        .doc
        .resources
        .insert_bitmap(native(W, H, gradient(W, H, 90)));
    // The same placement, twenty times larger: 1200 × 800 px at 100 %.
    let origin = bm.origin;
    s.dispatch(&Swap(
        node,
        xarast_doc::BitmapNode {
            image: big,
            major: Vector::raw(bm.major.dx.0 * 20, bm.major.dy.0 * 20),
            minor: Vector::raw(bm.minor.dx.0 * 20, bm.minor.dy.0 * 20),
            origin,
            ..bm
        },
    ))
    .unwrap();
    s.apply(Intent::Select {
        nodes: vec![node],
        mode: SelectMode::Replace,
    })
    .unwrap();
    s.apply(Intent::ZoomTo(ZoomTarget::Selection)).unwrap();
    let cache = s.decoded_images().clone();
    let mut surface = Surface::filled(1280, 800, [255, 255, 255, 255]);
    // At rest: the master registered, its pyramid built (off the clock).
    s.rebuild_scene(None).unwrap();
    draw(s, &mut surface);

    let mut frames: Vec<Duration> = Vec::new();
    let mut walks: Vec<Duration> = Vec::new();
    for i in 0..30 {
        let t = Instant::now();
        s.apply(Intent::PhotoPanel(PhotoPanelOp::Preview(slider(i))))
            .unwrap();
        s.rebuild_scene(None).unwrap();
        let walked = t.elapsed();
        draw(s, &mut surface);
        frames.push(t.elapsed());
        walks.push(walked);
        let p = s.photo_proxies()[0];
        assert!(p.evaluated);
        assert!(p.level >= 1, "{p:?}");
        assert!(
            u64::from(p.width) * u64::from(p.height) <= xarast_app::walker::PROXY_MAX_PIXELS,
            "{p:?}"
        );
    }
    assert_eq!(cache.stats().derived, 0, "no full-resolution evaluation");
    frames.sort();
    walks.sort();
    let p = s.photo_proxies()[0];
    let ms = |d: Duration| d.as_secs_f64() * 1e3;
    println!(
        "24 Mpx slider frame, proxy level {} ({} × {}): intent + walk median {:.1} ms, max {:.1} ms; \
         with the CPU render of 1280 × 800 median {:.1} ms, p90 {:.1} ms, max {:.1} ms",
        p.level,
        p.width,
        p.height,
        ms(walks[walks.len() / 2]),
        ms(walks[walks.len() - 1]),
        ms(frames[frames.len() / 2]),
        ms(frames[frames.len() * 9 / 10]),
        ms(frames[frames.len() - 1]),
    );
    assert!(
        frames[frames.len() / 2] <= Duration::from_millis(33),
        "median frame {:.1} ms",
        ms(frames[frames.len() / 2])
    );

    // The release: one full-resolution evaluation, then the view is the
    // committed chain.
    let t = Instant::now();
    s.apply(Intent::PhotoPanel(PhotoPanelOp::Commit)).unwrap();
    s.rebuild_scene(None).unwrap();
    println!(
        "24 Mpx commit (full-resolution evaluation on the walk): {:.1} ms",
        ms(t.elapsed())
    );
    assert_eq!(cache.stats().derived, 1);
    assert!(s.photo_proxies().is_empty());
}
