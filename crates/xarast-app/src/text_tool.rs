//! The text tool (phase 9, W9.4 T9.4.1, with T9.4.4's keyboard navigation
//! and the mouse gestures of T9.4.5).
//!
//! # The state machine
//!
//! ```text
//!             click on text                 drag on text
//!   Idle ─────────────────────▶ Story ◀──────────────────── (select by drag)
//!    │ ▲                          │ ▲  arrows, Home/End, PgUp/PgDn, Shift extends,
//!    │ │ Esc                      │ │  Ctrl+A, double click word, triple click line
//!    │ └──────────────────────────┘ │
//!    │ click on empty canvas         │ click on empty canvas
//!    ▼                               ▼
//!   Pending { at, column: None } ◀──┘    drag on empty canvas ─▶ Pending { column: Some(width) }
//! ```
//!
//! * **Story**: a caret (and maybe a selection) in an existing story. The
//!   caret is [`TextSelection`], tool state only: nothing is written to the
//!   document (`research/02 §10.11`, "Change 1").
//! * **Pending**: a caret where typing *will* create a new story, a point
//!   story or a column of the dragged width. No story is created until the
//!   first character is typed (T9.4.6), so leaving the tool never leaves an
//!   empty story behind and a stray click costs no undo step.
//!
//! The tool never mutates the document (tools invariant 1): typing and
//! deleting emit [`EditCommand::TypeText`], [`EditCommand::DeleteText`]
//! and, at a pending caret, [`EditCommand::CreateText`], which the session
//! runs through the bus. Delete, Backspace and Enter belong to the text
//! while a caret is up, so they never delete the story object under it.
//!
//! # Typing bursts (T9.4.6)
//!
//! Keys of one kind (typing, or deleting) less than [`TYPING_BURST_MS`]
//! apart, each starting where the previous one left the caret, make one
//! undo step: they share a burst number, which is their coalesce key.
//! Anything else ends the burst — a caret move, a click, a selection,
//! another command changing the document (the tool notes the document's
//! epoch after its own edit), switching between typing and deleting. The
//! first burst at a pending caret creates the story, so one undo removes
//! the story with everything typed into it.
//!
//! # Layouts
//!
//! The tool lays stories out itself, as the walker does
//! ([`crate::text::lay_story`]), and keeps them in a cache keyed by the
//! document's epoch: any committed edit (undo included) drops them all. A
//! story on a path keeps its fit, so the caret, hit tests and highlight
//! follow the drawn text ([`CaretMap::on_path`]).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use kurbo::Affine;
use xarast_doc::{
    AttrValue, Document, NodeId, NodeKind, StoryText, TextCursor, TextLayout, TextStoryNode,
};
use xarast_geom::{Matrix, Mp, Vector};

use crate::edit::{SelectMode, ToolId};
use crate::fonts::FontService;
use crate::geometry::{DocPoint, DocRect};
use crate::ops::EditCommand;
use crate::text_edit::{Caret, CaretMap, CaretMotion};
use crate::tool::{
    CursorKind, GestureEvent, Infobar, InfobarItem, InteractionState, OverlayShape, TextInput,
    TextInputKind, TextKey, TextNav, Tool, ToolAction, ToolCtx, ToolView,
};

/// Typing keys closer together than this, in milliseconds, make one undo
/// step (`phase-09` W9.4).
pub const TYPING_BURST_MS: u64 = 500;

/// Burst numbers: process-wide, so no two bursts of any document share one.
static NEXT_BURST: AtomicU64 = AtomicU64::new(1);

fn next_burst() -> u64 {
    NEXT_BURST.fetch_add(1, Ordering::Relaxed)
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum BurstKind {
    Typing,
    Deleting,
}

/// The typing burst in progress: what the next key must match to join it.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct Burst {
    id: u64,
    kind: BurstKind,
    /// The story it edits; `None` until the story it creates exists.
    story: Option<NodeId>,
    /// Where it left the caret.
    caret: usize,
    /// When its last key was pressed.
    last_ms: u64,
    /// The document's epoch right after its last edit applied; `None`
    /// until then (and for good when the edit failed).
    epoch: Option<u64>,
}

