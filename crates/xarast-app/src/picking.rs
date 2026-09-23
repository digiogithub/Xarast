//! Precise picking (`phase-07 §W3`, XARA-T-0152): which object is under the
//! pointer, by its painted fill and stroke rather than its bounding box.
//!
//! This is the application half of the contract in `geometry.md`
//! ("Integration contract for `xarast-app`"). `xarast-geom` provides the
//! bounds index ([`HitIndex`]) and the exact tests ([`HitShape::hit`]);
//! this module decides what goes into the index and how a hit maps back to
//! what a click selects.
//!
//! * **What is indexed**: every ink leaf — a node with ink of its own,
//!   painted at the point the render walk paints it — on a visible,
//!   unlocked, non-guide layer. Groups, live-effect containers and text
//!   stories are not leaves; their contents are. A top-level object with no
//!   pickable leaf (a text story until phase 9 draws text, an unrendered
//!   live effect) is indexed as its bounding box, so it stays selectable.
//! * **Z** is the leaf's rank in render order, `rank << 16`.
//! * **Fill** is tested only when the fill is painted (not "no colour",
//!   and the path is filled); the **stroke** only when the line colour is
//!   not "no colour". A transparent interior does not hit.
//! * **When**: the index is built lazily on the first pick after a
//!   committed change, never on a drag frame, and never by undo itself —
//!   the undo budget is 1 ms and a rebuild at 100 000 objects is several.
//!   Incremental updates (`set_bounds`, `insert`, `remove` per committed
//!   transaction) are the follow-up that removes the rebuild entirely.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use xarast_doc::{AttrSlot, AttrStack, AttrValue, Document, NodeId, NodeKind, WalkEvent};
use xarast_geom::{
    Cap, FillRule, HitIndex, HitShape, HitTolerance, Join, Matrix, Mp, Path, RectMode, ShapeHit,
    StrokeStyle,
};

use crate::geometry::{DocPoint, DocRect};

/// How a pick maps the leaf it hit to what is selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickMode {
    /// A plain click: the outermost object under the layer.
    TopGroup,
    /// Constrain-click: the leaf itself, inside its groups.
    Leaf,
    /// Alternative-click: the topmost object *beneath* `below`.
    Under {
        /// The object to look beneath.
        below: NodeId,
    },
}

/// Which part of an object a pick landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum HitPart {
    /// Its painted fill.
    Fill,
    /// Its painted stroke.
    Stroke,
    /// Its bounding box: an object with no geometry the picker can test.
    Bounds,
}

/// What a pick found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HitResult {
    /// The leaf hit.
    pub node: NodeId,
    /// What the click selects: the outermost object containing the leaf in
    /// [`PickMode::TopGroup`], the leaf itself in [`PickMode::Leaf`].
    pub top_group: NodeId,
    /// Which part.
    pub part: HitPart,
}

#[derive(Debug)]
enum Geometry {
    Path(Arc<Path>),
    Bounds,
}

#[derive(Debug)]
struct Leaf {
    geometry: Geometry,
    fill: Option<FillRule>,
    stroke: Option<StrokeStyle>,
    top: NodeId,
    z: u64,
}

#[derive(Debug, Default)]
struct Built {
    index: HitIndex<NodeId>,
    leaves: HashMap<NodeId, Leaf>,
}

/// The pick index of one document, built lazily.
///
/// Holds no reference to the document: the session calls
/// [`Picker::invalidate`] after every mutation, and the next pick rebuilds.
#[derive(Debug, Default)]
pub struct Picker {
    built: RefCell<Option<Built>>,
}

fn half_width(stack: &AttrStack) -> Mp {
    match stack.get(AttrSlot::LineWidth) {
        AttrValue::LineWidth(w) => Mp::new(w.raw() / 2),
        _ => Mp::ZERO,
    }
}

fn stroke_style(attrs: &AttrStack) -> StrokeStyle {
    let width = match attrs.get(AttrSlot::LineWidth) {
        AttrValue::LineWidth(w) => *w,
        _ => Mp::ZERO,
    };
    let cap = match attrs.get(AttrSlot::StartCap) {
        AttrValue::LineCap(c) => *c,
        _ => Cap::Butt,
    };
    let join = match attrs.get(AttrSlot::JoinType) {
        AttrValue::JoinType(j) => *j,
        _ => Join::Mitre,
    };
    let mitre_limit = match attrs.get(AttrSlot::MitreLimit) {
        AttrValue::MitreLimit(m) => (m.to_f64() / f64::from(Mp::PER_PT)).max(1.0),
        _ => 4.0,
    };
    let dash = match attrs.get(AttrSlot::DashPattern) {
        AttrValue::DashPattern(d) if !d.elements.is_empty() => Some((**d).clone()),
        _ => None,
    };
    StrokeStyle {
        width,
        cap_start: cap,
        cap_end: cap,
        join,
        mitre_limit,
        dash,
    }
}

