//! The editing commands tools emit: the phase-7 `Command` enum.
//!
//! A tool never mutates the document (architecture §4). It builds an
//! [`EditCommand`] and hands it to a [`CommandSink`]; the session
//! dispatches it through the document's
//! [`CommandBus`](xarast_doc::CommandBus), which runs it inside a
//! [`Tx`] that records every inverse *before* the action applies. Undo is
//! therefore complete by construction, whatever the command does.
//!
//! The enum is called `EditCommand` rather than the phase document's
//! `Command` because `xarast_doc::Command` is the trait every command —
//! these and the layer commands of [`crate::commands`] — implements, and
//! the two names in one scope would be a trap.
//!
//! # Coalescing
//!
//! Coalescing is per command kind (`research/02 §10.13`): two commands
//! merge into one undo step only when [`EditCommand::coalesces_with`]
//! says so *and* they were dispatched inside the same gesture
//! ([`xarast_doc::CommandBus::begin_gesture`]). The label is part of the
//! merge key, so a wrong merge shows up as a wrong "Undo …" label.

use xarast_doc::{EditError, NodeId, Tx};
use xarast_geom::Matrix;

/// Every document mutation a tool can ask for. Grows with the phase.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EditCommand {
    /// Transforms nodes and everything under them. A pure translation is
    /// a "Move"; anything else a "Transform".
    TransformNodes {
        /// The nodes. A node whose ancestor is also listed is skipped, so
        /// a group and its member are not moved twice.
        nodes: Vec<NodeId>,
        /// The transform, in document space.
        xf: Matrix,
        /// Whether line widths scale with the objects. Reserved for the
        /// scale handles (W4); a translation ignores it.
        scale_line_widths: bool,
    },
    /// Deletes nodes and everything under them. They stay alive, retained
    /// by the undo step, until the history evicts it.
    DeleteNodes {
        /// The nodes.
        nodes: Vec<NodeId>,
    },
}

impl EditCommand {
    /// A move by a document-space displacement.
    #[must_use]
    pub fn translate(nodes: Vec<NodeId>, by: xarast_geom::Vector) -> EditCommand {
        EditCommand::TransformNodes {
            nodes,
            xf: Matrix::translate(by),
            scale_line_widths: false,
        }
    }

    /// Human-readable, shown in the Edit menu as "Undo Move".
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            EditCommand::TransformNodes { xf, .. } if xf.is_translation_only() => "Move",
            EditCommand::TransformNodes { .. } => "Transform",
            EditCommand::DeleteNodes { .. } => "Delete",
        }
    }

    /// Whether this command may merge with `prev` into one undo step when
    /// both belong to the same gesture: the same kind of transform on the
    /// same nodes. A delete never merges, and a move never merges with a
    /// scale.
    #[must_use]
    pub fn coalesces_with(&self, prev: &EditCommand) -> bool {
        match (self, prev) {
            (
                EditCommand::TransformNodes { nodes: a, .. },
                EditCommand::TransformNodes { nodes: b, .. },
            ) => a == b && self.label() == prev.label(),
            _ => false,
        }
    }

    /// Whether the command changes nothing, and so should not become an
    /// undo step at all: a move by zero, a delete of nothing.
    #[must_use]
    pub fn is_noop(&self) -> bool {
        match self {
            EditCommand::TransformNodes { nodes, xf, .. } => nodes.is_empty() || xf.is_identity(),
            EditCommand::DeleteNodes { nodes } => nodes.is_empty(),
        }
    }
}

/// The outermost of `nodes`: those with no listed ancestor, in order,
/// without duplicates.
fn outermost(tx: &Tx<'_>, nodes: &[NodeId]) -> Vec<NodeId> {
    let tree = &tx.doc().tree;
    let mut out: Vec<NodeId> = Vec::with_capacity(nodes.len());
    for &n in nodes {
        if out.contains(&n) {
            continue;
        }
        if nodes.iter().any(|&a| a != n && tree.is_ancestor(a, n)) {
            continue;
        }
        out.push(n);
    }
    out
}

impl xarast_doc::Command for EditCommand {
    fn label(&self) -> &'static str {
        EditCommand::label(self)
    }

    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        match self {
            EditCommand::TransformNodes { nodes, xf, .. } => {
                for n in outermost(tx, nodes) {
                    tx.transform(n, *xf)?;
                }
                Ok(())
            }
            EditCommand::DeleteNodes { nodes } => {
                for n in outermost(tx, nodes) {
                    tx.delete(n)?;
                }
                Ok(())
            }
        }
    }
}

/// Where a tool puts the commands it wants run.
///
/// The only route from a tool to the document: a tool holds a
/// `&mut dyn CommandSink`, never a `&mut Document`.
pub trait CommandSink {
    /// Queues one command.
    fn emit(&mut self, cmd: EditCommand);
}

impl CommandSink for Vec<EditCommand> {
    fn emit(&mut self, cmd: EditCommand) {
        self.push(cmd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xarast_geom::Vector;

    fn ids() -> (NodeId, NodeId) {
        let doc = xarast_doc::Document::new_empty();
        let spread = doc.active_spread();
        let layer = doc.active_layer(spread).unwrap();
        (layer, spread)
    }

    #[test]
    fn labels_name_what_the_user_did() {
        let (a, _) = ids();
        assert_eq!(
            EditCommand::translate(vec![a], Vector::raw(10, 0)).label(),
            "Move"
        );
        let scale = EditCommand::TransformNodes {
            nodes: vec![a],
            xf: Matrix::scale(2.0, 2.0),
            scale_line_widths: true,
        };
        assert_eq!(scale.label(), "Transform");
        assert_eq!(
            EditCommand::DeleteNodes { nodes: vec![a] }.label(),
            "Delete"
        );
    }

    #[test]
    fn only_like_transforms_of_the_same_nodes_coalesce() {
        let (a, b) = ids();
        let m1 = EditCommand::translate(vec![a], Vector::raw(10, 0));
        let m2 = EditCommand::translate(vec![a], Vector::raw(0, 10));
        let other = EditCommand::translate(vec![b], Vector::raw(0, 10));
        let scale = EditCommand::TransformNodes {
            nodes: vec![a],
            xf: Matrix::scale(2.0, 2.0),
            scale_line_widths: false,
        };
        let del = EditCommand::DeleteNodes { nodes: vec![a] };
        assert!(m2.coalesces_with(&m1));
        assert!(!other.coalesces_with(&m1));
        assert!(!scale.coalesces_with(&m1));
        assert!(!del.coalesces_with(&del));
        assert!(!m1.coalesces_with(&del));
    }

    #[test]
    fn a_move_by_nothing_is_no_step() {
        let (a, _) = ids();
        assert!(EditCommand::translate(vec![a], Vector::raw(0, 0)).is_noop());
        assert!(EditCommand::DeleteNodes { nodes: vec![] }.is_noop());
        assert!(!EditCommand::translate(vec![a], Vector::raw(1, 0)).is_noop());
    }
}