/// What typing inserts: `'\r'` (alone or before `'\n'`) becomes a
/// paragraph break, other control characters except tab are dropped.
fn typed(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .filter(|&c| c == '\n' || c == '\t' || !c.is_control())
        .collect()
}

/// A caret and selection in a story: the text between `anchor` and `head`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct TextSelection {
    /// The `TextStory` node.
    pub story: NodeId,
    /// Where the selection started.
    pub anchor: Caret,
    /// Where the caret is.
    pub head: Caret,
}

impl TextSelection {
    /// Whether nothing is selected.
    #[must_use]
    pub fn is_caret(&self) -> bool {
        self.anchor.byte == self.head.byte
    }

    /// The selected byte range, in order.
    #[must_use]
    pub fn range(&self) -> std::ops::Range<usize> {
        self.anchor.byte.min(self.head.byte)..self.anchor.byte.max(self.head.byte)
    }

    /// The document model's view of it: positions as line and item.
    /// `None` when the story is gone.
    #[must_use]
    pub fn cursor(&self, doc: &Document) -> Option<TextCursor> {
        let mut stack = xarast_doc::attr::resolve_inherited(&doc.tree, self.story, &doc.defaults);
        let st = StoryText::collect(&doc.tree, self.story, &mut stack, &mut |_, a| {
            Arc::new(a.value.clone())
        })?;
        Some(TextCursor {
            story: self.story,
            anchor: st.pos_of(self.anchor.byte)?,
            head: st.pos_of(self.head.byte)?,
        })
    }
}

/// What the text tool is editing.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum TextEditing {
    /// A caret in an existing story.
    Story(TextSelection),
    /// A caret where typing will create a story.
    Pending {
        /// The new story's origin: the first baseline's left end.
        at: DocPoint,
        /// The column width a drag asked for; a point story when `None`.
        column: Option<Mp>,
    },
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct Editing {
    state: TextEditing,
    /// The x a run of vertical moves keeps, in story space.
    goal_x: Option<Mp>,
}

#[derive(Copy, Clone, PartialEq, Debug)]
enum Drag {
    /// Selecting text by dragging over a story.
    Select {
        story: NodeId,
        anchor: Caret,
        before: Option<Editing>,
    },
    /// Dragging out a column on empty canvas.
    Column {
        from: DocPoint,
        to: DocPoint,
        before: Option<Editing>,
    },
}

/// A story laid out for editing, with its matrix.
#[derive(Debug)]
struct StoryView {
    map: CaretMap,
    /// Story space to document space.
    xf: Affine,
    /// Document space to story space.
    inv: Affine,
}

impl StoryView {
    fn build(doc: &Document, story: NodeId, fonts: &FontService) -> Option<StoryView> {
        let Some(NodeKind::TextStory(node)) = doc.tree.kind(story) else {
            return None;
        };
        let mut stack = xarast_doc::attr::resolve_inherited(&doc.tree, story, &doc.defaults);
        let st = StoryText::collect(&doc.tree, story, &mut stack, &mut |_, a| {
            Arc::new(a.value.clone())
        })?;
        // The walker's own layout: on a path, the path's column and the fit.
        let (_, layout, fit) = crate::text::lay_story(fonts, &doc.tree, &st, node);
        let text = crate::text::layout_text(&st).to_owned();
        let xf = node.transform.to_affine();
        let inv = if xf.determinant().abs() > f64::EPSILON {
            xf.inverse()
        } else {
            Affine::IDENTITY
        };
        let map = match fit {
            Some(fit) => CaretMap::on_path(text, layout, fit),
            None => CaretMap::new(text, layout),
        };
        Some(StoryView { map, xf, inv })
    }

