//! `DOWN`/`UP` tree reconstruction and the unknown-tag policy applied to it.
//!
//! The file is a pre-order walk of the document tree. An object record
//! becomes the next sibling at the current level, `TAG_DOWN` descends and
//! `TAG_UP` ascends. Attributes are children of the object they apply to,
//! which is why the tree is worth rebuilding at all rather than streaming
//! records straight at a consumer.
//!
//! The corpus is perfectly balanced — 201 121 of each — but a truncated file
//! will not be, so an unmatched `UP` is ignored and an unclosed `DOWN` is
//! closed at end of input, each with an
//! [`UnbalancedScope`](crate::DiagCode::UnbalancedScope) diagnostic.
//!
//! Nesting is capped at [`ReaderLimits::max_tree_depth`]. That is not
//! arbitrary caution: a tree of `Vec<RecordNode>` is dropped recursively, so
//! a file with a million `DOWN`s would overflow the stack in `Drop` — after
//! parsing had already succeeded, which is the worst possible place for it.

use std::collections::BTreeMap;

use crate::diag::{DiagCode, DiagSink, Diagnostic};
use crate::error::XarError;
use crate::reader::{BlockReport, ReaderLimits, Record, RecordReader};
use crate::tags::{TAG_ATOMICTAGS, TAG_DOWN, TAG_ESSENTIALTAGS, TAG_UP, TagPolicy, UnknownAction};

/// One node of the record tree.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RecordNode {
    /// The record that created the node.
    pub record: Record,
    /// The records that sat inside its `DOWN`/`UP` scope.
    pub children: Vec<RecordNode>,
}

impl RecordNode {
    /// The node's tag.
    #[must_use]
    pub const fn tag(&self) -> u32 {
        self.record.tag
    }

    /// Visits this node and its descendants in pre-order.
    pub fn walk(&self, f: &mut impl FnMut(&RecordNode, usize)) {
        self.walk_at(f, 0);
    }

    fn walk_at(&self, f: &mut impl FnMut(&RecordNode, usize), depth: usize) {
        f(self, depth);
        for c in &self.children {
            c.walk_at(f, depth.saturating_add(1));
        }
    }
}

/// The rebuilt record tree, plus the facts about how it was built.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct RecordTree {
    /// Top-level nodes, in file order.
    pub roots: Vec<RecordNode>,
    /// How many `TAG_DOWN` records were seen.
    pub down_count: u32,
    /// How many `TAG_UP` records were seen.
    pub up_count: u32,
    /// The deepest nesting reached. Roots are depth 0.
    pub max_depth: usize,
    /// Every record read, by tag.
    pub histogram: BTreeMap<u32, u32>,
    /// Records that a registered decoder understands.
    pub handled: u32,
    /// Records skipped because nothing understands their tag.
    pub skipped: u32,
    /// Records dropped as part of an atomic subtree.
    pub stripped: u32,
    /// How many nodes are in the tree.
    pub nodes: usize,
}

impl RecordTree {
    /// How many distinct tags occurred.
    #[must_use]
    pub fn distinct_tags(&self) -> usize {
        self.histogram.len()
    }

    /// Visits every node in pre-order.
    pub fn walk(&self, f: &mut impl FnMut(&RecordNode, usize)) {
        for r in &self.roots {
            r.walk(f);
        }
    }
}

/// Everything the physical and structural layers learned about one file.
#[derive(Clone, Debug)]
pub struct FileAnalysis {
    /// The parsed header.
    pub header: crate::FileHeader,
    /// The record tree.
    pub tree: RecordTree,
    /// One entry per compressed block.
    pub blocks: Vec<BlockReport>,
    /// Records handed out by the physical layer, including those later
    /// stripped.
    pub records_read: u32,
    /// Physical bytes after `TAG_ENDOFFILE`.
    pub trailing_bytes: u64,
    /// The atomic and essential lists the file declared.
    pub policy: TagPolicy,
    /// Everything recoverable that was found.
    pub diagnostics: DiagSink,
}

