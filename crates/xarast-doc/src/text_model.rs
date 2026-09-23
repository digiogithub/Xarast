//! The logical text of a story, and positions in it (phase 9, W9.2).
//!
//! A story's structure is story → line → item ([`crate::text`]). Layout
//! wants something else: one string per story, byte ranges of identical
//! character style, one style per paragraph, and manual kerns anchored to
//! byte offsets (`docs/memory/text.md`, "The contract for W9.2"). This
//! module derives that from the tree, **without changing it**:
//!
//! * [`StoryText::collect`] walks one story with an [`AttrStack`] exactly as
//!   the render walk does (so a character's attributes are the ones it would
//!   paint with) and returns the text, its attribute runs, its lines and the
//!   item index.
//! * [`TextPos`] ↔ byte offset through that index, in `O(log n)`.
//! * [`TextCursor`] is the caret and selection. It lives in the text tool's
//!   state, never in the tree.
//!
//! The mapping from items to text is fixed: `Char(c)` → `c`, `Tab` → `'\t'`,
//! `LineBreak(true)` → `'\n'`, and `LineBreak(false)` and `Kern` → nothing
//! (a kern becomes a manual kern at the byte offset of what follows it).
//!
//! Nothing here depends on fonts: turning [`CharRun`]s into a shaper's style
//! runs is the application's bridge, which knows both crates.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use crate::attr::{AttrNode, AttrStack, AttrValue, ResolvedAttrs};
use crate::kind::NodeKind;
use crate::text::{TextItem, TextLayout, TextStoryNode};
use crate::tree::{NodeId, Tree};

/// A position between items of a story: before the `item`-th text item of
/// `line`, counting only [`TextItem`] children (attributes do not count).
/// `item` equal to the line's item count is the end of the line.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct TextPos {
    /// The `TextLine` node.
    pub line: NodeId,
    /// Index among the line's text items.
    pub item: u32,
}

/// A caret with an optional selection: the text between `anchor` and `head`.
/// Tool state, never stored in the document.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct TextCursor {
    /// The `TextStory` node.
    pub story: NodeId,
    /// Where the selection started.
    pub anchor: TextPos,
    /// Where the caret is.
    pub head: TextPos,
}

impl TextCursor {
    /// Whether nothing is selected.
    #[must_use]
    pub fn is_caret(&self) -> bool {
        self.anchor == self.head
    }
}

/// One item of the story, in document order.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct ItemEntry {
    /// The `TextItem` node.
    pub node: NodeId,
    /// Index into [`StoryText::lines`].
    pub line: u32,
    /// Index among its line's text items.
    pub index: u32,
    /// First byte of what it contributes to the text (the next character's
    /// offset for an item that contributes nothing).
    pub byte: u32,
    /// Bytes it contributes: 0 for a kern or a soft break.
    pub len: u8,
}

/// One `TextLine` of the story.
#[derive(Clone, Debug)]
pub struct LineEntry {
    /// The `TextLine` node.
    pub node: NodeId,
    /// Index of its first item in [`StoryText::items`].
    pub first_item: u32,
    /// How many text items it holds.
    pub item_count: u32,
    /// Byte offset where it starts in the text.
    pub first_byte: u32,
    /// Whether it ends with a paragraph break.
    pub ends_paragraph: bool,
    /// The attributes in force at its first item (at its end when it has
    /// none): what the line-level attributes — alignment, spacing,
    /// margins, ruler — resolve to.
    pub attrs: ResolvedAttrs,
}

/// A maximal byte range whose characters paint with identical attributes.
#[derive(Clone, Debug)]
pub struct CharRun {
    /// Byte range in [`StoryText::text`].
    pub range: Range<usize>,
    /// The attributes in force.
    pub attrs: ResolvedAttrs,
}

