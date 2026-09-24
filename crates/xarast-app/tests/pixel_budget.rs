//! The pixel memory budget against real documents (W10.5): every corpus
//! file with bitmaps renders byte for byte the same under a budget that
//! evicts everything it can as under an unlimited one — whether the
//! evicted bases come back from spill files or from re-decoding their
//! encoded originals.
//!
//! The corpus is found through `XARAST_XAR_CORPUS` (default
//! `/home/user/xara-xtreme`) and never copied into this repository; with
//! no corpus the test skips with a notice, as `tests/corpus.rs` does.

use std::path::PathBuf;
use std::sync::Arc;

use xarast_app::headless::render_with_walker;
use xarast_app::{DeviceSize, DocumentId, HeadlessOptions, SceneWalker, Session};
use xarast_render::{BudgetConfig, BudgetStats, PixelBudget, RenderQuality};

const LOCK: &str = include_str!("../../../tests/corpus/corpus.lock");

fn corpus_files() -> Option<Vec<PathBuf>> {
    let root = PathBuf::from(
        std::env::var("XARAST_XAR_CORPUS").unwrap_or_else(|_| "/home/user/xara-xtreme".into()),
    );
    if !root.is_dir() {
        assert!(
            std::env::var("XARAST_CORPUS_REQUIRED").as_deref() != Ok("1"),
            "XARAST_CORPUS_REQUIRED=1 but {} is not a directory",
            root.display()
        );
        eprintln!("skipping: no corpus at {}", root.display());
        return None;
    }
    Some(
        LOCK.lines()
            .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
            .filter_map(|l| {
                let rel = l.split_whitespace().skip(2).collect::<Vec<_>>().join(" ");
                (!rel.is_empty()).then(|| root.join(rel))
            })
            .collect(),
    )
}

/// Evicts every level above the 1 × 1 one as soon as it is not pinned.
fn tiny(root: &std::path::Path, spill: bool) -> Arc<PixelBudget> {
    PixelBudget::new(BudgetConfig {
        limit_bytes: 0,
        proxy_cap: 1,
        proxy_default: 1,
        spill_root: Some(root.to_path_buf()),
        spill,
    })
}

fn render(session: &Session, budget: &Arc<PixelBudget>, quality: RenderQuality) -> Vec<u8> {
    let opts = HeadlessOptions {
        size: DeviceSize::new(640, 480),
        quality,
        ..HeadlessOptions::default()
    };
    let walker = SceneWalker::new().with_pixel_budget(Arc::clone(budget));
    let out = render_with_walker(session, &opts, walker).expect("renders");
    assert_eq!(out.walk.images_failed + out.walk.images_pending, 0);
    out.surface.data().to_vec()
}

