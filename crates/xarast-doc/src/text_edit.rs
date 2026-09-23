//! Text edit commands (phase 9, T9.2.4): typing into a story and deleting
//! from it, through the transaction like every other edit.
//!
//! Positions are **byte offsets into the story's logical text**
//! ([`StoryText::text`]), not [`TextPos`](crate::TextPos): the text tool,
//! layout and hit testing all speak offsets, and an offset survives the
//! line restructuring an edit does where a `(line, item)` pair does not.
//! [`StoryText::pos_of`]/[`StoryText::byte_of`] convert when needed.
//!
//! # What an edit does to the tree
//!
//! * **Insertion** puts one `TextItem` node per character right after the
//!   character before the offset, so the new text paints with that
//!   character's attributes (its own attribute children are copied onto
//!   each new item). At the start of a line, or after a paragraph break,
//!   the new items go before the first item at the offset instead.
//! * **`'\n'`** inserts a paragraph break and splits the line after it:
//!   everything that followed moves to a new `TextLine` right after, which
//!   starts with copies of the line's attributes in scope at the split (the
//!   last of each slot), so no character changes style. Every line keeps
//!   ending with its break.
//! * **Deletion** detaches the items in the range. Deleting a paragraph
//!   break does not merge lines: the line simply no longer ends with a
//!   break and its paragraph runs on into the next line, which keeps every
//!   character's attributes exact. A line left without items goes, unless
//!   it is the story's last.
//!
//! Lines are derived state (`phase-09`, "word wrap restructures the tree");
//! nothing here reflows. The undo record is the per-node actions of the
//! edit, proportional to the text typed or deleted, not to the story.

use std::ops::Range;

use crate::attr::{AttrNode, AttrValue};
use crate::history::{CoalesceKey, Command, EditError, Tx};
use crate::kind::NodeKind;
use crate::text::{TextItem, TextLineNode, TextStoryNode};
use crate::text_model::StoryText;
use crate::tree::{Attach, NodeId};

/// Types `text` into a story at a byte offset. `'\n'` starts a paragraph,
/// `'\t'` is a tab, `'\r'` and other control characters are dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InsertText {
    /// The `TextStory` node.
    pub story: NodeId,
    /// Byte offset in the story's logical text.
    pub at: usize,
    /// What to type.
    pub text: String,
    /// The key consecutive typing merges on (a typing burst).
    pub coalesce: Option<CoalesceKey>,
}

/// Deletes a byte range of a story's logical text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteRange {
    /// The `TextStory` node.
    pub story: NodeId,
    /// Byte range in the story's logical text.
    pub range: Range<usize>,
    /// The key consecutive deletions merge on.
    pub coalesce: Option<CoalesceKey>,
}

impl Command for InsertText {
    fn label(&self) -> &'static str {
        "Typing"
    }

    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        insert_text(tx, self.story, self.at, &self.text).map(|_| ())
    }

    fn coalesce_key(&self) -> Option<CoalesceKey> {
        self.coalesce
    }
}

impl Command for DeleteRange {
    fn label(&self) -> &'static str {
        "Delete Text"
    }

    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        delete_range(tx, self.story, self.range.clone())
    }

    fn coalesce_key(&self) -> Option<CoalesceKey> {
        self.coalesce
    }
}

/// The story's logical text and index, as it stands in the transaction.
fn collect(tx: &Tx<'_>, story: NodeId) -> Result<StoryText, EditError> {
    let doc = tx.doc();
    StoryText::collect_simple(&doc.tree, &doc.defaults, story).ok_or(EditError::WrongKind(story))
}

/// Where the next inserted item goes.
#[derive(Copy, Clone, Debug)]
enum Slot {
    /// Right after this item.
    After(NodeId),
    /// Right before this item.
    Before(NodeId),
    /// At the end of this line.
    End(NodeId),
}