/// A manual kern, as stored: `amount` is the raw item value, which is
/// thousandths of an em (`Kernel/nodetext.cpp:1763-1767`).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct KernAt {
    /// Byte offset of the character it precedes.
    pub at: usize,
    /// The kern, em/1000.
    pub amount: i32,
}

/// A story's logical text, attribute runs and index.
#[derive(Clone, Debug)]
pub struct StoryText {
    /// The `TextStory` node.
    pub story: NodeId,
    /// The logical text.
    pub text: String,
    /// Attribute runs covering `text` without gaps, in order. Empty when the
    /// text is.
    pub runs: Vec<CharRun>,
    /// The story's lines, in order.
    pub lines: Vec<LineEntry>,
    /// Every text item, in document order.
    pub items: Vec<ItemEntry>,
    /// Manual kerns, in order.
    pub kerns: Vec<KernAt>,
    /// The attributes in force at the story itself (after its own attribute
    /// children): the fallback for an empty story.
    pub story_attrs: ResolvedAttrs,
    line_of: HashMap<NodeId, u32>,
}

impl StoryText {
    /// Collects the story `story`.
    ///
    /// `attrs` must be the attribute state in force where the story sits,
    /// as a render walk has it on visiting the story; it is returned to that
    /// state. `attr_value` turns an attribute node into the shared value to
    /// push (a walker passes its cache; [`StoryText::collect_simple`] clones).
    ///
    /// Returns `None` when `story` is not a `TextStory`.
    pub fn collect(
        tree: &Tree,
        story: NodeId,
        attrs: &mut AttrStack,
        attr_value: &mut dyn FnMut(NodeId, &AttrNode) -> Arc<AttrValue>,
    ) -> Option<StoryText> {
        if !matches!(tree.kind(story), Some(NodeKind::TextStory(_))) {
            return None;
        }
        let story_attrs = attrs.snapshot();
        let mut c = Collector {
            tree,
            attrs,
            attr_value,
            out: StoryText {
                story,
                text: String::new(),
                runs: Vec::new(),
                lines: Vec::new(),
                items: Vec::new(),
                kerns: Vec::new(),
                story_attrs,
                line_of: HashMap::new(),
            },
            dirty: true,
            current: None,
            line: None,
            line_attrs_pending: false,
        };
        c.attrs.push_scope();
        c.children(story, 0);
        c.out.story_attrs = c.attrs.snapshot();
        c.attrs.pop_scope();
        c.close_run();
        Some(c.out)
    }

    /// [`StoryText::collect`] from the document defaults, for callers with
    /// no walk in progress. Attributes above the story are **not** applied.
    pub fn collect_simple(
        tree: &Tree,
        defaults: &crate::attr::DefaultAttrs,
        story: NodeId,
    ) -> Option<StoryText> {
        let mut attrs = AttrStack::with_defaults(defaults);
        StoryText::collect(tree, story, &mut attrs, &mut |_, a| {
            Arc::new(a.value.clone())
        })
    }

    /// The line entry for a `TextLine` node.
    #[must_use]
    pub fn line(&self, node: NodeId) -> Option<&LineEntry> {
        self.line_of
            .get(&node)
            .and_then(|&i| self.lines.get(i as usize))
    }

    /// The byte offset of a position, or `None` when it is not in the story.
    #[must_use]
    pub fn byte_of(&self, pos: TextPos) -> Option<usize> {
        let li = *self.line_of.get(&pos.line)?;
        let line = self.lines.get(li as usize)?;
        if pos.item > line.item_count {
            return None;
        }
        if pos.item == line.item_count {
            // The end of the line: where the next one starts.
            return Some(
                self.lines
                    .get(li as usize + 1)
                    .map_or(self.text.len(), |n| n.first_byte as usize),
            );
        }
        let e = self.items.get((line.first_item + pos.item) as usize)?;
        Some(e.byte as usize)
    }

