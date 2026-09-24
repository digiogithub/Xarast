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
