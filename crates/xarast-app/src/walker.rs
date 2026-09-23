//! The arena→[`Scene`] walker.
//!
//! This is the seam the architecture puts in this crate on purpose
//! (`10-architecture.md` §2): `xarast-render` must never depend on
//! `xarast-doc`, so the one component that knows about both lives here.
//! Everything below it — the display list, the tiler, the backends — is
//! testable from hand-built scenes with no document present, and that is
//! what the separation buys.
//!
//! # What the walk guarantees
//!
//! 1. **It never mutates the document.** It takes `&Document` and nothing
//!    else. Bounds are read from the cache where it is warm and computed
//!    on the stack where it is not; the cache is never filled here,
//!    because filling it would make rendering an edit.
//! 2. **Paint order is tree order.** The walk is `Tree::walk_render`, and
//!    the scene's op list is in the order the original paints.
//! 3. **The attribute stack is pushed and popped exactly once per
//!    scope.** `EnterScope` pushes, `LeaveScope` pops, an attribute node
//!    pushes a value into the current scope — which is the protocol
//!    `xarast_doc::walk` documents.
//! 4. **An ink node is painted at `LeaveScope` when it has children and
//!    at `Visit` when it has none.** A `.xar` path stores its fill as its
//!    own child, and a parent paints after its children, so the child
//!    scope must still be in force when the parent's ink is emitted. The
//!    walk emits scope events only for nodes that have children, so the
//!    two cases never overlap.
//! 5. **It is output-deterministic.** The same document, edit state and
//!    viewport produce a byte-identical scene, which is what the corpus
//!    stability test asserts.
//!
//! # Culling
//!
//! A dirty rectangle in device space is mapped back into document space
//! and every subtree whose bounding box misses it is pruned with
//! `Descend::Skip`. Pruning a *subtree* is safe even though attributes
//! scope across siblings: an attribute node has no bounding box and is
//! never pruned, and a pruned node's own attribute children only ever
//! applied to that node.

use std::collections::HashMap;
use std::sync::Arc;

use xarast_doc::fill::Tiling;
use xarast_doc::resources::BitmapId;
use xarast_doc::{AttrSlot, AttrStack, AttrValue, Document, Epoch, NodeId, NodeKind, WalkEvent};
use xarast_geom::{Cap, FillRule, Join, Mp, Path, Point, Rect, StrokeStyle, Vector};
use xarast_render::{
    CacheHint, ContentHash, DeviceRect, ImageId, ImageRef, PathRef, RenderQuality, Resolver, Scene,
    SceneBuilder, SceneError, SceneNodeId, SceneStats, Transparency,
};

use crate::edit::EditState;
use crate::paint::PaintCtx;
use crate::viewport::Viewport;

/// What one walk found that the user may want to know about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WalkStats {
    /// Nodes the walk visited.
    pub visited: usize,
    /// Subtrees pruned because they missed the dirty rectangle.
    pub culled: usize,
    /// Objects whose bitmap resource has not been decoded yet, so they
    /// were left out. Decoding is Phase 10.
    pub images_pending: usize,
    /// `ClipView` nodes whose "keep the outside" mode the renderer cannot
    /// express yet, so the clip was dropped.
    pub clips_unsupported: usize,
    /// Text nodes skipped: shaping is Phase 9.
    pub text_pending: usize,
    /// Live effects skipped: regeneration is Phase 13.
    pub live_pending: usize,
    /// Quick shapes — stars and polygons — with no cached path, so
    /// nothing to draw. Generating a path from the parameters is
    /// Phase 7.
    pub shapes_pending: usize,
}

impl WalkStats {
    /// Whether the walk produced a complete picture of the document.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.images_pending == 0
            && self.clips_unsupported == 0
            && self.text_pending == 0
            && self.live_pending == 0
            && self.shapes_pending == 0
    }
}