impl Slot {
    fn put(self, tx: &mut Tx<'_>, node: NodeId) -> Result<Slot, EditError> {
        Ok(match self {
            Slot::After(a) => {
                tx.attach(node, a, Attach::Next)?;
                Slot::After(node)
            }
            Slot::Before(b) => {
                tx.attach(node, b, Attach::Prev)?;
                self
            }
            Slot::End(line) => {
                tx.attach(node, line, Attach::LastChild)?;
                self
            }
        })
    }

    /// The line the slot is in.
    fn line(self, tx: &Tx<'_>) -> Option<NodeId> {
        match self {
            Slot::After(n) | Slot::Before(n) => tx.doc().tree.links(n).parent,
            Slot::End(line) => Some(line),
        }
    }
}

/// Types `text` into `story` at byte offset `at`. Returns the number of
/// bytes the story's text grew by (dropped control characters do not
/// count).
///
/// # Errors
///
/// [`EditError::WrongKind`] when `story` is not a story,
/// [`EditError::TextEdit`] when `at` is past the text or inside a
/// character, and whatever the transaction refuses (a locked story).
pub fn insert_text(
    tx: &mut Tx<'_>,
    story: NodeId,
    at: usize,
    text: &str,
) -> Result<usize, EditError> {
    let st = collect(tx, story)?;
    if at > st.text.len() || !st.text.is_char_boundary(at) {
        return Err(EditError::TextEdit("offset outside the story's text"));
    }
    let tree = &tx.doc().tree;
    // The item after which the text goes: the character ending at `at`,
    // unless it is a paragraph break (the text then starts the next line).
    let k = st.items.partition_point(|e| (e.byte as usize) < at);
    let prev = st.items[..k]
        .iter()
        .rev()
        .find(|e| e.len > 0)
        .filter(|e| e.byte as usize + e.len as usize == at)
        .filter(|e| {
            !matches!(
                tree.kind(e.node),
                Some(NodeKind::TextItem(TextItem::LineBreak(true)))
            )
        });
    // Attribute children of the character the text continues, copied onto
    // every new item so it paints the same.
    let mut style: Vec<AttrNode> = Vec::new();
    let mut slot = if let Some(p) = prev {
        style = tree
            .children(p.node)
            .filter_map(|c| match tree.kind(c) {
                Some(NodeKind::Attr(a)) => Some((**a).clone()),
                _ => None,
            })
            .collect();
        Slot::After(p.node)
    } else if let Some(next) = st.items.get(k) {
        Slot::Before(next.node)
    } else if let Some(last) = st.lines.last() {
        if last.ends_paragraph {
            // The text ends with a paragraph break and nothing follows:
            // the new text is a new line.
            let line = new_line_after(tx, last.node)?;
            Slot::End(line)
        } else {
            Slot::End(last.node)
        }
    } else {
        let line = tx.create(NodeKind::TextLine(Box::default()))?;
        tx.attach(line, story, Attach::LastChild)?;
        Slot::End(line)
    };
    let mut grew = 0;
    for c in text.chars() {
        let item = match c {
            '\n' => TextItem::LineBreak(true),
            '\t' => TextItem::Tab,
            c if c.is_control() => continue,
            c => TextItem::Char(c),
        };
        let node = tx.create(NodeKind::TextItem(item))?;
        slot = slot.put(tx, node)?;
        for a in &style {
            let attr = tx.create(NodeKind::Attr(Box::new(a.clone())))?;
            tx.attach(attr, node, Attach::LastChild)?;
        }
        grew += c.len_utf8();
        if c == '\n' {
            let line = slot
                .line(tx)
                .ok_or(EditError::TextEdit("item outside a line"))?;
            let next = split_line(tx, line, node)?;
            let tree = &tx.doc().tree;
            let first_item = tree
                .children(next)
                .find(|&n| matches!(tree.kind(n), Some(NodeKind::TextItem(_))));
            slot = first_item.map_or(Slot::End(next), Slot::Before);
        }
    }
    Ok(grew)
}