    fn to_doc(&self, p: kurbo::Point) -> DocPoint {
        let p = self.xf * p;
        DocPoint::from_f64_round(p.x, p.y)
    }

    fn to_story(&self, p: DocPoint) -> kurbo::Point {
        self.inv * kurbo::Point::new(p.x.to_f64(), p.y.to_f64())
    }

    /// The caret a document point hits, and whether it is on the story
    /// (within `tol` document units of a line or, on a path, of a fitted
    /// cluster).
    fn hit(&self, p: DocPoint, tol: f64) -> (Caret, bool) {
        let (caret, dist) = self.map.hit_point(self.to_story(p));
        (caret, dist <= tol * self.story_scale())
    }

    /// Story units per document unit: how far a device tolerance reaches
    /// in the story.
    fn story_scale(&self) -> f64 {
        let d = self.inv.determinant().abs().sqrt();
        if d.is_finite() && d > 0.0 { d } else { 1.0 }
    }
}

#[derive(Debug, Default)]
struct Cache {
    epoch: Option<u64>,
    stories: HashMap<NodeId, Option<Arc<StoryView>>>,
}

/// How far, in device pixels, a click may miss a line box and still land
/// in the story.
const STORY_HIT_PX: f64 = 4.0;

/// The size a pending caret is drawn at when no current font size is set:
/// the model's default (`text.md`, 16 pt).
const DEFAULT_SIZE: Mp = Mp::new(16_000);

/// A drag narrower than this, in device pixels, makes a point story, not a
/// column.
const MIN_COLUMN_PX: f64 = 8.0;

/// The text tool.
#[derive(Debug, Default)]
pub struct TextTool {
    /// Fonts to lay out with; the process's shared service when `None`.
    fonts: Option<Arc<FontService>>,
    cache: Mutex<Cache>,
    editing: Option<Editing>,
    drag: Option<Drag>,
    /// Arrow keys move in logical rather than visual order (the
    /// preference `phase-09` names; visual is the default).
    pub logical_arrows: bool,
    /// Counts caret changes, so the interface restarts the blink.
    moved: u64,
    /// The typing burst the next key may join.
    burst: Option<Burst>,
}

impl TextTool {
    /// A text tool laying stories out with `fonts` rather than the process's
    /// shared service: for tests with pinned fonts.
    #[must_use]
    pub fn with_fonts(fonts: Arc<FontService>) -> TextTool {
        TextTool {
            fonts: Some(fonts),
            ..TextTool::default()
        }
    }

    fn fonts(&self) -> Arc<FontService> {
        self.fonts.clone().unwrap_or_else(crate::fonts::shared)
    }

    fn cache(&self) -> MutexGuard<'_, Cache> {
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The story laid out, from the cache when the document has not changed.
    fn view(&self, doc: &Document, story: NodeId) -> Option<Arc<StoryView>> {
        let mut cache = self.cache();
        if cache.epoch != Some(doc.epoch.0) {
            cache.stories.clear();
            cache.epoch = Some(doc.epoch.0);
        }
        if let Some(v) = cache.stories.get(&story) {
            return v.clone();
        }
        let v = if doc.tree.contains(story) && doc.tree.is_reachable(story) {
            StoryView::build(doc, story, &self.fonts()).map(Arc::new)
        } else {
            None
        };
        cache.stories.insert(story, v.clone());
        v
    }

    /// The topmost editable story under a document point.
    /// The topmost editable story under a document point, and the caret
    /// the point hits in it.
    fn story_at(&self, cx: &ToolCtx<'_>, at: DocPoint) -> Option<(NodeId, Arc<StoryView>, Caret)> {
        let tol = STORY_HIT_PX * cx.device_px();
        let mut found = None;
        for id in editable_stories(cx.doc) {
            let Some(v) = self.view(cx.doc, id) else {
                continue;
            };
            let (caret, on) = v.hit(at, tol);
            if on {
                found = Some((id, v, caret));
            }
        }
        found
    }