    /// The position of a byte offset: before the first item that starts at
    /// or after it, or the end of the last line. `None` for an empty story.
    #[must_use]
    pub fn pos_of(&self, byte: usize) -> Option<TextPos> {
        let k = self.items.partition_point(|e| (e.byte as usize) < byte);
        match self.items.get(k) {
            Some(e) => Some(TextPos {
                line: self.lines.get(e.line as usize)?.node,
                item: e.index,
            }),
            None => self.lines.last().map(|l| TextPos {
                line: l.node,
                item: l.item_count,
            }),
        }
    }

    /// Index of the run holding byte `at`.
    #[must_use]
    pub fn run_at(&self, at: usize) -> Option<usize> {
        let k = self.runs.partition_point(|r| r.range.end <= at);
        (k < self.runs.len()).then_some(k)
    }

    /// Paragraph index of every line (a paragraph ends at a line that ends
    /// with a paragraph break).
    #[must_use]
    pub fn paragraph_first_lines(&self) -> Vec<usize> {
        let mut out = Vec::new();
        let mut start = true;
        for (i, l) in self.lines.iter().enumerate() {
            if start {
                out.push(i);
            }
            start = l.ends_paragraph;
        }
        out
    }
}

/// How a story is laid out, reduced to what layout needs.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum StoryFlow {
    /// Free text about an anchor.
    Point,
    /// A column of this width (millipoints).
    Column {
        /// Width.
        width: i32,
        /// Whether words wrap.
        wrap: bool,
    },
    /// Text on a path: laid out as a point story until W9.5 fits it.
    OnPath,
}

impl StoryFlow {
    /// The flow of a story node.
    #[must_use]
    pub fn of(story: &TextStoryNode) -> StoryFlow {
        match story.layout {
            TextLayout::AtPoint => StoryFlow::Point,
            TextLayout::InColumn { width, word_wrap } => StoryFlow::Column {
                width: width.raw(),
                wrap: word_wrap,
            },
            TextLayout::OnPath { .. } => StoryFlow::OnPath,
        }
    }
}

struct Collector<'a> {
    tree: &'a Tree,
    attrs: &'a mut AttrStack,
    attr_value: &'a mut dyn FnMut(NodeId, &AttrNode) -> Arc<AttrValue>,
    out: StoryText,
    /// The stack changed since the current run's snapshot.
    dirty: bool,
    /// The open run: its start and its attributes.
    current: Option<(usize, ResolvedAttrs)>,
    /// The line being collected.
    line: Option<u32>,
    /// The line's attributes are still to be taken, at its first item.
    line_attrs_pending: bool,
}

