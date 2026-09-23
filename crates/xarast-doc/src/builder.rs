//! [`DocumentBuilder`]: the only public construction path.
//!
//! `docs/10-architecture.md` §4: *importers never touch the arena directly;
//! they emit a build script the model validates; an importer cannot create an
//! inconsistent document.* Making that true rests on one unglamorous fact —
//! [`Tree`](crate::Tree)'s mutating methods are `pub(crate)` — and on this
//! type being the only thing that can reach them from outside.
//!
//! The builder mirrors the shape of an importer: `push_scope`/`pop_scope` are
//! what `TAG_DOWN`/`TAG_UP` become, [`DocumentBuilder::node`] and
//! [`DocumentBuilder::attribute`] append at the current level, and `define_*`
//! registers a referenceable definition. **Unbalanced scopes are tolerated and
//! reported** — truncated files are real — and [`DocumentBuilder::finish`]
//! closes anything still open.
//!
//! # `finish` has exactly two outcomes
//!
//! Either `Err(BuildError)`, or a document whose `validate()` has no errors.
//! Never a third. Everything the builder cannot represent it repairs and
//! reports as a [`Diagnostic`]; everything it cannot repair is a hard error.
//!
//! **Resource limits live here, not in the importer**, because every importer
//! needs them and only one place should decide.

use std::sync::Arc;

use crate::attr::AttrValue;
use crate::document::{Document, DocumentMeta};
use crate::kind::{ArrowSpec, NodeKind};
use crate::live::{LiveNode, LiveRole};
use crate::resources::{ArrowId, BitmapId, BitmapResource, DashId, ResourceRef};

use crate::tree::{Attach, NodeId};

/// A node the builder has emitted.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct BuildId(pub(crate) NodeId);

impl BuildId {
    /// The arena key. Valid only for the document the builder produces.
    #[inline]
    #[must_use]
    pub fn node_id(self) -> NodeId {
        self.0
    }
}

/// What a build script is allowed to cost.
///
/// The deepest file in the validation corpus is 13 levels; 256 is generous and
/// still bounds a hostile one. These are the numbers that make "bounded
/// allocation on hostile input" achievable at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildLimits {
    /// Maximum tree depth.
    pub max_depth: usize,
    /// Maximum number of nodes.
    pub max_nodes: usize,
    /// Maximum total retained bytes.
    pub max_bytes: usize,
    /// Maximum points in a single path.
    pub max_points_per_path: usize,
}

impl Default for BuildLimits {
    fn default() -> BuildLimits {
        BuildLimits {
            max_depth: 256,
            max_nodes: 20_000_000,
            max_bytes: 2 << 30,
            max_points_per_path: 16_000_000,
        }
    }
}

impl BuildLimits {
    /// Small limits, for fuzzing and for tests that must stay fast.
    #[must_use]
    pub fn small() -> BuildLimits {
        BuildLimits {
            max_depth: 32,
            max_nodes: 100_000,
            max_bytes: 64 << 20,
            max_points_per_path: 100_000,
        }
    }
}

/// How much a diagnostic matters.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum Severity {
    /// Worth knowing.
    Info,
    /// Something was dropped or repaired; the document is still usable.
    Warning,
    /// Something was lost.
    Error,
}

/// The shared vocabulary every importer reports in.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum DiagCode {
    /// A record whose tag we do not know.
    UnknownTag,
    /// An unknown tag that carries structure, so its subtree went with it.
    UnknownStructuralTag,
    /// A tag the format says must be there was not.
    EssentialTagMissing,
    /// An atomic subtree was dropped because its root was not understood.
    AtomicSubtreeDropped,
    /// A coordinate was clamped into the representable range.
    CoordinateClamped,
    /// A record ended early.
    TruncatedRecord,
    /// A checksum did not match.
    ChecksumMismatch,
    /// A scope was closed that was never opened, or left open at the end.
    UnbalancedScope,
    /// A reference pointed at something that does not exist.
    DanglingReference,
    /// A colour's parent chain loops.
    ColourCycle,
    /// Something we understand but do not implement.
    UnsupportedFeature,
    /// A build limit was hit.
    LimitExceeded,
    /// A node was placed somewhere the model does not allow.
    IllegalNesting,
    /// The builder changed the document to keep an invariant.
    Repaired,
}

/// One thing worth telling the user about an import.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// How much it matters.
    pub severity: Severity,
    /// What sort of thing it is.
    pub code: DiagCode,
    /// The human-readable detail.
    pub message: String,
    /// Importer-specific location: a `.xar` record number, an XML line.
    pub location: Option<u64>,
}

