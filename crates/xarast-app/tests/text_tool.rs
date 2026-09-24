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
    DevicePoint, DocumentId, Intent, OverlayShape, PointerButton, PointerSample, Session,
    TextInput, TextInputKind, TextKey, TextNav, ToolAction, ToolId,
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
    let at = selection(&s).head.byte;
    // Edit › Delete deletes the character after the caret, Enter breaks
    // the paragraph: text edits, never the story object.
    s.apply(Intent::DeleteSelection).unwrap();
    s.apply(Intent::ToolAction(ToolAction::Finish)).unwrap();
    assert!(s.doc.tree.is_reachable(story));
    let mut want = String::from("Hello world\nSecond line\n");
    want.remove(at);
    want.insert(at, '\n');
    assert_eq!(story_text(&s, story), want);
    s.apply(Intent::Undo).unwrap();
    s.apply(Intent::Undo).unwrap();
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

// ── typing, deleting, undo per burst (T9.4.6) ────────────────────────────

fn story_text(s: &Session, story: NodeId) -> String {
    xarast_doc::StoryText::collect_simple(&s.doc.tree, &s.doc.defaults, story)
        .unwrap()
        .text
}

fn input(s: &mut Session, kind: TextInputKind, time_ms: u64) {
    s.apply(Intent::TextInput(TextInput { kind, time_ms }))
        .unwrap();
}

fn type_str(s: &mut Session, text: &str, time_ms: u64) {
    input(s, TextInputKind::Insert(text.to_owned()), time_ms);
}

fn backspace(s: &mut Session, time_ms: u64) {
    input(s, TextInputKind::Backspace { word: false }, time_ms);
}

/// How many undo steps the history holds.
fn steps(s: &Session) -> usize {
    s.bus.history().len()
}

fn primary_caret(s: &Session) -> Point {
    s.overlay()
        .into_iter()
        .find_map(|o| match o {
            OverlayShape::Caret {
                from,
                primary: true,
                ..
            } => Some(from),
            _ => None,
        })
        .unwrap()
}

#[test]
fn typing_200_characters_is_one_undo_step() {
    // phase-09 acceptance criterion 14.
    let (mut s, story) = fixture();
    let before = s.doc.canonical_digest();
    text_tool(&mut s);
    click(&mut s, X0 + 200, Y0 + 5_000, 0);
    let typed: String = "The quick brown fox jumps over the lazy dog. "
        .chars()
        .cycle()
        .take(200)
        .collect();
    for (i, c) in typed.chars().enumerate() {
        type_str(&mut s, &c.to_string(), 1_000 + 120 * i as u64);
    }
    assert_eq!(
        story_text(&s, story),
        format!("{typed}Hello world\nSecond line\n")
    );
    assert_eq!(selection(&s).head.byte, 200, "the caret follows the typing");
    assert_eq!(steps(&s), 1);
    assert_eq!(s.undo_label(), Some("Typing"));
    s.apply(Intent::Undo).unwrap();
    assert_eq!(
        s.doc.canonical_digest(),
        before,
        "one Ctrl+Z removes all 200"
    );
    assert!(s.undo_label().is_none());
    // The caret stays valid on the shorter text.
    nav(&mut s, TextKey::Right, false, false);
    assert!(selection(&s).head.byte <= 23);
}

#[test]
fn a_pause_a_caret_move_or_another_edit_ends_the_burst() {
    let (mut s, story) = fixture();
    text_tool(&mut s);
    click(&mut s, X0 + 200, Y0 + 5_000, 0);
    type_str(&mut s, "a", 1_000);
    type_str(&mut s, "b", 1_499);
    assert_eq!(steps(&s), 1);
    // 500 ms apart: a new step.
    type_str(&mut s, "c", 1_999);
    assert_eq!(steps(&s), 2);
    // A caret move ends the burst, even when it comes back.
    nav(&mut s, TextKey::Left, false, false);
    nav(&mut s, TextKey::Right, false, false);
    type_str(&mut s, "d", 2_050);
    assert_eq!(steps(&s), 3);
    // Deleting is its own burst; so is typing again after it.
    backspace(&mut s, 2_100);
    backspace(&mut s, 2_200);
    assert_eq!(steps(&s), 4);
    assert_eq!(s.undo_label(), Some("Delete Text"));
    type_str(&mut s, "e", 2_300);
    assert_eq!(steps(&s), 5);
    assert_eq!(story_text(&s, story), "abeHello world\nSecond line\n");
    // Undo in the middle of a burst: typing on starts a new step rather
    // than merging into whatever the undo left last.
    type_str(&mut s, "f", 2_400);
    s.apply(Intent::Undo).unwrap();
    type_str(&mut s, "g", 2_450);
    assert_eq!(steps(&s), 5);
    s.apply(Intent::Undo).unwrap();
    assert_eq!(story_text(&s, story), "abHello world\nSecond line\n");
}

