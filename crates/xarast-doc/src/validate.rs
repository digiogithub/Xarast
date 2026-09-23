//! `validate()`: the twelve invariants, checked.
//!
//! Every one of them is cheap enough to run in a test after every single
//! operation, which is exactly what the property tests do.
//!
//! One of them is a **warning, not an error**: attribute nodes coming before
//! the first ink node in a child list is *desirable*, not required, because
//! real `.xar` files violate it. An importer that refused those files would be
//! useless.

use slotmap::SecondaryMap;

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
    /// How many distinct nodes are reachable from the root. When it equals
    /// [`Tree::node_count`], every node alive in the arena is part of the
    /// document, and a caller may iterate the arena instead of walking it.
    pub reachable: usize,
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
}

/// Checks every structural invariant of a tree.
#[must_use]
pub fn validate_tree(tree: &Tree) -> ValidationReport {
    validate(tree, None)
}

/// The tree checks, plus the resource checks when `resources` is given. One
/// walk over the reachable tree serves both.
fn validate(
    tree: &Tree,
    resources: Option<&crate::resources::DocumentResources>,
) -> ValidationReport {
    let mut r = ValidationReport::default();
    let mut refs = Vec::new();

    // 7. Tag uniqueness and the bijection with the arena. `by_tag` is a
    //    function, so if every node's tag maps back to that node, no two
    //    nodes can share a tag: whichever one the map does not name fails.
    //    That makes a separate set of seen tags redundant.
    for (id, data) in tree.iter() {
        if tree.by_tag(data.tag) != Some(id) {
            r.errors.push(Invariant::DuplicateTag { tag: data.tag });
        }
    }

    // 1. Acyclicity, over the whole arena, not just the reachable part.
    //    Each parent chain is walked once: a node whose chain is known to
    //    end at a parentless node is never walked again, so this is O(n)
    //    rather than O(n × depth).
    check_acyclic(tree, &mut r);

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
    let mut reachable: SecondaryMap<NodeId, ()> = SecondaryMap::with_capacity(tree.node_count());
    for id in tree.preorder(tree.root()) {
        if reachable.insert(id, ()).is_some() {
            r.errors.push(Invariant::Cycle { at: id });
            continue;
        }
        let Some(data) = tree.get(id) else { continue };
        r.reachable += 1;

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

        // 9. Every referenced resource exists.
        if let Some(resources) = resources {
            refs.clear();
            crate::resources::refs_of(&data.kind, &mut refs);
            for rf in &refs {
                if !resources.contains(*rf) {
                    let missing = Invariant::MissingResource {
                        node: id,
                        resource: *rf,
                    };
                    // A colour reference resolves through the table's own
                    // fallback, so a dangling one still renders and is only a
                    // warning; `DocumentBuilder::finish` keeps such nodes on
                    // exactly that basis. A missing bitmap, dash or arrow
                    // cannot be drawn at all and is an error.
                    if matches!(rf, ResourceRef::Colour(_)) {
                        r.warnings.push(missing);
                    } else {
                        r.errors.push(missing);
                    }
                }
            }
        }
    }

    r
}

/// Reports every node that is its own ancestor.
///
/// Colours each node by what its parent chain is known to do: unvisited,
/// on the chain being walked now, or ending at a parentless node. A walk
/// that meets its own chain has found a cycle. Every node on that chain
/// leads into the cycle, but only the nodes *on* the cycle are reported,
/// matching `is_ancestor(id, id)`.
fn check_acyclic(tree: &Tree, r: &mut ValidationReport) {
    const ON_CHAIN: u8 = 1;
    const DONE: u8 = 2;
    let mut state: SecondaryMap<NodeId, u8> = SecondaryMap::with_capacity(tree.node_count());
    let mut chain: Vec<NodeId> = Vec::new();
    for (start, _) in tree.iter() {
        if state.contains_key(start) {
            continue;
        }
        chain.clear();
        let mut cur = Some(start);
        while let Some(id) = cur {
            match state.get(id).copied() {
                Some(DONE) => break,
                Some(_) => {
                    // `id` is on the current chain: the cycle is the part of
                    // the chain from `id` onwards.
                    if let Some(pos) = chain.iter().position(|c| *c == id) {
                        for c in &chain[pos..] {
                            r.errors.push(Invariant::Cycle { at: *c });
                        }
                    }
                    break;
                }
                None => {
                    if !tree.contains(id) {
                        break;
                    }
                    state.insert(id, ON_CHAIN);
                    chain.push(id);
                    cur = tree.links(id).parent;
                }
            }
        }
        for c in &chain {
            state.insert(*c, DONE);
        }
    }
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
    validate(&doc.tree, Some(&doc.resources))
}