    /// The text being edited, if any.
    #[must_use]
    pub fn editing(&self) -> Option<TextEditing> {
        self.editing.map(|e| e.state)
    }

    fn set(&mut self, state: Option<TextEditing>, goal_x: Option<Mp>, cx: &mut ToolCtx<'_>) {
        let new = state.map(|state| Editing { state, goal_x });
        // Anything that sets the caret ends a typing burst; typing starts
        // its own again right after.
        self.burst = None;
        if new != self.editing {
            self.moved = self.moved.wrapping_add(1);
        }
        self.editing = new;
        cx.requests.overlay_changed = true;
    }

    fn select_story(&mut self, story: NodeId, anchor: Caret, head: Caret, cx: &mut ToolCtx<'_>) {
        let was = match self.editing {
            Some(Editing {
                state: TextEditing::Story(s),
                ..
            }) => Some(s.story),
            _ => None,
        };
        if was != Some(story) || !cx.edit.is_selected(story) || cx.edit.selection_len() != 1 {
            cx.requests.select(vec![story], SelectMode::Replace);
        }
        self.set(
            Some(TextEditing::Story(TextSelection {
                story,
                anchor,
                head,
            })),
            None,
            cx,
        );
    }

    /// The selection in force, checked against the document: a story that
    /// is gone ends the editing, and offsets past the text are clamped.
    fn current(&mut self, doc: &Document) -> Option<(TextSelection, Arc<StoryView>)> {
        let Some(Editing {
            state: TextEditing::Story(s),
            ..
        }) = self.editing
        else {
            return None;
        };
        let Some(v) = self.view(doc, s.story) else {
            self.editing = None;
            return None;
        };
        let clamp = |c: Caret| v.map.snap(c);
        Some((
            TextSelection {
                anchor: clamp(s.anchor),
                head: clamp(s.head),
                ..s
            },
            v,
        ))
    }

    fn click(&mut self, at: DocPoint, count: u8, cx: &mut ToolCtx<'_>) {
        let Some((story, v, hit)) = self.story_at(cx, at) else {
            self.pending(at, None, cx);
            return;
        };
        match count {
            1 => {
                let anchor = match self.current(cx.doc) {
                    Some((s, _)) if cx.modifiers.adjust && s.story == story => s.anchor,
                    _ => hit,
                };
                self.select_story(story, anchor, hit, cx);
            }
            2 => {
                let w = v.map.word_at(hit.byte);
                self.select_story(story, Caret::at(w.start), Caret::after(w.end), cx);
            }
            _ => {
                let line = v.map.line_of(hit);
                let r = v.map.line_range(line);
                self.select_story(story, Caret::at(r.start), Caret::after(r.end), cx);
            }
        }
    }

    fn pending(&mut self, at: DocPoint, column: Option<Mp>, cx: &mut ToolCtx<'_>) {
        if !cx.edit.is_selection_empty() {
            cx.requests.select(Vec::new(), SelectMode::Replace);
        }
        self.set(Some(TextEditing::Pending { at, column }), None, cx);
    }

    /// The size a new story's caret is drawn at: the current font size.
    fn pending_size(edit: &crate::edit::EditState) -> Mp {
        edit.current
            .values()
            .iter()
            .find_map(|v| match v {
                AttrValue::FontSize(s) if s.raw() > 0 => Some(*s),
                _ => None,
            })
            .unwrap_or(DEFAULT_SIZE)
    }

