//! Non-destructive photo adjustments in the application (phase 10 W10.6):
//! the view shows the master put through the chain, the evaluation is
//! cached and shared by every walker of the document, an edit is one undo
//! step that repaints only its object, and edited-away chains do not pile
//! up images.

use std::sync::Arc;

use xarast_app::headless::{HeadlessFrame, HeadlessOptions, render};
use xarast_app::{AppState, DeviceSize, Intent, Session, build_scene};
use xarast_doc::photo::{PhotoOp, PhotoOps, PhotoOrient, PixelRect};
use xarast_doc::resources::{BitmapData, BitmapInfo, BitmapResource};
use xarast_doc::{Command, EditError, NodeId, NodeKind, Tx};
use xarast_geom::{Matrix, Vector};

fn gradient(w: u32, h: u32, seed: u8) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            [(x * 255 / w) as u8, (y * 255 / h) as u8, seed, 255]
        })
        .collect()
}

/// Moves a node: an edit that is not a photo adjustment.
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

fn bitmaps(s: &Session) -> Vec<NodeId> {
    s.doc
        .tree
        .preorder(s.doc.tree.root())
        .filter(|n| matches!(s.doc.tree.kind(*n), Some(NodeKind::Bitmap(_))))
        .collect()
}

/// A new document on a 400 × 300 canvas at 100 % with two pasted
/// pictures side by side (encoded PNGs, decoded by the walker).
fn app() -> AppState {
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
    let second = bitmaps(s)[1];
    s.dispatch(&MoveBy(second, Vector::raw(120_000, 0)))
        .unwrap();
    s.rebuild_scene(None).unwrap();
    app
}

fn adjustments(brightness: f32) -> PhotoOps {
    PhotoOps {
        ops: vec![
            PhotoOp::Brightness(brightness),
            PhotoOp::Contrast(0.3),
            PhotoOp::Gamma(1.4),
            PhotoOp::Saturation(-0.5),
        ],
    }
}

#[test]
fn an_adjustment_is_one_undo_step_that_repaints_only_its_object() {
    let mut app = app();
    let s = app.active_mut().unwrap();
    let [first, second] = bitmaps(s)[..] else {
        panic!("two bitmaps")
    };
    let (s0, r0) = (s.scene_snapshot(), s.resolver_snapshot());
    let view = s.view_params();
    let undo_before = s.undo_label();

    assert_eq!(
        s.set_photo_ops(first, &adjustments(0.2)).unwrap(),
        Some("Adjust Photo")
    );
    // The same chain again, in another order: nothing to do, no step.
    let mut shuffled = adjustments(0.2);
    shuffled.ops.reverse();
    assert_eq!(s.set_photo_ops(first, &shuffled).unwrap(), None);
    s.rebuild_scene(None).unwrap();
    let (s1, r1) = (s.scene_snapshot(), s.resolver_snapshot());
    assert!(s.walk_stats().is_complete(), "{:?}", s.walk_stats());

    let damage = xarast_render::scene_damage((&s0, &r0), (&s1, &r1), &view, 8).expect("comparable");
    assert!(!damage.rects.is_empty());
    assert_eq!((damage.removed, damage.added), (1, 1), "{damage:?}");
    // Nothing of the other picture is repainted.
    let other = xarast_app::viewport::nodes_rect(&s.doc, [second]);
    let a = s.viewport.doc_to_device(other.lo);
    let b = s.viewport.doc_to_device(other.hi);
    let (ox0, ox1) = (a.x.min(b.x), a.x.max(b.x));
    for r in &damage.rects {
        assert!(
            f64::from(r.x1) <= ox0 + 1.0 || f64::from(r.x0) >= ox1 - 1.0,
            "{r:?} reaches the other picture ({ox0}..{ox1})"
        );
    }

    // One step back is the old picture exactly.
    assert_eq!(s.undo(), Some("Adjust Photo"));
    assert_eq!(s.undo_label(), undo_before);
    s.rebuild_scene(None).unwrap();
    let (s2, r2) = (s.scene_snapshot(), s.resolver_snapshot());
    let back = xarast_render::scene_damage((&s0, &r0), (&s2, &r2), &view, 8).expect("comparable");
    assert!(back.rects.is_empty(), "{back:?}");
}

