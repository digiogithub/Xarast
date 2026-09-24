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
