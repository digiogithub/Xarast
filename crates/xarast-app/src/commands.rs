//! The bridge from tool intent to the document's command bus.
//!
//! Every mutation in Xarast is a [`xarast_doc::Command`] dispatched
//! through a [`CommandBus`](xarast_doc::CommandBus), which records its own
//! inverse before applying it. That rule is what makes undo complete by
//! construction (architecture §4), and the reason tools live above this
//! module rather than below it: a tool that cannot reach the arena cannot
//! make an edit the undo log never saw.
//!
//! The commands here are only the ones Phase 5 needs — layer visibility,
//! locking, renaming, the active layer. Phase 7 adds the editing ones and
//! changes nothing about the plumbing.

use std::sync::Arc;

use xarast_doc::{Action, Command, EditError, LayerNode, NodeId, NodeKind, Tx};

/// Reads a layer node, or fails the transaction rather than silently
/// doing nothing to a node that is not a layer.
fn layer_of<'t>(tx: &'t Tx<'_>, id: NodeId) -> Result<&'t LayerNode, EditError> {
    match tx.doc().tree.kind(id) {
        Some(NodeKind::Layer(l)) => Ok(l),
        _ => Err(EditError::WrongKind(id)),
    }
}

/// Shows or hides a layer.
#[derive(Debug, Clone, Copy)]
pub struct SetLayerVisible {
    /// The layer.
    pub layer: NodeId,
    /// Whether it is drawn.
    pub visible: bool,
}

impl Command for SetLayerVisible {
    fn label(&self) -> &'static str {
        "Show or hide layer"
    }

    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        let mut l = layer_of(tx, self.layer)?.clone();
        if l.visible == self.visible {
            return Ok(());
        }
        l.visible = self.visible;
        tx.set_kind(self.layer, NodeKind::Layer(Box::new(l)))
    }
}

/// Locks or unlocks a layer.
#[derive(Debug, Clone, Copy)]
pub struct SetLayerLocked {
    /// The layer.
    pub layer: NodeId,
    /// Whether it is locked against editing.
    pub locked: bool,
}

impl Command for SetLayerLocked {
    fn label(&self) -> &'static str {
        "Lock or unlock layer"
    }

    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        let mut l = layer_of(tx, self.layer)?.clone();
        if l.locked == self.locked {
            return Ok(());
        }
        l.locked = self.locked;
        tx.set_kind(self.layer, NodeKind::Layer(Box::new(l)))
    }
}

/// Renames a layer.
#[derive(Debug, Clone)]
pub struct RenameLayer {
    /// The layer.
    pub layer: NodeId,
    /// Its new name.
    pub name: Arc<str>,
}

impl Command for RenameLayer {
    fn label(&self) -> &'static str {
        "Rename layer"
    }

    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        let mut l = layer_of(tx, self.layer)?.clone();
        if l.name == self.name {
            return Ok(());
        }
        l.name = Arc::clone(&self.name);
        tx.set_kind(self.layer, NodeKind::Layer(Box::new(l)))
    }
}

/// Makes a layer its spread's active layer.
///
/// "One spread, one active layer" is a command-level invariant, not an
/// arena-level one (`document-model.md` decision 23): this transaction
/// clears the flag on the previous holder itself, so undo puts it back.
///
/// # Why this one uses `Tx::act`
///
/// Moving the flag necessarily passes through a state with zero or two
/// active layers. The model's repair, `keep_one_active_layer`, runs once
/// at commit and only over the spreads the transaction touched, so that
/// intermediate state is never seen; what matters is that the command
/// leaves exactly one. `Tx::act` applies each action and records its
/// inverse directly, which keeps undo exact — and it is correct by the
/// invariant's own terms, because the invariant
/// is "a transaction may not *leave* it broken" (`document-model.md`
/// invariant 19), and this one does not.
///
/// A guide layer is refused: the model's own repair never elects one, so
/// letting a command do it would produce a document the next repair
/// would silently undo.
#[derive(Debug, Clone, Copy)]
pub struct SetActiveLayer {
    /// The layer.
    pub layer: NodeId,
}

impl Command for SetActiveLayer {
    fn label(&self) -> &'static str {
        "Change active layer"
    }

    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        let target = layer_of(tx, self.layer)?.clone();
        if target.guide {
            return Err(EditError::WrongKind(self.layer));
        }
        if target.active {
            return Ok(());
        }
        let Some(parent) = tx.doc().tree.links(self.layer).parent else {
            return Err(EditError::WrongKind(self.layer));
        };
        let siblings: Vec<NodeId> = tx
            .doc()
            .tree
            .children(parent)
            .filter(|c| {
                *c != self.layer
                    && matches!(tx.doc().tree.kind(*c), Some(NodeKind::Layer(l)) if l.active)
            })
            .collect();
        for s in siblings {
            let mut l = layer_of(tx, s)?.clone();
            l.active = false;
            tx.act(Action::SetKind {
                node: s,
                new: Box::new(NodeKind::Layer(Box::new(l))),
            })?;
        }
        let mut l = target;
        l.active = true;
        tx.act(Action::SetKind {
            node: self.layer,
            new: Box::new(NodeKind::Layer(Box::new(l))),
        })
    }
}

/// Adds an empty layer above every existing one in a spread.
///
/// The layer panel needs one command that changes the shape of the tree,
/// and this is the cheapest honest one. Phase 7 owns the rest.
#[derive(Debug, Clone)]
pub struct AddLayer {
    /// The spread the layer goes into.
    pub spread: NodeId,
    /// Its name.
    pub name: Arc<str>,
}

impl Command for AddLayer {
    fn label(&self) -> &'static str {
        "Add layer"
    }

    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        if !matches!(tx.doc().tree.kind(self.spread), Some(NodeKind::Spread(_))) {
            return Err(EditError::WrongKind(self.spread));
        }
        let node = tx.create(NodeKind::Layer(Box::new(LayerNode {
            name: Arc::clone(&self.name),
            active: false,
            ..LayerNode::default()
        })))?;
        tx.attach(node, self.spread, xarast_doc::Attach::LastChild)
    }
}

/// Deletes a node and everything under it.
///
/// The node stays alive in the arena, retained by the transaction, until
/// the history evicts it — which is what makes undo of a deletion exact
/// (`document-model.md` decision 3).
#[derive(Debug, Clone, Copy)]
pub struct DeleteNode {
    /// The node.
    pub node: NodeId,
}

impl Command for DeleteNode {
    fn label(&self) -> &'static str {
        "Delete"
    }

    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        tx.delete(self.node)
    }
}