fn parallelogram(origin: DocPoint, major: xarast_geom::Vector, minor: xarast_geom::Vector) -> Path {
    let mut b = Path::builder();
    b.move_to(origin)
        .line_to(origin + major)
        .line_to(origin + major + minor)
        .line_to(origin + minor)
        .close();
    b.build()
}

/// The geometry a node paints, in document space.
pub(crate) fn geometry_of(kind: &NodeKind) -> Option<Arc<Path>> {
    match kind {
        NodeKind::Path(p) => Some(Arc::clone(&p.data)),
        NodeKind::QuickShape(q) => q.path.clone(),
        NodeKind::Shape(s) => Some(Arc::new(match s.shape {
            xarast_doc::ShapeKind::Rect => parallelogram(s.origin, s.major, s.minor),
            xarast_doc::ShapeKind::Ellipse => {
                let half = |v: xarast_geom::Vector| {
                    xarast_geom::Vector::new(Mp::new(v.dx.raw() / 2), Mp::new(v.dy.raw() / 2))
                };
                let (hm, hn) = (half(s.major), half(s.minor));
                xarast_geom::regular_shape_outline(&xarast_geom::RegularShapeSpec {
                    sides: 4,
                    circular: true,
                    stellated: false,
                    primary_curved: false,
                    stellation_curved: false,
                    centre: s.origin + hm + hn,
                    major: hm,
                    minor: hn,
                    stellation_radius: 1.0,
                    stellation_offset: 0.0,
                    primary_curvature: 0.0,
                    stellation_curvature: 0.0,
                    primary_edge: None,
                    secondary_edge: None,
                })?
            }
        })),
        NodeKind::Bitmap(bm) => Some(Arc::new(parallelogram(bm.origin, bm.major, bm.minor))),
        _ => None,
    }
}

/// Whether a colour attribute paints anything.
fn paints(doc: &Document, paint: &xarast_doc::fill::Paint) -> bool {
    match paint {
        xarast_doc::fill::FillGeometry::Flat { value } => {
            value.resolve(&doc.resources.colours).to_rgba8().a != 0
        }
        _ => true,
    }
}

/// A node with ink of its own (not a container, not an attribute).
fn own_ink(kind: Option<&NodeKind>) -> bool {
    !matches!(
        kind,
        None | Some(
            NodeKind::Attr(_)
                | NodeKind::Document(_)
                | NodeKind::Chapter
                | NodeKind::Spread(_)
                | NodeKind::Page(_)
                | NodeKind::Grid(_)
                | NodeKind::Layer(_)
                | NodeKind::Group(_)
                | NodeKind::Live(_)
                | NodeKind::ClipView(_)
                | NodeKind::TextStory(_)
                | NodeKind::TextLine(_)
                | NodeKind::Guideline(_)
                | NodeKind::Opaque(_)
        )
    )
}

fn editable_layer(l: &xarast_doc::LayerNode) -> bool {
    l.visible && !l.locked && !l.guide
}

impl Built {
    fn build(doc: &Document) -> Built {
        let tree = &doc.tree;
        let mut rank: u64 = 0;
        let mut entries: Vec<(NodeId, DocRect, u64)> = Vec::new();
        let mut leaves: HashMap<NodeId, Leaf> = HashMap::new();
        let layers: Vec<NodeId> = tree
            .preorder(tree.root())
            .filter(|id| matches!(tree.kind(*id), Some(NodeKind::Layer(l)) if editable_layer(l)))
            .collect();
        for layer in layers {
            let mut stack = AttrStack::with_defaults(&doc.defaults);
            // The layer's child on the path to the current node.
            let top_of = |node: NodeId| -> NodeId {
                std::iter::once(node)
                    .chain(tree.ancestors(node))
                    .find(|n| tree.links(*n).parent == Some(layer))
                    .unwrap_or(node)
            };
            let mut with_leaf: HashSet<NodeId> = HashSet::new();
            let mut add = |node: NodeId, stack: &AttrStack, rank: &mut u64| {
                let Some(kind) = tree.kind(node) else { return };
                let Some(path) = geometry_of(kind) else {
                    return;
                };
                let (filled, stroked) = match kind {
                    NodeKind::Path(p) => (p.filled, p.stroked),
                    NodeKind::Bitmap(_) => (true, false),
                    _ => (true, true),
                };
                let bitmap = matches!(kind, NodeKind::Bitmap(_));
                let fill = (filled
                    && (bitmap
                        || matches!(stack.get(AttrSlot::FillGeometry),
                            AttrValue::Fill(p) if paints(doc, p))))
                .then(|| match stack.get(AttrSlot::WindingRule) {
                    AttrValue::WindingRule(r) => *r,
                    _ => FillRule::NonZero,
                });
                let stroke = (stroked
                    && matches!(stack.get(AttrSlot::StrokeColour),
                        AttrValue::StrokeColour(p) if paints(doc, p)))
                .then(|| stroke_style(stack));
                if fill.is_none() && stroke.is_none() {
                    return;
                }
                let reach = stroke.as_ref().map_or(Mp::ZERO, |s| {
                    Mp::from_f64_round(half_width(stack).to_f64() * s.mitre_limit.max(1.0))
                });
                let bounds = xarast_doc::bounds::compute_bounds_with(tree, node, reach);
                *rank += 1;
                let z = *rank << 16;
                let top = top_of(node);
                with_leaf.insert(top);
                entries.push((node, bounds, z));
                leaves.insert(
                    node,
                    Leaf {
                        geometry: Geometry::Path(path),
                        fill,
                        stroke,
                        top,
                        z,
                    },
                );
            };
            for ev in tree.walk_render(layer) {
                match ev {
                    WalkEvent::EnterScope { .. } => stack.push_scope(),
                    WalkEvent::Visit { node } => match tree.kind(node) {
                        Some(NodeKind::Attr(a)) => stack.push(Arc::new(a.value.clone())),
                        k if own_ink(k) && tree.links(node).first_child.is_none() => {
                            add(node, &stack, &mut rank);
                        }
                        _ => {}
                    },
                    WalkEvent::LeaveScope { parent } => {
                        if own_ink(tree.kind(parent)) {
                            add(parent, &stack, &mut rank);
                        }
                        stack.pop_scope();
                    }
                }
            }
            // Objects with nothing testable stay selectable by their box.
            for top in tree.children(layer) {
                let ink = tree.kind(top).is_some_and(|k| k.is_ink() && !k.is_attr());
                if !ink || with_leaf.contains(&top) {
                    continue;
                }
                let bounds = crate::viewport::nodes_rect(doc, [top]);
                if bounds.is_empty() {
                    continue;
                }
                rank += 1;
                let z = rank << 16;
                entries.push((top, bounds, z));
                leaves.insert(
                    top,
                    Leaf {
                        geometry: Geometry::Bounds,
                        fill: None,
                        stroke: None,
                        top,
                        z,
                    },
                );
            }
        }
        Built {
            index: HitIndex::from_entries(entries),
            leaves,
        }
    }

