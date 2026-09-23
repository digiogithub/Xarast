//! Convert text to shapes (phase 9, W9.6, XARA-US-0049): a story becomes
//! a group of glyph outlines that renders exactly as the text did, keeps
//! the story's text, and undoes exactly — on a synthetic story and on
//! every story of the `TextDesigns/` corpus (pinned fonts).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kurbo::Shape;
use xarast_app::convert::ConvertCommand;
use xarast_app::fonts::FontService;
use xarast_app::headless::{self, HeadlessFrame, HeadlessOptions};
use xarast_app::{
    AppCommand, DevicePoint, DeviceSize, DocRect, DocumentId, EditState, SceneWalker, SelectMode,
    Session, Viewport,
};
use xarast_color::{Colour, ColourValue};
use xarast_doc::builder::{BuildLimits, skeleton};
use xarast_doc::{
    AttrSlot, AttrValue, Document, NodeId, NodeKind, TextItem, TextStoryNode, TypefaceRef,
    is_text_slot,
};
use xarast_geom::{FillRule, Matrix, Mp, Point, Rect, Vector};
use xarast_render::RenderQuality;

fn fonts() -> Arc<FontService> {
    FontService::from_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../xarast-text/tests/fonts"))
}

fn fill(r: f32, g: f32, b: f32) -> AttrValue {
    AttrValue::Fill(xarast_doc::fill::Paint::Flat {
        value: Colour::Direct(ColourValue::rgbt(r, g, b, 0.0)),
    })
}

/// A layer whose winding rule is even-odd, holding a group holding one
/// named 20 pt Noto Sans story: "Hello " in red, "world" in blue, then a
/// second paragraph "Second line" in blue.
fn fixture() -> (Document, NodeId, NodeId) {
    let mut b = skeleton(BuildLimits::default()).unwrap();
    b.attribute(AttrValue::WindingRule(FillRule::EvenOdd))
        .unwrap();
    let group = b.node(NodeKind::Group(Box::default())).unwrap().node_id();
    b.push_scope().unwrap();
    let story = b
        .node(NodeKind::TextStory(Box::new(TextStoryNode {
            transform: Matrix::translate(Vector::new(Mp::new(100_000), Mp::new(400_000))),
            ..TextStoryNode::default()
        })))
        .unwrap()
        .node_id();
    b.push_scope().unwrap();
    b.attribute(AttrValue::FontTypeface(Arc::new(TypefaceRef {
        full_name: Arc::from("Noto Sans"),
        family: Arc::from("Noto Sans"),
        panose: None,
    })))
    .unwrap();
    b.attribute(AttrValue::FontSize(Mp::new(20_000))).unwrap();
    b.attribute(AttrValue::ObjectName(Arc::from("Greeting")))
        .unwrap();
    b.attribute(fill(0.9, 0.1, 0.1)).unwrap();
    b.node(NodeKind::TextLine(Box::default())).unwrap();
    b.push_scope().unwrap();
    for c in "Hello ".chars() {
        b.node(NodeKind::TextItem(TextItem::Char(c))).unwrap();
    }
    b.attribute(fill(0.1, 0.2, 0.9)).unwrap();
    for c in "world".chars() {
        b.node(NodeKind::TextItem(TextItem::Char(c))).unwrap();
    }
    b.node(NodeKind::TextItem(TextItem::LineBreak(true)))
        .unwrap();
    b.pop_scope();
    b.node(NodeKind::TextLine(Box::default())).unwrap();
    b.push_scope().unwrap();
    b.attribute(fill(0.1, 0.2, 0.9)).unwrap();
    for c in "Second line".chars() {
        b.node(NodeKind::TextItem(TextItem::Char(c))).unwrap();
    }
    b.node(NodeKind::TextItem(TextItem::LineBreak(true)))
        .unwrap();
    b.pop_scope();
    b.pop_scope();
    b.pop_scope();
    let (doc, _) = b.finish().unwrap();
    (doc, group, story)
}

fn render(s: &Session, fonts: &Arc<FontService>, frame: DocRect) -> Vec<u8> {
    let opts = HeadlessOptions {
        size: DeviceSize::new(800, 600),
        frame: HeadlessFrame::Fit(frame),
        ..HeadlessOptions::default()
    };
    let r = headless::render_with_fonts(s, &opts, Some(Arc::clone(fonts))).expect("render");
    r.surface.data().to_vec()
}

