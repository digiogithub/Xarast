//! The text tool driven through intents, as the shell drives it (phase 9,
//! T9.4.1–T9.4.4): entering a story, the caret and selection, keyboard
//! navigation, pending new stories — and that none of it touches the
//! document.

use std::path::Path;
use std::sync::Arc;

use xarast_app::fonts::FontService;
use xarast_app::text_edit::Caret;
use xarast_app::text_tool::{TextEditing, TextSelection};
use xarast_app::tool::DRAG_THRESHOLD_PX;
use xarast_app::{
    DevicePoint, DocumentId, Intent, OverlayShape, PointerButton, PointerSample, Session, TextKey,
    TextNav, ToolAction, ToolId,
};
use xarast_doc::builder::{BuildLimits, skeleton};
use xarast_doc::{AttrValue, NodeId, NodeKind, TextItem, TextStoryNode, TypefaceRef};
use xarast_geom::{Matrix, Mp, Point, Vector};

fn fonts() -> Arc<FontService> {
    FontService::from_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../xarast-text/tests/fonts"))
}

/// The story's origin (its first baseline's left end), in millipoints.
const X0: i32 = 100_000;
const Y0: i32 = 400_000;

/// A session holding one point story at (100 pt, 400 pt), 20 pt Noto Sans:
/// "Hello world" and "Second line" as two paragraphs.
fn fixture() -> (Session, NodeId) {
    xarast_app::fonts::set_shared(fonts());
    let mut b = skeleton(BuildLimits::default()).unwrap();
    let story = b
        .node(NodeKind::TextStory(Box::new(TextStoryNode {
            transform: Matrix::translate(Vector::new(Mp::new(X0), Mp::new(Y0))),
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
    for line in ["Hello world", "Second line"] {
        b.node(NodeKind::TextLine(Box::default())).unwrap();
        b.push_scope().unwrap();
        for c in line.chars() {
            b.node(NodeKind::TextItem(TextItem::Char(c))).unwrap();
        }
        b.node(NodeKind::TextItem(TextItem::LineBreak(true)))
            .unwrap();
        b.pop_scope();
    }
    b.pop_scope();
    let (doc, _) = b.finish().unwrap();
    let mut s = Session::adopt(DocumentId(1), doc, None);
    // Frame the story at a known zoom.
    s.apply(Intent::SetZoom {
        zoom: 1.0,
        anchor: None,
    })
    .unwrap();
    let c = s.viewport.doc_to_device(Point::raw(X0 + 60_000, Y0));
    let size = s.viewport.size();
    s.apply(Intent::Pan {
        dx: f64::from(size.width) / 2.0 - c.x,
        dy: f64::from(size.height) / 2.0 - c.y,
    })
    .unwrap();
    (s, story)
}

fn dev(s: &Session, x: i32, y: i32) -> DevicePoint {
    s.viewport.doc_to_device(Point::raw(x, y))
}

fn sample(at: DevicePoint, t: u64) -> PointerSample {
    PointerSample {
        at,
        pressure: None,
        time_ms: t,
    }
}

fn press(s: &mut Session, at: DevicePoint, t: u64) {
    s.apply(Intent::PointerMove(sample(at, t))).unwrap();
    s.apply(Intent::PointerDown {
        button: PointerButton::Primary,
        sample: sample(at, t),
    })
    .unwrap();
}

fn release(s: &mut Session, at: DevicePoint, t: u64) {
    s.apply(Intent::PointerUp {
        button: PointerButton::Primary,
        sample: sample(at, t),
    })
    .unwrap();
}

fn click(s: &mut Session, x: i32, y: i32, t: u64) {
    let p = dev(s, x, y);
    press(s, p, t);
    release(s, p, t);
}

fn drag(s: &mut Session, from: (i32, i32), to: (i32, i32)) {
    let a = dev(s, from.0, from.1);
    let b = dev(s, to.0, to.1);
    press(s, a, 10_000);
    let mid = DevicePoint::new(a.x + DRAG_THRESHOLD_PX + 2.0, a.y);
    s.apply(Intent::PointerMove(sample(mid, 10_010))).unwrap();
    s.apply(Intent::PointerMove(sample(b, 10_020))).unwrap();
    release(s, b, 10_030);
}

fn nav(s: &mut Session, key: TextKey, word: bool, extend: bool) {
    s.apply(Intent::TextNav(TextNav { key, word, extend }))
        .unwrap();
}

fn selection(s: &Session) -> TextSelection {
    match s.text_state() {
        Some(TextEditing::Story(sel)) => sel,
        other => panic!("not editing a story: {other:?}"),
    }
}

fn text_tool(s: &mut Session) {
    s.apply(Intent::ChooseTool(ToolId::Text)).unwrap();
    assert_eq!(s.tools().current(), ToolId::Text);
}

/// Nothing the text tool did so far is a document edit.
fn untouched<D: PartialEq>(s: &Session, digest: &D, before: &D) {
    assert!(digest == before, "the document changed");
    assert!(s.undo_label().is_none(), "an undo step was recorded");
}

#[test]
fn a_click_in_a_story_places_the_caret_and_the_keys_move_it() {
    let (mut s, story) = fixture();
    let before = s.doc.canonical_digest();
    text_tool(&mut s);
    assert!(!s.text_editing());
    // Just left of the "H": the start of the story.
    click(&mut s, X0 + 200, Y0 + 5_000, 0);
    let sel = selection(&s);
    assert_eq!(sel.story, story);
    assert_eq!(sel.head.byte, 0);
    assert!(sel.is_caret());
    assert!(s.text_editing());
    assert_eq!(s.edit.selection().collect::<Vec<_>>(), [story]);

    nav(&mut s, TextKey::Right, false, false);
    assert_eq!(selection(&s).head.byte, 1);
    nav(&mut s, TextKey::Right, true, false);
    assert_eq!(selection(&s).head.byte, 6, "Ctrl+Right: the next word");
    nav(&mut s, TextKey::End, false, false);
    assert_eq!(selection(&s).head.byte, 11);
    nav(&mut s, TextKey::Home, false, true);
    let sel = selection(&s);
    assert_eq!((sel.anchor.byte, sel.head.byte), (11, 0), "Shift extends");
    // An arrow without Shift collapses the selection to its end.
    nav(&mut s, TextKey::Right, false, false);
    assert_eq!(selection(&s).range(), 11..11);
    nav(&mut s, TextKey::Down, false, false);
    let down = selection(&s).head.byte;
    assert!((12..=23).contains(&down), "on the second line: {down}");
    nav(&mut s, TextKey::End, true, false);
    assert_eq!(selection(&s).head.byte, 23, "Ctrl+End: the story's end");
    nav(&mut s, TextKey::Home, true, true);
    assert_eq!(selection(&s).range(), 0..23);
    // The model's view of the caret: line and item.
    let cur = selection(&s).cursor(&s.doc).unwrap();
    assert_eq!(cur.story, story);
    assert_eq!(cur.head.item, 0);
    untouched(&s, &s.doc.canonical_digest(), &before);
}

#[test]
fn the_overlay_draws_the_caret_and_the_selection() {
    let (mut s, _) = fixture();
    text_tool(&mut s);
    click(&mut s, X0 + 200, Y0 + 5_000, 0);
    let carets = |s: &Session| {
        s.overlay()
            .into_iter()
            .filter_map(|o| match o {
                OverlayShape::Caret {
                    from,
                    to,
                    primary,
                    moved,
                } => Some((from, to, primary, moved)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let c = carets(&s);
    assert_eq!(c.len(), 1);
    let (from, to, primary, moved) = c[0];
    assert!(primary);
    // Upright story: a vertical caret across the baseline at the origin.
    assert_eq!(from.x, to.x);
    assert!((from.x.raw() - X0).abs() < 500, "{from:?}");
    assert!(from.y.raw() < Y0 && to.y.raw() > Y0 + 10_000);
    nav(&mut s, TextKey::Right, false, true);
    nav(&mut s, TextKey::Right, false, true);
    let o = s.overlay();
    let highlights = o
        .iter()
        .filter(|o| matches!(o, OverlayShape::Highlight { .. }))
        .count();
    assert_eq!(highlights, 1);
    // The caret moved, so the blink restarts.
    assert_ne!(carets(&s)[0].3, moved);
}

#[test]
fn esc_leaves_the_text_and_delete_never_deletes_the_story() {
    let (mut s, story) = fixture();
    let before = s.doc.canonical_digest();
    text_tool(&mut s);
    click(&mut s, X0 + 30_000, Y0 + 5_000, 0);
    s.apply(Intent::DeleteSelection).unwrap();
    s.apply(Intent::ToolAction(ToolAction::Finish)).unwrap();
    assert!(s.doc.tree.contains(story));
    untouched(&s, &s.doc.canonical_digest(), &before);
    s.apply(Intent::SelectAll).unwrap();
    assert_eq!(selection(&s).range(), 0..23);
    s.apply(Intent::Cancel).unwrap();
    assert!(!s.text_editing());
    assert_eq!(
        s.edit.selection().collect::<Vec<_>>(),
        [story],
        "Esc leaves the story selected"
    );
    // With no caret, Delete is the object's again.
    s.apply(Intent::DeleteSelection).unwrap();
    assert!(!s.doc.tree.is_reachable(story));
}

#[test]
fn clicks_and_drags_on_empty_canvas_prepare_a_new_story() {
    let (mut s, _) = fixture();
    let before = s.doc.canonical_digest();
    text_tool(&mut s);
    click(&mut s, 300_000, 200_000, 0);
    match s.text_state() {
        Some(TextEditing::Pending { at, column: None }) => {
            assert!((at.x.raw() - 300_000).abs() < 1_500, "{at:?}");
            assert!((at.y.raw() - 200_000).abs() < 1_500, "{at:?}");
        }
        other => panic!("{other:?}"),
    }
    assert!(s.edit.is_selection_empty());
    assert!(s.text_editing(), "the keys are the new text's");
    drag(&mut s, (300_000, 300_000), (420_000, 250_000));
    match s.text_state() {
        Some(TextEditing::Pending {
            at,
            column: Some(w),
        }) => {
            assert!((w.raw() - 120_000).abs() < 1_500, "{w:?}");
            assert!((at.x.raw() - 300_000).abs() < 1_500, "{at:?}");
            // The first line hangs from the top of the drag.
            assert!(at.y.raw() < 300_000 && at.y.raw() > 280_000, "{at:?}");
        }
        other => panic!("{other:?}"),
    }
    let rects = s
        .overlay()
        .iter()
        .filter(|o| matches!(o, OverlayShape::Rect { dashed: true, .. }))
        .count();
    assert_eq!(rects, 1, "the column is outlined");
    untouched(&s, &s.doc.canonical_digest(), &before);
}

#[test]
fn double_click_selects_a_word_triple_click_a_line_drag_a_range() {
    let (mut s, _) = fixture();
    text_tool(&mut s);
    // Inside "world".
    let (x, y) = (X0 + 80_000, Y0 + 5_000);
    click(&mut s, x, y, 0);
    click(&mut s, x, y, 100);
    assert_eq!(selection(&s).range(), 6..11);
    click(&mut s, x, y, 200);
    assert_eq!(selection(&s).range(), 0..11);
    // A drag from the start of the first line into the second.
    drag(&mut s, (X0 + 200, Y0 + 5_000), (X0 + 30_000, Y0 - 20_000));
    let r = selection(&s).range();
    assert_eq!(r.start, 0);
    assert!(r.end > 12, "{r:?}");
    assert!(s.text_editing());
}

#[test]
fn a_cancelled_drag_restores_the_caret() {
    let (mut s, _) = fixture();
    text_tool(&mut s);
    click(&mut s, X0 + 200, Y0 + 5_000, 0);
    let before = selection(&s);
    let a = dev(&s, X0 + 200, Y0 + 5_000);
    let b = dev(&s, X0 + 60_000, Y0 + 5_000);
    press(&mut s, a, 5_000);
    s.apply(Intent::PointerMove(sample(b, 5_010))).unwrap();
    assert!(!selection(&s).is_caret());
    s.apply(Intent::Cancel).unwrap();
    assert_eq!(selection(&s), before);
}

#[test]
fn choosing_the_tool_with_a_story_selected_edits_it() {
    let (mut s, story) = fixture();
    s.apply(Intent::Select {
        nodes: vec![story],
        mode: xarast_app::SelectMode::Replace,
    })
    .unwrap();
    text_tool(&mut s);
    let sel = selection(&s);
    assert_eq!(sel.story, story);
    assert_eq!(sel.head, Caret::after(23));
    // Another tool ends the editing.
    s.apply(Intent::ChooseTool(ToolId::Selector)).unwrap();
    assert!(!s.text_editing());
    assert!(s.text_state().is_none());
}

#[test]
fn the_text_tool_is_chosen_with_f8_and_shows_an_i_beam() {
    let (mut s, _) = fixture();
    let c = xarast_app::AppCommand::Tool(ToolId::Text);
    assert_eq!(c.primary_shortcut().unwrap().to_string(), "F8");
    s.apply(c.intent(DevicePoint::new(0.0, 0.0))).unwrap();
    assert_eq!(s.cursor(), xarast_app::CursorKind::Text);
    assert!(ToolId::Text.is_available() && ToolId::Text.is_implemented());
}

#[test]
fn undoing_under_the_caret_keeps_it_valid() {
    // The story is deleted and restored under a caret: the tool never
    // points at a story that is gone.
    let (mut s, story) = fixture();
    s.apply(Intent::Select {
        nodes: vec![story],
        mode: xarast_app::SelectMode::Replace,
    })
    .unwrap();
    s.apply(Intent::DeleteSelection).unwrap();
    s.apply(Intent::Undo).unwrap();
    text_tool(&mut s);
    click(&mut s, X0 + 200, Y0 + 5_000, 0);
    assert!(s.text_editing());
    s.apply(Intent::ChooseTool(ToolId::Selector)).unwrap();
    s.apply(Intent::Select {
        nodes: vec![story],
        mode: xarast_app::SelectMode::Replace,
    })
    .unwrap();
    s.apply(Intent::DeleteSelection).unwrap();
    text_tool(&mut s);
    assert!(!s.text_editing());
    nav(&mut s, TextKey::Right, false, false);
    assert!(s.text_state().is_none());
}

#[test]
fn a_double_click_on_text_with_the_selector_opens_the_text_tool() {
    let (mut s, story) = fixture();
    assert_eq!(s.tools().current(), ToolId::Selector);
    click(&mut s, X0 + 30_000, Y0 + 5_000, 0);
    click(&mut s, X0 + 30_000, Y0 + 5_000, 100);
    assert_eq!(s.tools().current(), ToolId::Text);
    assert_eq!(selection(&s).story, story);
}