    fn hit(&self, leaf: &Leaf, p: DocPoint, tol: HitTolerance) -> Option<HitPart> {
        match &leaf.geometry {
            Geometry::Bounds => Some(HitPart::Bounds),
            Geometry::Path(path) => HitShape {
                path,
                transform: Matrix::IDENTITY,
                fill: leaf.fill,
                stroke: leaf.stroke.as_ref(),
            }
            .hit(p, tol)
            .map(|h| match h {
                ShapeHit::Fill => HitPart::Fill,
                ShapeHit::Stroke => HitPart::Stroke,
            }),
        }
    }
}

impl Picker {
    /// An empty picker; the first pick builds the index.
    #[must_use]
    pub fn new() -> Picker {
        Picker::default()
    }

    /// The document changed: the next pick rebuilds.
    pub fn invalidate(&mut self) {
        *self.built.get_mut() = None;
    }

    fn with<R>(&self, doc: &Document, f: impl FnOnce(&Built) -> R) -> R {
        let mut slot = self.built.borrow_mut();
        let built = slot.get_or_insert_with(|| Built::build(doc));
        f(built)
    }

    /// The object under `at`, within `radius_px` device pixels at
    /// `mp_per_px` millipoints a pixel.
    #[must_use]
    pub fn pick(
        &self,
        doc: &Document,
        at: DocPoint,
        radius_px: f64,
        mp_per_px: f64,
        mode: PickMode,
    ) -> Option<HitResult> {
        let tol = HitTolerance::from_device(radius_px, mp_per_px);
        let radius = Mp::from_f64_round(tol.radius + tol.min_stroke_width);
        self.with(doc, |b| {
            let below_z = match mode {
                PickMode::Under { below } => b
                    .leaves
                    .values()
                    .filter(|l| l.top == below)
                    .map(|l| l.z)
                    .min(),
                _ => None,
            };
            b.index.candidates_at(at, radius).find_map(|(node, z)| {
                if below_z.is_some_and(|bz| z >= bz) {
                    return None;
                }
                let leaf = b.leaves.get(&node)?;
                let part = b.hit(leaf, at, tol)?;
                Some(HitResult {
                    node,
                    top_group: if mode == PickMode::Leaf {
                        node
                    } else {
                        leaf.top
                    },
                    part,
                })
            })
        })
    }

    /// The objects a marquee over `rect` selects: every top-level object
    /// whose bounds lie inside it, in paint order.
    #[must_use]
    pub fn enclosed(&self, doc: &Document, rect: DocRect) -> Vec<NodeId> {
        let mut leaves = Vec::new();
        let tops: HashSet<NodeId> = self.with(doc, |b| {
            b.index.query_rect(rect, RectMode::Touch, &mut leaves);
            leaves
                .iter()
                .filter_map(|l| b.leaves.get(l).map(|x| x.top))
                .collect()
        });
        crate::edit::selectable_objects(doc)
            .filter(|t| tops.contains(t))
            .filter(|t| {
                let b = crate::viewport::nodes_rect(doc, [*t]);
                !b.is_empty() && rect.contains_rect(b)
            })
            .collect()
    }
}