    /// A typing key: characters, Enter, Backspace, Delete.
    fn input(&mut self, input: &TextInput, cx: &mut ToolCtx<'_>) -> bool {
        let Some(editing) = self.editing else {
            return false;
        };
        if let TextEditing::Pending { at, column } = editing.state {
            if let TextInputKind::Insert(t) = &input.kind {
                self.start_story(at, column, typed(t), input.time_ms, cx);
            }
            // Nothing to delete yet.
            return true;
        }
        let Some((sel, v)) = self.current(cx.doc) else {
            return true;
        };
        let map = &v.map;
        let head = sel.head.byte;
        let (kind, range, text) = match &input.kind {
            TextInputKind::Insert(t) => (BurstKind::Typing, sel.range(), typed(t)),
            _ if !sel.is_caret() => (BurstKind::Deleting, sel.range(), String::new()),
            TextInputKind::Backspace { word } => {
                let from = if *word {
                    map.move_caret(sel.head, CaretMotion::Word, false, None)
                        .byte
                        .min(head)
                } else {
                    xarast_text::prev_grapheme(map.text(), head)
                };
                (BurstKind::Deleting, from..head, String::new())
            }
            TextInputKind::Delete { word } => {
                let to = if *word {
                    map.move_caret(sel.head, CaretMotion::Word, true, None)
                        .byte
                        .max(head)
                } else {
                    xarast_text::next_grapheme(map.text(), head)
                };
                (BurstKind::Deleting, head..to, String::new())
            }
        };
        if range.is_empty() && text.is_empty() {
            return true;
        }
        let joins = self.burst.is_some_and(|b| {
            b.kind == kind
                && b.story == Some(sel.story)
                && sel.is_caret()
                && b.caret == head
                && b.epoch == Some(cx.doc.epoch.0)
                && input.time_ms >= b.last_ms
                && input.time_ms - b.last_ms < TYPING_BURST_MS
        });
        let id = match self.burst {
            Some(b) if joins => b.id,
            _ => next_burst(),
        };
        let caret = range.start + text.len();
        cx.commands.emit(match kind {
            BurstKind::Typing => EditCommand::TypeText {
                story: sel.story,
                replace: range,
                text,
                burst: id,
            },
            BurstKind::Deleting => EditCommand::DeleteText {
                story: sel.story,
                range,
                burst: id,
            },
        });
        self.set(
            Some(TextEditing::Story(TextSelection {
                story: sel.story,
                anchor: Caret::at(caret),
                head: Caret::at(caret),
            })),
            None,
            cx,
        );
        self.burst = Some(Burst {
            id,
            kind,
            story: Some(sel.story),
            caret,
            last_ms: input.time_ms,
            epoch: None,
        });
        true
    }

    /// The first characters typed at a pending caret: a new story holding
    /// them, on the active layer, with the current attributes.
    fn start_story(
        &mut self,
        at: DocPoint,
        column: Option<Mp>,
        text: String,
        time_ms: u64,
        cx: &mut ToolCtx<'_>,
    ) {
        let Some(layer) = cx.edit.active_layer() else {
            return;
        };
        if text.is_empty() {
            return;
        }
        let layout = match column {
            Some(width) => TextLayout::InColumn {
                width,
                word_wrap: true,
            },
            None => TextLayout::AtPoint,
        };
        let id = next_burst();
        let caret = text.len();
        cx.commands.emit(EditCommand::CreateText {
            layer,
            story: Box::new(TextStoryNode {
                transform: Matrix::translate(Vector::new(at.x, at.y)),
                layout,
                ..TextStoryNode::default()
            }),
            attrs: cx.edit.current.values().to_vec(),
            text,
            burst: id,
        });
        self.burst = Some(Burst {
            id,
            kind: BurstKind::Typing,
            story: None,
            caret,
            last_ms: time_ms,
            epoch: None,
        });
    }