/// Builds scenes from documents.
///
/// Hold one per view and reuse it: it carries the interned ramps, the
/// registered images and a small attribute cache across frames, all of
/// which are pure caches — dropping the walker changes no output.
#[derive(Debug, Default)]
pub struct SceneWalker {
    resolver: Resolver,
    images: HashMap<BitmapId, ImageId>,
    attr_cache: HashMap<NodeId, Arc<AttrValue>>,
    attr_epoch: Epoch,
    stats: WalkStats,
    scene_stats: SceneStats,
}

/// What a scope opened in the scene, so that `LeaveScope` can close it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Frame {
    node: NodeId,
    group: bool,
    clip: bool,
    /// The child that supplied the clipping path and must not be painted.
    clip_child: Option<NodeId>,
}

impl SceneWalker {
    /// A walker with empty caches.
    #[must_use]
    pub fn new() -> SceneWalker {
        SceneWalker::default()
    }

    /// The ramps and images the last scene refers to.
    ///
    /// A [`Scene`] is not self-contained: its paints hold ids into this
    /// table. Hand both to the backend, or the gradients come out
    /// transparent.
    #[must_use]
    pub const fn resolver(&self) -> &Resolver {
        &self.resolver
    }

    /// Takes the resolver, leaving the walker empty.
    ///
    /// For a caller that builds one scene and then drops the walker.
    #[must_use]
    pub fn into_resolver(self) -> Resolver {
        self.resolver
    }

    /// What the last walk found.
    #[must_use]
    pub const fn stats(&self) -> WalkStats {
        self.stats
    }

    /// What the last scene contains.
    #[must_use]
    pub const fn scene_stats(&self) -> SceneStats {
        self.scene_stats
    }

    /// Forgets every cache. Call it when the document is replaced.
    pub fn reset(&mut self) {
        self.resolver = Resolver::new();
        self.images.clear();
        self.attr_cache.clear();
        self.attr_epoch = Epoch::default();
    }

    /// Walks the document into `scene`.
    ///
    /// `dirty` is a device-space rectangle; `None` means the whole
    /// viewport. The scene is cleared first, so a rebuild is always a
    /// whole scene — the dirty rectangle prunes the *walk*, not the
    /// output's validity.
    ///
    /// # Errors
    ///
    /// [`SceneError`] if the recording came out unbalanced, which would
    /// be a bug in this walker rather than in the document.
    pub fn rebuild(
        &mut self,
        doc: &Document,
        edit: &EditState,
        vp: &Viewport,
        quality: RenderQuality,
        dirty: Option<DeviceRect>,
        scene: &mut Scene,
    ) -> Result<SceneStats, SceneError> {
        self.sync_caches(doc);
        self.stats = WalkStats::default();

        let clip = dirty.map(|d| doc_rect_of(vp, d));
        let mut b = SceneBuilder::begin(scene, quality);
        let mut attrs = AttrStack::with_defaults(&doc.defaults);
        let mut frames: Vec<Frame> = Vec::new();

        let root = doc.tree.root();
        let mut walk = doc.tree.walk_render(root);
        while let Some(ev) = walk.next() {
            match ev {
                WalkEvent::Visit { node } => {
                    self.stats.visited += 1;
                    if let Some(frame) = frames.last()
                        && frame.clip_child == Some(node)
                    {
                        // The clipping path is geometry, not ink.
                        walk.control(xarast_doc::Descend::Skip);
                        continue;
                    }
                    let Some(kind) = doc.tree.kind(node) else {
                        walk.control(xarast_doc::Descend::Skip);
                        continue;
                    };
                    match kind {
                        NodeKind::Attr(a) => {
                            attrs.push(self.attr_value(node, a));
                            continue;
                        }
                        NodeKind::Opaque(_) | NodeKind::Guideline(_) => {
                            walk.control(xarast_doc::Descend::Skip);
                            continue;
                        }
                        NodeKind::Layer(l) => {
                            if !l.visible || l.guide {
                                walk.control(xarast_doc::Descend::Skip);
                            }
                            continue;
                        }
                        _ => {}
                    }
                    if let Some(c) = clip
                        && is_culled(doc, node, c, &attrs)
                    {
                        self.stats.culled += 1;
                        walk.control(xarast_doc::Descend::Skip);
                        continue;
                    }
                    if doc.tree.links(node).first_child.is_none() {
                        self.paint(doc, edit, node, &attrs, quality, &mut b);
                    }
                }
                WalkEvent::EnterScope { parent } => {
                    attrs.push_scope();
                    frames.push(self.open(doc, parent, &attrs, &mut b));
                }
                WalkEvent::LeaveScope { parent } => {
                    self.paint(doc, edit, parent, &attrs, quality, &mut b);
                    if let Some(f) = frames.pop() {
                        debug_assert_eq!(f.node, parent);
                        if f.clip {
                            b.pop_clip();
                        }
                        if f.group {
                            b.pop_group();
                        }
                    }
                    attrs.pop_scope();
                }
            }
        }

        let stats = b.finish()?;
        self.scene_stats = stats;
        Ok(stats)
    }