#[test]
fn backspace_and_delete_remove_whole_grapheme_clusters() {
    let (mut s, story) = fixture();
    text_tool(&mut s);
    click(&mut s, X0 + 200, Y0 + 5_000, 0);
    // "e" + combining acute, then a family emoji (a ZWJ sequence).
    type_str(&mut s, "e\u{301}", 1_000);
    type_str(&mut s, "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}", 1_100);
    let family = 3 * 4 + 2 * 3;
    assert_eq!(selection(&s).head.byte, 3 + family);
    backspace(&mut s, 1_200);
    assert_eq!(selection(&s).head.byte, 3);
    assert_eq!(story_text(&s, story), "e\u{301}Hello world\nSecond line\n");
    backspace(&mut s, 1_300);
    assert_eq!(selection(&s).head.byte, 0);
    assert_eq!(story_text(&s, story), "Hello world\nSecond line\n");
    // Backspace at the start does nothing, and records nothing.
    let n = steps(&s);
    backspace(&mut s, 1_400);
    assert_eq!(steps(&s), n);
    type_str(&mut s, "a\u{308}", 5_000);
    nav(&mut s, TextKey::Home, false, false);
    input(&mut s, TextInputKind::Delete { word: false }, 5_100);
    assert_eq!(story_text(&s, story), "Hello world\nSecond line\n");
    assert_eq!(selection(&s).head.byte, 0);
    // Ctrl+Delete: on to the next word (the caret motion of Ctrl+Right);
    // Ctrl+Backspace: back to the start of the word.
    input(&mut s, TextInputKind::Delete { word: true }, 9_000);
    assert_eq!(story_text(&s, story), "world\nSecond line\n");
    nav(&mut s, TextKey::End, false, false);
    input(&mut s, TextInputKind::Backspace { word: true }, 9_100);
    assert_eq!(story_text(&s, story), "\nSecond line\n");
    // Delete at the story's end never takes its final break.
    nav(&mut s, TextKey::End, true, false);
    let n = steps(&s);
    input(&mut s, TextInputKind::Delete { word: false }, 9_200);
    assert_eq!(steps(&s), n);
}

#[test]
fn typing_replaces_the_selection_and_enter_splits_the_paragraph() {
    let (mut s, story) = fixture();
    text_tool(&mut s);
    // "world" selected by a double click.
    click(&mut s, X0 + 80_000, Y0 + 5_000, 0);
    click(&mut s, X0 + 80_000, Y0 + 5_000, 100);
    assert_eq!(selection(&s).range(), 6..11);
    type_str(&mut s, "there", 1_000);
    assert_eq!(story_text(&s, story), "Hello there\nSecond line\n");
    assert!(selection(&s).is_caret());
    // Enter after "Hello": a new paragraph; the caret starts it.
    nav(&mut s, TextKey::Home, false, false);
    for _ in 0..5 {
        nav(&mut s, TextKey::Right, false, false);
    }
    type_str(&mut s, "\n", 2_000);
    assert_eq!(story_text(&s, story), "Hello\n there\nSecond line\n");
    assert_eq!(selection(&s).head.byte, 6);
    // The caret is drawn on the new second line, below the first.
    let y = primary_caret(&s).y.raw();
    assert!(y < Y0 - 10_000, "{y}");
    // Backspace joins the paragraphs again.
    backspace(&mut s, 3_000);
    assert_eq!(story_text(&s, story), "Hello there\nSecond line\n");
    // A selection across the paragraph break, deleted.
    nav(&mut s, TextKey::Down, false, true);
    input(&mut s, TextInputKind::Delete { word: false }, 4_000);
    let t = story_text(&s, story);
    assert!(
        t.starts_with("Hello") && t.matches('\n').count() == 1,
        "{t:?}"
    );
    assert!(
        xarast_doc::validate::validate_document(&s.doc)
            .errors
            .is_empty()
    );
}