impl Collector<'_> {
    fn children(&mut self, parent: NodeId, depth: usize) {
        // Stories are three levels deep; anything deeper is not text.
        if depth > 8 {
            return;
        }
        let kids: Vec<NodeId> = self.tree.children(parent).collect();
        for child in kids {
            let Some(kind) = self.tree.kind(child) else {
                continue;
            };
            match kind {
                NodeKind::Attr(a) => {
                    let v = (self.attr_value)(child, a);
                    self.attrs.push(v);
                    self.dirty = true;
                }
                NodeKind::TextLine(_) => {
                    self.begin_line(child);
                    self.scoped(child, depth);
                    self.end_line();
                }
                NodeKind::TextItem(item) => {
                    // An item's own attribute children apply to it: the
                    // render walk paints a node with children at the end of
                    // their scope.
                    let item = *item;
                    if self.tree.links(child).first_child.is_some() {
                        self.attrs.push_scope();
                        self.children(child, depth + 1);
                        self.dirty = true;
                        self.item(child, item);
                        self.attrs.pop_scope();
                        self.dirty = true;
                    } else {
                        self.item(child, item);
                    }
                }
                // A story's path (text on a path) and anything else: not text.
                _ => {}
            }
        }
    }

    /// Visits a node's children in their own attribute scope, as the render
    /// walk does for a node with children.
    fn scoped(&mut self, node: NodeId, depth: usize) {
        if self.tree.links(node).first_child.is_none() {
            return;
        }
        self.attrs.push_scope();
        self.children(node, depth + 1);
        self.attrs.pop_scope();
        self.dirty = true;
    }

    fn begin_line(&mut self, node: NodeId) {
        let idx = self.out.lines.len() as u32;
        self.out.line_of.insert(node, idx);
        self.out.lines.push(LineEntry {
            node,
            first_item: self.out.items.len() as u32,
            item_count: 0,
            first_byte: self.out.text.len() as u32,
            ends_paragraph: false,
            attrs: self.attrs.snapshot(),
        });
        self.line = Some(idx);
        self.line_attrs_pending = true;
    }

    fn end_line(&mut self) {
        if self.line_attrs_pending {
            let snap = self.attrs.snapshot();
            if let Some(l) = self.out.lines.last_mut() {
                l.attrs = snap;
            }
            self.line_attrs_pending = false;
        }
        self.line = None;
    }

    fn item(&mut self, node: NodeId, item: TextItem) {
        let Some(line) = self.line else {
            return;
        };
        let byte = self.out.text.len();
        let piece: Option<char> = match item {
            TextItem::Char(c) => Some(c),
            TextItem::Tab => Some('\t'),
            TextItem::LineBreak(true) => Some('\n'),
            TextItem::LineBreak(false) => None,
            TextItem::Kern(k) => {
                self.out.kerns.push(KernAt {
                    at: byte,
                    amount: k.raw(),
                });
                None
            }
        };
        if self.line_attrs_pending || (piece.is_some() && self.dirty) {
            let snap = self.attrs.snapshot();
            if self.line_attrs_pending {
                if let Some(l) = self.out.lines.last_mut() {
                    l.attrs = snap.clone();
                }
                self.line_attrs_pending = false;
            }
            if piece.is_some() && self.dirty {
                let same = self.current.as_ref().is_some_and(|(_, a)| *a == snap);
                if !same {
                    self.close_run();
                    self.current = Some((byte, snap));
                }
                self.dirty = false;
            }
        }
        let len = piece.map_or(0, |c| {
            self.out.text.push(c);
            c.len_utf8()
        });
        let l = &mut self.out.lines[line as usize];
        let index = l.item_count;
        l.item_count += 1;
        if matches!(item, TextItem::LineBreak(true)) {
            l.ends_paragraph = true;
        }
        self.out.items.push(ItemEntry {
            node,
            line,
            index,
            byte: byte as u32,
            len: len as u8,
        });
    }

    fn close_run(&mut self) {
        if let Some((start, attrs)) = self.current.take() {
            let end = self.out.text.len();
            if end > start {
                self.out.runs.push(CharRun {
                    range: start..end,
                    attrs,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attr::{AttrSlot, AttrValue};
    use crate::builder::{BuildLimits, skeleton};
    use xarast_geom::Mp;

    /// Story: size 20 at story level; line 1 "ab" + a 12 pt "c" + kern +
    /// "d" + break; line 2 tab + "é".
    fn doc() -> (crate::Document, NodeId) {
        let mut b = skeleton(BuildLimits::default()).unwrap();
        let story = b
            .node(NodeKind::TextStory(Box::default()))
            .unwrap()
            .node_id();
        b.push_scope().unwrap();
        b.attribute(AttrValue::FontSize(Mp::new(20_000))).unwrap();
        b.node(NodeKind::TextLine(Box::default())).unwrap();
        b.push_scope().unwrap();
        for c in ['a', 'b'] {
            b.node(NodeKind::TextItem(TextItem::Char(c))).unwrap();
        }
        b.attribute(AttrValue::FontSize(Mp::new(12_000))).unwrap();
        b.node(NodeKind::TextItem(TextItem::Char('c'))).unwrap();
        b.node(NodeKind::TextItem(TextItem::Kern(Mp::new(-50))))
            .unwrap();
        b.node(NodeKind::TextItem(TextItem::Char('d'))).unwrap();
        b.node(NodeKind::TextItem(TextItem::LineBreak(true)))
            .unwrap();
        b.pop_scope();
        b.node(NodeKind::TextLine(Box::default())).unwrap();
        b.push_scope().unwrap();
        b.node(NodeKind::TextItem(TextItem::Tab)).unwrap();
        b.node(NodeKind::TextItem(TextItem::Char('é'))).unwrap();
        b.pop_scope();
        b.pop_scope();
        let (doc, _) = b.finish().unwrap();
        (doc, story)
    }

    fn size(a: &ResolvedAttrs) -> i32 {
        match a.get(AttrSlot::TxtFontSize) {
            AttrValue::FontSize(v) => v.raw(),
            _ => 0,
        }
    }

    #[test]
    fn the_logical_text_runs_and_kerns_follow_the_items() {
        let (doc, story) = doc();
        let t = StoryText::collect_simple(&doc.tree, &doc.defaults, story).unwrap();
        assert_eq!(t.text, "abcd\n\té");
        assert_eq!(t.kerns, [KernAt { at: 3, amount: -50 }]);
        let runs: Vec<(Range<usize>, i32)> = t
            .runs
            .iter()
            .map(|r| (r.range.clone(), size(&r.attrs)))
            .collect();
        // The 12 pt attribute scopes to the rest of line 1; line 2 is back
        // at the story's 20 pt.
        assert_eq!(runs, [(0..2, 20_000), (2..5, 12_000), (5..8, 20_000)]);
        assert_eq!(t.lines.len(), 2);
        assert!(t.lines[0].ends_paragraph && !t.lines[1].ends_paragraph);
        assert_eq!(t.paragraph_first_lines(), [0, 1]);
    }

    #[test]
    fn positions_and_byte_offsets_round_trip() {
        let (doc, story) = doc();
        let t = StoryText::collect_simple(&doc.tree, &doc.defaults, story).unwrap();
        let l1 = t.lines[0].node;
        let l2 = t.lines[1].node;
        assert_eq!(t.byte_of(TextPos { line: l1, item: 0 }), Some(0));
        // Item 3 is the kern: it sits at the byte of the 'd' it precedes.
        assert_eq!(t.byte_of(TextPos { line: l1, item: 3 }), Some(3));
        assert_eq!(t.byte_of(TextPos { line: l1, item: 4 }), Some(3));
        assert_eq!(t.byte_of(TextPos { line: l1, item: 6 }), Some(5));
        assert_eq!(t.byte_of(TextPos { line: l1, item: 7 }), None);
        assert_eq!(t.byte_of(TextPos { line: l2, item: 2 }), Some(8));
        assert_eq!(t.pos_of(0), Some(TextPos { line: l1, item: 0 }));
        assert_eq!(t.pos_of(5), Some(TextPos { line: l2, item: 0 }));
        assert_eq!(t.pos_of(6), Some(TextPos { line: l2, item: 1 }));
        assert_eq!(t.pos_of(8), Some(TextPos { line: l2, item: 2 }));
        for item in 0..=2 {
            let p = TextPos { line: l2, item };
            assert_eq!(t.pos_of(t.byte_of(p).unwrap()), Some(p));
        }
        assert_eq!(t.run_at(2), Some(1));
        assert_eq!(t.run_at(99), None);
    }

    #[test]
    fn collecting_leaves_the_stack_as_it_found_it() {
        let (doc, story) = doc();
        let mut attrs = AttrStack::with_defaults(&doc.defaults);
        let depth = attrs.depth();
        let before = attrs.snapshot();
        StoryText::collect(&doc.tree, story, &mut attrs, &mut |_, a| {
            Arc::new(a.value.clone())
        })
        .unwrap();
        assert_eq!(attrs.depth(), depth);
        assert!(attrs.snapshot() == before);
    }
}