    /// Drops the caches that a document change invalidated.
    fn sync_caches(&mut self, doc: &Document) {
        if self.attr_epoch != doc.epoch {
            self.attr_cache.clear();
            self.attr_epoch = doc.epoch;
        }
        self.register_images(doc);
    }

    /// Registers every decoded bitmap with the renderer.
    ///
    /// The `.xar` importer keeps a bitmap's encoded bytes and leaves
    /// `pixels` empty until Phase 10 decodes them, so most documents
    /// register nothing today. The seam is here and not in the walk
    /// itself so that the registry is built once per frame rather than
    /// once per object.
    fn register_images(&mut self, doc: &Document) {
        for (id, res) in doc.resources.bitmaps() {
            if self.images.contains_key(&id) {
                continue;
            }
            let (w, h) = (res.info.width, res.info.height);
            let expected = w as usize * h as usize * 4;
            if expected == 0 || res.pixels.pixels.len() != expected {
                continue;
            }
            let image = ImageRef::new(w, h, res.pixels.pixels.to_vec());
            let rid = self.resolver.images.insert(image);
            self.images.insert(id, rid);
        }
    }

    fn attr_value(&mut self, node: NodeId, a: &xarast_doc::AttrNode) -> Arc<AttrValue> {
        self.attr_cache
            .entry(node)
            .or_insert_with(|| Arc::new(a.value.clone()))
            .clone()
    }

    /// Opens whatever scene construct a node's child list needs.
    fn open(
        &mut self,
        doc: &Document,
        node: NodeId,
        attrs: &AttrStack,
        b: &mut SceneBuilder<'_>,
    ) -> Frame {
        let mut f = Frame {
            node,
            group: false,
            clip: false,
            clip_child: None,
        };
        match doc.tree.kind(node) {
            Some(NodeKind::Group(_)) => {
                b.push_group(
                    scene_id(doc, node),
                    xarast_render::Transform2D::IDENTITY,
                    CacheHint::Auto,
                );
                f.group = true;
            }
            Some(NodeKind::ClipView(cv)) => {
                if cv.mode == xarast_doc::ClipViewMode::Outside {
                    self.stats.clips_unsupported += 1;
                } else if let Some(child) = doc.tree.links(node).first_child {
                    if let Some(path) = geometry_of(doc, child) {
                        b.push_clip(&path, winding(attrs));
                        f.clip = true;
                        f.clip_child = Some(child);
                    } else {
                        self.stats.clips_unsupported += 1;
                    }
                }
                b.push_group(
                    scene_id(doc, node),
                    xarast_render::Transform2D::IDENTITY,
                    CacheHint::Auto,
                );
                f.group = true;
            }
            _ => {}
        }
        f
    }