/// `(differing pixels, largest channel difference)`.
fn diff(a: &[u8], b: &[u8]) -> (usize, u8) {
    assert_eq!(a.len(), b.len());
    let mut n = 0;
    let mut max = 0u8;
    for (pa, pb) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0) {
        let d = pa
            .iter()
            .zip(pb)
            .map(|(x, y)| x.abs_diff(*y))
            .max()
            .unwrap_or(0);
        if d > 0 {
            n += 1;
            max = max.max(d);
        }
    }
    (n, max)
}

/// The drawn text's box, as the walker measures it.
fn text_ink(doc: &Document, fonts: &Arc<FontService>) -> Rect {
    let mut w = SceneWalker::with_fonts(Arc::clone(fonts));
    let mut scene = xarast_render::Scene::new();
    let edit = EditState::for_document(doc);
    let vp = Viewport::new(DeviceSize::new(800, 600));
    w.rebuild(doc, &edit, &vp, RenderQuality::Final, None, &mut scene)
        .unwrap();
    w.text_ink()
}

/// The tight box of every path under `nodes`.
fn outline_bounds(doc: &Document, nodes: &[NodeId]) -> Rect {
    let mut r = Rect::EMPTY;
    for &g in nodes {
        for n in doc.tree.preorder(g) {
            if let Some(NodeKind::Path(p)) = doc.tree.kind(n) {
                let b = p.data.to_bez_path().bounding_box();
                let b = Rect::new(
                    Point::new(Mp::from_f64_round(b.x0), Mp::from_f64_round(b.y0)),
                    Point::new(Mp::from_f64_round(b.x1), Mp::from_f64_round(b.y1)),
                );
                r = if r.is_empty() { b } else { r.union(b) };
            }
        }
    }
    r
}

fn within_1mp(a: Rect, b: Rect) -> bool {
    let d = |x: Mp, y: Mp| (x.raw() - y.raw()).abs() <= 1;
    d(a.lo.x, b.lo.x) && d(a.lo.y, b.lo.y) && d(a.hi.x, b.hi.x) && d(a.hi.y, b.hi.y)
}

#[test]
fn a_story_becomes_a_group_of_run_outlines_that_draws_the_same() {
    let fonts = fonts();
    xarast_app::fonts::set_shared(Arc::clone(&fonts));
    let (doc, group, story) = fixture();
    let mut s = Session::adopt(DocumentId(1), doc, None);
    let frame = xarast_app::viewport::drawing_or_page_rect_with(&s.doc, Some(&fonts));
    let ink = text_ink(&s.doc, &fonts);
    let before = render(&s, &fonts, frame);
    let digest = s.doc.canonical_digest();

    // Selecting the enclosing group converts the story inside it.
    s.apply(xarast_app::Intent::Select {
        nodes: vec![group],
        mode: SelectMode::Replace,
    })
    .unwrap();
    s.apply(AppCommand::ConvertToShapes.intent(DevicePoint::new(0.0, 0.0)))
        .unwrap();
    assert_eq!(s.undo_label(), Some("Convert to Editable Shapes"));
    assert_eq!(s.bus.history().len(), 1);

    // The story is gone from the tree; a group took its place.
    assert!(
        !s.doc.tree.preorder(group).any(|n| n == story),
        "story still attached"
    );
    let kids: Vec<NodeId> = s.doc.tree.children(group).collect();
    assert_eq!(kids.len(), 1);
    let shapes = kids[0];
    let Some(NodeKind::Group(g)) = s.doc.tree.kind(shapes) else {
        panic!("not a group: {:?}", s.doc.tree.kind(shapes));
    };
    assert_eq!(g.source_text.as_deref(), Some("Hello world\nSecond line"));
    // The selection is the group that was selected (unchanged).
    assert_eq!(s.edit.selection().collect::<Vec<_>>(), vec![group]);

    // One path per run (red, blue); the object name moved to the group;
    // no text attribute on a path; each path winds non-zero.
    let mut paths = 0;
    let mut names = 0;
    for c in s.doc.tree.children(shapes) {
        match s.doc.tree.kind(c) {
            Some(NodeKind::Path(_)) => {
                paths += 1;
                let own: Vec<&AttrValue> = s
                    .doc
                    .tree
                    .children(c)
                    .filter_map(|a| match s.doc.tree.kind(a) {
                        Some(NodeKind::Attr(a)) => Some(&a.value),
                        _ => None,
                    })
                    .collect();
                assert!(
                    own.iter()
                        .all(|v| v.slot().is_some_and(|slot| !is_text_slot(slot))),
                    "{own:?}"
                );
                assert!(
                    own.contains(&&AttrValue::WindingRule(FillRule::NonZero)),
                    "{own:?}"
                );
                assert!(own.iter().any(|v| v.slot() == Some(AttrSlot::FillGeometry)));
            }
            Some(NodeKind::Attr(a)) => {
                assert_eq!(a.value, AttrValue::ObjectName(Arc::from("Greeting")));
                names += 1;
            }
            other => panic!("unexpected child {other:?}"),
        }
    }
    assert_eq!((paths, names), (2, 1));

    // Same pixels, same ink box.
    let after = render(&s, &fonts, frame);
    assert_eq!(diff(&before, &after), (0, 0));
    assert!(
        within_1mp(outline_bounds(&s.doc, &[shapes]), ink),
        "{:?} vs {ink:?}",
        outline_bounds(&s.doc, &[shapes])
    );

    // Exact undo and redo.
    let converted = s.doc.canonical_digest();
    assert_eq!(s.undo(), Some("Convert to Editable Shapes"));
    assert_eq!(s.doc.canonical_digest(), digest);
    assert_eq!(s.redo(), Some("Convert to Editable Shapes"));
    assert_eq!(s.doc.canonical_digest(), converted);
}