/// A new, empty line right after `line`, with the same ruler.
fn new_line_after(tx: &mut Tx<'_>, line: NodeId) -> Result<NodeId, EditError> {
    let kind = match tx.doc().tree.kind(line) {
        Some(NodeKind::TextLine(l)) => l.clone(),
        _ => Box::<TextLineNode>::default(),
    };
    let next = tx.create(NodeKind::TextLine(kind))?;
    tx.attach(next, line, Attach::Next)?;
    Ok(next)
}

/// Splits `line` after its child `after`: everything following moves to a
/// new line right after it, which starts with the attributes the line had
/// in scope at that point (the last of each slot). Returns the new line.
fn split_line(tx: &mut Tx<'_>, line: NodeId, after: NodeId) -> Result<NodeId, EditError> {
    let tree = &tx.doc().tree;
    let kids: Vec<NodeId> = tree.children(line).collect();
    let Some(cut) = kids.iter().position(|&k| k == after) else {
        return Err(EditError::TextEdit("split point outside the line"));
    };
    let mut scope: Vec<AttrNode> = Vec::new();
    for &k in &kids[..cut] {
        if let Some(NodeKind::Attr(a)) = tree.kind(k) {
            if let Some(slot) = a.value.slot() {
                scope.retain(|s: &AttrNode| s.value.slot() != Some(slot));
            }
            scope.push((**a).clone());
        }
    }
    let rest = kids[cut + 1..].to_vec();
    let next = new_line_after(tx, line)?;
    for a in scope {
        let attr = tx.create(NodeKind::Attr(Box::new(a)))?;
        tx.attach(attr, next, Attach::LastChild)?;
    }
    for r in rest {
        tx.move_node(r, next, Attach::LastChild)?;
    }
    Ok(next)
}

/// Deletes the byte range `range` of `story`'s text.
///
/// A kern or a soft break strictly inside the range goes with it; one at
/// its start stays, before whatever now follows.
///
/// # Errors
///
/// [`EditError::WrongKind`] when `story` is not a story,
/// [`EditError::TextEdit`] when the range is reversed, past the text or
/// cuts a character, and whatever the transaction refuses.
pub fn delete_range(tx: &mut Tx<'_>, story: NodeId, range: Range<usize>) -> Result<(), EditError> {
    let st = collect(tx, story)?;
    if range.start > range.end
        || range.end > st.text.len()
        || !st.text.is_char_boundary(range.start)
        || !st.text.is_char_boundary(range.end)
    {
        return Err(EditError::TextEdit("range outside the story's text"));
    }
    if range.is_empty() {
        return Ok(());
    }
    let mut left = vec![0u32; st.lines.len()];
    let mut doomed = Vec::new();
    for e in &st.items {
        let (b, len) = (e.byte as usize, e.len as usize);
        let inside = if len > 0 {
            b >= range.start && b + len <= range.end
        } else {
            b > range.start && b < range.end
        };
        if inside {
            doomed.push(e.node);
        } else if let Some(l) = left.get_mut(e.line as usize) {
            *l += 1;
        }
    }
    for n in doomed {
        tx.delete(n)?;
    }
    // Lines left without items go; the story keeps at least its last line.
    let emptied: Vec<NodeId> = st
        .lines
        .iter()
        .zip(&left)
        .filter(|(l, n)| **n == 0 && l.item_count > 0)
        .map(|(l, _)| l.node)
        .collect();
    let keep_one = emptied.len() == st.lines.len();
    for (i, line) in emptied.into_iter().enumerate() {
        if keep_one && i + 1 == st.lines.len() {
            break;
        }
        tx.delete(line)?;
    }
    Ok(())
}

