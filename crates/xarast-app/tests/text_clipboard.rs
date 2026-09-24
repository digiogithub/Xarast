//! The text clipboard and input-method composition in the text tool
//! (phase 9, T9.4.7–T9.4.8), driven through `AppState` intents as the shell
//! drives them: no window, no system clipboard, no input method.

use std::path::Path;
use std::sync::Arc;

use xarast_app::fonts::FontService;
use xarast_app::text_tool::{TextEditing, TextSelection};
use xarast_app::tool::Preedit;
use xarast_app::{
    AppState, Intent, OverlayShape, PlatformRequest, PointerButton, PointerSample, Session,
    TextInput, TextInputKind, TextKey, TextNav, ToolId,
};
use xarast_doc::builder::{BuildLimits, skeleton};
use xarast_doc::{
    AttrSlot, AttrValue, NodeId, NodeKind, StoryText, TextItem, TextStoryNode, TypefaceRef,
};
use xarast_geom::{Matrix, Mp, Point, Vector};

fn fonts() -> Arc<FontService> {
    FontService::from_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../xarast-text/tests/fonts"))
}

const X0: i32 = 100_000;
const Y0: i32 = 400_000;

/// One point story at (100 pt, 400 pt), 20 pt Noto Sans: "Hello world",
/// then "Second line" in bold.
fn fixture() -> (AppState, NodeId) {
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
    for (line, bold) in [("Hello world", false), ("Second line", true)] {
        b.node(NodeKind::TextLine(Box::default())).unwrap();
        b.push_scope().unwrap();
        if bold {
            b.attribute(AttrValue::Bold(true)).unwrap();
        }
        for c in line.chars() {
            b.node(NodeKind::TextItem(TextItem::Char(c))).unwrap();
        }
        b.node(NodeKind::TextItem(TextItem::LineBreak(true)))
            .unwrap();
        b.pop_scope();
    }
    b.pop_scope();
    let (doc, _) = b.finish().unwrap();
    let mut app = AppState::new();
    app.adopt(doc);
    app.apply(Intent::SetZoom {
        zoom: 1.0,
        anchor: None,
    })
    .unwrap();
    let s = app.active().unwrap();
    let c = s.viewport.doc_to_device(Point::raw(X0 + 60_000, Y0));
    let size = s.viewport.size();
    let (dx, dy) = (
        f64::from(size.width) / 2.0 - c.x,
        f64::from(size.height) / 2.0 - c.y,
    );
    app.apply(Intent::Pan { dx, dy }).unwrap();
    app.apply(Intent::ChooseTool(ToolId::Text)).unwrap();
    (app, story)
}

fn s(app: &AppState) -> &Session {
    app.active().unwrap()
}

fn click(app: &mut AppState, x: i32, y: i32) {
    let at = s(app).viewport.doc_to_device(Point::raw(x, y));
    let sample = PointerSample {
        at,
        pressure: None,
        time_ms: 0,
    };
    app.apply(Intent::PointerMove(sample)).unwrap();
    app.apply(Intent::PointerDown {
        button: PointerButton::Primary,
        sample,
    })
    .unwrap();
    app.apply(Intent::PointerUp {
        button: PointerButton::Primary,
        sample,
    })
    .unwrap();
}

fn nav(app: &mut AppState, key: TextKey, word: bool, extend: bool) {
    app.apply(Intent::TextNav(TextNav { key, word, extend }))
        .unwrap();
}

fn selection(app: &AppState) -> TextSelection {
    match s(app).text_state() {
        Some(TextEditing::Story(sel)) => sel,
        other => panic!("not editing a story: {other:?}"),
    }
}

/// The caret in the story, at the start of line `line` (0 or 1).
fn caret_at_line_start(app: &mut AppState, line: i32) {
    click(app, X0 + 60_000, Y0 - line * 24_000 + 5_000);
    nav(app, TextKey::Home, false, false);
}

fn text(app: &AppState, story: NodeId) -> StoryText {
    let s = s(app);
    StoryText::collect_simple(&s.doc.tree, &s.doc.defaults, story).unwrap()
}