#[test]
fn the_first_character_at_a_pending_caret_creates_the_story() {
    let (mut s, _) = fixture();
    let before = s.doc.canonical_digest();
    let stories = |s: &Session| {
        s.doc
            .tree
            .preorder(s.doc.tree.root())
            .filter(|&n| matches!(s.doc.tree.kind(n), Some(NodeKind::TextStory(_))))
            .collect::<Vec<_>>()
    };
    text_tool(&mut s);
    click(&mut s, 300_000, 200_000, 0);
    // Nothing typed yet: Backspace and Delete create nothing.
    backspace(&mut s, 500);
    input(&mut s, TextInputKind::Delete { word: false }, 600);
    untouched(&s, &s.doc.canonical_digest(), &before);
    type_str(&mut s, "H", 1_000);
    type_str(&mut s, "i", 1_100);
    let all = stories(&s);
    assert_eq!(all.len(), 2);
    let new = all[1];
    assert_eq!(story_text(&s, new), "Hi\n");
    let sel = selection(&s);
    assert_eq!((sel.story, sel.head.byte), (new, 2));
    assert_eq!(s.edit.selection().collect::<Vec<_>>(), [new]);
    // One step: the story and its text go together.
    assert_eq!(steps(&s), 1);
    assert_eq!(s.undo_label(), Some("New Text"));
    s.apply(Intent::Undo).unwrap();
    assert_eq!(s.doc.canonical_digest(), before);
    assert_eq!(stories(&s).len(), 1);
    // Leaving a pending caret leaves nothing behind.
    click(&mut s, 300_000, 150_000, 5_000);
    s.apply(Intent::Cancel).unwrap();
    s.apply(Intent::ChooseTool(ToolId::Selector)).unwrap();
    assert_eq!(stories(&s).len(), 1);
}

#[test]
fn a_column_wraps_as_it_is_typed_and_the_caret_follows() {
    let (mut s, _) = fixture();
    text_tool(&mut s);
    drag(&mut s, (300_000, 300_000), (400_000, 250_000));
    let words = "one two three four five six seven eight nine ten";
    for (i, c) in words.chars().enumerate() {
        type_str(&mut s, &c.to_string(), 20_000 + 50 * i as u64);
    }
    let sel = selection(&s);
    match s.doc.tree.kind(sel.story) {
        Some(NodeKind::TextStory(t)) => assert!(matches!(
            t.layout,
            xarast_doc::TextLayout::InColumn {
                word_wrap: true,
                ..
            }
        )),
        other => panic!("{other:?}"),
    }
    assert_eq!(sel.head.byte, words.len());
    assert_eq!(steps(&s), 1);
    let map = xarast_app::text_tool::caret_map(&s.doc, sel.story, &fonts()).unwrap();
    assert!(map.layout().lines.len() > 1, "a 100 pt column wraps");
    // The caret sits inside the column.
    let x = primary_caret(&s).x.raw();
    assert!((300_000..=400_000).contains(&x), "{x}");
}

// ── A story emptied by deletion is removed (XARA-T-0237) ─────────────────

fn stories(s: &Session) -> Vec<NodeId> {
    s.doc
        .tree
        .preorder(s.doc.tree.root())
        .filter(|&n| matches!(s.doc.tree.kind(n), Some(NodeKind::TextStory(_))))
        .collect()
}

