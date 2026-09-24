//! Text edit commands (phase 9, T9.2.4): typing into a story, deleting
//! from it and setting text attributes on it, through the transaction like
//! every other edit.
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
//! * **A character attribute** ([`SetTextAttr`]: typeface, size, bold,
//!   tracking, features…) becomes an attribute child of every item in the
//!   range, replacing the item's own child of that slot. An item's own
//!   children apply to it alone, so nothing outside the range changes, and
//!   text typed after it copies them (insertion above).
//! * **A paragraph attribute** (justification, line spacing, margins,
//!   first-line indent, ruler) is read from a line's first item, and a
//!   paragraph from its first line; it is set on **every line of every
//!   paragraph the range touches**, as a line-level attribute right before
//!   the line's first item (any other of that slot there goes), so the
//!   lines of a paragraph keep agreeing.
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

use crate::attr::{AttrNode, AttrSlot, AttrValue};
use crate::history::{CoalesceKey, Command, EditError, Tx};
use crate::kind::NodeKind;
use crate::text::{TextItem, TextLineNode, TextStoryNode};
use crate::text_model::StoryText;
use crate::tree::{Attach, NodeId};

/// Sets one text attribute on a byte range of a story (T9.2.4): a
/// character attribute on the characters in the range, a paragraph
/// attribute ([`is_paragraph_slot`]) on the paragraphs it touches (the
/// caret's paragraph for an empty range).
#[derive(Clone, Debug, PartialEq)]
pub struct SetTextAttr {
    /// The `TextStory` node.
    pub story: NodeId,
    /// Byte range in the story's logical text.
    pub range: Range<usize>,
    /// The value; its slot must be a text slot.
    pub value: AttrValue,
    /// The key it merges on (the typing burst it styles).
    pub coalesce: Option<CoalesceKey>,
}

impl Command for SetTextAttr {
    fn label(&self) -> &'static str {
        self.value.slot().map_or("Text Attribute", text_attr_label)
    }

    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        set_text_attr(tx, self.story, self.range.clone(), &self.value)
    }

    fn coalesce_key(&self) -> Option<CoalesceKey> {
        self.coalesce
    }
}

/// Whether a text slot is read per paragraph (from the first item of a
/// paragraph's first line) rather than per character.
#[must_use]
pub fn is_paragraph_slot(slot: AttrSlot) -> bool {
    matches!(
        slot,
        AttrSlot::TxtJustification
            | AttrSlot::TxtLineSpace
            | AttrSlot::TxtLeftMargin
            | AttrSlot::TxtRightMargin
            | AttrSlot::TxtFirstIndent
            | AttrSlot::TxtRuler
    )
}

/// The Edit menu's name for setting a text attribute of `slot`.
#[must_use]
pub fn text_attr_label(slot: AttrSlot) -> &'static str {
    match slot {
        AttrSlot::TxtFontTypeface => "Font",
        AttrSlot::TxtBold => "Bold",
        AttrSlot::TxtItalic => "Italic",
        AttrSlot::TxtUnderline => "Underline",
        AttrSlot::TxtFontSize => "Font Size",
        AttrSlot::TxtAspectRatio => "Aspect Ratio",
        AttrSlot::TxtTracking => "Tracking",
        AttrSlot::TxtScript => "Superscript/Subscript",
        AttrSlot::TxtBaseline => "Baseline Shift",
        AttrSlot::TxtJustification => "Justification",
        AttrSlot::TxtLineSpace => "Line Spacing",
        AttrSlot::TxtLeftMargin => "Left Margin",
        AttrSlot::TxtRightMargin => "Right Margin",
        AttrSlot::TxtFirstIndent => "First-Line Indent",
        AttrSlot::TxtRuler => "Tab Stops",
        AttrSlot::TxtFeatures => "OpenType Features",
        _ => "Text Attribute",
    }
}

