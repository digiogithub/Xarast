//! Convert a text story to shapes (phase 9, T9.6.3 and T9.6.4): the
//! document half.
//!
//! Laying a story out needs fonts, which this crate does not have; the
//! caller (`xarast-app`) lays the story out and hands over one outline per
//! attribute run, in document space, with the attributes that run paints
//! with. This module turns that into the tree edit, inside the caller's
//! transaction:
//!
//! * a [`GroupNode`] takes the story's place (same parent, same z-order)
//!   and keeps the story's logical text in
//!   [`GroupNode::source_text`], for accessibility and search;
//! * one filled, stroked [`PathNode`] per run goes inside it, carrying as
//!   its own attribute children exactly the non-text attributes whose
//!   value differs from what the group inherits, so each path paints as
//!   its run did (the story's own attributes and the run's character
//!   attributes are gone with the story);
//! * glyphs fill by the **non-zero** rule whatever the winding attribute
//!   says (that is how the walker draws text), so a path whose inherited
//!   rule is even-odd gets an explicit non-zero one;
//! * the story's own attributes that have no slot (an object name, say)
//!   move to the group;
//! * a story on a path keeps the path it followed, as the group's first
//!   child (under the text), with the attributes it painted with: the
//!   curve stays editable and, when it had a colour, visible;
//! * the story is deleted, retained by the undo step.
//!
//! Text attributes (typeface, size, tracking, …) mean nothing to a path
//! and are not copied.

use std::sync::Arc;

use xarast_geom::{FillRule, Path};

use crate::attr::{ALL_ATTR_SLOTS, AttrNode, AttrSlot, AttrValue, ResolvedAttrs};
use crate::attr::{resolve_inherited, resolve_uncached};
use crate::history::{EditError, Tx};
use crate::kind::{GroupNode, NodeKind, PathNode};
use crate::tree::{Attach, NodeId};

/// One attribute run of a laid-out story: its glyph outlines (and
/// underline) in document space and the attributes it paints with.
#[derive(Clone, Debug)]
pub struct OutlineRun {
    /// The outlines, document space.
    pub path: Arc<Path>,
    /// The attributes in force for the run, as the walker paints it.
    pub attrs: ResolvedAttrs,
}

/// Whether a slot is a text attribute, meaningless on a path.
#[must_use]
pub fn is_text_slot(slot: AttrSlot) -> bool {
    matches!(
        slot,
        AttrSlot::TxtFontTypeface
            | AttrSlot::TxtBold
            | AttrSlot::TxtItalic
            | AttrSlot::TxtAspectRatio
            | AttrSlot::TxtJustification
            | AttrSlot::TxtTracking
            | AttrSlot::TxtUnderline
            | AttrSlot::TxtFontSize
            | AttrSlot::TxtScript
            | AttrSlot::TxtBaseline
            | AttrSlot::TxtLineSpace
            | AttrSlot::TxtLeftMargin
            | AttrSlot::TxtRightMargin
            | AttrSlot::TxtFirstIndent
            | AttrSlot::TxtRuler
    )
}

/// Replaces `story` with a group of one path per run (see the module
/// documentation). Returns the group.
///
/// # Errors
///
/// [`EditError::WrongKind`] when `story` is not a text story or `runs` is
/// empty (a story with no ink is left alone rather than replaced by an
/// empty group); whatever the transaction refuses.
pub fn convert_story_to_shapes(
    tx: &mut Tx<'_>,
    story: NodeId,
    runs: &[OutlineRun],
    source: &str,
) -> Result<NodeId, EditError> {
    if !matches!(tx.doc().tree.kind(story), Some(NodeKind::TextStory(_))) || runs.is_empty() {
        return Err(EditError::WrongKind(story));
    }
    let doc = tx.doc();
    let inherited = resolve_inherited(&doc.tree, story, &doc.defaults);
    // What each path must say for itself, computed before the tree changes.
    let per_run: Vec<Vec<AttrValue>> = runs
        .iter()
        .map(|r| {
            ALL_ATTR_SLOTS
                .iter()
                .filter(|s| !is_text_slot(**s))
                .filter_map(|&s| {
                    let want = match s {
                        AttrSlot::WindingRule => &AttrValue::WindingRule(FillRule::NonZero),
                        _ => r.attrs.get(s),
                    };
                    (want != inherited.get(s)).then(|| want.clone())
                })
                .collect()
        })
        .collect();
    let slotless: Vec<AttrValue> = doc
        .tree
        .children(story)
        .filter_map(|c| match doc.tree.kind(c) {
            Some(NodeKind::Attr(a)) if a.value.slot().is_none() => Some(a.value.clone()),
            _ => None,
        })
        .collect();

    // The path a story on a path follows: its first path child, painted
    // with the story's attributes before it and its own.
    let on_path = matches!(
        doc.tree.kind(story),
        Some(NodeKind::TextStory(s)) if matches!(s.layout, crate::TextLayout::OnPath { .. })
    );
    let followed: Option<(PathNode, Vec<AttrValue>)> = if on_path {
        doc.tree
            .children(story)
            .find_map(|c| match doc.tree.kind(c) {
                Some(NodeKind::Path(p)) => {
                    let own = resolve_uncached(&doc.tree, c, &doc.defaults);
                    let attrs = ALL_ATTR_SLOTS
                        .iter()
                        .filter(|s| !is_text_slot(**s))
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

    let group = tx.create(NodeKind::Group(Box::new(GroupNode {
        source_text: Some(Arc::from(source)),
        ..GroupNode::default()
    })))?;
    tx.attach(group, story, Attach::Prev)?;
    for v in slotless {
        let a = tx.create(NodeKind::Attr(Box::new(AttrNode::new(v))))?;
        tx.attach(a, group, Attach::LastChild)?;
    }
    if let Some((node, attrs)) = followed {
        let path = tx.create(NodeKind::Path(Box::new(node)))?;
        tx.attach(path, group, Attach::LastChild)?;
        for v in attrs {
            let a = tx.create(NodeKind::Attr(Box::new(AttrNode::new(v))))?;
            tx.attach(a, path, Attach::LastChild)?;
        }
    }
    for (run, attrs) in runs.iter().zip(per_run) {
        let path = tx.create(NodeKind::Path(Box::new(PathNode {
            data: Arc::clone(&run.path),
            filled: true,
            stroked: true,
        })))?;
        tx.attach(path, group, Attach::LastChild)?;
        for v in attrs {
            let a = tx.create(NodeKind::Attr(Box::new(AttrNode::new(v))))?;
            tx.attach(a, path, Attach::LastChild)?;
        }
    }
    tx.delete(story)?;
    Ok(group)
}