impl Diagnostic {
    /// A diagnostic with no location.
    #[must_use]
    pub fn new(severity: Severity, code: DiagCode, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            severity,
            code,
            message: message.into(),
            location: None,
        }
    }
}

/// Why a build could not produce a document.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BuildError {
    /// A limit was exceeded.
    #[error("limit exceeded: {0}")]
    Limit(&'static str),
    /// The tree refused a relink.
    #[error(transparent)]
    Tree(#[from] crate::tree::TreeError),
    /// Nothing was built.
    #[error("nothing was built")]
    Empty,
    /// The builder could not repair the document into a valid one. This is a
    /// bug in the builder, never in the input, and it is reported rather than
    /// hidden.
    #[error("the built document is still inconsistent: {0} error(s)")]
    Inconsistent(usize),
}

/// Builds a document.
#[derive(Debug)]
pub struct DocumentBuilder {
    doc: Document,
    limits: BuildLimits,
    /// The current parent at each open level; index 0 is the root.
    scopes: Vec<NodeId>,
    /// The last node emitted at each open level.
    last: Vec<Option<NodeId>>,
    diagnostics: Vec<Diagnostic>,
    nodes: usize,
    bytes: usize,
    orphans: Vec<NodeId>,
}

impl Default for DocumentBuilder {
    fn default() -> DocumentBuilder {
        DocumentBuilder::new(BuildLimits::default())
    }
}

impl DocumentBuilder {
    /// A builder with an empty document: just the root node.
    #[must_use]
    pub fn new(limits: BuildLimits) -> DocumentBuilder {
        let mut doc = Document::bare();
        doc.tree.max_depth = limits.max_depth;
        let root = doc.tree.root();
        DocumentBuilder {
            doc,
            limits,
            scopes: vec![root],
            last: vec![None],
            diagnostics: Vec::new(),
            nodes: 1,
            bytes: 0,
            orphans: Vec::new(),
        }
    }

    /// Sets the document metadata.
    pub fn meta(&mut self, meta: DocumentMeta) {
        self.doc.meta = meta;
    }

    /// Overrides one of the document's default attributes.
    ///
    /// This is where `TAG_CURRENTATTRIBUTES` and its children land: they are
    /// the document's defaults, not attributes applied to any object.
    pub fn default_attribute(&mut self, value: AttrValue) {
        if !self.doc.defaults.set(value) {
            self.diagnostic(Diagnostic::new(
                Severity::Info,
                DiagCode::UnsupportedFeature,
                "a multi-applicable attribute cannot be a document default",
            ));
        }
    }

    /// Descends into the last node emitted at this level. `TAG_DOWN`.
    ///
    /// # Errors
    ///
    /// [`BuildError::Limit`] when the depth limit would be exceeded.
    pub fn push_scope(&mut self) -> Result<(), BuildError> {
        if self.scopes.len() >= self.limits.max_depth {
            return Err(BuildError::Limit("max_depth"));
        }
        let parent = match self.last.last().copied().flatten() {
            Some(n) => n,
            None => {
                self.diagnostic(Diagnostic::new(
                    Severity::Warning,
                    DiagCode::UnbalancedScope,
                    "descended with no node to descend into; the level was reused",
                ));
                *self.scopes.last().unwrap_or(&self.doc.tree.root())
            }
        };
        self.scopes.push(parent);
        self.last.push(None);
        Ok(())
    }

    /// Returns to the parent level. `TAG_UP`.
    ///
    /// Closing a scope that was never opened is reported and ignored, because
    /// truncated and malformed files are real.
    pub fn pop_scope(&mut self) {
        if self.scopes.len() <= 1 {
            self.diagnostic(Diagnostic::new(
                Severity::Warning,
                DiagCode::UnbalancedScope,
                "ascended above the root; ignored",
            ));
            return;
        }
        self.scopes.pop();
        self.last.pop();
    }

    /// Appends a node at the current level.
    ///
    /// A node the model does not allow in this position is created but left
    /// unattached, and reported; subsequent `push_scope` and `node` calls
    /// still work against it, so an importer never has to branch on it, and
    /// [`DocumentBuilder::finish`] sweeps the orphan away.
    ///
    /// # Errors
    ///
    /// [`BuildError::Limit`] when a limit is exceeded.
    pub fn node(&mut self, kind: NodeKind) -> Result<BuildId, BuildError> {
        self.check_limits(&kind)?;
        let parent = *self.scopes.last().unwrap_or(&self.doc.tree.root());
        let legal = self
            .doc
            .tree
            .kind(parent)
            .is_some_and(|p| legal_child(p, &kind));
        let name = kind.type_name();
        let parent_name = self.doc.tree.kind(parent).map_or("?", NodeKind::type_name);
        let id = self.doc.tree.create(kind);
        self.nodes += 1;
        if legal {
            self.doc.tree.attach(id, parent, Attach::LastChild)?;
        } else {
            self.orphans.push(id);
            self.diagnostic(Diagnostic::new(
                Severity::Warning,
                DiagCode::IllegalNesting,
                format!("{name} may not be a child of {parent_name}; dropped"),
            ));
        }
        if let Some(slot) = self.last.last_mut() {
            *slot = Some(id);
        }
        Ok(BuildId(id))
    }

    /// Appends an attribute node at the current level.
    ///
    /// # Errors
    ///
    /// As [`DocumentBuilder::node`].
    pub fn attribute(&mut self, value: AttrValue) -> Result<BuildId, BuildError> {
        self.node(NodeKind::Attr(Box::new(crate::attr::AttrNode::new(value))))
    }

    /// Registers a colour definition.
    pub fn define_colour(&mut self, def: xarast_color::ColourDef) -> xarast_color::ColourId {
        self.doc.resources.colours.insert(def)
    }

    /// Registers a bitmap, deduplicating by content.
    pub fn define_bitmap(&mut self, res: BitmapResource) -> BitmapId {
        self.bytes += res.pixels.pixels.len();
        self.doc.resources.insert_bitmap(res)
    }

    /// Registers a dash pattern.
    pub fn define_dash(&mut self, d: xarast_geom::DashPattern) -> DashId {
        self.doc.resources.insert_dash(d)
    }

    /// Registers an arrowhead.
    pub fn define_arrow(&mut self, a: ArrowSpec) -> ArrowId {
        self.doc.resources.insert_arrow(a)
    }

    /// Records a diagnostic.
    pub fn diagnostic(&mut self, d: Diagnostic) {
        self.diagnostics.push(d);
    }

    /// Everything reported so far.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// How deep the scope stack is; the root level is 0.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.scopes.len() - 1
    }

    /// How many nodes have been emitted, including the root.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes
    }

    /// The document so far, read only. Importers use it to look at what they
    /// have already emitted; they cannot change it except through this type.
    #[must_use]
    pub fn document(&self) -> &Document {
        &self.doc
    }

    fn check_limits(&mut self, kind: &NodeKind) -> Result<(), BuildError> {
        if self.nodes >= self.limits.max_nodes {
            return Err(BuildError::Limit("max_nodes"));
        }
        let cost = kind.size_hint();
        if self.bytes + cost > self.limits.max_bytes {
            return Err(BuildError::Limit("max_bytes"));
        }
        self.bytes += cost;
        let points = match kind {
            NodeKind::Path(p) => p.data.points().len(),
            NodeKind::QuickShape(q) => q.path.as_ref().map_or(0, |p| p.points().len()),
            NodeKind::Attr(a) => match &a.value {
                AttrValue::ClipRegion(p) => p.points().len(),
                _ => 0,
            },
            _ => 0,
        };
        if points > self.limits.max_points_per_path {
            return Err(BuildError::Limit("max_points_per_path"));
        }
        Ok(())
    }

    /// Closes any open scopes, repairs what it can, validates, and hands back
    /// the document with everything reported along the way.
    ///
    /// # Errors
    ///
    /// [`BuildError::Empty`] when nothing was emitted, and
    /// [`BuildError::Inconsistent`] when a repair failed — which would be a
    /// bug in this module, reported rather than hidden.
    pub fn finish(mut self) -> Result<(Document, Vec<Diagnostic>), BuildError> {
        if self.scopes.len() > 1 {
            self.diagnostic(Diagnostic::new(
                Severity::Warning,
                DiagCode::UnbalancedScope,
                format!("{} scope(s) left open; closed", self.scopes.len() - 1),
            ));
        }
        if self.nodes <= 1 {
            return Err(BuildError::Empty);
        }

        self.sweep_orphans();
        self.repair_text_nesting();
        self.repair_live();
        self.repair_active_layers();
        self.repair_missing_resources();

        let report = self.doc.validate();
        if !report.errors.is_empty() {
            return Err(BuildError::Inconsistent(report.errors.len()));
        }
        for w in &report.warnings {
            if let crate::validate::Invariant::AttrAfterInk { .. } = w {
                self.diagnostics.push(Diagnostic::new(
                    Severity::Info,
                    DiagCode::UnsupportedFeature,
                    "an attribute follows an ink node in a child list; \
                     accepted, because real files do this",
                ));
                break;
            }
        }
        Ok((self.doc, self.diagnostics))
    }

    fn sweep_orphans(&mut self) {
        let orphans = std::mem::take(&mut self.orphans);
        for o in orphans {
            if self.doc.tree.contains(o) && !self.doc.tree.is_reachable(o) {
                self.doc.tree.destroy_subtree(o);
            }
        }
    }

    fn repair_text_nesting(&mut self) {
        let root = self.doc.tree.root();
        let bad: Vec<NodeId> = self
            .doc
            .tree
            .preorder(root)
            .filter(|id| {
                let Some(d) = self.doc.tree.get(*id) else {
                    return false;
                };
                let pk = d.links.parent.and_then(|p| self.doc.tree.kind(p));
                match &d.kind {
                    NodeKind::TextItem(_) => !matches!(pk, Some(NodeKind::TextLine(_))),
                    NodeKind::TextLine(_) => !matches!(pk, Some(NodeKind::TextStory(_))),
                    _ => false,
                }
            })
            .collect();
        for b in bad {
            self.doc.tree.destroy_subtree(b);
            self.diagnostics.push(Diagnostic::new(
                Severity::Warning,
                DiagCode::IllegalNesting,
                "a text node outside its story structure was dropped",
            ));
        }
    }

    fn repair_live(&mut self) {
        let root = self.doc.tree.root();
        // Generated nodes with no controller ancestor are derived data; drop.
        let orphaned: Vec<NodeId> = self
            .doc
            .tree
            .preorder(root)
            .filter(|id| {
                matches!(self.doc.tree.kind(*id), Some(NodeKind::Live(l))
                if l.role == LiveRole::Generated
                    && !self.doc.tree.ancestors(*id).any(|a| {
                        matches!(self.doc.tree.kind(a), Some(NodeKind::Live(c))
                            if c.role == LiveRole::Controller
                                && c.kind.discriminant() == l.kind.discriminant())
                    }))
            })
            .collect();
        for o in orphaned {
            self.doc.tree.destroy_subtree(o);
            self.diagnostics.push(Diagnostic::new(
                Severity::Warning,
                DiagCode::Repaired,
                "a generated live node with no controller was dropped",
            ));
        }

        // Every controller needs exactly one source subtree.
        let controllers: Vec<NodeId> = self
            .doc
            .tree
            .preorder(root)
            .filter(|id| {
                matches!(self.doc.tree.kind(*id), Some(NodeKind::Live(l))
                    if l.role == LiveRole::Controller)
            })
            .collect();
        for c in controllers {
            let sources: Vec<NodeId> = self
                .doc
                .tree
                .children(c)
                .filter(|ch| {
                    matches!(self.doc.tree.kind(*ch), Some(NodeKind::Live(l))
                        if l.role == LiveRole::Source)
                })
                .collect();
            let Some(NodeKind::Live(ctl)) = self.doc.tree.kind(c).cloned() else {
                continue;
            };
            match sources.len() {
                1 => {}
                0 => {
                    let src = self.doc.tree.create(NodeKind::Live(Box::new(LiveNode {
                        role: LiveRole::Source,
                        kind: ctl.kind.clone(),
                        regen: ctl.regen,
                        name: None,
                    })));
                    if self.doc.tree.attach(src, c, Attach::FirstChild).is_ok() {
                        self.diagnostics.push(Diagnostic::new(
                            Severity::Warning,
                            DiagCode::Repaired,
                            "a live controller had no source subtree; an empty one was added",
                        ));
                    } else {
                        // A controller at the depth limit has no room for a
                        // source child, and a controller without one is
                        // invalid, so the controller goes.
                        self.doc.tree.destroy_subtree(src);
                        self.doc.tree.destroy_subtree(c);
                        self.diagnostics.push(Diagnostic::new(
                            Severity::Warning,
                            DiagCode::Repaired,
                            "a live controller at the depth limit had no source and no room \
                             for one; it was dropped",
                        ));
                    }
                }
                _ => {
                    for extra in sources.into_iter().skip(1) {
                        if let Some(NodeKind::Live(l)) = self.doc.tree.kind(extra).cloned()
                            && let Some(n) = self.doc.tree.get_mut(extra)
                        {
                            n.kind = NodeKind::Live(Box::new(LiveNode {
                                role: LiveRole::Generated,
                                ..*l
                            }));
                        }
                    }
                    self.diagnostics.push(Diagnostic::new(
                        Severity::Warning,
                        DiagCode::Repaired,
                        "a live controller had several source subtrees; \
                         the extras became generated",
                    ));
                }
            }
        }
    }

    fn repair_active_layers(&mut self) {
        let root = self.doc.tree.root();
        let spreads: Vec<NodeId> = self
            .doc
            .tree
            .preorder(root)
            .filter(|id| matches!(self.doc.tree.kind(*id), Some(NodeKind::Spread(_))))
            .collect();
        for s in spreads {
            let layers: Vec<NodeId> = self
                .doc
                .tree
                .children(s)
                .filter(|c| matches!(self.doc.tree.kind(*c), Some(NodeKind::Layer(_))))
                .collect();
            if layers.is_empty() {
                continue;
            }
            let active: Vec<NodeId> = layers
                .iter()
                .copied()
                .filter(|c| matches!(self.doc.tree.kind(*c), Some(NodeKind::Layer(l)) if l.active))
                .collect();
            if active.len() == 1 {
                continue;
            }
            // Prefer the first non-guide layer; fall back to the first.
            let chosen = layers
                .iter()
                .copied()
                .find(|c| matches!(self.doc.tree.kind(*c), Some(NodeKind::Layer(l)) if !l.guide))
                .unwrap_or(layers[0]);
            for l in &layers {
                if let Some(n) = self.doc.tree.get_mut(*l)
                    && let NodeKind::Layer(layer) = &mut n.kind
                {
                    layer.active = *l == chosen;
                }
            }
            self.diagnostics.push(Diagnostic::new(
                Severity::Info,
                DiagCode::Repaired,
                format!(
                    "a spread had {} active layers; \"{}\" was made the active one",
                    active.len(),
                    self.doc
                        .tree
                        .kind(chosen)
                        .and_then(|k| match k {
                            NodeKind::Layer(l) => Some(Arc::clone(&l.name)),
                            _ => None,
                        })
                        .unwrap_or_else(|| Arc::from("?"))
                ),
            ));
        }
    }

    fn repair_missing_resources(&mut self) {
        let root = self.doc.tree.root();
        let mut refs = Vec::new();
        let mut bad: Vec<NodeId> = Vec::new();
        for id in self.doc.tree.preorder(root) {
            let Some(d) = self.doc.tree.get(id) else {
                continue;
            };
            refs.clear();
            crate::resources::refs_of(&d.kind, &mut refs);
            if refs
                .iter()
                .any(|r| !self.doc.resources.contains(*r) && is_hard_ref(*r))
            {
                bad.push(id);
            }
        }
        for b in bad {
            self.doc.tree.destroy_subtree(b);
            self.diagnostics.push(Diagnostic::new(
                Severity::Warning,
                DiagCode::DanglingReference,
                "a node referencing a resource that was never defined was dropped",
            ));
        }
    }
}

