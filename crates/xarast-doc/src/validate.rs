//! `validate()`: the twelve invariants, checked.
//!
//! Every one of them is cheap enough to run in a test after every single
//! operation, which is exactly what the property tests do.
//!
//! One of them is a **warning, not an error**: attribute nodes coming before
//! the first ink node in a child list is *desirable*, not required, because
//! real `.xar` files violate it. An importer that refused those files would be
//! useless.

use std::collections::{HashMap, HashSet};

use crate::kind::NodeKind;
use crate::live::{LiveKind, LiveRole};
use crate::resources::ResourceRef;
use crate::tree::{NodeFlags, NodeId, Tag, Tree};

/// One broken invariant.
#[derive(Clone, Debug, PartialEq)]
pub enum Invariant {
    /// A node is its own ancestor.
    Cycle {
        /// Where the cycle was found.
        at: NodeId,
    },
    /// `a.next == Some(b)` without `b.prev == Some(a)`, or the converse.
    BrokenSiblingLink {
        /// The node whose `next` is wrong.
        a: NodeId,
        /// What it points at.
        b: NodeId,
    },
    /// A child does not point at the parent whose list it is in.
    ChildParentMismatch {
        /// The child.
        child: NodeId,
        /// The parent it claims.
        claims: Option<NodeId>,
        /// The parent whose list it is actually in.
        actual: NodeId,
    },
    /// A node flagged `DETACHED` is reachable from the root.
    DetachedReachable {
        /// The node.
        node: NodeId,
    },
    /// A generated live node has no controller ancestor of its own kind.
    GeneratedWithoutController {
        /// The node.
        node: NodeId,
    },
    /// An attribute node comes after an ink node in the same child list.
    /// **Warning only**: real `.xar` files violate it.
    AttrAfterInk {
        /// The attribute node.
        attr: NodeId,
    },
    /// A spread has layers but not exactly one active layer.
    NoActiveLayer {
        /// The spread.
        spread: NodeId,
    },
    /// Two nodes carry the same tag.
    DuplicateTag {
        /// The tag.
        tag: Tag,
    },
    /// A node's bounding box is cached as valid while a descendant's is not.
    StaleBounds {
        /// The node with the stale box.
        node: NodeId,
    },
    /// A node references a resource that is not in the tables.
    MissingResource {
        /// The node.
        node: NodeId,
        /// The reference.
        resource: ResourceRef,
    },
    /// A `TextItem` is not under a `TextLine`, or a `TextLine` is not under a
    /// `TextStory`.
    TextItemOutsideLine {
        /// The node.
        node: NodeId,
    },
    /// A live controller does not have exactly one source subtree.
    ControllerWithoutSource {
        /// The controller.
        node: NodeId,
    },
}

/// What `validate()` found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ValidationReport {
    /// Invariants that must hold.
    pub errors: Vec<Invariant>,
    /// Invariants that are desirable but that real files break.
    pub warnings: Vec<Invariant>,
}

impl ValidationReport {
    /// Whether nothing is wrong.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.errors.is_empty()
    }

    /// Panics with a readable message when there are errors. For tests.
    ///
    /// # Panics
    ///
    /// When `errors` is not empty.
    pub fn assert_clean(&self) {
        assert!(
            self.errors.is_empty(),
            "invariants broken: {:#?}",
            self.errors
        );
    }

    fn merge(&mut self, other: ValidationReport) {
        self.errors.extend(other.errors);
        self.warnings.extend(other.warnings);
    }
}

