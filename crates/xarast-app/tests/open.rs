//! Opening, replacing and closing documents through intents, the recent
//! files and the platform requests (XARA-US-0082).

use std::path::{Path, PathBuf};

use xarast_app::{AppCommand, AppState, Changed, DevicePoint, Intent, PlatformRequest};

/// A scratch directory of its own for each test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("xarast-open-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A small `.xar` the importer accepts — document, chapter, spread and
/// one layer — built in memory, so no corpus is needed.
fn tiny_xar(dir: &Path, name: &str) -> PathBuf {
    let mut spread = Vec::new();
    for v in [600_000i32, 450_000, 0, 0] {
        spread.extend_from_slice(&v.to_le_bytes());
    }
    spread.push(2);
    let mut layer = vec![0x01 | 0x04 | 0x08];
    for u in "Layer 1".encode_utf16().chain([0]) {
        layer.extend_from_slice(&u.to_le_bytes());
    }
    let bytes = xarast_xar::synth::XarBuilder::new()
        .record(40, &[])
        .down()
        .record(41, &[])
        .down()
        .record(42, &[])
        .down()
        .record(45, &spread)
        .record(43, &[])
        .down()
        .record(48, &layer)
        .up()
        .up()
        .up()
        .up()
        .end_of_file()
        .finish();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn opening_a_file_replaces_the_current_document() {
    let dir = scratch("replace");
    let (a, b) = (tiny_xar(&dir, "a.xar"), tiny_xar(&dir, "b.xar"));
    let mut app = AppState::new();
    let changed = app.apply(Intent::OpenFile(a.clone())).unwrap();
    assert!(changed.contains(Changed::ACTIVE) && changed.needs_scene());
    app.apply(Intent::OpenFile(b.clone())).unwrap();
    assert_eq!(app.docs.len(), 1, "single-document model");
    assert_eq!(app.active().unwrap().path.as_deref(), Some(b.as_path()));
    assert_eq!(app.recent.paths(), [b, a]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_failed_open_keeps_the_current_document_and_reports() {
    let dir = scratch("fail");
    let good = tiny_xar(&dir, "good.xar");
    let bad = dir.join("bad.xar");
    std::fs::write(&bad, b"this is not a xar file").unwrap();
    let mut app = AppState::new();
    app.apply(Intent::OpenFile(good.clone())).unwrap();
    let problems = app.diagnostics.entries().len();
    for path in [bad, dir.join("missing.xar"), dir.clone()] {
        assert!(app.apply(Intent::OpenFile(path)).is_err());
    }
    assert_eq!(app.active().unwrap().path.as_deref(), Some(good.as_path()));
    assert_eq!(app.diagnostics.entries().len(), problems + 3);
    assert_eq!(app.recent.paths(), [good], "failures are not remembered");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn close_leaves_nothing_open_and_says_the_active_document_changed() {
    let dir = scratch("close");
    let mut app = AppState::new();
    app.apply(Intent::OpenFile(tiny_xar(&dir, "a.xar")))
        .unwrap();
    let changed = app.apply(Intent::CloseDocument).unwrap();
    assert!(changed.contains(Changed::ACTIVE));
    assert!(app.active().is_none());
    assert!(app.apply(Intent::CloseDocument).unwrap().is_none());
    // A view command with nothing open is harmless.
    let c = AppCommand::ZoomIn.intent(DevicePoint::new(0.0, 0.0));
    assert!(app.apply(c).unwrap().is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn platform_intents_are_queued_for_the_shell() {
    let mut app = AppState::new();
    app.apply(AppCommand::Open.intent(DevicePoint::new(0.0, 0.0)))
        .unwrap();
    app.apply(Intent::Quit).unwrap();
    assert_eq!(
        app.take_requests(),
        [PlatformRequest::ShowOpenDialog, PlatformRequest::Quit]
    );
    assert!(app.take_requests().is_empty());
}

#[test]
fn the_recent_list_persists_prunes_and_forgets_what_fails() {
    let dir = scratch("recent");
    let store = dir.join("state/xarast/recent");
    let a = tiny_xar(&dir, "a.xar");
    let b = tiny_xar(&dir, "b.xar");
    {
        let mut app = AppState::new().with_recent_store(store.clone());
        app.apply(Intent::OpenFile(a.clone())).unwrap();
        app.apply(Intent::OpenFile(b.clone())).unwrap();
    }
    let app = AppState::new().with_recent_store(store.clone());
    assert_eq!(app.recent.paths(), [b.clone(), a.clone()]);

    // A file that went away is pruned at the next start.
    std::fs::remove_file(&a).unwrap();
    let mut app = AppState::new().with_recent_store(store.clone());
    assert_eq!(app.recent.paths(), std::slice::from_ref(&b));

    // A listed file that stopped being a document is dropped when it fails.
    std::fs::write(&b, b"garbage").unwrap();
    assert!(app.apply(Intent::OpenFile(b)).is_err());
    assert!(app.recent.is_empty());
    assert!(
        AppState::new()
            .with_recent_store(store.clone())
            .recent
            .is_empty()
    );

    // And a corrupt store is an empty list, not a failure to start.
    std::fs::write(&store, b"\x00\xffnot a list").unwrap();
    let mut app = AppState::new().with_recent_store(store.clone());
    assert!(app.recent.is_empty());
    app.apply(Intent::OpenFile(tiny_xar(&dir, "c.xar")))
        .unwrap();
    app.apply(Intent::ClearRecent).unwrap();
    assert!(AppState::new().with_recent_store(store).recent.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}
