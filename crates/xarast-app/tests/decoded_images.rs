//! A session's decoded bitmaps (XARA-T-0281): every walker made for one
//! document — the session's own, `build_scene`, thumbnails, export —
//! decodes each bitmap once between them, and sharing the images changes
//! neither the pictures nor the damage between two scenes.

use std::sync::Arc;

use xarast_app::{AppState, DeviceSize, Intent, Session, build_scene};
use xarast_doc::resources::{BitmapData, BitmapInfo, BitmapResource};

fn gradient(w: u32, h: u32, seed: u8) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            [(x * 255 / w) as u8, (y * 255 / h) as u8, seed, 255]
        })
        .collect()
}

/// A new document on a 400 × 300 canvas at 100 %, with a pasted picture
/// placed on it (stored as an encoded PNG, decoded by the walker) and a
/// bitmap resource holding native pixels (copied by it).
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
    app.apply(Intent::PasteImage {
        width: 120,
        height: 80,
        rgba: Arc::from(gradient(120, 80, 7)),
    })
    .unwrap();
    app.active_mut()
        .unwrap()
        .doc
        .resources
        .insert_bitmap(BitmapResource {
            name: Arc::from("native"),
            info: BitmapInfo {
                width: 64,
                height: 48,
                bpp: 32,
                dpi_x: 96,
                dpi_y: 96,
            },
            pixels: Arc::new(BitmapData {
                pixels: Arc::from(gradient(64, 48, 99)),
                palette: Arc::from(Vec::new()),
            }),
            original: None,
            procedural: None,
            transparent_index: None,
        });
    app
}

fn session(app: &AppState) -> &Session {
    app.active().unwrap()
}

#[test]
fn every_walker_of_a_session_shares_one_decode_per_bitmap() {
    let mut app = app();
    let cache = session(&app).decoded_images().clone();
    // The paste walked the document already: both bitmaps are filed.
    let decoded = cache.stats().decoded;
    assert!(decoded <= 2, "{:?}", cache.stats());

    let first = build_scene(session(&app), None);
    assert_eq!(first.resolver.images.len(), 2);
    assert!(first.stats.images > 0, "the picture is in the scene");
    assert_eq!(cache.stats().decoded, 2, "{:?}", cache.stats());

    // Again, the session's own walker, and a thumbnail: nothing decodes.
    let second = build_scene(session(&app), None);
    app.active_mut().unwrap().rebuild_scene(None).unwrap();
    let s = session(&app);
    let thumb = xarast_app::thumbnail::thumbnail_png_with(&s.doc, Some(s.decoded_images()));
    assert!(thumb.is_some());
    assert_eq!(cache.stats().decoded, 2, "{:?}", cache.stats());
    assert!(cache.stats().hits >= 6, "{:?}", cache.stats());

    // The shared images are the same images: no damage between the two
    // scenes, and the thumbnail is the one an uncached walk makes.
    let damage = xarast_render::scene_damage(
        (&first.scene, &first.resolver),
        (&second.scene, &second.resolver),
        &first.view,
        4,
    )
    .expect("comparable");
    assert!(damage.rects.is_empty(), "{damage:?}");
    assert_eq!(thumb, xarast_app::thumbnail::thumbnail_png(&s.doc));
}

#[test]
fn a_new_resource_misses_and_a_gone_one_is_dropped() {
    let a = app();
    let s = session(&a);
    let cache = s.decoded_images().clone();
    let first = build_scene(s, None);
    let before = cache.stats();

    // The same document built again: the same bytes in new allocations,
    // so new resources. Walked with the first one's cache they miss, one
    // decode each; the first document's entries, whose resources the
    // walked document does not hold, are dropped. The images still
    // compare equal by content, so there is no damage.
    let b = app();
    let other = session(&b);
    let mut walker = xarast_app::SceneWalker::new().with_decoded_images(cache.clone());
    let mut scene = xarast_render::Scene::new();
    walker
        .rebuild(
            &other.doc,
            &other.edit,
            &s.viewport,
            s.quality,
            None,
            &mut scene,
        )
        .expect("walks");
    let after = cache.stats();
    assert_eq!(after.decoded - before.decoded, 2, "{after:?}");
    assert_eq!(after.hits, before.hits, "{after:?}");
    assert_eq!(after.pruned - before.pruned, 2, "{after:?}");
    assert_eq!(cache.len(), 2);
    let damage = xarast_render::scene_damage(
        (&first.scene, &first.resolver),
        (&scene, walker.resolver()),
        &first.view,
        4,
    )
    .expect("comparable");
    assert!(damage.rects.is_empty(), "{damage:?}");
}

/// Opens the bitmap gallery and waits for every thumbnail it asked for.
fn gallery_thumbnails(app: &mut AppState) -> Vec<Option<Arc<xarast_app::bitmap_gallery::Thumb>>> {
    app.bitmap_gallery_view().expect("a document is open");
    app.settle_thumbnails(std::time::Duration::from_secs(60));
    app.bitmap_gallery_view()
        .unwrap()
        .entries
        .into_iter()
        .map(|e| e.thumbnail)
        .collect()
}

#[test]
fn the_gallery_decodes_nothing_the_view_has_decoded() {
    // XARA-T-0293: the thumbnails come from the session's decoded images.
    let mut app = app();
    let cache = session(&app).decoded_images().clone();
    let _ = build_scene(session(&app), None);
    let before = cache.stats();
    assert_eq!(before.decoded, 2, "{before:?}");

    let thumbs = gallery_thumbnails(&mut app);
    assert_eq!(thumbs.len(), 2);
    assert!(thumbs.iter().all(Option::is_some), "{thumbs:?}");
    let after = cache.stats();
    assert_eq!(after.decoded, before.decoded, "{after:?}");
    assert_eq!(after.hits - before.hits, 2, "{after:?}");
}

#[test]
fn the_view_decodes_nothing_the_gallery_has_decoded() {
    // A bitmap the view has never walked: the gallery decodes and files
    // it, and the next walk finds it there.
    let mut app = app();
    let _ = build_scene(session(&app), None);
    let bytes = {
        let mut out = Vec::new();
        xarast_io::png::encode_png(
            &mut out,
            xarast_io::png::PngHeader {
                width: 90,
                height: 30,
                colour: xarast_io::PngColour::Rgba,
                depth: xarast_io::PngDepth::Eight,
                interlace: false,
                ppm: None,
                level: 1,
            },
            &gradient(90, 30, 42),
        )
        .unwrap();
        out
    };
    let img = xarast_app::place::image_from_bytes(Arc::from(bytes), "strip.png").unwrap();
    app.active_mut()
        .unwrap()
        .doc
        .resources
        .insert_bitmap(img.resource);
    let cache = session(&app).decoded_images().clone();
    let before = cache.stats();

    let thumbs = gallery_thumbnails(&mut app);
    assert_eq!(thumbs.len(), 3);
    let strip = thumbs[2].as_ref().expect("made");
    assert_eq!((strip.width, strip.height), (64, 21));
    let mid = cache.stats();
    assert_eq!(mid.decoded - before.decoded, 1, "only the new one: {mid:?}");

    let scene = build_scene(session(&app), None);
    assert_eq!(scene.resolver.images.len(), 3);
    let after = cache.stats();
    assert_eq!(
        after.decoded, mid.decoded,
        "the walk decodes nothing: {after:?}"
    );
    assert_eq!(after.hits - mid.hits, 3, "{after:?}");
}