    fn navigate(&mut self, nav: TextNav, cx: &mut ToolCtx<'_>) -> bool {
        let Some((sel, v)) = self.current(cx.doc) else {
            // A pending caret has nowhere to go; the keys are still the
            // text's, not the view's.
            return self.editing.is_some();
        };
        let map = &v.map;
        let base_rtl = map
            .layout()
            .lines
            .get(map.line_of(sel.head))
            .is_some_and(|l| l.base_rtl);
        let right = matches!(nav.key, TextKey::Right);
        // Arrow keys in logical order go backwards in a right-to-left
        // paragraph when they point right.
        let logical_forward = right != base_rtl;
        let goal = self.editing.and_then(|e| e.goal_x);
        let page = Mp::from_f64_round(
            f64::from(cx.viewport.size().height) * cx.device_px() * v.story_scale(),
        );
        let (motion, forward) = match nav.key {
            TextKey::Left | TextKey::Right if nav.word => (CaretMotion::Word, logical_forward),
            TextKey::Left | TextKey::Right if self.logical_arrows => {
                (CaretMotion::Character { visual: false }, logical_forward)
            }
            TextKey::Left | TextKey::Right => (CaretMotion::Character { visual: true }, right),
            TextKey::Up => (CaretMotion::Line, false),
            TextKey::Down => (CaretMotion::Line, true),
            TextKey::Home if nav.word => (CaretMotion::StoryStart, false),
            TextKey::End if nav.word => (CaretMotion::StoryEnd, true),
            TextKey::Home => (CaretMotion::LineStart, false),
            TextKey::End => (CaretMotion::LineEnd, true),
            TextKey::PageUp => (CaretMotion::Page { height: page }, false),
            TextKey::PageDown => (CaretMotion::Page { height: page }, true),
        };
        let vertical = matches!(motion, CaretMotion::Line | CaretMotion::Page { .. });
        let head =
            if !nav.extend && !sel.is_caret() && matches!(motion, CaretMotion::Character { .. }) {
                // A selection collapses to the end the arrow points at.
                let r = sel.range();
                if logical_forward {
                    Caret::after(r.end)
                } else {
                    Caret::at(r.start)
                }
            } else {
                map.move_caret(sel.head, motion, forward, goal)
            };
        let goal_x = if vertical {
            Some(goal.unwrap_or_else(|| map.caret_x(sel.head)))
        } else {
            None
        };
        let anchor = if nav.extend { sel.anchor } else { head };
        self.set(
            Some(TextEditing::Story(TextSelection {
                story: sel.story,
                anchor,
                head,
            })),
            goal_x,
            cx,
        );
        true
    }
}

/// The caret map of a story laid out with `fonts`, as the text tool sees
/// it: for tests and for callers that want caret geometry without a tool.
/// `None` when `story` is not a text story.
#[must_use]
pub fn caret_map(doc: &Document, story: NodeId, fonts: &FontService) -> Option<CaretMap> {
    StoryView::build(doc, story, fonts).map(|v| v.map)
}

/// Every story on a visible, unlocked, non-guide layer, in paint order.
fn editable_stories(doc: &Document) -> impl Iterator<Item = NodeId> + '_ {
    let tree = &doc.tree;
    tree.preorder(tree.root()).filter(move |&id| {
        matches!(tree.kind(id), Some(NodeKind::TextStory(_)))
            && tree.ancestors(id).any(|a| {
                matches!(tree.kind(a), Some(NodeKind::Layer(l)) if l.visible && !l.locked && !l.guide)
            })
    })
}

impl Tool for TextTool {
    fn id(&self) -> ToolId {
        ToolId::Text
    }

    fn on_activate(&mut self, cx: &mut ToolCtx<'_>) {
        // One story selected: edit it, the caret at its end.
        if cx.edit.selection_len() == 1
            && let Some(story) = cx.edit.selection().next()
            && matches!(cx.doc.tree.kind(story), Some(NodeKind::TextStory(_)))
            && let Some(v) = self.view(cx.doc, story)
        {
            let end = v
                .map
                .move_caret(Caret::at(0), CaretMotion::StoryEnd, true, None);
            self.select_story(story, end, end, cx);
        }
    }

    fn on_deactivate(&mut self, cx: &mut ToolCtx<'_>) {
        self.drag = None;
        self.set(None, None, cx);
    }