impl FileAnalysis {
    /// Whether every compressed block verified.
    #[must_use]
    pub fn blocks_ok(&self) -> bool {
        self.blocks.iter().all(|b| b.ok)
    }

    /// Tags with no decoder whose [`TagClass`](crate::TagClass) is
    /// `Structural`, or that the file declared essential.
    ///
    /// This is the number the phase's acceptance criteria count: an
    /// unhandled attribute loses a colour, but an unhandled structural tag
    /// means the tree is not the tree the file describes.
    #[must_use]
    pub fn unknown_structural_tags(&self) -> Vec<u32> {
        let mut out: Vec<u32> = self
            .tree
            .histogram
            .keys()
            .copied()
            .filter(|&t| {
                !crate::decode::has_decoder(t)
                    && (crate::tags::class_of(t) == Some(crate::TagClass::Structural)
                        || self.policy.is_essential(t))
            })
            .collect();
        out.sort_unstable();
        out
    }
}

/// Reads a whole file: physical layer, tree, tag policy.
///
/// # Errors
///
/// [`XarError`] for anything that stops the byte stream being walked,
/// including [`XarError::EssentialTag`] when the file declares a tag
/// essential that no decoder understands.
pub fn analyse(bytes: &[u8], limits: ReaderLimits) -> Result<FileAnalysis, XarError> {
    let reader = RecordReader::new(bytes, limits)?;
    build(reader, limits)
}

/// Builds the tree from an already-opened reader.
///
/// # Errors
///
/// As [`analyse`].
pub fn build_record_tree(
    reader: RecordReader<'_>,
    limits: ReaderLimits,
) -> Result<(RecordTree, DiagSink), XarError> {
    let a = build(reader, limits)?;
    Ok((a.tree, a.diagnostics))
}

/// Where the stripping state machine is.
enum Strip {
    /// Nothing is being stripped.
    No,
    /// An atomic record was just dropped; if the next record is a `DOWN`,
    /// its whole scope goes too. If it is anything else, the atomic node had
    /// no children and stripping is over.
    Pending,
    /// Inside a stripped scope, with this many `DOWN`s still open.
    Active(u32),
}

