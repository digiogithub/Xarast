//! The cached attribute resolver.

use std::collections::HashMap;

use crate::attr::{AttrStack, DefaultAttrs, ResolvedAttrs};
use crate::kind::NodeKind;
use crate::tree::{NodeId, Tree};

/// Resolved attributes per node, memoised.
///
/// # Invalidation
///
/// Phase 2 ships the **correct but conservative** policy: any structural or
/// attribute change drops the whole cache. Tuning it — dropping only the
/// affected subtree, and only when the change could matter — is Phase 4's job,
/// and [`AttrResolver::invalidate_subtree`] is the hook it will sharpen. A
/// resolver that is merely slow is a performance bug; one that is stale paints
/// the wrong colours, so the conservative version ships first.
#[derive(Debug, Default)]
pub struct AttrResolver {
    cache: HashMap<NodeId, ResolvedAttrs>,
    hits: u64,
    misses: u64,
}

impl AttrResolver {
    /// An empty cache.
    #[must_use]
    pub fn new() -> AttrResolver {
        AttrResolver::default()
    }

    /// The attributes in force for a node.
    ///
    /// Those are the defaults, overridden by every attribute sibling that
    /// precedes the node's ancestor at each level, and finally by the node's
    /// own attribute children — which is where a `.xar` path keeps its fill.
    pub fn resolve(&mut self, tree: &Tree, id: NodeId, defaults: &DefaultAttrs) -> &ResolvedAttrs {
        if self.cache.contains_key(&id) {
            self.hits += 1;
        } else {
            self.misses += 1;
            let resolved = resolve_uncached(tree, id, defaults);
            self.cache.insert(id, resolved);
        }
        self.cache
            .get(&id)
            .unwrap_or_else(|| unreachable!("just inserted"))
    }

    /// Drops the cached entries for a subtree.
    ///
    /// The conservative implementation drops everything, because an attribute
    /// inside the subtree can affect the subtree's *following siblings* too.
    pub fn invalidate_subtree(&mut self, _tree: &Tree, _id: NodeId) {
        self.cache.clear();
    }

    /// Drops the whole cache.
    pub fn invalidate_all(&mut self) {
        self.cache.clear();
    }

    /// How many entries are cached.
    #[must_use]
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Cache hits and misses since the resolver was created.
    #[must_use]
    pub fn stats(&self) -> (u64, u64) {
        (self.hits, self.misses)
    }
}

/// Resolves without consulting or filling a cache.
///
/// This is the reference implementation the property tests check the cache and
/// the [`AttrStack`] walk against.
#[must_use]
pub fn resolve_uncached(tree: &Tree, id: NodeId, defaults: &DefaultAttrs) -> ResolvedAttrs {
    let mut chain: smallvec::SmallVec<[NodeId; 16]> = smallvec::SmallVec::new();
    chain.push(id);
    chain.extend(tree.ancestors(id));
    chain.reverse();

    let mut stack = AttrStack::with_defaults(defaults);
    for w in chain.windows(2) {
        let (parent, child) = (w[0], w[1]);
        for sib in tree.children(parent) {
            if sib == child {
                break;
            }
            push_if_attr(tree, &mut stack, sib);
        }
    }
    for child in tree.children(id) {
        push_if_attr(tree, &mut stack, child);
    }
    stack.snapshot()
}

fn push_if_attr(tree: &Tree, stack: &mut AttrStack, node: NodeId) {
    if let Some(NodeKind::Attr(a)) = tree.kind(node) {
        stack.push(std::sync::Arc::new(a.value.clone()));
    }
}
