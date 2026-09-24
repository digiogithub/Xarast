//! The text clipboard (phase 9, T9.4.8): copy, cut and paste of text while
//! a text caret is up.
//!
//! Copying a text selection keeps two things: the plain text, which goes on
//! the system clipboard, and a [`StyledText`] — the same text with the
//! character attributes of each run — which stays in the application. A
//! paste whose system clipboard text is exactly that plain text pastes the
//! styled copy; anything else is pasted as plain text, which takes the
//! style of the text it lands next to (as typing does).
//!
//! What "with attributes" carries: the character text attributes
//! ([`CHAR_SLOTS`]: font, bold, italic, underline, size, aspect, tracking,
//! script, baseline shift, OpenType features) and, within the document it
//! was copied from, the text's fill and line colour ([`PAINT_SLOTS`]; a
//! colour may name the source document's palette, so another document
//! gets the text without them). Paragraph attributes stay with the
//! paragraph the text is pasted into.
//!
//! Pasting styled text only writes an attribute where the pasted character
//! would otherwise differ, so pasting into text of the same style adds no
//! attribute nodes at all.

use std::ops::Range;
use std::sync::Arc;

use xarast_doc::{AttrSlot, AttrValue, Document, NodeId, StoryText};

/// The character text attributes a styled copy carries, in slot order.
pub const CHAR_SLOTS: [AttrSlot; 10] = [
    AttrSlot::TxtFontTypeface,
    AttrSlot::TxtBold,
    AttrSlot::TxtItalic,
    AttrSlot::TxtAspectRatio,
    AttrSlot::TxtTracking,
    AttrSlot::TxtUnderline,
    AttrSlot::TxtFontSize,
    AttrSlot::TxtScript,
    AttrSlot::TxtBaseline,
    AttrSlot::TxtFeatures,
];

/// The paint attributes a styled copy carries within its own document.
pub const PAINT_SLOTS: [AttrSlot; 2] = [AttrSlot::FillGeometry, AttrSlot::StrokeColour];

/// Every slot a styled run holds a value for: [`CHAR_SLOTS`], then
/// [`PAINT_SLOTS`].
fn carried_slots() -> impl Iterator<Item = AttrSlot> {
    CHAR_SLOTS.into_iter().chain(PAINT_SLOTS)
}

/// Text with the character attributes of each run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyledText {
    /// The text: `'\n'` is a paragraph break, `'\t'` a tab.
    pub text: String,
    /// Byte ranges of `text` and the values in force there: one per
    /// [`CHAR_SLOTS`] entry, then (when the copy still carries paint) one
    /// per [`PAINT_SLOTS`] entry. Empty for plain text, which takes the
    /// style where it is pasted.
    pub runs: Vec<(Range<usize>, Vec<AttrValue>)>,
}

impl StyledText {
    /// Plain text from another application: line ends become paragraph
    /// breaks, other control characters but tab are dropped (as typing
    /// does).
    #[must_use]
    pub fn plain(text: &str) -> StyledText {
        StyledText {
            text: typed(text),
            runs: Vec::new(),
        }
    }

    /// Whether there is no text.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The same text without its colours: what another document gets.
    #[must_use]
    pub fn without_paint(&self) -> StyledText {
        StyledText {
            text: self.text.clone(),
            runs: self
                .runs
                .iter()
                .map(|(r, v)| {
                    (
                        r.clone(),
                        v.iter().take(CHAR_SLOTS.len()).cloned().collect(),
                    )
                })
                .collect(),
        }
    }
}

/// A clipboard operation on the text being edited.
#[derive(Clone, Debug, PartialEq)]
pub enum TextClipOp {
    /// Delete the selected text (its copy has already been taken).
    Cut,
    /// Replace the selection (or insert at the caret) with this text.
    Paste(Arc<StyledText>),
}

/// What typing or pasting inserts: `'\r'` (alone or before `'\n'`) becomes
/// a paragraph break, other control characters except tab are dropped.
#[must_use]
pub fn typed(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .filter(|&c| c == '\n' || c == '\t' || !c.is_control())
        .collect()
}

/// The story's text with the attributes in force above it applied, as the
/// walker and the text tool see it.
#[must_use]
pub fn story_text(doc: &Document, story: NodeId) -> Option<StoryText> {
    let mut stack = xarast_doc::attr::resolve_inherited(&doc.tree, story, &doc.defaults);
    StoryText::collect(&doc.tree, story, &mut stack, &mut |_, a| {
        Arc::new(a.value.clone())
    })
}

/// The styled copy of `range` of a story's text. The range is clamped to
/// the text and to character boundaries.
#[must_use]
pub fn copy_range(st: &StoryText, range: Range<usize>) -> StyledText {
    let floor = |i: usize| {
        let mut i = i.min(st.text.len());
        while !st.text.is_char_boundary(i) {
            i -= 1;
        }
        i
    };
    let (start, end) = (floor(range.start), floor(range.end));
    if start >= end {
        return StyledText::default();
    }
    let mut runs: Vec<(Range<usize>, Vec<AttrValue>)> = Vec::new();
    for run in &st.runs {
        let (a, b) = (run.range.start.max(start), run.range.end.min(end));
        if a >= b {
            continue;
        }
        let values: Vec<AttrValue> = carried_slots().map(|s| run.attrs.get(s).clone()).collect();
        match runs.last_mut() {
            // Runs that differ only in slots the copy does not carry merge.
            Some((r, v)) if *v == values && r.end == a - start => r.end = b - start,
            _ => runs.push((a - start..b - start, values)),
        }
    }
    StyledText {
        text: st.text[start..end].to_owned(),
        runs,
    }
}

/// The attribute edits that give text pasted at byte `at` of `st` (the
/// story *after* the insertion) the style of `clip`: for each run of the
/// clip and each carried slot, the parts of the pasted range where the
/// story's value differs, merged into ranges.
#[must_use]
pub fn paste_edits(st: &StoryText, at: usize, clip: &StyledText) -> Vec<(Range<usize>, AttrValue)> {
    let mut edits: Vec<(Range<usize>, AttrValue)> = Vec::new();
    for (range, values) in &clip.runs {
        let target = at + range.start..at + range.end;
        for (slot, value) in carried_slots().zip(values) {
            for run in &st.runs {
                let (a, b) = (
                    run.range.start.max(target.start),
                    run.range.end.min(target.end),
                );
                if a >= b || run.attrs.get(slot) == value {
                    continue;
                }
                match edits.last_mut() {
                    Some((r, v)) if v == value && r.end == a => r.end = b,
                    _ => edits.push((a..b, value.clone())),
                }
            }
        }
    }
    edits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_normalised_as_typing_is() {
        let p = StyledText::plain("a\r\nb\rc\u{7}\td");
        assert_eq!(p.text, "a\nb\nc\td");
        assert!(p.runs.is_empty());
    }
}