#[test]
fn the_view_shows_the_master_through_the_chain() {
    // The same picture, adjusted by the chain or baked into a bitmap of
    // its own beforehand, renders to the same pixels.
    let ops = PhotoOps {
        ops: vec![
            PhotoOp::Crop(PixelRect {
                x: 10,
                y: 5,
                width: 40,
                height: 30,
            }),
            PhotoOp::Orient(PhotoOrient::CW),
            PhotoOp::Brightness(0.1),
            PhotoOp::Greyscale,
        ],
    };
    let mut app = app();
    let s = app.active_mut().unwrap();
    let first = bitmaps(s)[0];
    s.set_photo_ops(first, &ops).unwrap();
    let opts = HeadlessOptions {
        size: DeviceSize::new(300, 200),
        frame: HeadlessFrame::FitDrawing,
        ..HeadlessOptions::default()
    };
    let adjusted = render(s, &opts).unwrap();
    assert!(adjusted.walk.is_complete(), "{:?}", adjusted.walk);

    // Bake it by hand: the master decoded as the walker does, through the
    // chain, stored as native pixels; the object shows that with no chain.
    let Some(NodeKind::Bitmap(bm)) = s.doc.tree.kind(first) else {
        panic!()
    };
    let bm = (**bm).clone();
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
        &ops,
    )
    .unwrap();
    assert_eq!((w, h), (30, 40));
    let baked = s.doc.resources.insert_bitmap(BitmapResource {
        name: Arc::from("baked"),
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
    });
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
    s.dispatch(&Swap(
        first,
        xarast_doc::BitmapNode {
            image: baked,
            photo_ops: PhotoOps::new(),
            ..bm
        },
    ))
    .unwrap();
    let plain = render(s, &opts).unwrap();
    assert_eq!(adjusted.view, plain.view);
    assert!(
        adjusted.surface.data() == plain.surface.data(),
        "the adjusted render differs from the baked one"
    );
}

#[test]
fn walkers_share_one_evaluation_and_edits_do_not_pile_up_images() {
    let mut app = app();
    let s = app.active_mut().unwrap();
    let first = bitmaps(s)[0];
    let cache = s.decoded_images().clone();
    let slots = s.resolver_snapshot().images.len();
    s.set_photo_ops(first, &adjustments(0.1)).unwrap();
    s.rebuild_scene(None).unwrap();
    let evaluated = cache.stats().derived;
    assert_eq!(evaluated, 1, "{:?}", cache.stats());
    assert_eq!(s.resolver_snapshot().images.len(), slots + 1);

    // An export's walker finds it.
    let built = build_scene(s, None);
    assert!(built.stats.images > 0);
    assert_eq!(cache.stats().derived, evaluated, "{:?}", cache.stats());
    assert!(cache.stats().derived_hits >= 1, "{:?}", cache.stats());

    // A slider dragged through twenty values: each value is evaluated
    // once, the old ones are dropped, and the registry does not grow.
    for i in 0..20 {
        s.set_photo_ops(first, &adjustments(0.01 * i as f32 - 0.3))
            .unwrap();
        s.rebuild_scene(None).unwrap();
    }
    assert!(
        s.resolver_snapshot().images.len() <= slots + 2,
        "{} slots",
        s.resolver_snapshot().images.len()
    );
    assert!(cache.derived_len() <= 1, "{}", cache.derived_len());
    assert!(cache.stats().derived_pruned >= 19, "{:?}", cache.stats());
    // Undo all the way: every step renders, and ends at the master.
    while s.undo_label() == Some("Adjust Photo") {
        s.undo();
        s.rebuild_scene(None).unwrap();
        assert!(s.walk_stats().is_complete());
    }
    let Some(NodeKind::Bitmap(bm)) = s.doc.tree.kind(first) else {
        panic!()
    };
    assert!(bm.photo_ops.is_empty());
}