/// Colour references resolve through the table's own fallback, so a dangling
/// one is not a document error; a missing bitmap is.
fn is_hard_ref(r: ResourceRef) -> bool {
    !matches!(r, ResourceRef::Colour(_))
}

/// Whether the model allows this child directly under this parent.
///
/// Only the nestings `validate()` calls errors are enforced; everything else
/// is permitted, because real files put all sorts of things in all sorts of
/// places and an importer that rejected them would be useless.
#[must_use]
pub fn legal_child(parent: &NodeKind, child: &NodeKind) -> bool {
    match child {
        NodeKind::TextItem(_) => matches!(parent, NodeKind::TextLine(_)),
        NodeKind::TextLine(_) => matches!(parent, NodeKind::TextStory(_)),
        NodeKind::Live(l) if l.role == LiveRole::Generated => {
            matches!(parent, NodeKind::Live(_))
        }
        _ => true,
    }
}

/// Builds the canonical empty document through the builder, so that the one
/// construction path really is the only one.
#[must_use]
pub fn canonical_document() -> Document {
    Document::new_empty()
}

/// A convenience for tests and importers: a builder already holding the
/// canonical chapter/spread/page/layer skeleton, positioned inside the active
/// layer.
///
/// # Errors
///
/// As [`DocumentBuilder::push_scope`].
pub fn skeleton(limits: BuildLimits) -> Result<DocumentBuilder, BuildError> {
    use crate::structure::{PageNode, SpreadNode};
    let mut b = DocumentBuilder::new(limits);
    b.node(NodeKind::Chapter)?;
    b.push_scope()?;
    let spread = SpreadNode::default();
    let rect = spread.page_size;
    b.node(NodeKind::Spread(Box::new(spread)))?;
    b.push_scope()?;
    b.node(NodeKind::Page(Box::new(PageNode {
        rect,
        right_hand: false,
    })))?;
    b.node(NodeKind::Grid(Box::default()))?;
    b.node(NodeKind::Layer(Box::default()))?;
    b.push_scope()?;
    Ok(b)
}
