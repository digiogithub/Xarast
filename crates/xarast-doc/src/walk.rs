//! Traversal iterators, including the render walk with scope events.

use crate::tree::{NodeId, Tree};

/// The children of a node, in order.
#[derive(Debug, Clone)]
pub struct Children<'t> {
    tree: &'t Tree,
    next: Option<NodeId>,
    back: Option<NodeId>,
    done: bool,
}

impl<'t> Children<'t> {
    pub(crate) fn new(tree: &'t Tree, id: NodeId) -> Children<'t> {
        let links = tree.links(id);
        Children {
            tree,
            next: links.first_child,
            back: links.last_child,
            done: links.first_child.is_none(),
        }
    }
}

impl Iterator for Children<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        if self.done {
            return None;
        }
        let cur = self.next?;
        if Some(cur) == self.back {
            self.done = true;
        }
        self.next = self.tree.links(cur).next;
        Some(cur)
    }
}

impl DoubleEndedIterator for Children<'_> {
    fn next_back(&mut self) -> Option<NodeId> {
        if self.done {
            return None;
        }
        let cur = self.back?;
        if Some(cur) == self.next {
            self.done = true;
        }
        self.back = self.tree.links(cur).prev;
        Some(cur)
    }
}

/// The ancestors of a node, nearest first.
#[derive(Debug, Clone)]
pub struct Ancestors<'t> {
    tree: &'t Tree,
    next: Option<NodeId>,
    guard: usize,
}

impl<'t> Ancestors<'t> {
    pub(crate) fn new(tree: &'t Tree, id: NodeId) -> Ancestors<'t> {
        Ancestors {
            tree,
            next: tree.links(id).parent,
            guard: tree.node_count() + 1,
        }
    }
}

impl Iterator for Ancestors<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        if self.guard == 0 {
            return None;
        }
        self.guard -= 1;
        let cur = self.next?;
        self.next = self.tree.links(cur).parent;
        Some(cur)
    }
}

/// Depth-first, parent before children.
#[derive(Debug, Clone)]
pub struct Preorder<'t> {
    tree: &'t Tree,
    stack: Vec<NodeId>,
    guard: usize,
}

impl<'t> Preorder<'t> {
    pub(crate) fn new(tree: &'t Tree, root: NodeId) -> Preorder<'t> {
        let stack = if tree.contains(root) {
            vec![root]
        } else {
            Vec::new()
        };
        Preorder {
            tree,
            stack,
            guard: tree.node_count() + 1,
        }
    }
}

impl Iterator for Preorder<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        if self.guard == 0 {
            return None;
        }
        let cur = self.stack.pop()?;
        self.guard -= 1;
        // Push children in reverse so the first child pops first.
        let mut kids: smallvec::SmallVec<[NodeId; 8]> = smallvec::SmallVec::new();
        kids.extend(self.tree.children(cur));
        for k in kids.into_iter().rev() {
            self.stack.push(k);
        }
        Some(cur)
    }
}

/// Depth-first, children before the parent: the ink paint order.
#[derive(Debug, Clone)]
pub struct Postorder<'t> {
    tree: &'t Tree,
    stack: Vec<(NodeId, bool)>,
    guard: usize,
}

impl<'t> Postorder<'t> {
    pub(crate) fn new(tree: &'t Tree, root: NodeId) -> Postorder<'t> {
        let stack = if tree.contains(root) {
            vec![(root, false)]
        } else {
            Vec::new()
        };
        Postorder {
            tree,
            stack,
            guard: 2 * tree.node_count() + 2,
        }
    }
}

impl Iterator for Postorder<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        loop {
            if self.guard == 0 {
                return None;
            }
            self.guard -= 1;
            let (cur, expanded) = self.stack.pop()?;
            if expanded {
                return Some(cur);
            }
            self.stack.push((cur, true));
            let mut kids: smallvec::SmallVec<[NodeId; 8]> = smallvec::SmallVec::new();
            kids.extend(self.tree.children(cur));
            for k in kids.into_iter().rev() {
                self.stack.push((k, false));
            }
        }
    }
}

/// Events of the render traversal.
///
/// A consumer keeps its [`AttrStack`](crate::AttrStack) correct with no
/// bookkeeping of its own: `push_scope` on [`WalkEvent::EnterScope`],
/// `pop_scope` on [`WalkEvent::LeaveScope`], `push` on visiting an attribute.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum WalkEvent {
    /// The walk is about to enter the child list of `parent`.
    EnterScope {
        /// The node whose children follow.
        parent: NodeId,
    },
    /// A node is visited.
    Visit {
        /// The node.
        node: NodeId,
    },
    /// The walk has left the child list of `parent`.
    LeaveScope {
        /// The node whose children just ended.
        parent: NodeId,
    },
}