/// Sets `value` on the byte range `range` of `story` (see
/// [`SetTextAttr`]). An empty range sets a paragraph attribute on the
/// paragraph holding it and a character attribute nowhere.
///
/// # Errors
///
/// [`EditError::WrongKind`] when `story` is not a story,
/// [`EditError::TextEdit`] when `value` is not a text attribute or the
/// range is reversed, past the text or cuts a character, and whatever the
/// transaction refuses (a locked story).
pub fn set_text_attr(
    tx: &mut Tx<'_>,
    story: NodeId,
    range: Range<usize>,
    value: &AttrValue,
) -> Result<(), EditError> {
    let Some(slot) = value
        .slot()
        .filter(|s| crate::text_convert::is_text_slot(*s))
    else {
        return Err(EditError::TextEdit("not a text attribute"));
    };
    let st = collect(tx, story)?;
    if range.start > range.end
        || range.end > st.text.len()
        || !st.text.is_char_boundary(range.start)
        || !st.text.is_char_boundary(range.end)
    {
        return Err(EditError::TextEdit("range outside the story's text"));
    }
    if is_paragraph_slot(slot) {
        for line in paragraph_lines(&st, &range) {
            set_line_attr(tx, line, slot, value)?;
        }
        return Ok(());
    }
    let items: Vec<NodeId> = st
        .items
        .iter()
        .filter(|e| e.len > 0 && e.byte as usize >= range.start && (e.byte as usize) < range.end)
        .map(|e| e.node)
        .collect();
    for item in items {
        set_own_attr(tx, item, slot, value)?;
    }
    Ok(())
}

/// The line index holding byte `b`: the last line starting at or before
/// it.
fn line_at(st: &StoryText, b: usize) -> usize {
    st.lines
        .partition_point(|l| l.first_byte as usize <= b)
        .saturating_sub(1)
}

/// The lines (indices into [`StoryText::lines`]) of every paragraph the
/// byte range `range` touches: the caret's paragraph for an empty range.
/// Empty for a story with no lines.
#[must_use]
pub fn paragraph_line_range(st: &StoryText, range: &Range<usize>) -> Range<usize> {
    if st.lines.is_empty() {
        return 0..0;
    }
    let first = line_at(st, range.start);
    let last = if range.end > range.start {
        line_at(st, range.end - 1)
    } else {
        first
    };
    // Back to the first line of the first paragraph, on to the last line
    // of the last one.
    let mut a = first;
    while a > 0 && !st.lines[a - 1].ends_paragraph {
        a -= 1;
    }
    let mut b = last.max(a);
    while b + 1 < st.lines.len() && !st.lines[b].ends_paragraph {
        b += 1;
    }
    a..b + 1
}

/// Every line of every paragraph `range` touches.
fn paragraph_lines(st: &StoryText, range: &Range<usize>) -> Vec<NodeId> {
    st.lines[paragraph_line_range(st, range)]
        .iter()
        .map(|l| l.node)
        .collect()
}

fn attr_of(tx: &Tx<'_>, node: NodeId, slot: AttrSlot) -> bool {
    matches!(tx.doc().tree.kind(node), Some(NodeKind::Attr(a)) if a.value.slot() == Some(slot))
}

/// Gives `item` its own attribute child `value`, replacing any it has of
/// that slot.
fn set_own_attr(
    tx: &mut Tx<'_>,
    item: NodeId,
    slot: AttrSlot,
    value: &AttrValue,
) -> Result<(), EditError> {
    let own: Vec<NodeId> = tx
        .doc()
        .tree
        .children(item)
        .filter(|&c| attr_of(tx, c, slot))
        .collect();
    match own.split_last() {
        Some((&last, rest)) => {
            for &r in rest {
                tx.delete(r)?;
            }
            if !matches!(tx.doc().tree.kind(last), Some(NodeKind::Attr(a)) if a.value == *value) {
                tx.set_attr(last, value.clone())?;
            }
        }
        None => {
            let attr = tx.create(NodeKind::Attr(Box::new(AttrNode::new(value.clone()))))?;
            tx.attach(attr, item, Attach::LastChild)?;
        }
    }
    Ok(())
}

/// Sets a paragraph attribute on `line`: one attribute of the slot right
/// before its first item, none of that slot before it or on the first
/// item itself. A line with no items reads its attributes from outside
/// itself and is left alone.
fn set_line_attr(
    tx: &mut Tx<'_>,
    line: NodeId,
    slot: AttrSlot,
    value: &AttrValue,
) -> Result<(), EditError> {
    let tree = &tx.doc().tree;
    let kids: Vec<NodeId> = tree.children(line).collect();
    let Some(first) = kids
        .iter()
        .position(|&k| matches!(tree.kind(k), Some(NodeKind::TextItem(_))))
    else {
        return Ok(());
    };
    let first_item = kids[first];
    let before: Vec<NodeId> = kids[..first]
        .iter()
        .copied()
        .filter(|&k| attr_of(tx, k, slot))
        .collect();
    let on_item: Vec<NodeId> = tx
        .doc()
        .tree
        .children(first_item)
        .filter(|&c| attr_of(tx, c, slot))
        .collect();
    for n in on_item {
        tx.delete(n)?;
    }
    match before.split_last() {
        Some((&last, rest)) => {
            for &r in rest {
                tx.delete(r)?;
            }
            if !matches!(tx.doc().tree.kind(last), Some(NodeKind::Attr(a)) if a.value == *value) {
                tx.set_attr(last, value.clone())?;
            }
        }
        None => {
            let attr = tx.create(NodeKind::Attr(Box::new(AttrNode::new(value.clone()))))?;
            tx.attach(attr, first_item, Attach::Prev)?;
        }
    }
    Ok(())
}

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