fn bold_at(st: &StoryText, b: usize) -> bool {
    let run = st.runs.iter().find(|r| r.range.contains(&b)).unwrap();
    *run.attrs.get(AttrSlot::TxtBold) == AttrValue::Bold(true)
}

fn clipboard_text(app: &mut AppState) -> Option<String> {
    app.take_requests().into_iter().find_map(|r| match r {
        PlatformRequest::SetClipboardText(t) => Some(t),
        _ => None,
    })
}

fn paste(app: &mut AppState, text: Option<&str>) {
    app.apply(Intent::Paste { in_place: false }).unwrap();
    assert!(
        app.take_requests()
            .iter()
            .any(|r| matches!(r, PlatformRequest::ReadClipboard { .. })),
        "a paste asks the shell for the clipboard"
    );
    app.apply(Intent::PasteText {
        text: text.map(str::to_owned),
        in_place: false,
    })
    .unwrap();
}

/// Selects the bold word "Second" (the start of line 2).
fn select_second(app: &mut AppState) {
    caret_at_line_start(app, 1);
    for _ in 0..6 {
        nav(app, TextKey::Right, false, true);
    }
    assert_eq!(selection(app).range(), 12..18);
}

#[test]
fn copy_puts_plain_text_out_and_paste_keeps_the_style_inside() {
    let (mut app, story) = fixture();
    select_second(&mut app);
    app.apply(Intent::Copy).unwrap();
    assert_eq!(clipboard_text(&mut app).as_deref(), Some("Second"));
    assert!(app.clipboard().is_none(), "text, not objects");
    let before = s(&app).doc.canonical_digest();

    // After "Hello " (plain), paste what the system clipboard still holds.
    caret_at_line_start(&mut app, 0);
    for _ in 0..6 {
        nav(&mut app, TextKey::Right, false, false);
    }
    paste(&mut app, Some("Second"));
    let st = text(&app, story);
    assert_eq!(st.text, "Hello Secondworld\nSecond line\n");
    assert!(!bold_at(&st, 5) && bold_at(&st, 6) && bold_at(&st, 11));
    assert!(!bold_at(&st, 12), "the text after it keeps its style");
    assert_eq!(selection(&app).head.byte, 12, "the caret ends after it");
    assert_eq!(s(&app).undo_label(), Some("Paste"));
    assert!(s(&app).text_editing(), "still typing");

    // One step undoes it.
    app.apply(Intent::Undo).unwrap();
    assert_eq!(s(&app).doc.canonical_digest(), before);
}

#[test]
fn pasting_styled_text_into_the_same_style_writes_no_attributes() {
    let (mut app, story) = fixture();
    select_second(&mut app);
    app.apply(Intent::Copy).unwrap();
    let nodes = s(&app).doc.tree.preorder(story).count();
    // Into the bold line, before "line".
    caret_at_line_start(&mut app, 1);
    nav(&mut app, TextKey::End, false, false);
    paste(&mut app, None);
    assert_eq!(text(&app, story).text, "Hello world\nSecond lineSecond\n");
    assert_eq!(
        s(&app).doc.tree.preorder(story).count(),
        nodes + 6,
        "six characters, no attribute nodes"
    );
}

#[test]
fn text_from_elsewhere_is_pasted_plain_in_the_style_it_lands_in() {
    let (mut app, story) = fixture();
    caret_at_line_start(&mut app, 1);
    paste(&mut app, Some("a\r\nb\u{7}"));
    let st = text(&app, story);
    assert_eq!(st.text, "Hello world\na\nbSecond line\n");
    assert!(
        bold_at(&st, 12) && bold_at(&st, 14),
        "the bold line's style"
    );
}

#[test]
fn cut_removes_the_text_and_is_one_step_named_cut() {
    let (mut app, story) = fixture();
    let before = s(&app).doc.canonical_digest();
    select_second(&mut app);
    app.apply(Intent::Cut).unwrap();
    assert_eq!(clipboard_text(&mut app).as_deref(), Some("Second"));
    assert_eq!(text(&app, story).text, "Hello world\n line\n");
    assert_eq!(s(&app).undo_label(), Some("Cut"));
    assert!(s(&app).text_editing());
    app.apply(Intent::Undo).unwrap();
    assert_eq!(s(&app).doc.canonical_digest(), before);
}