/// Pruning control, mirroring the original's subtree render states.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum Descend {
    /// Do not enter the child list. The node has already been reported.
    Skip,
    /// The node only; equivalent to [`Descend::Skip`] as far as the walk is
    /// concerned, and kept distinct because the caller treats them differently.
    SelfOnly,
    /// The default: enter the child list.
    SelfAndChildren,
    /// A render-cache hit: suppress every event until the given node is
    /// visited.
    JumpTo(NodeId),
    /// Advance to the given node, still reporting scope events and attribute
    /// nodes so that the attribute stack stays correct, and suppressing
    /// everything else.
    RunTo(NodeId),
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Step {
    Enter(NodeId),
    Visit(NodeId),
    Leave(NodeId),
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Suppress {
    Everything(NodeId),
    AllButScope(NodeId),
}

/// The render traversal.
///
/// # Where an ink node is painted
///
/// An attribute applies to its **following siblings and their subtrees**, and
/// also to its parent's own ink, because a parent paints after its children —
/// which is exactly what makes a `.xar` path's fill, stored as the path's
/// child, apply to the path.
///
/// So a consumer paints an ink node at [`WalkEvent::LeaveScope`] when it has
/// children (the child scope, holding its own attribute block, is still in
/// force) and at [`WalkEvent::Visit`] when it has none. Scope events are
/// emitted only for nodes that actually have children, so the two cases never
/// overlap.
#[derive(Debug)]
pub struct RenderWalk<'t> {
    tree: &'t Tree,
    stack: Vec<Step>,
    pending: Option<NodeId>,
    descend: Descend,
    suppress: Option<Suppress>,
    guard: usize,
}

impl<'t> RenderWalk<'t> {
    pub(crate) fn new(tree: &'t Tree, root: NodeId) -> RenderWalk<'t> {
        let stack = if tree.contains(root) {
            vec![Step::Visit(root)]
        } else {
            Vec::new()
        };
        RenderWalk {
            tree,
            stack,
            pending: None,
            descend: Descend::SelfAndChildren,
            suppress: None,
            guard: 3 * tree.node_count() + 3,
        }
    }

    /// Applies to the node most recently returned by `next()`.
    ///
    /// Calling it after a scope event has no effect.
    pub fn control(&mut self, d: Descend) {
        if self.pending.is_some() {
            self.descend = d;
        }
    }

    fn schedule_children(&mut self, n: NodeId) {
        let links = self.tree.links(n);
        if links.first_child.is_none() {
            return;
        }
        self.stack.push(Step::Leave(n));
        let mut kids: smallvec::SmallVec<[NodeId; 8]> = smallvec::SmallVec::new();
        kids.extend(self.tree.children(n));
        for k in kids.into_iter().rev() {
            self.stack.push(Step::Visit(k));
        }
        self.stack.push(Step::Enter(n));
    }
}

impl Iterator for RenderWalk<'_> {
    type Item = WalkEvent;

    fn next(&mut self) -> Option<WalkEvent> {
        loop {
            if self.guard == 0 {
                return None;
            }
            self.guard -= 1;

            if let Some(n) = self.pending.take() {
                match self.descend {
                    Descend::Skip | Descend::SelfOnly => {}
                    Descend::SelfAndChildren => self.schedule_children(n),
                    Descend::JumpTo(target) => {
                        self.suppress = Some(Suppress::Everything(target));
                        self.schedule_children(n);
                    }
                    Descend::RunTo(target) => {
                        self.suppress = Some(Suppress::AllButScope(target));
                        self.schedule_children(n);
                    }
                }
                self.descend = Descend::SelfAndChildren;
            }

            let step = self.stack.pop()?;
            let event = match step {
                Step::Enter(n) => WalkEvent::EnterScope { parent: n },
                Step::Leave(n) => WalkEvent::LeaveScope { parent: n },
                Step::Visit(n) => {
                    self.pending = Some(n);
                    WalkEvent::Visit { node: n }
                }
            };

            match self.suppress {
                None => return Some(event),
                Some(mode) => {
                    let target = match mode {
                        Suppress::Everything(t) | Suppress::AllButScope(t) => t,
                    };
                    if let WalkEvent::Visit { node } = event
                        && node == target
                    {
                        self.suppress = None;
                        return Some(event);
                    }
                    if let Suppress::AllButScope(_) = mode {
                        let keep = match event {
                            WalkEvent::EnterScope { .. } | WalkEvent::LeaveScope { .. } => true,
                            WalkEvent::Visit { node } => self
                                .tree
                                .kind(node)
                                .is_some_and(crate::kind::NodeKind::is_attr),
                        };
                        if keep {
                            return Some(event);
                        }
                    }
                    // Suppressed: keep walking.
                }
            }
        }
    }
}
