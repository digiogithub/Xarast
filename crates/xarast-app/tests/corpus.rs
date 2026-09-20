//! The scene walker against real input.
//!
//! Every assertion here is about a property the walker must hold for
//! **any** document, checked against the 59 real `.xar` files of the
//! Xara Xtreme fork rather than against fixtures we wrote ourselves. A
//! synthetic document only ever exercises what we thought of.
//!
//! **No corpus byte enters this repository.** The files are found
//! through `XARAST_XAR_CORPUS` (default `/home/user/xara-xtreme`) and
//! verified against `tests/corpus/corpus.lock`, which records paths,
//! sizes and SHA-256 hashes and nothing else — the same harness
//! `xarast-xar` uses. With no corpus present every test skips with a
//! printed notice, so CI stays green; `XARAST_CORPUS_REQUIRED=1` turns
//! that skip into a failure.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use xarast_app::{DeviceSize, DocumentId, HeadlessOptions, Session, build_scene, headless};
use xarast_render::{DeviceRect, RenderQuality};

const LOCK: &str = include_str!("../../../tests/corpus/corpus.lock");

#[derive(Clone, Debug)]
struct CorpusFile {
    rel: String,
    size: u64,
    sha256: String,
}

#[derive(Debug)]
struct Corpus {
    root: PathBuf,
    files: Vec<CorpusFile>,
}

fn lock_entries() -> Vec<CorpusFile> {
    LOCK.lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let sha256 = it.next()?.to_owned();
            let size = it.next()?.parse().ok()?;
            let rel = it.collect::<Vec<_>>().join(" ");
            Some(CorpusFile { rel, size, sha256 })
        })
        .collect()
}

impl Corpus {
    fn discover() -> Option<Corpus> {
        let required = std::env::var("XARAST_CORPUS_REQUIRED").as_deref() == Ok("1");
        let root = PathBuf::from(
            std::env::var("XARAST_XAR_CORPUS").unwrap_or_else(|_| "/home/user/xara-xtreme".into()),
        );
        let files = lock_entries();
        assert_eq!(files.len(), 59, "corpus.lock must list exactly 59 files");
        if !root.is_dir() {
            assert!(
                !required,
                "XARAST_CORPUS_REQUIRED=1 but {} is not a directory",
                root.display()
            );
            return None;
        }
        for f in &files {
            let p = root.join(&f.rel);
            let Ok(bytes) = std::fs::read(&p) else {
                assert!(
                    !required,
                    "XARAST_CORPUS_REQUIRED=1 but {} is missing",
                    p.display()
                );
                return None;
            };
            assert_eq!(bytes.len() as u64, f.size, "{} changed size", f.rel);
            assert_eq!(
                format!("{:x}", Sha256::digest(&bytes)),
                f.sha256,
                "{} changed content",
                f.rel
            );
        }
        Some(Corpus { root, files })
    }

    fn path(&self, f: &CorpusFile) -> PathBuf {
        self.root.join(&f.rel)
    }
}

macro_rules! corpus_or_skip {
    () => {
        match Corpus::discover() {
            Some(c) => c,
            None => {
                println!(
                    "skipping: no .xar corpus (set XARAST_XAR_CORPUS; \
                     XARAST_CORPUS_REQUIRED=1 to make this a failure)"
                );
                return;
            }
        }
    };
}