fn build(mut reader: RecordReader<'_>, limits: ReaderLimits) -> Result<FileAnalysis, XarError> {
    let mut tree = RecordTree::default();
    let mut policy = TagPolicy::new();
    let mut stack: Vec<Vec<RecordNode>> = Vec::new();
    let mut cur: Vec<RecordNode> = Vec::new();
    let mut over_depth: u32 = 0;
    let mut strip = Strip::No;
    let mut diags = DiagSink::new();

    while let Some(rec) = reader.next_record() {
        let rec = rec?;
        let tag = rec.tag;
        let slot = tree.histogram.entry(tag).or_insert(0);
        *slot = slot.saturating_add(1);

        match strip {
            Strip::Pending => {
                if tag == TAG_DOWN {
                    tree.stripped = tree.stripped.saturating_add(1);
                    tree.down_count = tree.down_count.saturating_add(1);
                    strip = Strip::Active(1);
                    continue;
                }
                strip = Strip::No;
            }
            Strip::Active(open) => {
                tree.stripped = tree.stripped.saturating_add(1);
                if tag == TAG_DOWN {
                    tree.down_count = tree.down_count.saturating_add(1);
                    strip = Strip::Active(open.saturating_add(1));
                } else if tag == TAG_UP {
                    tree.up_count = tree.up_count.saturating_add(1);
                    let left = open.saturating_sub(1);
                    strip = if left == 0 {
                        Strip::No
                    } else {
                        Strip::Active(left)
                    };
                }
                continue;
            }
            Strip::No => {}
        }

        match tag {
            TAG_DOWN => {
                tree.down_count = tree.down_count.saturating_add(1);
                tree.handled = tree.handled.saturating_add(1);
                if stack.len() >= limits.max_tree_depth {
                    if over_depth == 0 {
                        diags.push(
                            Diagnostic::new(DiagCode::DepthLimit)
                                .at(rec.number, tag)
                                .with_detail(stack.len() as u64),
                        );
                    }
                    over_depth = over_depth.saturating_add(1);
                } else {
                    stack.push(core::mem::take(&mut cur));
                    tree.max_depth = tree.max_depth.max(stack.len());
                }
            }
            TAG_UP => {
                tree.up_count = tree.up_count.saturating_add(1);
                tree.handled = tree.handled.saturating_add(1);
                if over_depth > 0 {
                    over_depth = over_depth.saturating_sub(1);
                } else if let Some(parent_level) = stack.pop() {
                    let mut finished = core::mem::replace(&mut cur, parent_level);
                    // Appended, not assigned: a node descended into twice
                    // (`N DOWN a UP DOWN b UP`) keeps both groups of
                    // children, since each `DOWN` makes the records after
                    // it children of the last node inserted.
                    match cur.last_mut() {
                        Some(parent) => parent.children.append(&mut finished),
                        None => cur.extend(finished),
                    }
                } else {
                    diags.push(
                        Diagnostic::new(DiagCode::UnbalancedScope)
                            .at(rec.number, tag)
                            .with_detail(1),
                    );
                }
            }
            _ => {
                if tag == TAG_ATOMICTAGS {
                    policy.absorb_atomic(&rec.data);
                } else if tag == TAG_ESSENTIALTAGS {
                    policy.absorb_essential(&rec.data);
                }
                if crate::decode::has_decoder(tag) {
                    tree.handled = tree.handled.saturating_add(1);
                } else {
                    match policy.unknown_action(tag) {
                        UnknownAction::Abort => {
                            diags.push(
                                Diagnostic::new(DiagCode::EssentialTagMissing).at(rec.number, tag),
                            );
                            return Err(XarError::EssentialTag(tag));
                        }
                        UnknownAction::StripSubtree => {
                            diags.push(
                                Diagnostic::new(DiagCode::AtomicSubtreeDropped).at(rec.number, tag),
                            );
                            tree.stripped = tree.stripped.saturating_add(1);
                            strip = Strip::Pending;
                            continue;
                        }
                        UnknownAction::Skip => {
                            tree.skipped = tree.skipped.saturating_add(1);
                            diags.push(
                                Diagnostic::new(DiagCode::UnknownTag)
                                    .at(rec.number, tag)
                                    .with_detail(rec.data.len() as u64),
                            );
                        }
                    }
                }
                tree.nodes = tree.nodes.saturating_add(1);
                cur.push(RecordNode {
                    record: rec,
                    children: Vec::new(),
                });
            }
        }
    }

    if !stack.is_empty() {
        diags.push(Diagnostic::new(DiagCode::UnbalancedScope).with_detail(stack.len() as u64));
    }
    while let Some(parent_level) = stack.pop() {
        let mut finished = core::mem::replace(&mut cur, parent_level);
        match cur.last_mut() {
            Some(parent) => parent.children.append(&mut finished),
            None => cur.extend(finished),
        }
    }
    tree.roots = cur;

    let (header, reader_diags, blocks, records_read, trailing_bytes) = reader.into_parts();
    let mut all = reader_diags;
    all.absorb(&diags);
    Ok(FileAnalysis {
        header,
        tree,
        blocks,
        records_read,
        trailing_bytes,
        policy,
        diagnostics: all,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::XarBuilder;

    fn analyse_bytes(bytes: &[u8]) -> FileAnalysis {
        analyse(bytes, ReaderLimits::default()).unwrap()
    }

    #[test]
    fn a_balanced_tree_nests() {
        let bytes = XarBuilder::new()
            .record(40, &[])
            .down()
            .record(41, &[])
            .down()
            .record(104, &[])
            .up()
            .up()
            .end_of_file()
            .finish();
        let a = analyse_bytes(&bytes);
        assert_eq!(a.tree.roots.len(), 3); // header, document, end of file
        assert_eq!(a.tree.roots[1].tag(), 40);
        assert_eq!(a.tree.roots[1].children[0].tag(), 41);
        assert_eq!(a.tree.roots[1].children[0].children[0].tag(), 104);
        assert_eq!(a.tree.max_depth, 2);
        assert_eq!(a.tree.down_count, a.tree.up_count);
    }

    #[test]
    fn an_unmatched_up_is_ignored() {
        let bytes = XarBuilder::new()
            .record(40, &[])
            .up()
            .record(41, &[])
            .end_of_file()
            .finish();
        let a = analyse_bytes(&bytes);
        assert_eq!(a.tree.roots.len(), 4);
        assert!(
            a.diagnostics
                .items()
                .iter()
                .any(|d| d.code == DiagCode::UnbalancedScope)
        );
    }

    #[test]
    fn an_unclosed_down_is_closed_at_end_of_input() {
        let bytes = XarBuilder::new()
            .record(40, &[])
            .down()
            .record(41, &[])
            .end_of_file()
            .finish();
        let a = analyse_bytes(&bytes);
        assert_eq!(a.tree.roots.len(), 2); // header, document
        assert_eq!(a.tree.roots[1].children.len(), 2); // 41 and ENDOFFILE
        assert!(
            a.diagnostics
                .items()
                .iter()
                .any(|d| d.code == DiagCode::UnbalancedScope)
        );
    }

    #[test]
    fn an_unknown_atomic_tag_takes_its_subtree_with_it() {
        let bytes = XarBuilder::new()
            .record(10, &9001u32.to_le_bytes())
            .record(40, &[])
            .down()
            .record(9001, &[])
            .down()
            .record(104, &[])
            .record(104, &[])
            .up()
            .record(43, &[])
            .up()
            .end_of_file()
            .finish();
        let a = analyse_bytes(&bytes);
        let doc = &a.tree.roots[2];
        assert_eq!(doc.tag(), 40);
        // Only the layer survives: the atomic node and its two children went.
        assert_eq!(doc.children.len(), 1);
        assert_eq!(doc.children[0].tag(), 43);
        assert_eq!(a.tree.stripped, 5);
        assert!(
            a.diagnostics
                .items()
                .iter()
                .any(|d| d.code == DiagCode::AtomicSubtreeDropped)
        );
    }

    #[test]
    fn an_atomic_tag_with_no_subtree_strips_only_itself() {
        let bytes = XarBuilder::new()
            .record(10, &9001u32.to_le_bytes())
            .record(9001, &[])
            .record(43, &[])
            .end_of_file()
            .finish();
        let a = analyse_bytes(&bytes);
        let tags: Vec<u32> = a.tree.roots.iter().map(RecordNode::tag).collect();
        assert_eq!(tags, vec![2, 10, 43, 3]);
    }

    #[test]
    fn an_unknown_essential_tag_aborts() {
        let bytes = XarBuilder::new()
            .record(11, &9002u32.to_le_bytes())
            .record(9002, &[])
            .end_of_file()
            .finish();
        assert_eq!(
            analyse(&bytes, ReaderLimits::default()).unwrap_err(),
            XarError::EssentialTag(9002)
        );
    }

    #[test]
    fn deep_nesting_is_capped_rather_than_overflowing_the_stack() {
        let limits = ReaderLimits {
            max_tree_depth: 8,
            ..ReaderLimits::default()
        };
        let mut b = XarBuilder::new();
        for _ in 0..2_000 {
            b = b.record(104, &[]).down();
        }
        for _ in 0..2_000 {
            b = b.up();
        }
        let a = analyse(&b.end_of_file().finish(), limits).unwrap();
        assert_eq!(a.tree.max_depth, 8);
        assert_eq!(a.tree.down_count, a.tree.up_count);
    }

    #[test]
    fn the_histogram_counts_every_record() {
        let bytes = XarBuilder::new()
            .record(104, &[])
            .record(104, &[])
            .down()
            .up()
            .end_of_file()
            .finish();
        let a = analyse_bytes(&bytes);
        assert_eq!(a.tree.histogram.get(&104), Some(&2));
        assert_eq!(a.tree.histogram.get(&2), Some(&1));
        assert_eq!(a.records_read, 6);
    }
}