#[test]
fn cutting_all_the_text_removes_the_story_in_the_same_step() {
    let (mut app, story) = fixture();
    let before = s(&app).doc.canonical_digest();
    caret_at_line_start(&mut app, 0);
    app.apply(Intent::SelectAll).unwrap();
    app.apply(Intent::Cut).unwrap();
    assert_eq!(
        clipboard_text(&mut app).as_deref(),
        Some("Hello world\nSecond line")
    );
    assert!(
        !s(&app).doc.tree.is_reachable(story),
        "the empty story is gone"
    );
    assert_eq!(s(&app).undo_label(), Some("Cut"));
    assert_eq!(s(&app).bus.history().len(), 1);
    assert!(matches!(
        s(&app).text_state(),
        Some(TextEditing::Pending { .. })
    ));
    // Pasting it back at the pending caret makes the story again.
    paste(&mut app, Some("Hello world\nSecond line"));
    let sel = selection(&app);
    assert_eq!(text(&app, sel.story).text, "Hello world\nSecond line\n");
    assert!(bold_at(&text(&app, sel.story), 12), "the styled copy");
    app.apply(Intent::Undo).unwrap();
    app.apply(Intent::Undo).unwrap();
    assert_eq!(s(&app).doc.canonical_digest(), before);
}

#[test]
fn with_no_text_selected_copy_and_cut_do_nothing_to_the_story() {
    let (mut app, story) = fixture();
    let before = s(&app).doc.canonical_digest();
    caret_at_line_start(&mut app, 1);
    assert!(s(&app).edit.is_selected(story), "the story is selected");
    app.apply(Intent::Copy).unwrap();
    app.apply(Intent::Cut).unwrap();
    assert_eq!(clipboard_text(&mut app), None);
    assert!(app.clipboard().is_none() && app.text_clipboard().is_none());
    assert_eq!(
        s(&app).doc.canonical_digest(),
        before,
        "the story is not cut"
    );
}

#[test]
fn copied_objects_are_not_pasted_into_text() {
    let (mut app, story) = fixture();
    // Copy the story as an object with the selector.
    app.apply(Intent::ChooseTool(ToolId::Selector)).unwrap();
    click(&mut app, X0 + 30_000, Y0 + 5_000);
    app.apply(Intent::Copy).unwrap();
    let svg = clipboard_text(&mut app).expect("the object copy");
    let before = s(&app).doc.canonical_digest();
    app.apply(Intent::ChooseTool(ToolId::Text)).unwrap();
    caret_at_line_start(&mut app, 0);
    paste(&mut app, Some(&svg));
    assert_eq!(s(&app).doc.canonical_digest(), before);
    assert!(app.take_notice().is_some_and(|n| n.contains("objects")));
    assert_eq!(text(&app, story).text, "Hello world\nSecond line\n");
}

#[test]
fn paste_at_a_pending_caret_makes_a_story_in_one_step() {
    let (mut app, _) = fixture();
    click(&mut app, X0, Y0 - 200_000);
    assert!(matches!(
        s(&app).text_state(),
        Some(TextEditing::Pending { .. })
    ));
    let before = s(&app).doc.canonical_digest();
    paste(&mut app, Some("New"));
    let sel = selection(&app);
    assert_eq!(text(&app, sel.story).text, "New\n");
    assert_eq!(sel.head.byte, 3, "the caret goes on after the paste");
    assert_eq!(s(&app).undo_label(), Some("Paste"));
    app.apply(Intent::Undo).unwrap();
    assert_eq!(s(&app).doc.canonical_digest(), before);
}

// ── input method composition (T9.4.7) ────────────────────────────────────

fn preedit(app: &mut AppState, text: &str, cursor: Option<(usize, usize)>) {
    app.apply(Intent::TextPreedit(Some(Preedit {
        text: text.to_owned(),
        cursor,
    })))
    .unwrap();
}