fn open(path: &Path) -> Session {
    Session::open(DocumentId(1), path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Every corpus file walks into a well-formed scene.
///
/// "Well formed" is the walker's whole contract: the recording is
/// balanced (`SceneBuilder::finish` rejects an unmatched push or pop, so
/// an `Ok` here *is* the assertion), every primitive belongs to a node
/// the scene knows about, and no command carries an unrenderable paint.
#[test]
fn every_corpus_file_walks_into_a_well_formed_scene() {
    let c = corpus_or_skip!();
    let mut with_ink = 0usize;
    let mut total_primitives = 0usize;
    for f in &c.files {
        let path = c.path(f);
        let mut s = open(&path);
        let stats = s
            .rebuild_scene(None)
            .unwrap_or_else(|e| panic!("{}: unbalanced scene: {e}", f.rel));

        total_primitives += stats.primitives();
        if stats.primitives() > 0 {
            with_ink += 1;
        }

        // Every group the scene opened is a node it can name, and every
        // node it named lies inside the op list.
        let ops = s.scene().len();
        for (id, info) in s.scene().nodes() {
            assert!(
                info.first_op <= info.last_op && info.last_op <= ops,
                "{}: node {id:?} spans {}..{} of {ops} ops",
                f.rel,
                info.first_op,
                info.last_op
            );
        }

        // A scene cannot hold more primitives than the document holds
        // nodes: every fill, stroke and image comes from exactly one ink
        // node, and a node emits at most a fill and a stroke.
        assert!(
            stats.primitives() <= 2 * s.doc.tree.node_count(),
            "{}: {} primitives from {} nodes",
            f.rel,
            stats.primitives(),
            s.doc.tree.node_count()
        );
    }
    println!(
        "{} of {} files produced ink; {total_primitives} primitives in total",
        with_ink,
        c.files.len()
    );
    assert!(
        with_ink >= 36,
        "only {with_ink} of {} corpus files produced any ink at all",
        c.files.len()
    );
    assert!(
        total_primitives > 20_000,
        "the corpus should be substantial"
    );
}

/// A file that draws nothing must draw nothing for a **named** reason.
///
/// This is the test that keeps the walker honest as the phases land: a
/// silent regression that stops drawing paths would show up as a blank
/// file with no pending counter to explain it. Twenty-one corpus files
/// are blank today, and every one of them is an empty template, a
/// text-only design (Phase 9) or a quick shape with no cached path
/// (Phase 7).
#[test]
fn a_blank_file_is_blank_for_a_reason_the_walker_reports() {
    let c = corpus_or_skip!();
    let mut blank = Vec::new();
    for f in &c.files {
        let mut s = open(&c.path(f));
        let stats = s.rebuild_scene(None).expect("scene");
        if stats.primitives() > 0 {
            continue;
        }
        let w = s.walk_stats();
        let has_ink_nodes = s
            .doc
            .tree
            .preorder(s.doc.tree.root())
            .any(|id| s.doc.tree.kind(id).is_some_and(|k| k.is_ink()));
        assert!(
            !has_ink_nodes || w.text_pending > 0 || w.shapes_pending > 0 || w.images_pending > 0,
            "{}: drew nothing and reported no reason: {w:?}",
            f.rel
        );
        blank.push(f.rel.clone());
    }
    println!("{} blank files, all explained: {blank:?}", blank.len());
}

/// The same inputs produce the same scene, every time.
///
/// Stability is what makes the scene diffable and the render cache
/// sound. It is not automatic: a `HashMap` iterated into the op list, a
/// `NodeId` used as a scene id, or an interning cache keyed on pointer
/// identity would all break it.
#[test]
fn the_walk_is_stable_across_runs_and_across_walkers() {
    let c = corpus_or_skip!();
    for f in &c.files {
        let s = open(&c.path(f));
        let a = build_scene(&s, None);
        let b = build_scene(&s, None);
        assert_eq!(a.scene, b.scene, "{}: two walks disagreed", f.rel);

        // And a walker reused across frames agrees with a fresh one.
        let mut reused = open(&c.path(f));
        reused.rebuild_scene(None).expect("first");
        let first = reused.scene().clone();
        reused.rebuild_scene(None).expect("second");
        assert_eq!(&first, reused.scene(), "{}: a reused walker drifted", f.rel);
        assert_eq!(
            &a.scene,
            reused.scene(),
            "{}: reuse changed the scene",
            f.rel
        );
    }
}

/// Walking never changes the document.
///
/// The walker takes `&Document` so this cannot happen by assignment, but
/// it could still happen through an interior-mutability cache. The
/// canonical digest is the check that costs nothing to write and would
/// have caught it.
#[test]
fn walking_does_not_mutate_the_document() {
    let c = corpus_or_skip!();
    for f in &c.files {
        let mut s = open(&c.path(f));
        let before = s.doc.canonical_digest();
        s.rebuild_scene(None).expect("scene");
        let _ = build_scene(&s, Some(DeviceRect::new(0, 0, 64, 64)));
        assert_eq!(
            s.doc.canonical_digest(),
            before,
            "{}: the walk mutated the document",
            f.rel
        );
    }
}

/// A dirty rectangle prunes the walk without producing a scene the full
/// walk could not have produced.
#[test]
fn dirty_rect_culling_only_ever_removes_work() {
    let c = corpus_or_skip!();
    let mut culled_anything = false;
    for f in &c.files {
        let mut s = open(&c.path(f));
        s.apply(xarast_app::Intent::Resize(DeviceSize::new(512, 512)))
            .expect("resize");

        let full = s.rebuild_scene(None).expect("full");
        // A dirty rectangle the size of the viewport still culls what
        // lies off screen, which is the point of it. A rectangle far
        // larger than the document must not.
        let everything = s
            .rebuild_scene(Some(DeviceRect::new(-1 << 20, -1 << 20, 1 << 20, 1 << 20)))
            .expect("everything");
        assert_eq!(
            full.primitives(),
            everything.primitives(),
            "{}: a dirty rect larger than the document must cull nothing",
            f.rel
        );
        let whole = s
            .rebuild_scene(Some(DeviceRect::new(0, 0, 512, 512)))
            .expect("whole viewport");
        assert!(
            whole.primitives() <= full.primitives(),
            "{}: culling to the viewport produced more work than the full walk",
            f.rel
        );

        let tiny = s
            .rebuild_scene(Some(DeviceRect::new(250, 250, 258, 258)))
            .expect("tiny");
        assert!(
            tiny.primitives() <= full.primitives(),
            "{}: culling produced more work, not less",
            f.rel
        );
        if tiny.primitives() < full.primitives() {
            culled_anything = true;
            assert!(
                s.walk_stats().culled > 0,
                "{}: culled work but counted none",
                f.rel
            );
        }

        // An empty dirty rectangle is not a licence to draw nothing
        // wrong: it must still balance.
        s.rebuild_scene(Some(DeviceRect::EMPTY))
            .expect("empty dirty rect");
    }
    assert!(culled_anything, "no corpus file exercised culling at all");
}

/// Draft and Final differ in the tables they intern, not in the shape of
/// the scene.
#[test]
fn quality_changes_the_tables_and_not_the_command_list() {
    let c = corpus_or_skip!();
    for f in &c.files {
        let mut s = open(&c.path(f));
        let final_stats = s.rebuild_scene(None).expect("final");
        s.quality = RenderQuality::Draft;
        let draft_stats = s.rebuild_scene(None).expect("draft");
        assert_eq!(
            final_stats, draft_stats,
            "{}: quality changed the command counts",
            f.rel
        );
    }
}

/// The headless path renders every corpus file to pixels, with no
/// window, no GPU and no compositor — and draws something.
#[test]
fn the_headless_path_renders_the_whole_corpus() {
    let c = corpus_or_skip!();
    let opts = HeadlessOptions {
        size: DeviceSize::new(192, 192),
        ..HeadlessOptions::default()
    };
    let mut blank = Vec::new();
    for f in &c.files {
        let s = open(&c.path(f));
        let out = headless::render(&s, &opts).unwrap_or_else(|e| panic!("{}: {e}", f.rel));
        assert_eq!(out.surface.width(), 192);
        assert_eq!(out.surface.height(), 192);
        let painted = out
            .surface
            .data()
            .chunks_exact(4)
            .filter(|p| p[0] != 255 || p[1] != 255 || p[2] != 255)
            .count();
        if painted == 0 {
            blank.push(f.rel.clone());
        }
        // Whatever it drew, it drew inside the surface and left it
        // fully opaque: the background is opaque and nothing composites
        // a hole in it.
        assert!(
            out.surface.data().chunks_exact(4).all(|p| p[3] == 255),
            "{}: the render punched a hole in an opaque background",
            f.rel
        );
    }
    println!(
        "{} of {} files rendered blank: {blank:?}",
        blank.len(),
        c.files.len()
    );
    assert!(
        blank.len() <= 24,
        "{} corpus files rendered blank, which is more than the known empty, \
         text-only and quick-shape ones: {blank:?}",
        blank.len()
    );
}

/// The same document renders to the same pixels twice: the CPU backend
/// is the deterministic path and the walker must not spoil it.
#[test]
fn the_headless_render_is_reproducible() {
    let c = corpus_or_skip!();
    let opts = HeadlessOptions {
        size: DeviceSize::new(128, 128),
        ..HeadlessOptions::default()
    };
    for f in c.files.iter().take(12) {
        let s = open(&c.path(f));
        let a = headless::render(&s, &opts).expect("a");
        let b = headless::render(&s, &opts).expect("b");
        assert_eq!(
            xarast_render::golden::digest(&a.surface),
            xarast_render::golden::digest(&b.surface),
            "{}: two headless renders disagreed",
            f.rel
        );
    }
}