    fn on_gesture(&mut self, ev: &GestureEvent, cx: &mut ToolCtx<'_>) {
        match ev {
            GestureEvent::Click { at, count, .. } => self.click(*at, *count, cx),
            GestureEvent::DragStart { from, .. } => {
                let before = self.editing;
                self.drag = Some(match self.story_at(cx, *from) {
                    Some((story, _, hit)) => {
                        let anchor = match self.current(cx.doc) {
                            Some((s, _)) if cx.modifiers.adjust && s.story == story => s.anchor,
                            _ => hit,
                        };
                        self.select_story(story, anchor, hit, cx);
                        Drag::Select {
                            story,
                            anchor,
                            before,
                        }
                    }
                    None => Drag::Column {
                        from: *from,
                        to: *from,
                        before,
                    },
                });
                cx.requests.overlay_changed = true;
            }
            GestureEvent::DragUpdate { to, .. } => match &mut self.drag {
                Some(Drag::Select { story, anchor, .. }) => {
                    let (story, anchor) = (*story, *anchor);
                    if let Some(v) = self.view(cx.doc, story) {
                        let head = v.map.hit_point(v.to_story(*to)).0;
                        self.set(
                            Some(TextEditing::Story(TextSelection {
                                story,
                                anchor,
                                head,
                            })),
                            None,
                            cx,
                        );
                    }
                }
                Some(Drag::Column { to: t, .. }) => {
                    *t = *to;
                    cx.requests.overlay_changed = true;
                }
                None => {}
            },
            GestureEvent::DragEnd { to, .. } => match self.drag.take() {
                Some(Drag::Column { from, .. }) => {
                    let width = (to.x.to_f64() - from.x.to_f64()).abs();
                    let size = Self::pending_size(cx.edit);
                    let left = from.x.min(to.x);
                    let top = from.y.max(to.y);
                    if width < MIN_COLUMN_PX * cx.device_px() {
                        self.pending(from, None, cx);
                    } else {
                        // The first line's top sits at the top of the drag.
                        let ascent = size.mul_ratio(4, 5);
                        let at = DocPoint::new(left, top.saturating_sub(ascent));
                        self.pending(at, Some(Mp::from_f64_round(width)), cx);
                    }
                }
                Some(Drag::Select { .. }) | None => {
                    cx.requests.overlay_changed = true;
                }
            },
            GestureEvent::Cancel => {
                if let Some(Drag::Select { before, .. } | Drag::Column { before, .. }) =
                    self.drag.take()
                {
                    self.editing = before;
                    self.moved = self.moved.wrapping_add(1);
                }
                cx.requests.overlay_changed = true;
            }
            GestureEvent::Hover { .. } | GestureEvent::ModifiersChanged { .. } => {}
        }
    }

    fn overlay(&self, view: ToolView<'_>, out: &mut Vec<OverlayShape>) {
        if let Some(Drag::Column { from, to, .. }) = self.drag {
            out.push(OverlayShape::Rect {
                rect: DocRect::new(from, to),
                dashed: true,
            });
            return;
        }
        match self.editing.map(|e| e.state) {
            Some(TextEditing::Pending { at, column }) => {
                let size = Self::pending_size(view.edit);
                let (ascent, descent) = (size.mul_ratio(4, 5), size.mul_ratio(1, 5));
                let from = DocPoint::new(at.x, at.y.saturating_sub(descent));
                let to = DocPoint::new(at.x, at.y.saturating_add(ascent));
                if let Some(w) = column {
                    out.push(OverlayShape::Rect {
                        rect: DocRect::new(from, DocPoint::new(at.x.saturating_add(w), to.y)),
                        dashed: true,
                    });
                }
                out.push(OverlayShape::Caret {
                    from,
                    to,
                    primary: true,
                    moved: self.moved,
                });
            }
            Some(TextEditing::Story(sel)) => {
                let Some(v) = self.view(view.doc, sel.story) else {
                    return;
                };
                let map = &v.map;
                let (anchor, head) = (map.snap(sel.anchor), map.snap(sel.head));
                // In story space: on a path, bent with the fitted text.
                for q in map.selection_quads(anchor.byte, head.byte) {
                    out.push(OverlayShape::Highlight {
                        corners: q.map(|p| v.to_doc(p)),
                    });
                }
                let (primary, secondary) = map.caret_segments(head);
                out.push(OverlayShape::Caret {
                    from: v.to_doc(primary.bottom),
                    to: v.to_doc(primary.top),
                    primary: true,
                    moved: self.moved,
                });
                if let Some(s) = secondary {
                    // The secondary caret: half height, from the baseline side.
                    out.push(OverlayShape::Caret {
                        from: v.to_doc(s.bottom),
                        to: v.to_doc(s.bottom.midpoint(s.top)),
                        primary: false,
                        moved: self.moved,
                    });
                }
            }
            None => {}
        }
    }