#[test]
fn the_story_itself_selected_selects_the_group_it_became() {
    xarast_app::fonts::set_shared(fonts());
    let (doc, _, story) = fixture();
    let mut s = Session::adopt(DocumentId(1), doc, None);
    s.apply(xarast_app::Intent::Select {
        nodes: vec![story],
        mode: SelectMode::Replace,
    })
    .unwrap();
    s.apply(AppCommand::ConvertToShapes.intent(DevicePoint::new(0.0, 0.0)))
        .unwrap();
    let sel: Vec<NodeId> = s.edit.selection().collect();
    assert_eq!(sel.len(), 1);
    assert!(matches!(
        s.doc.tree.kind(sel[0]),
        Some(NodeKind::Group(g)) if g.source_text.is_some()
    ));
}

#[test]
fn nothing_convertible_is_no_undo_step() {
    xarast_app::fonts::set_shared(fonts());
    let (doc, _, story) = fixture();
    let mut s = Session::adopt(DocumentId(1), doc, None);
    // A story of spaces only has no ink: left alone.
    let digest = s.doc.canonical_digest();
    let empty = {
        let mut b = skeleton(BuildLimits::default()).unwrap();
        b.node(NodeKind::TextStory(Box::default())).unwrap();
        b.push_scope().unwrap();
        b.node(NodeKind::TextLine(Box::default())).unwrap();
        b.push_scope().unwrap();
        b.node(NodeKind::TextItem(TextItem::Char(' '))).unwrap();
        b.node(NodeKind::TextItem(TextItem::LineBreak(true)))
            .unwrap();
        b.pop_scope();
        b.pop_scope();
        b.finish().unwrap().0
    };
    let mut e = Session::adopt(DocumentId(2), empty, None);
    let blank = e
        .doc
        .tree
        .preorder(e.doc.tree.root())
        .find(|n| matches!(e.doc.tree.kind(*n), Some(NodeKind::TextStory(_))))
        .unwrap();
    let c = e
        .convert_to_shapes(ConvertCommand::new(vec![blank]))
        .unwrap();
    assert!(!c.contains(xarast_app::Changed::DOCUMENT));
    assert_eq!(e.bus.history().len(), 0);
    // An empty selection too.
    s.convert_to_shapes(ConvertCommand::new(Vec::new()))
        .unwrap();
    assert_eq!(s.bus.history().len(), 0);
    assert_eq!(s.doc.canonical_digest(), digest);
    let _ = story;
}

// ───────────────────────────────────────── the TextDesigns corpus

fn corpus() -> Option<PathBuf> {
    let root = PathBuf::from(
        std::env::var("XARAST_XAR_CORPUS").unwrap_or_else(|_| "/home/user/xara-xtreme".into()),
    );
    if root.join("TextDesigns").is_dir() {
        Some(root)
    } else {
        assert!(
            std::env::var("XARAST_CORPUS_REQUIRED").as_deref() != Ok("1"),
            "XARAST_CORPUS_REQUIRED=1 but no corpus"
        );
        eprintln!("skipping: no .xar corpus (set XARAST_XAR_CORPUS)");
        None
    }
}