#[test]
fn corpus_bitmaps_render_byte_for_byte_under_a_tiny_budget() {
    let Some(files) = corpus_files() else { return };
    let scratch =
        std::env::temp_dir().join(format!("xarast-app-budget-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    let spilling = tiny(&scratch, true);
    let decoding = tiny(&scratch, false);
    let mut with_bitmaps = Vec::new();
    for path in &files {
        let session = Session::open(DocumentId(1), path)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        if session.doc.resources.bitmaps().next().is_none() {
            continue;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        for quality in [RenderQuality::Final, RenderQuality::Draft] {
            let free = PixelBudget::new(BudgetConfig::unlimited());
            let reference = render(&session, &free, quality);
            assert_eq!(free.stats().evictions, 0);
            for (way, budget) in [("spill", &spilling), ("re-decode", &decoding)] {
                let got = render(&session, budget, quality);
                assert!(
                    got == reference,
                    "{name} at {quality:?} differs under a tiny budget ({way})"
                );
            }
        }
        with_bitmaps.push(name);
    }
    for must in ["Groucho2.xar", "leafgirl.xar"] {
        assert!(
            with_bitmaps.iter().any(|n| n.eq_ignore_ascii_case(must)),
            "{must} was not among the files with bitmaps: {with_bitmaps:?}"
        );
    }
    let check = |s: BudgetStats, what: &str| {
        assert!(s.evictions > 0, "{what}: {s:?}");
        assert_eq!(s.lost, 0, "{what}: {s:?}");
        assert_eq!(s.evictable_bytes, 0, "{what}: {s:?}");
    };
    let s = spilling.stats();
    check(s, "spill");
    assert!(s.spill_writes > 0 && s.from_spill > 0, "spill: {s:?}");
    let s = decoding.stats();
    check(s, "re-decode");
    assert!(s.from_source > 0 && s.spill_writes == 0, "re-decode: {s:?}");
    eprintln!(
        "{} corpus files with bitmaps; spill {:?}; re-decode {:?}",
        with_bitmaps.len(),
        spilling.stats(),
        decoding.stats()
    );
    drop(spilling);
    drop(decoding);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// What [`render_thread_frames`] saw.
struct ThreadRun {
    /// The first exact Final frame's pixels.
    exact: Vec<u8>,
    /// Frames published after the Final was submitted.
    published: usize,
    stats: xarast_app::RenderStats,
    /// Samplers that drew a substitute during the Final and its repair.
    final_substitutions: u64,
}

/// Draws `path` on the render thread under `budget`: a Draft zoomed out
/// (240 × 180, fitted), then a Final zoomed in (960 × 720, four
/// times the fitted zoom, on the drawing's centre), until an exact Final arrives. Between the two, once the
/// Draft's bases have come back, `between` runs.
fn render_thread_frames(
    path: &std::path::Path,
    budget: &Arc<PixelBudget>,
    between: impl FnOnce(),
) -> ThreadRun {
    use std::sync::mpsc;
    use std::time::Duration;
    use xarast_app::render_thread::RenderThread;

    let mut session = Session::open(DocumentId(1), path).expect("opens");
    let mut walker = SceneWalker::new().with_pixel_budget(Arc::clone(budget));
    let mut job_at = |quality: RenderQuality, size: DeviceSize, zoom: f64, epoch: u64| {
        session.viewport.resize(size);
        session
            .viewport
            .fit_rect(xarast_app::viewport::drawing_or_page_rect(&session.doc));
        let fitted = session.viewport.zoom();
        session.viewport.set_zoom(fitted * zoom);
        let mut scene = xarast_render::Scene::new();
        walker
            .rebuild(
                &session.doc,
                &session.edit,
                &session.viewport,
                quality,
                None,
                &mut scene,
            )
            .expect("walks");
        let mut job = session.frame_job([40, 40, 40, 255], [255, 255, 255, 255]);
        job.scene = Arc::new(scene);
        job.resolver = Arc::new(walker.resolver().clone());
        job.scene_epoch = epoch;
        job.view.quality = quality;
        job.ink = job.view.viewport;
        job
    };
    let draft = job_at(RenderQuality::Draft, DeviceSize::new(240, 180), 1.0, 1);
    let fin = job_at(RenderQuality::Final, DeviceSize::new(960, 720), 1.0, 2);

    let (tx, rx) = mpsc::channel::<()>();
    let tx = std::sync::Mutex::new(tx);
    let mut rt = RenderThread::spawn(Box::new(move || {
        let _ = tx.lock().map(|t| t.send(()));
    }))
    .expect("a render thread");
    let wait = || {
        rx.recv_timeout(Duration::from_secs(120))
            .expect("a frame within two minutes");
    };

    let g = rt.submit(draft);
    loop {
        wait();
        if rt.take_latest().is_some_and(|f| f.generation == g) {
            break;
        }
    }
    // The helper brings the Draft's bases back after the frame is out;
    // let it finish, so that what the Final finds is not a race.
    let mut last = budget.stats().rematerialised;
    let mut quiet = 0;
    while quiet < 3 {
        std::thread::sleep(Duration::from_millis(100));
        let now = budget.stats().rematerialised;
        quiet = if now == last { quiet + 1 } else { 0 };
        last = now;
    }
    between();
    let before = budget.stats().substituted;
    rt.submit(fin);
    let mut published = 0;
    let exact = loop {
        wait();
        let Some(f) = rt.take_latest() else { continue };
        published += 1;
        if f.view.quality == RenderQuality::Final && f.exact {
            break f;
        }
    };
    let stats = rt.stats();
    rt.shutdown();
    ThreadRun {
        exact: exact.surface.data().to_vec(),
        published,
        stats,
        final_substitutions: budget.stats().substituted - before,
    }
}

/// XARA-T-0281 on the render thread: with every evicted base on disk, a
/// zoomed-out Draft and the Final after it never read a base back on the
/// render thread — a helper thread does — and the exact Final that
/// settles the view (the worker's repair of what it drew from
/// substitutes) is the unlimited budget's picture byte for byte.
#[test]
fn the_render_thread_never_reads_a_base_back_and_converges() {
    let Some(files) = corpus_files() else { return };
    let scratch =
        std::env::temp_dir().join(format!("xarast-app-async-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    let mut repaired = Vec::new();
    for name in ["Groucho2.xar", "leafgirl.xar"] {
        let path = files
            .iter()
            .find(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(name)))
            .unwrap_or_else(|| panic!("{name} is in the corpus lock"));
        let free = PixelBudget::new(BudgetConfig::unlimited());
        let reference = render_thread_frames(path, &free, || {});
        assert_eq!(reference.stats.substituted, 0, "{name}");
        // The product's proxy sizes and nothing evictable kept resident
        // through the Draft. Before the Final, everything the Draft's
        // helper brought back is evicted again, and the limit then leaves
        // room for what the Final's helper brings back, as a real budget
        // does (under a limit of 0 the repair itself would have to read).
        let squeezed = PixelBudget::new(BudgetConfig {
            limit_bytes: 0,
            spill_root: Some(scratch.clone()),
            ..BudgetConfig::unlimited()
        });
        let got = render_thread_frames(path, &squeezed, || {
            squeezed.set_limit(0);
            squeezed.set_limit(u64::MAX);
        });
        let s = squeezed.stats();
        assert!(s.evictions > 0 && s.spill_writes > 0, "{name}: {s:?}");
        assert!(s.substituted > 0, "{name}: nothing was substituted: {s:?}");
        assert_eq!(
            s.from_spill + s.from_source,
            s.rematerialised,
            "{name}: a base was read back on the render thread: {s:?}"
        );
        if got.final_substitutions > 0 {
            assert!(
                got.stats.repaired >= 1 && got.published >= 2,
                "{name}: the Final was not repaired: {:?}",
                got.stats
            );
            repaired.push(name);
        }
        assert!(
            got.exact == reference.exact,
            "{name}: the repaired Final differs"
        );
        eprintln!(
            "{name}: {} frames after the Final; {:?}; {s:?}",
            got.published, got.stats
        );
    }
    assert!(!repaired.is_empty(), "no Final drew a substitute");
    let _ = std::fs::remove_dir_all(&scratch);
}