fn valid(s: &Session) {
    let errors = xarast_doc::validate::validate_document(&s.doc).errors;
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn deleting_all_the_text_removes_the_story_in_the_same_undo_step() {
    let (mut s, story) = fixture();
    let before = s.doc.canonical_digest();
    text_tool(&mut s);
    click(&mut s, X0 + 30_000, Y0 + 5_000, 0);
    s.apply(Intent::SelectAll).unwrap();
    input(&mut s, TextInputKind::Delete { word: false }, 1_000);
    assert!(!s.doc.tree.is_reachable(story), "the empty story is gone");
    assert!(stories(&s).is_empty());
    assert_eq!(steps(&s), 1, "no step of its own");
    assert_eq!(s.undo_label(), Some("Delete Text"));
    assert_eq!(s.edit.selection().count(), 0, "nothing left selected");
    // The caret stays where the story began, as a pending one.
    assert_eq!(
        s.text_state(),
        Some(TextEditing::Pending {
            at: Point::raw(X0, Y0),
            column: None
        })
    );
    valid(&s);
    // One undo brings back the story and its text, exactly.
    s.apply(Intent::Undo).unwrap();
    assert_eq!(s.doc.canonical_digest(), before);
    assert!(s.undo_label().is_none());
    s.apply(Intent::Redo).unwrap();
    assert!(!s.doc.tree.is_reachable(story));
}

#[test]
fn typing_after_the_removal_makes_a_story_in_the_old_ones_place_and_style() {
    let (mut s, story) = fixture();
    let before = s.doc.canonical_digest();
    text_tool(&mut s);
    click(&mut s, X0 + 30_000, Y0 + 5_000, 0);
    s.apply(Intent::SelectAll).unwrap();
    backspace(&mut s, 1_000);
    type_str(&mut s, "Hi", 2_000);
    let sel = selection(&s);
    assert_ne!(sel.story, story);
    assert_eq!(story_text(&s, sel.story), "Hi\n");
    match s.doc.tree.kind(sel.story) {
        Some(NodeKind::TextStory(t)) => {
            assert_eq!((t.transform.e.raw(), t.transform.f.raw()), (X0, Y0));
        }
        other => panic!("{other:?}"),
    }
    let st =
        xarast_doc::StoryText::collect_simple(&s.doc.tree, &s.doc.defaults, sel.story).unwrap();
    let attrs = &st.runs[0].attrs;
    assert_eq!(
        attrs.get(xarast_doc::AttrSlot::TxtFontSize),
        &AttrValue::FontSize(Mp::new(20_000)),
        "the old story's size"
    );
    match attrs.get(xarast_doc::AttrSlot::TxtFontTypeface) {
        AttrValue::FontTypeface(t) => assert_eq!(&*t.family, "Noto Sans"),
        other => panic!("{other:?}"),
    }
    assert_eq!(steps(&s), 2);
    s.apply(Intent::Undo).unwrap();
    s.apply(Intent::Undo).unwrap();
    assert_eq!(s.doc.canonical_digest(), before);
}

#[test]
fn a_backspace_burst_that_empties_a_new_story_undoes_in_one_step() {
    let (mut s, _) = fixture();
    let before = s.doc.canonical_digest();
    text_tool(&mut s);
    click(&mut s, 300_000, 200_000, 0);
    type_str(&mut s, "H", 1_000);
    type_str(&mut s, "i", 1_100);
    let new = selection(&s).story;
    let typed = s.doc.canonical_digest();
    backspace(&mut s, 3_000);
    backspace(&mut s, 3_100);
    assert!(!s.doc.tree.is_reachable(new));
    assert_eq!(steps(&s), 2, "New Text, then one Delete Text");
    assert_eq!(
        s.text_state(),
        Some(TextEditing::Pending {
            at: Point::raw(300_000, 200_000),
            column: None
        })
    );
    // Backspace at the pending caret does nothing more.
    backspace(&mut s, 3_200);
    assert_eq!(steps(&s), 2);
    s.apply(Intent::Undo).unwrap();
    assert_eq!(s.doc.canonical_digest(), typed);
    assert_eq!(story_text(&s, new), "Hi\n");
    s.apply(Intent::Undo).unwrap();
    assert_eq!(s.doc.canonical_digest(), before);
}

#[test]
fn a_story_left_with_only_paragraph_breaks_stays_when_the_text_is_left() {
    // Decided: removal happens at the deletion that empties a story, never
    // when editing ends, so leaving the text is still no edit (invariant
    // 11); a story holding only breaks is not empty.
    let (mut s, _) = fixture();
    text_tool(&mut s);
    click(&mut s, 300_000, 200_000, 0);
    type_str(&mut s, "\n", 1_000);
    let new = selection(&s).story;
    assert_eq!(story_text(&s, new), "\n\n");
    let n = steps(&s);
    s.apply(Intent::Cancel).unwrap();
    s.apply(Intent::ChooseTool(ToolId::Selector)).unwrap();
    assert!(s.doc.tree.is_reachable(new));
    assert_eq!(steps(&s), n);
}

/// A session holding one story on a straight path from (X0, Y0) 200 pt
/// right, 20 pt Noto Sans "Hi", the story's line width painting the path.
fn on_path_fixture() -> (Session, NodeId) {
    xarast_app::fonts::set_shared(fonts());
    let mut pb = xarast_geom::Path::builder();
    pb.move_to(Point::raw(X0, Y0))
        .line_to(Point::raw(X0 + 200_000, Y0));
    let mut b = skeleton(BuildLimits::default()).unwrap();
    let story = b
        .node(NodeKind::TextStory(Box::new(TextStoryNode {
            layout: xarast_doc::TextLayout::OnPath {
                reversed: false,
                tangential: true,
                left_indent: Mp::ZERO,
                right_indent: Mp::ZERO,
                chars: xarast_doc::CharsTransform::default(),
            },
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
    b.attribute(AttrValue::LineWidth(Mp::new(2_000))).unwrap();
    b.node(NodeKind::Path(Box::new(xarast_doc::PathNode::new(
        pb.build(),
    ))))
    .unwrap();
    b.node(NodeKind::TextLine(Box::default())).unwrap();
    b.push_scope().unwrap();
    for c in "Hi".chars() {
        b.node(NodeKind::TextItem(TextItem::Char(c))).unwrap();
    }
    b.node(NodeKind::TextItem(TextItem::LineBreak(true)))
        .unwrap();
    b.pop_scope();
    b.pop_scope();
    let (doc, _) = b.finish().unwrap();
    (Session::adopt(DocumentId(1), doc, None), story)
}

#[test]
fn emptying_a_story_on_a_path_leaves_the_path_as_a_shape() {
    let (mut s, story) = on_path_fixture();
    let before = s.doc.canonical_digest();
    let layer = s.doc.tree.links(story).parent.unwrap();
    s.edit.select([story], xarast_app::SelectMode::Replace);
    text_tool(&mut s);
    assert_eq!(selection(&s).story, story);
    s.apply(Intent::SelectAll).unwrap();
    input(&mut s, TextInputKind::Delete { word: false }, 1_000);
    assert!(!s.doc.tree.is_reachable(story));
    let path = s
        .doc
        .tree
        .children(layer)
        .find(|&n| matches!(s.doc.tree.kind(n), Some(NodeKind::Path(_))))
        .expect("the path stays, as an ordinary shape");
    // It still paints with the story's line width.
    let attrs = xarast_doc::attr::resolve_uncached(&s.doc.tree, path, &s.doc.defaults);
    assert_eq!(
        attrs.get(xarast_doc::AttrSlot::LineWidth),
        &AttrValue::LineWidth(Mp::new(2_000))
    );
    // No caret: there is no straight place to put one.
    assert_eq!(s.text_state(), None);
    assert_eq!(steps(&s), 1);
    valid(&s);
    s.apply(Intent::Undo).unwrap();
    assert_eq!(s.doc.canonical_digest(), before);
}

#[test]
fn removing_an_emptied_story_repaints_its_damage_exactly() {
    use std::sync::Mutex;
    use std::sync::mpsc;
    use xarast_app::render_thread::CpuFrameRenderer;
    use xarast_app::{FrameReuse, RenderThread, RenderedFrame};
    const BG: [u8; 4] = [128, 128, 132, 255];
    const PAGE: [u8; 4] = [255, 255, 255, 255];
    let t = std::time::Duration::from_secs(60);
    let thread = || {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let rt = RenderThread::spawn_with(
            CpuFrameRenderer::new(xarast_render::CpuConfig::deterministic()),
            Box::new(move || {
                let _ = tx.lock().map(|t| t.send(()));
            }),
        )
        .unwrap();
        (rt, rx)
    };
    let next = |rt: &RenderThread, w: &mpsc::Receiver<()>| -> RenderedFrame {
        w.recv_timeout(t).expect("a frame");
        rt.take_latest().expect("published before the wake")
    };

    let (mut s, story) = fixture();
    s.rebuild_scene(None).unwrap();
    let (mut rt, woken) = thread();
    rt.submit(s.frame_job(BG, PAGE));
    let first = next(&rt, &woken);

    text_tool(&mut s);
    click(&mut s, X0 + 30_000, Y0 + 5_000, 0);
    s.apply(Intent::SelectAll).unwrap();
    input(&mut s, TextInputKind::Delete { word: false }, 1_000);
    assert!(!s.doc.tree.is_reachable(story));
    s.rebuild_scene(None).unwrap();
    let job = s.frame_job(BG, PAGE);
    rt.submit(job.clone());
    let f = next(&rt, &woken);
    assert_eq!(f.reuse, FrameReuse::Repainted);
    assert!(!f.fresh.is_empty());
    let (mut full, full_woken) = thread();
    full.submit(job);
    let whole = next(&full, &full_woken);
    assert!(whole.surface != first.surface, "the text went");
    assert!(f.surface == whole.surface, "the repaint is the full frame");

    // Undo repaints the text back.
    s.apply(Intent::Undo).unwrap();
    s.rebuild_scene(None).unwrap();
    rt.submit(s.frame_job(BG, PAGE));
    let u = next(&rt, &woken);
    assert_eq!(u.reuse, FrameReuse::Repainted);
    assert!(u.surface == first.surface, "undo repaints the text exactly");
}