fn text_ink(s: &Session) -> xarast_geom::Rect {
    let mut walker = xarast_app::walker::SceneWalker::new();
    let mut scene = xarast_render::Scene::new();
    walker
        .rebuild_previewed(
            &s.doc,
            &s.edit,
            &s.viewport,
            s.quality,
            None,
            s.preview(),
            &mut scene,
        )
        .unwrap();
    walker.text_ink()
}

#[test]
fn a_composition_shows_in_the_story_and_only_its_commit_edits() {
    let (mut app, story) = fixture();
    caret_at_line_start(&mut app, 0);
    nav(&mut app, TextKey::End, false, false);
    let before = s(&app).doc.canonical_digest();
    let area = s(&app).ime_cursor_area().expect("a caret is up");
    let ink = text_ink(s(&app));

    preedit(&mut app, "abc", Some((3, 3)));
    // Nothing is in the document, but the story draws it and the caret
    // (and the candidate window) moved on past it.
    assert_eq!(s(&app).doc.canonical_digest(), before);
    assert!(s(&app).preview().text.is_some());
    assert!(
        text_ink(s(&app)).hi.x > ink.hi.x,
        "the composition is drawn"
    );
    let composing = s(&app).ime_cursor_area().unwrap();
    assert!(composing[0] > area[0] + 5.0, "{composing:?} vs {area:?}");
    let overlay = s(&app).overlay();
    assert!(
        overlay
            .iter()
            .any(|o| matches!(o, OverlayShape::Polyline { .. })),
        "the composition is underlined"
    );
    assert!(
        !overlay
            .iter()
            .any(|o| matches!(o, OverlayShape::Highlight { .. }))
    );

    // A selected segment is highlighted; no cursor hides the caret.
    preedit(&mut app, "abc", Some((0, 2)));
    assert!(
        s(&app)
            .overlay()
            .iter()
            .any(|o| matches!(o, OverlayShape::Highlight { .. }))
    );
    preedit(&mut app, "abc", None);
    assert!(
        !s(&app)
            .overlay()
            .iter()
            .any(|o| matches!(o, OverlayShape::Caret { .. }))
    );
    assert!(
        s(&app).ime_cursor_area().is_some(),
        "the window still follows"
    );

    // Cancelled: gone, nothing written.
    app.apply(Intent::TextPreedit(None)).unwrap();
    assert!(s(&app).preview().text.is_none());
    assert_eq!(s(&app).doc.canonical_digest(), before);
    assert_eq!(s(&app).ime_cursor_area(), Some(area));

    // Committed: typed, one step.
    preedit(&mut app, "日本", Some((6, 6)));
    app.apply(Intent::TextInput(TextInput {
        kind: TextInputKind::Insert("日本".to_owned()),
        time_ms: 5_000,
    }))
    .unwrap();
    assert!(s(&app).preview().text.is_none());
    assert_eq!(text(&app, story).text, "Hello world日本\nSecond line\n");
    assert_eq!(s(&app).undo_label(), Some("Typing"));
}

#[test]
fn a_composition_at_a_pending_caret_is_not_drawn_but_commits() {
    let (mut app, _) = fixture();
    click(&mut app, X0, Y0 - 200_000);
    let before = s(&app).doc.canonical_digest();
    let area = s(&app).ime_cursor_area().expect("a pending caret is up");
    preedit(&mut app, "abc", Some((3, 3)));
    assert!(s(&app).preview().text.is_none());
    assert_eq!(s(&app).doc.canonical_digest(), before);
    assert_eq!(s(&app).ime_cursor_area(), Some(area));
    app.apply(Intent::TextInput(TextInput {
        kind: TextInputKind::Insert("abc".to_owned()),
        time_ms: 5_000,
    }))
    .unwrap();
    assert_eq!(text(&app, selection(&app).story).text, "abc\n");
}

#[test]
fn a_click_elsewhere_ends_the_composition_display() {
    let (mut app, _) = fixture();
    caret_at_line_start(&mut app, 0);
    preedit(&mut app, "abc", Some((3, 3)));
    assert!(s(&app).preview().text.is_some());
    click(&mut app, X0, Y0 - 200_000);
    assert!(s(&app).preview().text.is_none());
}
