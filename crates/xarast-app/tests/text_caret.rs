//! Carets, selections and caret movement over laid-out text (phase 9,
//! T9.4.2–T9.4.4), with the pinned fonts. Bidi behaviour is asserted on
//! byte offsets and cluster indices, never on appearance (the phase's risk
//! table: "checkable without reading the script").

use std::path::Path;
use std::sync::Arc;

use xarast_app::fonts::FontService;
use xarast_app::text_edit::{Caret, CaretMap, CaretMotion};
use xarast_geom::Mp;
use xarast_text::{FontQuery, ParagraphStyle, StoryInput, StoryMode, StyleRange};

fn fonts() -> Arc<FontService> {
    FontService::from_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../xarast-text/tests/fonts"))
}

fn map_with(text: &str, mode: StoryMode) -> CaretMap {
    let f = fonts();
    let runs = [StyleRange::new(
        0..text.len(),
        FontQuery::new("Noto Sans"),
        Mp::new(10_000),
    )];
    let layout = f.ready().layout(&StoryInput {
        text,
        runs: &runs,
        paragraphs: &[ParagraphStyle::default()],
        kerns: &[],
        mode,
    });
    CaretMap::new(text.to_owned(), layout)
}

fn map(text: &str) -> CaretMap {
    map_with(text, StoryMode::Point)
}

const RIGHT: bool = true;
const LEFT: bool = false;
const VISUAL: CaretMotion = CaretMotion::Character { visual: true };
const LOGICAL: CaretMotion = CaretMotion::Character { visual: false };

/// Presses an arrow until the caret stops moving; the offsets visited.
fn walk(m: &CaretMap, from: Caret, motion: CaretMotion, forward: bool) -> Vec<Caret> {
    let mut out = Vec::new();
    let mut c = from;
    for _ in 0..1000 {
        let n = m.move_caret(c, motion, forward, None);
        if n == c {
            break;
        }
        out.push(n);
        c = n;
    }
    out
}

fn bytes(v: &[Caret]) -> Vec<usize> {
    v.iter().map(|c| c.byte).collect()
}

#[test]
fn right_visits_every_boundary_of_latin_text_and_left_comes_back() {
    let m = map("Hello, world");
    let there = walk(&m, Caret::at(0), VISUAL, RIGHT);
    assert_eq!(bytes(&there), (1..=12).collect::<Vec<_>>());
    let back = walk(&m, *there.last().unwrap(), VISUAL, LEFT);
    assert_eq!(bytes(&back), (0..=11).rev().collect::<Vec<_>>());
    // The caret moves right on screen at every step.
    let xs: Vec<Mp> = there.iter().map(|&c| m.caret_x(c)).collect();
    assert!(xs.windows(2).all(|w| w[0] < w[1]), "{xs:?}");
}

#[test]
fn in_a_right_to_left_line_the_left_arrow_reads_forward() {
    // Four Hebrew letters, two bytes each.
    let m = map("שלום");
    assert!(m.layout().lines[0].base_rtl);
    // Offset 0 is the line's logical start: its right end.
    let start = m.caret_x(Caret::at(0));
    let end = m.caret_x(Caret::after(8));
    assert!(start > end, "RTL starts on the right");
    let left = walk(&m, Caret::at(0), VISUAL, LEFT);
    assert_eq!(bytes(&left), [2, 4, 6, 8]);
    let right = walk(&m, Caret::after(8), VISUAL, RIGHT);
    assert_eq!(bytes(&right), [6, 4, 2, 0]);
    // Home and End are logical: the right and left ends.
    assert_eq!(
        m.move_caret(Caret::at(4), CaretMotion::LineStart, false, None)
            .byte,
        0
    );
    assert_eq!(
        m.move_caret(Caret::at(4), CaretMotion::LineEnd, true, None)
            .byte,
        8
    );
}

/// "abc שלום def": a0 b1 c2 ␣3 ש4 ל6 ו8 ם10 ␣12 d13 e14 f15, drawn as
/// "abc םולש def".
const MIXED: &str = "abc שלום def";