/// Checks every structural invariant of a tree.
#[must_use]
pub fn validate_tree(tree: &Tree) -> ValidationReport {
    let mut r = ValidationReport::default();

    // 7. Tag uniqueness and the bijection with the arena.
    let mut seen_tags: HashMap<Tag, NodeId> = HashMap::new();
    for (id, data) in tree.iter() {
        if let Some(other) = seen_tags.insert(data.tag, id)
            && other != id
        {
            r.errors.push(Invariant::DuplicateTag { tag: data.tag });
        }
        if tree.by_tag(data.tag) != Some(id) {
            r.errors.push(Invariant::DuplicateTag { tag: data.tag });
        }
    }

    // 1. Acyclicity, over the whole arena, not just the reachable part.
    for (id, _) in tree.iter() {
        if tree.is_ancestor(id, id) {
            r.errors.push(Invariant::Cycle { at: id });
        }
    }

    // 2. Link coherence.
    for (id, data) in tree.iter() {
        if let Some(next) = data.links.next
            && tree.get(next).map(|n| n.links.prev) != Some(Some(id))
        {
            r.errors
                .push(Invariant::BrokenSiblingLink { a: id, b: next });
        }
        if let Some(prev) = data.links.prev
            && tree.get(prev).map(|n| n.links.next) != Some(Some(id))
        {
            r.errors
                .push(Invariant::BrokenSiblingLink { a: prev, b: id });
        }
        if let Some(first) = data.links.first_child
            && tree.get(first).is_some_and(|n| n.links.prev.is_some())
        {
            r.errors
                .push(Invariant::BrokenSiblingLink { a: id, b: first });
        }
        if let Some(last) = data.links.last_child
            && tree.get(last).is_some_and(|n| n.links.next.is_some())
        {
            r.errors
                .push(Invariant::BrokenSiblingLink { a: id, b: last });
        }
        for child in tree.children(id) {
            let claims = tree.get(child).and_then(|n| n.links.parent);
            if claims != Some(id) {
                r.errors.push(Invariant::ChildParentMismatch {
                    child,
                    claims,
                    actual: id,
                });
            }
        }
    }

    // Walk the reachable tree once for the rest.
    let mut reachable: HashSet<NodeId> = HashSet::new();
    for id in tree.preorder(tree.root()) {
        if !reachable.insert(id) {
            r.errors.push(Invariant::Cycle { at: id });
            continue;
        }
        let Some(data) = tree.get(id) else { continue };

        // 3. `DETACHED` is transitive downwards: nothing under a detached node
        //    is reachable, so no reachable node may carry the flag.
        if data.flags.contains(NodeFlags::DETACHED) {
            r.errors.push(Invariant::DetachedReachable { node: id });
        }

        // 5. The attribute block. Desirable, not mandatory.
        let mut seen_ink = false;
        let mut layers = 0usize;
        let mut active_layers = 0usize;
        let mut sources = 0usize;
        for child in tree.children(id) {
            let Some(cd) = tree.get(child) else { continue };
            match &cd.kind {
                NodeKind::Attr(_) if seen_ink => {
                    r.warnings.push(Invariant::AttrAfterInk { attr: child });
                }
                NodeKind::Layer(l) => {
                    layers += 1;
                    if l.active {
                        active_layers += 1;
                    }
                }
                NodeKind::Live(l) if l.role == LiveRole::Source => sources += 1,
                _ => {}
            }
            if cd.kind.is_ink() {
                seen_ink = true;
            }
        }

        // 6. One spread, one active layer.
        if matches!(data.kind, NodeKind::Spread(_)) && layers > 0 && active_layers != 1 {
            r.errors.push(Invariant::NoActiveLayer { spread: id });
        }

        // 12. Every controller has exactly one source subtree.
        if let NodeKind::Live(l) = &data.kind
            && l.role == LiveRole::Controller
            && sources != 1
        {
            r.errors
                .push(Invariant::ControllerWithoutSource { node: id });
        }

        // 4. A generated node always has a controller ancestor of its kind.
        if let NodeKind::Live(l) = &data.kind
            && l.role == LiveRole::Generated
            && !has_controller_ancestor(tree, id, &l.kind)
        {
            r.errors
                .push(Invariant::GeneratedWithoutController { node: id });
        }

        // 11. Text nesting.
        let parent_kind = data.links.parent.and_then(|p| tree.kind(p));
        match &data.kind {
            NodeKind::TextItem(_) if !matches!(parent_kind, Some(NodeKind::TextLine(_))) => {
                r.errors.push(Invariant::TextItemOutsideLine { node: id });
            }
            NodeKind::TextLine(_) if !matches!(parent_kind, Some(NodeKind::TextStory(_))) => {
                r.errors.push(Invariant::TextItemOutsideLine { node: id });
            }
            _ => {}
        }

        // 8. An invalid box propagates upwards.
        if tree.bounds(id).is_valid() {
            for child in tree.children(id) {
                if !tree.bounds(child).is_valid() && has_box(tree, child) {
                    r.errors.push(Invariant::StaleBounds { node: id });
                    break;
                }
            }
        }
    }

    r
}

fn has_box(tree: &Tree, id: NodeId) -> bool {
    tree.kind(id).is_some_and(|k| {
        !matches!(
            k,
            NodeKind::Attr(_) | NodeKind::Opaque(_) | NodeKind::Grid(_) | NodeKind::Guideline(_)
        )
    })
}

fn has_controller_ancestor(tree: &Tree, id: NodeId, kind: &LiveKind) -> bool {
    tree.ancestors(id).any(|a| {
        matches!(tree.kind(a), Some(NodeKind::Live(l))
            if l.role == LiveRole::Controller && l.kind.discriminant() == kind.discriminant())
    })
}

/// Checks the tree's invariants plus the ones that need the resource tables.
#[must_use]
pub fn validate_document(doc: &crate::Document) -> ValidationReport {
    let mut r = validate_tree(&doc.tree);
    let mut refs = Vec::new();
    for id in doc.tree.preorder(doc.tree.root()) {
        let Some(data) = doc.tree.get(id) else {
            continue;
        };
        refs.clear();
        crate::resources::refs_of(&data.kind, &mut refs);
        for rf in &refs {
            if !doc.resources.contains(*rf) {
                r.merge(ValidationReport {
                    errors: vec![Invariant::MissingResource {
                        node: id,
                        resource: *rf,
                    }],
                    warnings: Vec::new(),
                });
            }
        }
    }
    r
}