    /// Emits one ink node's fill and stroke.
    fn paint(
        &mut self,
        doc: &Document,
        edit: &EditState,
        node: NodeId,
        attrs: &AttrStack,
        quality: RenderQuality,
        b: &mut SceneBuilder<'_>,
    ) {
        let _ = edit;
        let Some(kind) = doc.tree.kind(node) else {
            return;
        };
        match kind {
            NodeKind::Live(_) => {
                self.stats.live_pending += 1;
                return;
            }
            NodeKind::TextStory(_) | NodeKind::TextLine(_) | NodeKind::TextItem(_) => {
                self.stats.text_pending += 1;
                return;
            }
            NodeKind::QuickShape(q) if q.path.is_none() => {
                self.shapes_pending_inc();
                return;
            }
            NodeKind::Bitmap(bm) => {
                self.paint_bitmap(doc, node, bm, attrs, quality, b);
                return;
            }
            _ => {}
        }
        let Some(path) = geometry_of(doc, node) else {
            return;
        };
        let (filled, stroked) = match kind {
            NodeKind::Path(p) => (p.filled, p.stroked),
            _ => (true, true),
        };
        let id = scene_id(doc, node);
        // A bitmap *fill* whose image is not decoded paints nothing, like a
        // bitmap node; say so rather than drop it silently
        // (`Designs/leafgirl.xar`'s figure is 350 such paths).
        if filled
            && let AttrValue::Fill(xarast_doc::fill::FillGeometry::Bitmap { image, .. }) =
                attrs.get(AttrSlot::FillGeometry)
            && !self.images.contains_key(image)
        {
            self.stats.images_pending += 1;
        }
        let mut ctx = PaintCtx {
            colours: &doc.resources.colours,
            ramp_length: quality.ramp_length(),
            filter: quality.image_filter(),
            resolver: &mut self.resolver,
            images: &self.images,
        };

        if filled
            && let AttrValue::Fill(fill) = attrs.get(AttrSlot::FillGeometry)
            && let Some(paint) = crate::paint::colour_paint(
                fill,
                tiling(attrs, AttrSlot::FillMapping),
                effect(attrs),
                &mut ctx,
            )
        {
            let t = object_transparency(attrs, AttrSlot::TranspFillGeometry, &mut ctx);
            emit(b, t, |b| b.fill(id, &path, winding(attrs), paint.clone()));
        }

        if stroked
            && let AttrValue::StrokeColour(stroke) = attrs.get(AttrSlot::StrokeColour)
            && let Some(paint) =
                crate::paint::colour_paint(stroke, Tiling::None, effect(attrs), &mut ctx)
        {
            let style = stroke_style(attrs);
            let t = object_transparency(attrs, AttrSlot::StrokeTransp, &mut ctx);
            emit(b, t, |b| b.stroke(id, &path, style.clone(), paint.clone()));
        }

        b.finish_node(id, content_hash(doc, node));
    }

    fn shapes_pending_inc(&mut self) {
        self.stats.shapes_pending += 1;
    }

    fn paint_bitmap(
        &mut self,
        doc: &Document,
        node: NodeId,
        bm: &xarast_doc::BitmapNode,
        attrs: &AttrStack,
        quality: RenderQuality,
        b: &mut SceneBuilder<'_>,
    ) {
        let Some(image) = self.images.get(&bm.image).copied() else {
            self.stats.images_pending += 1;
            return;
        };
        let mapping = xarast_render::GradMapping::Affine {
            a: point64(bm.origin),
            b: point64(bm.origin + bm.minor),
            c: point64(bm.origin + bm.major),
        };
        let paint = xarast_render::Paint::Image {
            image,
            mapping,
            repeat: xarast_render::Repeat::Simple,
            filter: quality.image_filter(),
            contone: None,
            adjust: xarast_render::BitmapAdjust::default(),
        };
        let mut ctx = PaintCtx {
            colours: &doc.resources.colours,
            ramp_length: quality.ramp_length(),
            filter: quality.image_filter(),
            resolver: &mut self.resolver,
            images: &self.images,
        };
        let t = object_transparency(attrs, AttrSlot::TranspFillGeometry, &mut ctx);
        let id = scene_id(doc, node);
        emit(b, t, |b| b.image(id, image, mapping, paint.clone()));
        b.finish_node(id, content_hash(doc, node));
    }
}