#[test]
fn a_visual_walk_through_mixed_text_follows_the_screen() {
    let m = map(MIXED);
    let right = walk(&m, Caret::at(0), VISUAL, RIGHT);
    assert_eq!(
        bytes(&right),
        [1, 2, 3, 4, 10, 8, 6, 4, 13, 14, 15, 16],
        "left to right on screen: the Hebrew is crossed backwards"
    );
    let xs: Vec<Mp> = right.iter().map(|&c| m.caret_x(c)).collect();
    assert!(xs.windows(2).all(|w| w[0] < w[1]), "{xs:?}");
    let left = walk(&m, *right.last().unwrap(), VISUAL, LEFT);
    let xs: Vec<Mp> = left.iter().map(|&c| m.caret_x(c)).collect();
    assert!(xs.windows(2).all(|w| w[0] > w[1]), "{xs:?}");
    assert_eq!(left.last().unwrap().byte, 0);
}

#[test]
fn a_logical_walk_through_mixed_text_follows_the_bytes() {
    let m = map(MIXED);
    let fwd = walk(&m, Caret::at(0), LOGICAL, true);
    assert_eq!(bytes(&fwd), [1, 2, 3, 4, 6, 8, 10, 12, 13, 14, 15, 16]);
    let back = walk(&m, *fwd.last().unwrap(), LOGICAL, false);
    assert_eq!(bytes(&back), [15, 14, 13, 12, 10, 8, 6, 4, 3, 2, 1, 0]);
}

#[test]
fn the_caret_splits_at_a_direction_boundary() {
    let m = map(MIXED);
    let l = &m.layout().lines[0];
    let space = l.clusters.iter().find(|c| c.range == (3..4)).unwrap();
    let shin = l.clusters.iter().find(|c| c.range == (4..6)).unwrap();
    // Offset 4 is both "after the space" and "before shin", which is at
    // the far end of the Hebrew run.
    let g = m.caret_geometry(Caret::after(4));
    assert_eq!(g.x, space.x + space.width);
    assert_eq!(g.split, Some(shin.x + shin.width));
    // From the other side, the other one is primary.
    let g = m.caret_geometry(Caret::at(4));
    assert_eq!(g.x, shin.x + shin.width);
    assert_eq!(g.split, Some(space.x + space.width));
    // Inside a run of one direction the caret is whole.
    assert_eq!(m.caret_geometry(Caret::at(1)).split, None);
    assert_eq!(m.caret_geometry(Caret::at(6)).split, None);
}

#[test]
fn a_hit_lands_on_the_stop_it_was_drawn_at() {
    // The property of the test plan: hit(position_of(pos)) == pos, for
    // every caret position of a line.
    for text in ["Hello, world", "שלום עולם", MIXED] {
        let m = map(text);
        for s in m.stops(0) {
            let g = m.caret_geometry(s.caret);
            let y = g.bottom.midpoint(g.top);
            let hit = m.hit(g.x, y);
            assert_eq!(m.caret_x(hit), g.x, "{text:?} {s:?}");
            // Where two offsets meet at one x (a direction boundary) a
            // click exactly there may give either; each is drawn there.
            let here: Vec<usize> = m
                .stops(0)
                .iter()
                .filter(|t| t.x == g.x)
                .map(|t| t.caret.byte)
                .collect();
            assert!(here.contains(&hit.byte), "{text:?} {s:?} {hit:?}");
        }
        // Far outside the text: the nearest end.
        let far_left = m.hit(Mp::new(-1_000_000), Mp::ZERO);
        let leftmost = m.stops(0)[0].caret;
        assert_eq!(far_left.byte, leftmost.byte);
    }
}

#[test]
fn right_n_times_then_left_n_times_returns() {
    for text in ["Hello, world", "שלום עולם", MIXED, "a שלום 123 b"] {
        let m = map(text);
        for start in m.stops(0).iter().map(|s| s.caret) {
            for n in 1..6 {
                let mut c = start;
                let mut moved = 0;
                for _ in 0..n {
                    let next = m.move_caret(c, VISUAL, RIGHT, None);
                    if next == c {
                        break;
                    }
                    c = next;
                    moved += 1;
                }
                for _ in 0..moved {
                    c = m.move_caret(c, VISUAL, LEFT, None);
                }
                assert_eq!(
                    m.caret_x(c),
                    m.caret_x(start),
                    "{text:?} from {start:?}, {n} steps"
                );
            }
        }
    }
}