/// Whether a story holds no text: nothing but the final paragraph break
/// every story ends with. `None` when `story` is not a story.
#[must_use]
pub fn is_story_empty(doc: &crate::Document, story: NodeId) -> Option<bool> {
    let st = StoryText::collect_simple(&doc.tree, &doc.defaults, story)?;
    Some(st.text.strip_suffix('\n').unwrap_or(&st.text).is_empty())
}

/// What [`remove_empty_story`] did.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EmptyStory {
    /// The story still holds text (or is not a story): nothing changed.
    Kept,
    /// The story was deleted.
    Removed {
        /// The plain path a story on a path left in its place, if any.
        path: Option<NodeId>,
    },
}

/// Deletes `story` when it holds no text ([`is_story_empty`]), inside the
/// caller's transaction, so the deletion that emptied it and the removal
/// are one undo step (the original merges its story removal into the
/// operation before it). A story on a path leaves the path it followed in
/// its place as an ordinary path, carrying as its own attributes the
/// non-text attributes it painted with (the story's included), so it looks
/// exactly as it did under the text; text attributes mean nothing to a
/// path and are not copied.
///
/// # Errors
///
/// Whatever the transaction refuses (a locked story).
pub fn remove_empty_story(tx: &mut Tx<'_>, story: NodeId) -> Result<EmptyStory, EditError> {
    use crate::attr::{ALL_ATTR_SLOTS, resolve_inherited, resolve_uncached};
    let doc = tx.doc();
    let (Some(NodeKind::TextStory(node)), Some(true)) =
        (doc.tree.kind(story), is_story_empty(doc, story))
    else {
        return Ok(EmptyStory::Kept);
    };
    let followed = if matches!(node.layout, crate::TextLayout::OnPath { .. }) {
        let inherited = resolve_inherited(&doc.tree, story, &doc.defaults);
        doc.tree
            .children(story)
            .find_map(|c| match doc.tree.kind(c) {
                Some(NodeKind::Path(p)) => {
                    let own = resolve_uncached(&doc.tree, c, &doc.defaults);
                    let attrs: Vec<AttrValue> = ALL_ATTR_SLOTS
                        .iter()
                        .filter(|s| !crate::text_convert::is_text_slot(**s))
                        .filter(|&&s| own.get(s) != inherited.get(s))
                        .map(|&s| own.get(s).clone())
                        .collect();
                    Some(((**p).clone(), attrs))
                }
                _ => None,
            })
    } else {
        None
    };
    let path = match followed {
        Some((node, attrs)) => {
            let path = tx.create(NodeKind::Path(Box::new(node)))?;
            tx.attach(path, story, Attach::Prev)?;
            for v in attrs {
                let a = tx.create(NodeKind::Attr(Box::new(AttrNode::new(v))))?;
                tx.attach(a, path, Attach::LastChild)?;
            }
            Some(path)
        }
        None => None,
    };
    tx.delete(story)?;
    Ok(EmptyStory::Removed { path })
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

    fn attr_at(st: &StoryText, at: usize, slot: AttrSlot) -> AttrValue {
        st.runs[st.run_at(at).unwrap()].attrs.get(slot).clone()
    }

    fn set(bus: &mut CommandBus, doc: &mut Document, story: NodeId, r: Range<usize>, v: AttrValue) {
        let cmd = SetTextAttr {
            story,
            range: r,
            value: v,
            coalesce: None,
        };
        bus.dispatch(doc, &cmd).unwrap();
    }

    #[test]
    fn a_character_attribute_styles_exactly_the_range_and_typing_after_it() {
        let (mut doc, story) = doc();
        let before = doc.canonical_digest();
        let mut bus = CommandBus::new();
        // "abcd\nef\n": bold on "bcd\ne" (across the paragraph break).
        set(&mut bus, &mut doc, story, 1..6, AttrValue::Bold(true));
        let st = text(&doc, story);
        let bold: Vec<bool> = (0..st.text.len())
            .map(|i| attr_at(&st, i, AttrSlot::TxtBold) == AttrValue::Bold(true))
            .collect();
        assert_eq!(bold, [false, true, true, true, true, true, false, false]);
        // Sizes are untouched.
        assert_eq!(
            sizes(&st)[..4],
            [('a', 20_000), ('b', 20_000), ('c', 12_000), ('d', 12_000)]
        );
        // Setting it again changes nothing more; unbolding one character.
        set(&mut bus, &mut doc, story, 1..6, AttrValue::Bold(true));
        set(&mut bus, &mut doc, story, 2..3, AttrValue::Bold(false));
        let st = text(&doc, story);
        assert_eq!(attr_at(&st, 2, AttrSlot::TxtBold), AttrValue::Bold(false));
        assert_eq!(attr_at(&st, 3, AttrSlot::TxtBold), AttrValue::Bold(true));
        // Typed after a bold character, text is bold.
        insert(&mut bus, &mut doc, story, 4, "Z");
        let st = text(&doc, story);
        assert_eq!(&st.text[4..5], "Z");
        assert_eq!(attr_at(&st, 4, AttrSlot::TxtBold), AttrValue::Bold(true));
        assert!(crate::validate::validate_document(&doc).errors.is_empty());
        for _ in 0..4 {
            bus.undo(&mut doc).unwrap();
        }
        assert_eq!(doc.canonical_digest(), before);
    }

    #[test]
    fn a_paragraph_attribute_goes_on_every_line_of_the_touched_paragraphs() {
        let (mut doc, story) = doc();
        let before = doc.canonical_digest();
        let mut bus = CommandBus::new();
        let just =
            |st: &StoryText, l: usize| st.lines[l].attrs.get(AttrSlot::TxtJustification).clone();
        let centre = AttrValue::Justification(crate::text::Justification::Centre);
        // A caret in the second paragraph.
        set(&mut bus, &mut doc, story, 6..6, centre.clone());
        let st = text(&doc, story);
        assert_ne!(just(&st, 0), centre);
        assert_eq!(just(&st, 1), centre);
        // Paragraph 1 run on into paragraph 2 (its break deleted): one
        // paragraph of two lines; a caret in its first line styles both.
        bus.dispatch(
            &mut doc,
            &DeleteRange {
                story,
                range: 4..5,
                coalesce: None,
            },
        )
        .unwrap();
        let right = AttrValue::Justification(crate::text::Justification::Right);
        set(&mut bus, &mut doc, story, 1..1, right.clone());
        let st = text(&doc, story);
        assert_eq!((just(&st, 0), just(&st, 1)), (right.clone(), right));
        // Characters are not given paragraph attributes.
        assert!(
            st.items
                .iter()
                .all(|e| doc.tree.children(e.node).next().is_none())
        );
        assert!(crate::validate::validate_document(&doc).errors.is_empty());
        for _ in 0..3 {
            bus.undo(&mut doc).unwrap();
        }
        assert_eq!(doc.canonical_digest(), before);
    }

    #[test]
    fn a_non_text_attribute_or_a_bad_range_is_refused() {
        let (mut doc, story) = doc();
        let before = doc.canonical_digest();
        let mut bus = CommandBus::new();
        for (range, value) in [
            (0..2, AttrValue::LineWidth(Mp::new(1_000))),
            (3..99, AttrValue::Bold(true)),
        ] {
            let cmd = SetTextAttr {
                story,
                range,
                value,
                coalesce: None,
            };
            assert!(matches!(
                bus.dispatch(&mut doc, &cmd),
                Err(EditError::TextEdit(_))
            ));
        }
        assert_eq!(doc.canonical_digest(), before);
        assert_eq!(
            SetTextAttr {
                story,
                range: 0..1,
                value: AttrValue::FontSize(Mp::new(9_000)),
                coalesce: None
            }
            .label(),
            "Font Size"
        );
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

    /// Deletes a range and removes the story if that emptied it, as the
    /// app's text commands do: one undo step.
    #[derive(Debug)]
    struct DeleteAndTidy(NodeId, Range<usize>);

    impl Command for DeleteAndTidy {
        fn label(&self) -> &'static str {
            "Delete Text"
        }

        fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
            delete_range(tx, self.0, self.1.clone())?;
            remove_empty_story(tx, self.0).map(|_| ())
        }
    }

    #[test]
    fn a_story_emptied_by_a_deletion_goes_in_the_same_step() {
        let (mut doc, story) = doc();
        let before = doc.canonical_digest();
        let mut bus = CommandBus::new();
        // Leaving any text, or only paragraph breaks and text, keeps it.
        bus.dispatch(&mut doc, &DeleteAndTidy(story, 0..4)).unwrap();
        assert_eq!(text(&doc, story).text, "\nef\n");
        assert!(doc.tree.is_reachable(story));
        assert_eq!(is_story_empty(&doc, story), Some(false));
        bus.dispatch(&mut doc, &DeleteAndTidy(story, 0..3)).unwrap();
        assert!(!doc.tree.is_reachable(story), "the empty story is gone");
        assert!(crate::validate::validate_document(&doc).errors.is_empty());
        bus.undo(&mut doc).unwrap();
        assert!(doc.tree.is_reachable(story));
        assert_eq!(text(&doc, story).text, "\nef\n", "one undo: text and story");
        bus.undo(&mut doc).unwrap();
        assert_eq!(doc.canonical_digest(), before);
        // Nothing to remove in a story that still has text.
        let mut tx = Tx::begin(&mut doc);
        assert_eq!(remove_empty_story(&mut tx, story), Ok(EmptyStory::Kept));
        drop(tx);
        assert_eq!(doc.canonical_digest(), before);
    }

    #[test]
    fn an_emptied_story_on_a_path_leaves_its_path_looking_the_same() {
        let mut pb = xarast_geom::Path::builder();
        pb.move_to(xarast_geom::Point::raw(0, 0))
            .line_to(xarast_geom::Point::raw(100_000, 0));
        let mut b = skeleton(BuildLimits::default()).unwrap();
        let story = b
            .node(NodeKind::TextStory(Box::new(TextStoryNode {
                layout: crate::TextLayout::OnPath {
                    reversed: false,
                    tangential: true,
                    left_indent: Mp::ZERO,
                    right_indent: Mp::ZERO,
                    chars: crate::CharsTransform::default(),
                },
                ..TextStoryNode::default()
            })))
            .unwrap()
            .node_id();
        b.push_scope().unwrap();
        // The story's line width paints its path; its size does not.
        b.attribute(AttrValue::LineWidth(Mp::new(3_000))).unwrap();
        b.attribute(AttrValue::FontSize(Mp::new(20_000))).unwrap();
        let path = b
            .node(NodeKind::Path(Box::new(crate::PathNode::new(pb.build()))))
            .unwrap()
            .node_id();
        b.push_scope().unwrap();
        b.attribute(AttrValue::WindingRule(xarast_geom::FillRule::EvenOdd))
            .unwrap();
        b.pop_scope();
        b.node(NodeKind::TextLine(Box::default())).unwrap();
        b.push_scope().unwrap();
        b.node(NodeKind::TextItem(TextItem::Char('a'))).unwrap();
        b.node(NodeKind::TextItem(TextItem::LineBreak(true)))
            .unwrap();
        b.pop_scope();
        b.pop_scope();
        let (mut doc, _) = b.finish().unwrap();
        let layer = doc.tree.links(story).parent.unwrap();
        let painted = crate::attr::resolve_uncached(&doc.tree, path, &doc.defaults);
        let before = doc.canonical_digest();
        let mut bus = CommandBus::new();
        bus.dispatch(&mut doc, &DeleteAndTidy(story, 0..1)).unwrap();
        assert!(!doc.tree.is_reachable(story));
        let kids: Vec<NodeId> = doc.tree.children(layer).collect();
        let freed = *kids
            .iter()
            .find(|&&n| matches!(doc.tree.kind(n), Some(NodeKind::Path(_))))
            .expect("the path stays on the layer");
        let now = crate::attr::resolve_uncached(&doc.tree, freed, &doc.defaults);
        for s in crate::attr::ALL_ATTR_SLOTS {
            if !crate::text_convert::is_text_slot(s) {
                assert_eq!(now.get(s), painted.get(s), "{s:?}");
            }
        }
        // Text attributes are not carried onto the path.
        assert!(doc.tree.children(freed).all(|c| match doc.tree.kind(c) {
            Some(NodeKind::Attr(a)) => a.value.slot().is_some_and(|s| !crate::is_text_slot(s)),
            _ => true,
        }));
        assert!(crate::validate::validate_document(&doc).errors.is_empty());
        bus.undo(&mut doc).unwrap();
        assert_eq!(doc.canonical_digest(), before);
    }
}