/// Wraps one emission in its transparency scope, and only when the
/// transparency does something: `push`/`pop` around every opaque object
/// would double the op count for nothing.
fn emit<F: FnOnce(&mut SceneBuilder<'_>)>(b: &mut SceneBuilder<'_>, t: Transparency, f: F) {
    if t.is_opaque() {
        f(b);
    } else {
        b.push_transparency(t);
        f(b);
        b.pop_transparency();
    }
}

fn object_transparency(attrs: &AttrStack, slot: AttrSlot, ctx: &mut PaintCtx<'_>) -> Transparency {
    match attrs.get(slot) {
        AttrValue::TranspFill(t) | AttrValue::StrokeTransp(t) => {
            crate::paint::transparency(t, tiling(attrs, AttrSlot::TranspFillMapping), ctx)
        }
        _ => Transparency::OPAQUE,
    }
}

fn tiling(attrs: &AttrStack, slot: AttrSlot) -> Tiling {
    match attrs.get(slot) {
        AttrValue::FillMapping(t) | AttrValue::TranspFillMapping(t) => *t,
        _ => Tiling::None,
    }
}

fn effect(attrs: &AttrStack) -> xarast_color::FillEffect {
    match attrs.get(AttrSlot::FillEffect) {
        AttrValue::FillEffect(e) => *e,
        _ => xarast_color::FillEffect::Fade,
    }
}