#[test]
fn selection_rectangles_are_per_line_spans_in_visual_order() {
    let m = map(MIXED);
    // "c ש": c, the space and shin are not contiguous on screen.
    let r = m.selection_rects(2, 6);
    assert_eq!(r.len(), 2, "{r:?}");
    assert!(r[0].x1 <= r[1].x0, "left to right: {r:?}");
    // Plain text: one span covering exactly the clusters.
    let m = map("Hello, world");
    let r = m.selection_rects(0, 5);
    assert_eq!(r.len(), 1);
    let l = &m.layout().lines[0];
    assert_eq!(r[0].x0, l.clusters[0].x);
    assert_eq!(r[0].x1, l.clusters[4].x + l.clusters[4].width);
    assert!(m.selection_rects(3, 3).is_empty());
}

#[test]
fn a_selected_paragraph_break_shows_at_the_line_end() {
    let m = map("ab\ncd");
    let r = m.selection_rects(1, 4);
    let lines: Vec<usize> = r.iter().map(|r| r.line).collect();
    assert_eq!(lines, [0, 1]);
    // Line 0: "b" and the break block, merged into one span past "b".
    let l0 = &m.layout().lines[0];
    let b = &l0.clusters[1];
    assert_eq!(r[0].x0, b.x);
    assert!(r[0].x1 > b.x + b.width);
}

fn column() -> CaretMap {
    map_with(
        "The quick brown fox jumps over the lazy dog",
        StoryMode::column(Mp::new(60_000)),
    )
}

#[test]
fn a_soft_line_end_is_shown_by_affinity() {
    let m = column();
    assert!(m.layout().lines.len() >= 3);
    let r0 = m.line_range(0);
    let r1 = m.line_range(1);
    assert_eq!(r0.end, r1.start, "a soft break");
    assert_eq!(m.line_of(Caret::after(r0.end)), 0);
    assert_eq!(m.line_of(Caret::at(r0.end)), 1);
    // End on the first line stays on the first line.
    let e = m.move_caret(Caret::at(0), CaretMotion::LineEnd, true, None);
    assert_eq!(e, Caret::after(r0.end));
    assert_eq!(m.line_of(e), 0);
    // Right from there goes to the start of the next line, and back.
    let n = m.move_caret(e, VISUAL, RIGHT, None);
    assert_eq!(m.line_of(n), 1);
    assert_eq!(m.move_caret(n, VISUAL, LEFT, None), e);
}

#[test]
fn up_and_down_keep_the_goal_x() {
    let m = column();
    let start = m.hit_on_line(0, Mp::new(20_000));
    let goal = m.caret_x(start);
    let down = m.move_caret(start, CaretMotion::Line, true, Some(goal));
    assert_eq!(m.line_of(down), 1);
    assert!((m.caret_x(down) - goal).abs() < Mp::new(6_000));
    let down2 = m.move_caret(down, CaretMotion::Line, true, Some(goal));
    assert_eq!(m.line_of(down2), 2);
    let up = m.move_caret(down2, CaretMotion::Line, false, Some(goal));
    assert_eq!(up, down);
    // Off the top and bottom: the story's ends.
    assert_eq!(
        m.move_caret(start, CaretMotion::Line, false, None),
        Caret::at(0)
    );
    let last = m.layout().lines.len() - 1;
    let bottom = m.hit_on_line(last, Mp::ZERO);
    assert_eq!(
        m.move_caret(bottom, CaretMotion::Line, true, None).byte,
        m.len()
    );
    // A page is at least a line.
    let p = m.move_caret(start, CaretMotion::Page { height: Mp::new(1) }, true, None);
    assert_eq!(m.line_of(p), 1);
}

#[test]
fn word_motion_and_word_selection() {
    let m = map("Hello, big world");
    let fwd = walk(&m, Caret::at(0), CaretMotion::Word, true);
    assert_eq!(bytes(&fwd), [7, 11, 16]);
    let back = walk(&m, Caret::at(16), CaretMotion::Word, false);
    assert_eq!(bytes(&back), [11, 7, 0]);
    assert_eq!(m.word_at(8), 7..10);
    assert_eq!(m.word_at(16), 11..16);
}