    fn infobar(&self, _view: ToolView<'_>) -> Infobar {
        let note = match self.editing.map(|e| e.state) {
            Some(TextEditing::Story(s)) if s.is_caret() => "Editing text".to_owned(),
            Some(TextEditing::Story(s)) => {
                format!("Editing text: {} bytes selected", s.range().len())
            }
            Some(TextEditing::Pending {
                column: Some(_), ..
            }) => "New text column: typing starts it".to_owned(),
            Some(TextEditing::Pending { column: None, .. }) => {
                "New text: typing starts it".to_owned()
            }
            None => {
                "Click text to edit it, click the page for new text, drag for a column".to_owned()
            }
        };
        Infobar {
            items: vec![InfobarItem::Note(note)],
        }
    }

    fn cursor(&self, _state: InteractionState) -> CursorKind {
        CursorKind::Text
    }

    fn action(&mut self, action: ToolAction, cx: &mut ToolCtx<'_>) -> bool {
        if self.editing.is_none() {
            return false;
        }
        match action {
            ToolAction::Cancel => {
                // Esc leaves the text; the story stays selected.
                self.set(None, None, cx);
                true
            }
            ToolAction::SelectAll => {
                if let Some((sel, v)) = self.current(cx.doc) {
                    let len = v.map.len();
                    self.select_story(sel.story, Caret::at(0), Caret::after(len), cx);
                }
                true
            }
            // While a caret is up these belong to the text, never to the
            // objects: Edit › Delete deletes forwards, Enter breaks the
            // paragraph. Neither joins a typing burst.
            ToolAction::Delete | ToolAction::Finish => {
                let kind = if action == ToolAction::Delete {
                    TextInputKind::Delete { word: false }
                } else {
                    TextInputKind::Insert("\n".to_owned())
                };
                self.burst = None;
                self.input(&TextInput { kind, time_ms: 0 }, cx)
            }
            _ => false,
        }
    }

    fn text_nav(&mut self, nav: TextNav, cx: &mut ToolCtx<'_>) -> bool {
        self.navigate(nav, cx)
    }

    fn text_editing(&self) -> Option<TextEditing> {
        self.editing()
    }

    fn text_input(&mut self, input: &TextInput, cx: &mut ToolCtx<'_>) -> bool {
        self.input(input, cx)
    }

    fn after_commands(&mut self, doc: &Document, created: Option<NodeId>) {
        let Some(b) = &mut self.burst else {
            return;
        };
        b.epoch = Some(doc.epoch.0);
        if b.story.is_none()
            && let Some(story) =
                created.filter(|&n| matches!(doc.tree.kind(n), Some(NodeKind::TextStory(_))))
        {
            // The story typing created: the caret goes on in it.
            b.story = Some(story);
            let caret = Caret::at(b.caret);
            self.editing = Some(Editing {
                state: TextEditing::Story(TextSelection {
                    story,
                    anchor: caret,
                    head: caret,
                }),
                goal_x: None,
            });
            self.moved = self.moved.wrapping_add(1);
        }
    }
}