/// Creates an empty story as the last object of `parent`: the story node,
/// the given attributes as its own attribute children, and one line holding
/// only the final paragraph break every story ends with. Returns it.
///
/// # Errors
///
/// Whatever the transaction refuses.
pub fn new_story(
    tx: &mut Tx<'_>,
    parent: NodeId,
    node: TextStoryNode,
    attrs: &[AttrValue],
) -> Result<NodeId, EditError> {
    let story = tx.create(NodeKind::TextStory(Box::new(node)))?;
    tx.attach(story, parent, Attach::LastChild)?;
    for a in attrs {
        let attr = tx.create(NodeKind::Attr(Box::new(AttrNode::new(a.clone()))))?;
        tx.attach(attr, story, Attach::LastChild)?;
    }
    let line = tx.create(NodeKind::TextLine(Box::default()))?;
    tx.attach(line, story, Attach::LastChild)?;
    let eol = tx.create(NodeKind::TextItem(TextItem::LineBreak(true)))?;
    tx.attach(eol, line, Attach::LastChild)?;
    Ok(story)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Document;
    use crate::attr::{AttrSlot, ResolvedAttrs};
    use crate::builder::{BuildLimits, skeleton};
    use crate::history::CommandBus;
    use xarast_geom::Mp;

    /// Story at 20 pt: line 1 "ab" + a 12 pt "cd" + break; line 2 "ef" +
    /// the final break.
    fn doc() -> (Document, NodeId) {
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
        for c in ['c', 'd'] {
            b.node(NodeKind::TextItem(TextItem::Char(c))).unwrap();
        }
        b.node(NodeKind::TextItem(TextItem::LineBreak(true)))
            .unwrap();
        b.pop_scope();
        b.node(NodeKind::TextLine(Box::default())).unwrap();
        b.push_scope().unwrap();
        for c in ['e', 'f'] {
            b.node(NodeKind::TextItem(TextItem::Char(c))).unwrap();
        }
        b.node(NodeKind::TextItem(TextItem::LineBreak(true)))
            .unwrap();
        b.pop_scope();
        b.pop_scope();
        let (doc, _) = b.finish().unwrap();
        (doc, story)
    }

    fn text(doc: &Document, story: NodeId) -> StoryText {
        StoryText::collect_simple(&doc.tree, &doc.defaults, story).unwrap()
    }

    fn size(a: &ResolvedAttrs) -> i32 {
        match a.get(AttrSlot::TxtFontSize) {
            AttrValue::FontSize(v) => v.raw(),
            _ => 0,
        }
    }

    /// Every character's size, in text order.
    fn sizes(st: &StoryText) -> Vec<(char, i32)> {
        st.text
            .char_indices()
            .map(|(i, c)| (c, size(&st.runs[st.run_at(i).unwrap()].attrs)))
            .collect()
    }

    fn insert(bus: &mut CommandBus, doc: &mut Document, story: NodeId, at: usize, t: &str) {
        let cmd = InsertText {
            story,
            at,
            text: t.to_owned(),
            coalesce: None,
        };
        bus.dispatch(doc, &cmd).unwrap();
    }

    #[test]
    fn typed_text_takes_the_style_of_the_character_before_it() {
        let (mut doc, story) = doc();
        let before = doc.canonical_digest();
        let mut bus = CommandBus::new();
        insert(&mut bus, &mut doc, story, 2, "XY");
        insert(&mut bus, &mut doc, story, 6, "Z");
        insert(&mut bus, &mut doc, story, 0, "<");
        let st = text(&doc, story);
        assert_eq!(st.text, "<abXYcdZ\nef\n");
        let s = sizes(&st);
        assert_eq!(s[3], ('X', 20_000));
        assert_eq!(s[7], ('Z', 12_000));
        assert_eq!(s[0], ('<', 20_000));
        assert!(crate::validate::validate_document(&doc).errors.is_empty());
        for _ in 0..3 {
            bus.undo(&mut doc).unwrap();
        }
        assert_eq!(doc.canonical_digest(), before);
    }

    #[test]
    fn a_newline_splits_the_line_and_keeps_every_style() {
        let (mut doc, story) = doc();
        let before = doc.canonical_digest();
        let mut bus = CommandBus::new();
        let was = sizes(&text(&doc, story));
        // Between "c" and "d": "d" must stay 12 pt on its new line.
        insert(&mut bus, &mut doc, story, 3, "\n");
        let st = text(&doc, story);
        assert_eq!(st.text, "abc\nd\nef\n");
        assert_eq!(st.lines.len(), 3);
        assert!(st.lines.iter().all(|l| l.ends_paragraph));
        let mut now = sizes(&st);
        now.remove(3);
        assert_eq!(now, was);
        // At the end of the story's last line, before its final break.
        insert(&mut bus, &mut doc, story, 8, "\ng");
        assert_eq!(text(&doc, story).text, "abc\nd\nef\ng\n");
        assert_eq!(text(&doc, story).lines.len(), 4);
        assert!(crate::validate::validate_document(&doc).errors.is_empty());
        bus.undo(&mut doc).unwrap();
        bus.undo(&mut doc).unwrap();
        assert_eq!(doc.canonical_digest(), before);
    }

    #[test]
    fn deleting_a_break_joins_paragraphs_without_restyling() {
        let (mut doc, story) = doc();
        let before = doc.canonical_digest();
        let mut bus = CommandBus::new();
        let del = |range: Range<usize>| DeleteRange {
            story,
            range,
            coalesce: None,
        };
        bus.dispatch(&mut doc, &del(4..5)).unwrap();
        let st = text(&doc, story);
        assert_eq!(st.text, "abcdef\n");
        assert_eq!(st.paragraph_first_lines(), [0]);
        assert_eq!(
            sizes(&st)[..6],
            [
                ('a', 20_000),
                ('b', 20_000),
                ('c', 12_000),
                ('d', 12_000),
                ('e', 20_000),
                ('f', 20_000)
            ]
        );
        // Across the join: the first line goes empty and is removed.
        bus.dispatch(&mut doc, &del(0..5)).unwrap();
        let st = text(&doc, story);
        assert_eq!(st.text, "f\n");
        assert_eq!(st.lines.len(), 1);
        assert!(crate::validate::validate_document(&doc).errors.is_empty());
        bus.undo(&mut doc).unwrap();
        bus.undo(&mut doc).unwrap();
        assert_eq!(doc.canonical_digest(), before);
    }

    #[test]
    fn bad_offsets_are_refused_and_change_nothing() {
        let (mut doc, story) = doc();
        let before = doc.canonical_digest();
        let mut bus = CommandBus::new();
        let bad = InsertText {
            story,
            at: 99,
            text: "x".into(),
            coalesce: None,
        };
        assert!(matches!(
            bus.dispatch(&mut doc, &bad),
            Err(EditError::TextEdit(_))
        ));
        let bad = DeleteRange {
            story,
            range: 3..99,
            coalesce: None,
        };
        assert!(matches!(
            bus.dispatch(&mut doc, &bad),
            Err(EditError::TextEdit(_))
        ));
        assert_eq!(doc.canonical_digest(), before);
    }

    #[test]
    fn a_new_story_is_one_line_with_its_final_break() {
        let (mut doc, _) = doc();
        let layer = doc
            .tree
            .preorder(doc.tree.root())
            .find(|&n| matches!(doc.tree.kind(n), Some(NodeKind::Layer(_))))
            .unwrap();
        let mut tx = Tx::begin(&mut doc);
        let story = new_story(
            &mut tx,
            layer,
            TextStoryNode::default(),
            &[AttrValue::FontSize(Mp::new(9_000))],
        )
        .unwrap();
        insert_text(&mut tx, story, 0, "hi\r\n\u{7}there").unwrap();
        let _ = tx.commit("New Text");
        let st = text(&doc, story);
        assert_eq!(st.text, "hi\nthere\n");
        assert_eq!(st.lines.len(), 2);
        assert!(sizes(&st).iter().all(|&(_, s)| s == 9_000));
        assert!(crate::validate::validate_document(&doc).errors.is_empty());
    }
}