fn winding(attrs: &AttrStack) -> FillRule {
    match attrs.get(AttrSlot::WindingRule) {
        AttrValue::WindingRule(r) => *r,
        _ => FillRule::NonZero,
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

/// The geometry a node paints, in document space.
///
/// `QuickShape` uses the path the importer cached; generating one from
/// the parameters is Phase 7, and until then a quick shape with no cached
/// path draws nothing rather than drawing something wrong.
fn geometry_of(doc: &Document, node: NodeId) -> Option<PathRef> {
    match doc.tree.kind(node)? {
        NodeKind::Path(p) => Some(PathRef::from_arc(Arc::clone(&p.data))),
        NodeKind::Shape(s) => Some(PathRef::new(match s.shape {
            xarast_doc::ShapeKind::Rect => parallelogram_path(s.origin, s.major, s.minor),
            xarast_doc::ShapeKind::Ellipse => ellipse_path(s.origin, s.major, s.minor),
        })),
        NodeKind::QuickShape(q) => q.path.as_ref().map(|p| PathRef::from_arc(Arc::clone(p))),
        _ => None,
    }
}

fn parallelogram_path(origin: Point, major: Vector, minor: Vector) -> Path {
    let mut b = Path::builder();
    b.move_to(origin)
        .line_to(origin + major)
        .line_to(origin + major + minor)
        .line_to(origin + minor)
        .close();
    b.build()
}

/// The ellipse inscribed in the parallelogram, as four cubic arcs.
///
/// The magic constant is the usual circular-arc approximation: a Bézier
/// control point at `4/3 · (√2 − 1)` of the radius reproduces a quarter
/// circle to within 0.03 %. Applying it in the parallelogram's own frame
/// makes rotation and shear exact, which is the whole reason the model
/// stores an ellipse this way.
fn ellipse_path(origin: Point, major: Vector, minor: Vector) -> Path {
    const K: f64 = 0.552_284_749_830_793_4;
    let u = vscale(major, 0.5);
    let v = vscale(minor, 0.5);
    let centre = origin + u + v;
    let at = |su: f64, sv: f64| centre + vscale(u, su) + vscale(v, sv);
    let mut b = Path::builder();
    b.move_to(at(1.0, 0.0));
    b.cubic_to(at(1.0, K), at(K, 1.0), at(0.0, 1.0));
    b.cubic_to(at(-K, 1.0), at(-1.0, K), at(-1.0, 0.0));
    b.cubic_to(at(-1.0, -K), at(-K, -1.0), at(0.0, -1.0));
    b.cubic_to(at(K, -1.0), at(1.0, -K), at(1.0, 0.0));
    b.close();
    b.build()
}

/// A stable scene id for a node.
///
/// It is the node's [`Tag`](xarast_doc::Tag), not its [`NodeId`]: `Tag` is
/// unique within a document, stable across save and reload, and
/// deterministic, which is what makes two walks of the same document
/// produce byte-identical scenes. A `NodeId` is an arena slot and would
/// make the scene depend on allocation order.
fn scene_id(doc: &Document, node: NodeId) -> SceneNodeId {
    SceneNodeId(u64::from(doc.tree.get(node).map_or(0, |d| d.tag.0)))
}

/// A content hash that changes when the node's content may have changed.
///
/// It is deliberately coarse: the node's tag plus the document's epoch,
/// which is bumped by every committed transaction. A finer hash is worth
/// having once the per-node render cache is driven from this crate
/// (Phase 7); a coarse one is correct meanwhile because it only ever
/// over-invalidates.
fn content_hash(doc: &Document, node: NodeId) -> ContentHash {
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&u64::from(doc.tree.get(node).map_or(0, |d| d.tag.0)).to_le_bytes());
    bytes[8..].copy_from_slice(&doc.epoch.0.to_le_bytes());
    ContentHash(bytes)
}

/// Scales a displacement by a factor, saturating.
fn vscale(v: Vector, f: f64) -> Vector {
    Vector::new(v.dx.scale(f), v.dy.scale(f))
}

fn point64(p: Point) -> xarast_render::Point64 {
    let (x, y) = p.to_f64();
    xarast_render::Point64::new(x, y)
}

/// Whether a subtree can be skipped because it misses the dirty
/// rectangle.
///
/// The bounding box is read from the cache when it is warm and computed
/// from the geometry when it is not; neither path writes to the document.
/// A node with no bounds at all — a group whose children are all text, a
/// live effect with nothing generated — is never culled, because "no
/// bounds" and "empty bounds" are the same value and guessing wrong loses
/// pixels.
fn is_culled(doc: &Document, node: NodeId, clip: Rect, attrs: &AttrStack) -> bool {
    let b = match doc.tree.bounds(node).get() {
        Some(b) => b,
        None => xarast_doc::bounds::compute_bounds_with(&doc.tree, node, attrs.stroke_extent()),
    };
    !b.is_empty() && !b.intersects(clip)
}

/// A device rectangle mapped back into document space, rounded outwards.
fn doc_rect_of(vp: &Viewport, r: DeviceRect) -> Rect {
    use crate::geometry::{DevicePoint, DocPointF64Ext};
    if r.is_empty() {
        return Rect::EMPTY;
    }
    let corners = [
        vp.device_to_doc_f64(DevicePoint::new(f64::from(r.x0), f64::from(r.y0))),
        vp.device_to_doc_f64(DevicePoint::new(f64::from(r.x1), f64::from(r.y0))),
        vp.device_to_doc_f64(DevicePoint::new(f64::from(r.x0), f64::from(r.y1))),
        vp.device_to_doc_f64(DevicePoint::new(f64::from(r.x1), f64::from(r.y1))),
    ];
    let mut out = Rect::from_point(corners[0].to_doc_point());
    for c in &corners[1..] {
        out = out.union_point(c.to_doc_point());
    }
    out
}
