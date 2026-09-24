//! The bitmap gallery and the import UX (XARA-US-0055, W10.7): usage
//! counts through grouping, deleting and undo; Delete only for unused
//! bitmaps; a gallery drag that fills an object or places a new one, each
//! one exact undo step; background imports with progress and cancel;
//! pasting a file manager's copy; File › Import….

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use xarast_app::bitmap_gallery::{BitmapDragPoint, BitmapDropKind, BitmapGalleryOp};
use xarast_app::{AppState, DeviceSize, EditCommand, Intent, PlatformRequest, Session};
use xarast_doc::fill::FillGeometry;
use xarast_doc::{AttrValue, BitmapId, NodeId, NodeKind};
use xarast_geom::Point;

/// An app with one new document, a 400 × 300 px canvas at 100 %.
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
    app
}

fn s(app: &AppState) -> &Session {
    app.active().unwrap()
}

/// A PNG of `w × h` pixels of one colour, at `dpi` when given.
fn png(w: u32, h: u32, px: [u8; 4], dpi: Option<u32>) -> Vec<u8> {
    let mut out = Vec::new();
    xarast_io::png::encode_png(
        &mut out,
        xarast_io::png::PngHeader {
            width: w,
            height: h,
            colour: xarast_io::PngColour::Rgba,
            depth: xarast_io::PngDepth::Eight,
            interlace: false,
            ppm: dpi.map(|d| (f64::from(d) / 0.0254).round() as u32),
            level: 1,
        },
        &px.repeat((w * h) as usize),
    )
    .unwrap();
    out
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("xarast-us0055-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Adds a 96 × 48 px bitmap to the document, outside the history, and
/// returns it.
fn add_bitmap(app: &mut AppState, colour: [u8; 4]) -> BitmapId {
    let bytes = png(96, 48, colour, None);
    let img = xarast_app::place::image_from_bytes(Arc::from(bytes), "tile.png").unwrap();
    app.active_mut()
        .unwrap()
        .doc
        .resources
        .insert_bitmap(img.resource)
}

/// A 100 × 60 pt rectangle centred on `c`.
fn rectangle(app: &mut AppState, c: Point) -> NodeId {
    let sm = app.active_mut().unwrap();
    let layer = sm.doc.active_layer(sm.doc.active_spread()).unwrap();
    sm.apply_edit(EditCommand::CreateShape {
        layer,
        shape: Box::new(xarast_app::shapes::rectangle(c, 100_000.0, 60_000.0)),
        attrs: Vec::new(),
    })
    .unwrap();
    sm.doc.tree.children(layer).next_back().unwrap()
}

fn gallery(app: &mut AppState, op: BitmapGalleryOp) {
    app.apply(Intent::BitmapGallery(op)).unwrap();
}

fn entry(app: &mut AppState, id: BitmapId) -> xarast_app::bitmap_gallery::GalleryEntry {
    app.bitmap_gallery_view()
        .unwrap()
        .entries
        .into_iter()
        .find(|e| e.id == id)
        .expect("listed")
}

#[test]
fn usage_follows_grouping_deleting_and_undo_and_guards_delete() {
    let mut app = app();
    let id = add_bitmap(&mut app, [200, 10, 10, 255]);
    let e = entry(&mut app, id);
    assert_eq!((e.uses, e.held_by_history), (0, false));
    assert!(e.deletable());
    assert_eq!(e.pixels, (96, 48));
    assert_eq!(e.format, "PNG");
    // `xarast-io` writes an `sRGB` chunk.
    assert_eq!(e.colour_space, "sRGB");
    assert_eq!(e.decoded_bytes, 96 * 48 * 4);

    gallery(&mut app, BitmapGalleryOp::Place(id));
    assert_eq!(s(&app).undo_label(), Some("Place Bitmap"));
    let placed: Vec<NodeId> = s(&app).edit.selection().collect();
    assert_eq!(entry(&mut app, id).uses, 1);
    let r = rectangle(&mut app, Point::raw(100_000, 100_000));
    app.apply(Intent::Select {
        nodes: vec![placed[0], r],
        mode: xarast_app::SelectMode::Replace,
    })
    .unwrap();
    app.apply(Intent::Group).unwrap();
    assert_eq!(entry(&mut app, id).uses, 1, "grouped, still one use");
    app.apply(Intent::Ungroup).unwrap();
    assert_eq!(entry(&mut app, id).uses, 1, "ungrouped, still one use");

    // A used bitmap cannot be deleted.
    gallery(&mut app, BitmapGalleryOp::Delete(id));
    assert!(s(&app).doc.resources.bitmap(id).is_some());
    app.apply(Intent::Select {
        nodes: placed.clone(),
        mode: xarast_app::SelectMode::Replace,
    })
    .unwrap();
    app.apply(Intent::DeleteSelection).unwrap();
    let e = entry(&mut app, id);
    assert_eq!((e.uses, e.held_by_history), (0, true));
    assert!(!e.deletable(), "the history still holds it");
    gallery(&mut app, BitmapGalleryOp::Delete(id));
    assert!(s(&app).doc.resources.bitmap(id).is_some());
    app.apply(Intent::Undo).unwrap();
    assert_eq!(
        entry(&mut app, id).uses,
        1,
        "undoing the delete brings it back"
    );

    // An unused one goes, and only it.
    let idle = add_bitmap(&mut app, [0, 0, 0, 255]);
    let before = s(&app).bus.history().len();
    gallery(&mut app, BitmapGalleryOp::Delete(idle));
    assert!(s(&app).doc.resources.bitmap(idle).is_none());
    assert!(s(&app).doc.resources.bitmap(id).is_some());
    assert_eq!(s(&app).bus.history().len(), before, "not an undo step");
    assert!(
        app.bitmap_gallery_view()
            .unwrap()
            .entries
            .iter()
            .all(|e| e.id != idle)
    );
}

#[test]
fn a_gallery_drop_on_an_object_gives_it_a_bitmap_fill_as_one_exact_step() {
    let mut app = app();
    let id = add_bitmap(&mut app, [10, 200, 10, 255]);
    let c = Point::raw(200_000, 300_000);
    let r = rectangle(&mut app, c);
    let digest = s(&app).doc.canonical_digest();
    let history = s(&app).bus.history().len();

    gallery(&mut app, BitmapGalleryOp::DragBegin(id));
    let at = s(&app).viewport.doc_to_device(c);
    gallery(
        &mut app,
        BitmapGalleryOp::DragTo(BitmapDragPoint::Canvas(at)),
    );
    let drag = app.bitmap_gallery_view().unwrap().drag.expect("a drag");
    assert_eq!(drag.kind, BitmapDropKind::Fill);
    assert!(drag.status.contains("bitmap fill"), "{}", drag.status);
    assert_eq!(
        s(&app).doc.canonical_digest(),
        digest,
        "nothing before the drop"
    );
    gallery(&mut app, BitmapGalleryOp::DragDrop);

    let sm = s(&app);
    assert_eq!(sm.bus.history().len(), history + 1);
    assert_eq!(sm.undo_label(), Some("Set Fill"));
    let fill = sm
        .doc
        .tree
        .children(r)
        .find_map(|a| match sm.doc.tree.kind(a) {
            Some(NodeKind::Attr(a)) => match &a.value {
                AttrValue::Fill(g) => Some(g.clone()),
                _ => None,
            },
            _ => None,
        })
        .expect("the rectangle has its own fill");
    let FillGeometry::Bitmap {
        image,
        origin,
        axis_x,
        axis_y,
        ..
    } = fill
    else {
        panic!("not a bitmap fill: {fill:?}");
    };
    assert_eq!(image, id);
    // 96 × 48 px at 96 dpi: 72 × 36 pt, centred on the object.
    assert_eq!(origin, Point::raw(164_000, 282_000));
    assert_eq!(axis_x, Point::raw(236_000, 282_000));
    assert_eq!(axis_y, Point::raw(164_000, 318_000));
    assert!(app.bitmap_gallery_view().unwrap().drag.is_none());
    assert_eq!(entry(&mut app, id).uses, 1);
    app.apply(Intent::Undo).unwrap();
    assert_eq!(s(&app).doc.canonical_digest(), digest, "undo is exact");
}

#[test]
fn a_gallery_drop_on_empty_canvas_or_a_bitmap_places_a_new_object() {
    let mut app = app();
    let id = add_bitmap(&mut app, [10, 10, 200, 255]);
    let digest = s(&app).doc.canonical_digest();
    let c = Point::raw(150_000, 350_000);
    let at = s(&app).viewport.doc_to_device(c);
    gallery(&mut app, BitmapGalleryOp::DragBegin(id));
    gallery(
        &mut app,
        BitmapGalleryOp::DragTo(BitmapDragPoint::Canvas(at)),
    );
    assert_eq!(
        app.bitmap_gallery_view().unwrap().drag.unwrap().kind,
        BitmapDropKind::Place
    );
    gallery(&mut app, BitmapGalleryOp::DragDrop);
    assert_eq!(s(&app).undo_label(), Some("Place Bitmap"));
    assert_eq!(s(&app).bus.history().len(), 1);
    let first: NodeId = s(&app).edit.selection().next().unwrap();
    let Some(NodeKind::Bitmap(b)) = s(&app).doc.tree.kind(first) else {
        panic!("a bitmap object");
    };
    let far = b.origin + b.major + b.minor;
    assert_eq!(
        (
            (b.origin.x.raw() + far.x.raw()) / 2,
            (b.origin.y.raw() + far.y.raw()) / 2
        ),
        (150_000, 350_000)
    );
    // Dropped on a bitmap object it places another, never a fill.
    gallery(&mut app, BitmapGalleryOp::DragBegin(id));
    gallery(
        &mut app,
        BitmapGalleryOp::DragTo(BitmapDragPoint::Canvas(at)),
    );
    assert_eq!(
        app.bitmap_gallery_view().unwrap().drag.unwrap().kind,
        BitmapDropKind::Place
    );
    gallery(&mut app, BitmapGalleryOp::DragDrop);
    assert_eq!(s(&app).bus.history().len(), 2);
    assert_eq!(entry(&mut app, id).uses, 2);
    app.apply(Intent::Undo).unwrap();
    app.apply(Intent::Undo).unwrap();
    assert_eq!(s(&app).doc.canonical_digest(), digest, "undo is exact");
}

#[test]
fn a_cancelled_or_off_canvas_gallery_drag_changes_nothing() {
    let mut app = app();
    let id = add_bitmap(&mut app, [1, 2, 3, 255]);
    let digest = s(&app).doc.canonical_digest();
    let at = s(&app).viewport.doc_to_device(Point::raw(150_000, 350_000));
    gallery(&mut app, BitmapGalleryOp::DragBegin(id));
    gallery(
        &mut app,
        BitmapGalleryOp::DragTo(BitmapDragPoint::Canvas(at)),
    );
    gallery(&mut app, BitmapGalleryOp::DragCancel);
    gallery(&mut app, BitmapGalleryOp::DragDrop);
    gallery(&mut app, BitmapGalleryOp::DragBegin(id));
    gallery(
        &mut app,
        BitmapGalleryOp::DragTo(BitmapDragPoint::Elsewhere),
    );
    let drag = app.bitmap_gallery_view().unwrap().drag.unwrap();
    assert!(!drag.allowed());
    gallery(&mut app, BitmapGalleryOp::DragDrop);
    assert_eq!(s(&app).doc.canonical_digest(), digest);
    assert!(s(&app).bus.history().is_empty());
}

#[test]
fn thumbnails_arrive_off_the_main_thread() {
    let mut app = app();
    let id = add_bitmap(&mut app, [250, 128, 0, 255]);
    assert!(
        entry(&mut app, id).thumbnail.is_none(),
        "asked for, not ready"
    );
    app.settle_thumbnails(Duration::from_secs(30));
    let t = entry(&mut app, id).thumbnail.expect("made");
    assert_eq!((t.width, t.height), (64, 32));
    assert_eq!(&t.rgba[..4], &[250, 128, 0, 255]);
}

#[test]
fn a_large_import_runs_in_the_background_and_lands_as_one_step() {
    let mut app = app().with_inline_import_limit(0);
    let dir = temp_dir("bg");
    let bytes = png(300, 150, [0, 90, 200, 255], Some(300));
    let path = dir.join("big.png");
    std::fs::write(&path, &bytes).unwrap();
    // Registered first: the resource table is outside the history.
    let img = xarast_app::place::image_from_bytes(Arc::from(bytes), "big.png").unwrap();
    app.active_mut()
        .unwrap()
        .doc
        .resources
        .insert_bitmap(img.resource);
    let digest = s(&app).doc.canonical_digest();
    let c = Point::raw(120_000, 380_000);
    let at = s(&app).viewport.doc_to_device(c);
    app.apply(Intent::ImportImage {
        path: path.clone(),
        at: Some(at),
    })
    .unwrap();
    assert!(s(&app).bus.history().is_empty(), "nothing yet");
    let progress = app.import_progress();
    assert_eq!(progress.len(), 1);
    assert_eq!(progress[0].name, "big.png");
    assert!(app.take_notice().unwrap().starts_with("Importing big.png"));
    // The view moves before it arrives; it still lands on the drop point.
    app.apply(Intent::Pan { dx: 50.0, dy: 20.0 }).unwrap();
    let changed = app.wait_imports();
    assert!(changed.contains(xarast_app::Changed::DOCUMENT));
    assert!(app.import_progress().is_empty());
    assert_eq!(s(&app).undo_label(), Some("Import Bitmap"));
    assert_eq!(s(&app).bus.history().len(), 1);
    let n = s(&app).edit.selection().next().unwrap();
    let Some(NodeKind::Bitmap(b)) = s(&app).doc.tree.kind(n) else {
        panic!("a bitmap object");
    };
    let far = b.origin + b.major + b.minor;
    let centre = (
        (b.origin.x.raw() + far.x.raw()) / 2,
        (b.origin.y.raw() + far.y.raw()) / 2,
    );
    assert!(
        (centre.0 - c.x.raw()).abs() <= 1_000 && (centre.1 - c.y.raw()).abs() <= 1_000,
        "{centre:?}"
    );
    app.apply(Intent::Undo).unwrap();
    assert_eq!(s(&app).doc.canonical_digest(), digest, "undo is exact");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_cancelled_import_places_nothing() {
    let mut app = app().with_inline_import_limit(0);
    let dir = temp_dir("cancel");
    let path = dir.join("c.png");
    std::fs::write(&path, png(64, 64, [9, 9, 9, 255], None)).unwrap();
    let digest = s(&app).doc.canonical_digest();
    app.apply(Intent::ImportImage { path, at: None }).unwrap();
    assert_eq!(app.import_progress().len(), 1);
    app.apply(Intent::CancelImports).unwrap();
    assert!(app.import_progress().is_empty());
    assert_eq!(app.take_notice().as_deref(), Some("Import cancelled"));
    assert!(app.wait_imports().is_empty());
    assert_eq!(s(&app).doc.canonical_digest(), digest);
    assert_eq!(s(&app).doc.resources.bitmaps().count(), 0);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn pasting_a_file_managers_copy_imports_its_images() {
    let mut app = app();
    let dir = temp_dir("paste");
    let a = dir.join("one image.png");
    let b = dir.join("two.png");
    std::fs::write(&a, png(10, 10, [1, 1, 1, 255], None)).unwrap();
    std::fs::write(&b, png(20, 10, [2, 2, 2, 255], None)).unwrap();
    let text = format!(
        "copy\nfile://{}\nfile://{}\n",
        a.display().to_string().replace(' ', "%20"),
        b.display()
    );
    app.apply(Intent::PasteText {
        text: Some(text),
        in_place: false,
    })
    .unwrap();
    let sm = s(&app);
    assert_eq!(sm.bus.history().len(), 2, "one step per file");
    assert_eq!(sm.doc.resources.bitmaps().count(), 2);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn file_import_asks_the_platform_for_a_chooser_only_with_a_document() {
    let mut app = AppState::new();
    app.apply(Intent::ShowImportDialog).unwrap();
    assert!(app.take_requests().is_empty());
    app.new_document();
    app.apply(Intent::ShowImportDialog).unwrap();
    assert_eq!(app.take_requests(), [PlatformRequest::ShowImportDialog]);
}
