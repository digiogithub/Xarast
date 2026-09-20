//! Persistent checkpoints over the arena.
//!
//! A [`Snapshot`] is an `imbl::HashMap<NodeId, Arc<NodeData>>` **built from**
//! the arena. It is never the live store — that distinction is the whole of
//! `docs/10-architecture.md` §3.1. The HAMT's structural sharing is what makes
//! taking one every N transactions cheap enough to be the autosave source and,
//! from Phase 6, the persistent-history source.

use std::collections::HashMap;
use std::sync::Arc;

use crate::Document;
use crate::attr::DefaultAttrs;
use crate::document::DocumentMeta;
use crate::kind::NodeKind;
use crate::resources::DocumentResources;
use crate::tree::{Attach, NodeData, NodeFlags, NodeId, Tree};

/// A cheap photograph of a document.
#[derive(Clone, Debug)]
pub struct Snapshot {
    nodes: imbl::HashMap<NodeId, Arc<NodeData>>,
    root: NodeId,
    resources: Arc<DocumentResources>,
    defaults: DefaultAttrs,
    meta: DocumentMeta,
}

impl Snapshot {
    /// How many nodes it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether it holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The node the snapshot's tree hangs from.
    #[must_use]
    pub fn root(&self) -> NodeId {
        self.root
    }

    /// A node, as it was.
    #[must_use]
    pub fn node(&self, id: NodeId) -> Option<&Arc<NodeData>> {
        self.nodes.get(&id)
    }
}

impl Document {
    /// Takes a checkpoint.
    ///
    /// Only the nodes reachable from the root go in: a node the history is
    /// retaining belongs to the undo log, not to the document's state at this
    /// instant.
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        let mut nodes = imbl::HashMap::new();
        for id in self.tree.preorder(self.tree.root()) {
            if let Some(d) = self.tree.get(id) {
                nodes.insert(id, Arc::new(d.clone()));
            }
        }
        Snapshot {
            nodes,
            root: self.tree.root(),
            resources: Arc::new(self.resources.clone()),
            defaults: self.defaults.clone(),
            meta: self.meta.clone(),
        }
    }

    /// Restores a checkpoint.
    ///
    /// # What this invalidates
    ///
    /// The tree is rebuilt, so **every [`NodeId`] the caller was holding goes
    /// stale**, as does any undo log referring to them. [`Tag`](crate::Tag)
    /// values are carried across unchanged, which is why they, and not
    /// `NodeId`, are what the file formats serialise. Callers that keep a
    /// history must clear it.
    pub fn restore(&mut self, s: &Snapshot) {
        let root_kind = s
            .nodes
            .get(&s.root)
            .map_or(NodeKind::Chapter, |d| d.kind.clone());
        let mut tree = Tree::with_root(root_kind);
        let new_root = tree.root();
        if let Some(d) = s.nodes.get(&s.root) {
            tree.set_tag(new_root, d.tag);
            if let Some(n) = tree.get_mut(new_root) {
                n.flags = d.flags & !NodeFlags::DETACHED;
            }
        }

        let mut map: HashMap<NodeId, NodeId> = HashMap::new();
        map.insert(s.root, new_root);
        let mut stack = vec![s.root];
        while let Some(old) = stack.pop() {
            let Some(parent_new) = map.get(&old).copied() else {
                continue;
            };
            let Some(old_data) = s.nodes.get(&old) else {
                continue;
            };
            let mut child = old_data.links.first_child;
            while let Some(oc) = child {
                let Some(cd) = s.nodes.get(&oc) else { break };
                let n = tree.create(cd.kind.clone());
                tree.set_tag(n, cd.tag);
                let _ = tree.attach(n, parent_new, Attach::LastChild);
                if let Some(nd) = tree.get_mut(n) {
                    nd.flags = cd.flags & !NodeFlags::DETACHED;
                }
                map.insert(oc, n);
                stack.push(oc);
                child = cd.links.next;
            }
        }

        self.tree = tree;
        self.resources = (*s.resources).clone();
        self.defaults = s.defaults.clone();
        self.meta = s.meta.clone();
        self.attrs.invalidate_all();
        self.epoch = self.epoch.next();
    }
}