#[test]
fn story_ends_and_an_empty_story() {
    let m = map("ab\ncd");
    assert_eq!(
        m.move_caret(Caret::at(4), CaretMotion::StoryStart, false, None),
        Caret::at(0)
    );
    assert_eq!(
        m.move_caret(Caret::at(0), CaretMotion::StoryEnd, true, None),
        Caret::after(5)
    );
    // Crossing a paragraph break on the right arrow.
    assert_eq!(
        bytes(&walk(&m, Caret::at(0), VISUAL, RIGHT)),
        [1, 2, 3, 4, 5]
    );
    let e = map("");
    assert_eq!(e.line_count(), 1);
    let g = e.caret_geometry(Caret::at(0));
    assert!(g.top > g.bottom, "an empty story still has a caret");
    assert_eq!(
        e.move_caret(Caret::at(0), VISUAL, RIGHT, None),
        Caret::at(0)
    );
    assert_eq!(e.hit(Mp::new(5_000), Mp::ZERO), Caret::at(0));
    // Offsets past the end are clamped.
    assert_eq!(m.snap(Caret::at(99)).byte, 5);
}

#[test]
fn a_ligature_or_a_mark_is_one_stop() {
    // e + combining acute: one cluster, no stop between them.
    let m = map("e\u{301}x");
    let b = bytes(&walk(&m, Caret::at(0), VISUAL, RIGHT));
    assert_eq!(b, [3, 4]);
    assert_eq!(m.snap(Caret::at(1)).byte, 0);
}

/// The test plan's "bidi caret walk over `hebrew.xar`": on every line of
/// every story, the right arrow from the leftmost stop visits every caret
/// offset of the line, moving right on screen at each step, and the left
/// arrow walks back. Skipped without the corpus.
#[test]
fn a_bidi_caret_walk_over_hebrew_xar() {
    let root = std::path::PathBuf::from(
        std::env::var("XARAST_XAR_CORPUS").unwrap_or_else(|_| "/home/user/xara-xtreme".into()),
    );
    let path = root.join("TextDesigns/hebrew.xar");
    if !path.is_file() {
        println!("skipping: no corpus at {}", root.display());
        return;
    }
    let s = xarast_app::Session::open(xarast_app::DocumentId(1), &path).expect("opens");
    let fonts = fonts();
    let stories: Vec<_> = s
        .doc
        .tree
        .preorder(s.doc.tree.root())
        .filter(|&n| matches!(s.doc.tree.kind(n), Some(xarast_doc::NodeKind::TextStory(_))))
        .collect();
    assert!(!stories.is_empty());
    let (mut rtl_lines, mut rtl_steps) = (0, 0);
    for story in stories {
        let m = xarast_app::text_tool::caret_map(&s.doc, story, &fonts).expect("a story");
        for line in 0..m.line_count() {
            let stops = m.stops(line);
            let l = &m.layout().lines[line];
            if l.base_rtl {
                rtl_lines += 1;
            }
            let mut c = stops[0].caret;
            let mut seen = vec![c.byte];
            let mut x = m.caret_x(c);
            loop {
                let n = m.move_caret(c, VISUAL, RIGHT, None);
                if n == c || m.line_of(n) != line {
                    break;
                }
                let nx = m.caret_x(n);
                assert!(nx > x, "line {line}: {c:?} -> {n:?} moved left");
                if l.clusters.iter().any(|k| k.rtl) {
                    rtl_steps += 1;
                }
                seen.push(n.byte);
                (c, x) = (n, nx);
            }
            for s in &stops {
                assert!(
                    seen.contains(&s.caret.byte),
                    "line {line}: {s:?} never visited"
                );
            }
            // And back to where the walk started.
            loop {
                let n = m.move_caret(c, VISUAL, LEFT, None);
                if n == c || m.line_of(n) != line {
                    break;
                }
                assert!(m.caret_x(n) < m.caret_x(c));
                c = n;
            }
            assert_eq!(m.caret_x(c), stops[0].x);
        }
    }
    assert!(rtl_lines > 0, "hebrew.xar has right-to-left paragraphs");
    assert!(rtl_steps > 0);
}
