//! Acceptance criterion 10 of phase 7 over the real corpus: grouping and
//! then ungrouping every object of a layer renders pixel-identically on
//! the CPU backend, restores the z-order, and undoes to the canonical
//! digest.
//!
//! The corpus is found through `XARAST_XAR_CORPUS` and never copied into
//! the repository; with no corpus present the test skips.

use std::path::PathBuf;

use xarast_app::{
    DeviceSize, DocumentId, HeadlessFrame, HeadlessOptions, Intent, SelectMode, Session,
};

const LOCK: &str = include_str!("../../../tests/corpus/corpus.lock");

fn corpus_files() -> Option<Vec<PathBuf>> {
    let root = PathBuf::from(
        std::env::var("XARAST_XAR_CORPUS").unwrap_or_else(|_| "/home/user/xara-xtreme".into()),
    );
    if !root.is_dir() {
        eprintln!("skipped: no corpus at {}", root.display());
        return None;
    }
    Some(
        LOCK.lines()
            .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
            .filter_map(|l| {
                let rel = l.split_whitespace().skip(2).collect::<Vec<_>>().join(" ");
                let p = root.join(rel);
                p.is_file().then_some(p)
            })
            .collect(),
    )
}

fn render(s: &Session, frame: xarast_app::DocRect) -> Vec<u8> {
    xarast_app::headless::render(
        s,
        &HeadlessOptions {
            size: DeviceSize::new(200, 150),
            frame: HeadlessFrame::Fit(frame),
            ..HeadlessOptions::default()
        },
    )
    .expect("render")
    .surface
    .data()
    .to_vec()
}

#[test]
fn group_then_ungroup_is_pixel_identical_over_the_corpus() {
    let Some(files) = corpus_files() else {
        return;
    };
    let mut checked = 0;
    for path in files {
        let Ok(mut s) = Session::open(DocumentId(1), &path) else {
            continue;
        };
        // The editable layer with the most objects.
        let layer = s
            .doc
            .tree
            .preorder(s.doc.tree.root())
            .filter(|n| {
                matches!(s.doc.tree.kind(*n),
                    Some(xarast_doc::NodeKind::Layer(l)) if l.visible && !l.locked && !l.guide)
            })
            .max_by_key(|l| s.doc.tree.children(*l).count());
        let Some(layer) = layer else { continue };
        let objs: Vec<_> = xarast_app::edit::selectable_objects(&s.doc)
            .filter(|n| s.doc.tree.links(*n).parent == Some(layer))
            .collect();
        if objs.len() < 2 {
            continue;
        }
        let frame = xarast_app::viewport::drawing_or_page_rect(&s.doc);
        let digest = s.doc.canonical_digest();
        let before = render(&s, frame);
        s.apply(Intent::Select {
            nodes: objs.clone(),
            mode: SelectMode::Replace,
        })
        .unwrap();
        s.apply(Intent::Group).unwrap();
        assert_eq!(render(&s, frame), before, "{}: grouping", path.display());
        s.apply(Intent::Ungroup).unwrap();
        let after: Vec<_> = xarast_app::edit::selectable_objects(&s.doc)
            .filter(|n| s.doc.tree.links(*n).parent == Some(layer))
            .collect();
        assert_eq!(after, objs, "{}: z-order", path.display());
        assert_eq!(render(&s, frame), before, "{}: ungrouping", path.display());
        s.undo();
        s.undo();
        assert_eq!(s.doc.canonical_digest(), digest, "{}: undo", path.display());
        checked += 1;
    }
    eprintln!("group/ungroup checked on {checked} corpus files");
}