/// Acceptance criterion 9 of phase 9 and XARA-US-0049: converting every
/// story of each `TextDesigns/` file draws the same pixels (tolerance:
/// **zero** differing pixels at 800 × 600 on the CPU backend), the
/// outlines' box matches the text's ink box to 1 mp, and one undo
/// restores the document exactly (canonical digest) and its render.
#[test]
fn every_text_designs_story_converts_to_the_same_pixels_and_undoes_exactly() {
    let Some(root) = corpus() else {
        return;
    };
    let fonts = fonts();
    xarast_app::fonts::set_shared(Arc::clone(&fonts));
    let mut files: Vec<PathBuf> = std::fs::read_dir(root.join("TextDesigns"))
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "xar"))
        .collect();
    files.sort();
    assert_eq!(files.len(), 14);
    let mut failures = Vec::new();
    let mut total = 0;
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(path).unwrap();
        let mut s = Session::open_bytes(DocumentId(1), path, &bytes).unwrap();
        let stories: Vec<NodeId> = s
            .doc
            .tree
            .preorder(s.doc.tree.root())
            .filter(|n| matches!(s.doc.tree.kind(*n), Some(NodeKind::TextStory(_))))
            .collect();
        assert!(!stories.is_empty(), "{name}: no story");
        let frame = xarast_app::viewport::drawing_or_page_rect_with(&s.doc, Some(&fonts));
        let ink = text_ink(&s.doc, &fonts);
        let before = render(&s, &fonts, frame);
        let digest = s.doc.canonical_digest();

        s.convert_to_shapes(ConvertCommand::with_fonts(
            stories.clone(),
            Arc::clone(&fonts),
        ))
        .unwrap();
        assert_eq!(s.bus.history().len(), 1, "{name}: one undo step");
        let groups: Vec<NodeId> = s.edit.selection().collect();
        let left = s
            .doc
            .tree
            .preorder(s.doc.tree.root())
            .filter(|n| matches!(s.doc.tree.kind(*n), Some(NodeKind::TextStory(_))))
            .count();
        // Every story with ink converted; one with none stays.
        assert_eq!(groups.len(), stories.len(), "{name}");
        total += stories.len() - left;
        let converted_groups: Vec<NodeId> = groups
            .iter()
            .copied()
            .filter(|g| matches!(s.doc.tree.kind(*g), Some(NodeKind::Group(_))))
            .collect();
        assert_eq!(converted_groups.len(), stories.len() - left, "{name}");

        let after = render(&s, &fonts, frame);
        let (n, max) = diff(&before, &after);
        if n > 0 {
            failures.push(format!("{name}: {n} pixels differ, by up to {max}"));
        }
        if left == 0 {
            let bounds = outline_bounds(&s.doc, &converted_groups);
            if !within_1mp(bounds, ink) {
                failures.push(format!("{name}: outline box {bounds:?} vs ink {ink:?}"));
            }
        }

        // The converted document survives `.xarast`: same pixels, and
        // every group keeps its source text.
        let mut package = std::io::Cursor::new(Vec::new());
        let opts = xarast_format::SaveOptions {
            write: xarast_format::WriteOptions::deterministic(),
            ..xarast_format::SaveOptions::default()
        };
        xarast_format::save_to(&s.doc, &mut package, &opts).unwrap();
        let reopened = Session::open_bytes(
            DocumentId(2),
            Path::new("converted.xarast"),
            &package.into_inner(),
        )
        .unwrap();
        let was_text = reopened
            .doc
            .tree
            .preorder(reopened.doc.tree.root())
            .filter(|n| {
                matches!(reopened.doc.tree.kind(*n),
                    Some(NodeKind::Group(g)) if g.source_text.is_some())
            })
            .count();
        if was_text != converted_groups.len() {
            failures.push(format!(
                "{name}: {was_text} groups keep their text after .xarast"
            ));
        }
        let (rn, _) = diff(&after, &render(&reopened, &fonts, frame));
        if rn > 0 {
            failures.push(format!("{name}: {rn} pixels differ after .xarast"));
        }

        assert!(s.undo().is_some());
        if s.doc.canonical_digest() != digest {
            failures.push(format!("{name}: undo is not exact"));
        }
        let undone = render(&s, &fonts, frame);
        if diff(&before, &undone).0 != 0 {
            failures.push(format!("{name}: render after undo differs"));
        }
        eprintln!(
            "{name}: {} stories converted, {n} pixels differ",
            stories.len() - left
        );
    }
    assert!(total > 0);
    assert!(failures.is_empty(), "{failures:#?}");
}
