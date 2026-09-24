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

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use xarast_doc::fill::Tiling;
use xarast_doc::resources::BitmapId;
use xarast_doc::{
    AttrSlot, AttrStack, AttrValue, Document, Epoch, NodeId, NodeKind, ResolvedAttrs, StoryText,
    WalkEvent,
};
use xarast_geom::{Cap, FillRule, Join, Mp, Path, Point, Rect, StrokeStyle, Vector};
use xarast_render::{
    CacheHint, ContentHash, DeviceRect, ImageId, ImageRef, PathRef, RenderQuality, Resolver, Scene,
    SceneBuilder, SceneError, SceneNodeId, SceneStats, Transparency,
};
use xarast_text::FontSubstitution;

use crate::edit::EditState;
use crate::fonts::FontService;
use crate::paint::PaintCtx;
use crate::text::StoryGeometry;
use crate::tool::Preview;
use crate::viewport::Viewport;

/// What one walk found that the user may want to know about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WalkStats {
    /// Nodes the walk visited.
    pub visited: usize,
    /// Subtrees pruned because they missed the dirty rectangle.
    pub culled: usize,
    /// Objects whose bitmap resource has neither pixels nor encoded bytes
    /// the walker could decode (a `.xar` read with `skip_bitmaps`, say), so
    /// they were left out.
    pub images_pending: usize,
    /// Objects whose bitmap resource failed to decode — corrupt, refused
    /// by the decode limits, or in a format we do not read — so they were
    /// left out. The failure is cached: it is not retried every frame.
    pub images_failed: usize,
    /// `ClipView` nodes whose "keep the outside" mode the renderer cannot
    /// express yet, so the clip was dropped.
    pub clips_unsupported: usize,
    /// Text stories drawn.
    pub text_stories: usize,
    /// Text that could not be drawn: a story whose visible characters found
    /// no font at all, or a line or item outside any story.
    pub text_pending: usize,
    /// Stories on a path drawn along a straight baseline because their
    /// path is missing, has no length or cannot be brought into story
    /// space (a singular story matrix). Every other story on a path
    /// follows it (W9.5).
    pub text_on_path_pending: usize,
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
            && self.images_failed == 0
            && self.clips_unsupported == 0
            && self.text_pending == 0
            && self.text_on_path_pending == 0
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
    /// Whether the node being painted is one a gesture previews: its
    /// ramps are built at draft length (256 entries) whatever the quality,
    /// since a new one is made every frame (`phase-08` T8.5.3). The frame
    /// after the release walks it at the session's quality again.
    draft_ramps: bool,
    images: HashMap<BitmapId, ImageId>,
    /// Bitmaps whose decode failed: the negative cache, so that a bad
    /// bitmap costs one decode per walker, not one per frame.
    failed: HashSet<BitmapId>,
    attr_cache: HashMap<NodeId, Arc<AttrValue>>,
    attr_epoch: Epoch,
    stats: WalkStats,
    scene_stats: SceneStats,
    /// The fonts stories are laid out with; the process's shared service
    /// unless [`SceneWalker::with_fonts`] chose one.
    fonts: Option<Arc<FontService>>,
    /// Laid-out stories by node, dropped when the document's epoch moves:
    /// the derived text cache, outside the arena.
    stories: HashMap<NodeId, Arc<StoryGeometry>>,
    /// Every font substitution any walk made, in the order first seen.
    substitutions: Vec<FontSubstitution>,
    /// The union of the text the last walk drew, document space: text has
    /// no cached bounds, so whoever needs the ink extent adds this.
    text_ink: Rect,
    /// The text runs the last walk painted, with their glyphs: what an
    /// exporter embedding fonts needs besides the outlines.
    painted_text: Vec<xarast_io::TextRun>,
    /// The attribute-scope fingerprint in force at the node being painted:
    /// a fold of the tag and content revision of every attribute node
    /// pushed in the enclosing scopes, in order. See [`content_hash`].
    scope: u64,
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

    /// A walker that lays text out with `fonts` rather than the process's
    /// shared service: tests and golden renders pass pinned fonts.
    #[must_use]
    pub fn with_fonts(fonts: Arc<FontService>) -> SceneWalker {
        SceneWalker {
            fonts: Some(fonts),
            ..SceneWalker::default()
        }
    }

    /// Every font substitution the walks so far made, each once, in the
    /// order first seen. The UI reports them; they are never written back
    /// into the document.
    #[must_use]
    pub fn font_substitutions(&self) -> &[FontSubstitution] {
        &self.substitutions
    }

    /// The text the last walk painted: each run's outline path (the one in
    /// the scene's ops) with the glyphs it is made of, and the font
    /// database they come from. `None` when the walk drew no text.
    #[must_use]
    pub fn scene_text(&self) -> Option<xarast_io::SceneText> {
        if self.painted_text.is_empty() {
            return None;
        }
        let fonts = self.fonts.as_ref()?;
        Some(xarast_io::SceneText {
            fonts: Arc::clone(fonts.db()),
            runs: self.painted_text.clone(),
        })
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

    /// The document-space box of the text the last walk drew (empty when
    /// it drew none). Text stories have no cached bounds, so a caller that
    /// needs the drawing's extent unions this in.
    #[must_use]
    pub const fn text_ink(&self) -> Rect {
        self.text_ink
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
        self.failed.clear();
        self.attr_cache.clear();
        self.attr_epoch = Epoch::default();
        // Substitutions are kept: they are a history the UI reports once,
        // not a cache.
        self.stories.clear();
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
        self.rebuild_previewed(doc, edit, vp, quality, dirty, &Preview::default(), scene)
    }

    /// [`SceneWalker::rebuild`] with a tool's live [`Preview`] applied.
    ///
    /// A previewed node is wrapped in a scene group carrying the preview's
    /// transform, so the document is never touched and the display list
    /// moves the node's paths, fills and strokes as one. Hidden nodes are
    /// skipped with their subtrees. With a non-empty preview the dirty
    /// rectangle is ignored: culling reads the committed bounds, which a
    /// moved node has left.
    ///
    /// # Errors
    ///
    /// As [`SceneWalker::rebuild`].
    #[allow(clippy::too_many_arguments)]
    pub fn rebuild_previewed(
        &mut self,
        doc: &Document,
        edit: &EditState,
        vp: &Viewport,
        quality: RenderQuality,
        dirty: Option<DeviceRect>,
        preview: &Preview,
        scene: &mut Scene,
    ) -> Result<SceneStats, SceneError> {
        let dirty = if preview.is_empty() { dirty } else { None };
        let moved = preview.transformed_set();
        let hidden: std::collections::HashSet<NodeId> = preview.hidden.iter().copied().collect();
        // Attribute overrides: the value and a fingerprint of it, so a
        // previewed node's content hash changes with every drag frame.
        let overrides: HashMap<NodeId, (Arc<AttrValue>, u64)> = preview
            .attrs
            .iter()
            .map(|(n, v)| (*n, (Arc::new(v.clone()), value_fingerprint(v))))
            .collect();
        let preview_xf = preview
            .transform
            .as_ref()
            .map(|(_, m)| xarast_render::Transform2D::from_document(*m));
        // Previewed nodes with children whose group is still open, closed
        // at their `LeaveScope` after the node's own frame.
        let mut preview_open: Vec<NodeId> = Vec::new();
        self.sync_caches(doc);
        self.resolver.begin_frame();
        self.stats = WalkStats::default();
        self.text_ink = Rect::EMPTY;
        self.painted_text.clear();

        let clip = dirty.map(|d| doc_rect_of(vp, d));
        let mut b = SceneBuilder::begin(scene, quality);
        let mut attrs = AttrStack::with_defaults(&doc.defaults);
        let mut frames: Vec<Frame> = Vec::new();
        // The scope fingerprint, pushed and popped with the attribute
        // scopes; seeded with the resources' revision.
        self.scope = mix64(SCOPE_SEED, doc.tree.resources_rev());
        let mut scopes: Vec<u64> = Vec::new();

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
                            // A previewed node's own attribute of the
                            // overridden slot gives way to the preview.
                            let over = if overrides.is_empty() {
                                None
                            } else {
                                doc.tree
                                    .links(node)
                                    .parent
                                    .and_then(|p| overrides.get(&p))
                                    .filter(|(v, _)| v.slot() == a.value.slot())
                            };
                            match over {
                                Some((v, fp)) => {
                                    attrs.push(Arc::clone(v));
                                    self.scope = mix64(self.scope, *fp);
                                }
                                None => {
                                    attrs.push(self.attr_value(node, a));
                                    self.scope = mix64(self.scope, node_version(doc, node));
                                }
                            }
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
                    if !hidden.is_empty() && hidden.contains(&node) {
                        walk.control(xarast_doc::Descend::Skip);
                        continue;
                    }
                    // A story is painted whole at its visit: its lines and
                    // items are laid out together, not walked one by one.
                    let story = matches!(kind, NodeKind::TextStory(_));
                    let leaf = story || doc.tree.links(node).first_child.is_none();
                    let previewed = !moved.is_empty() && moved.contains(&node);
                    if previewed && let Some(xf) = preview_xf {
                        b.push_group(preview_id(doc, node), xf, CacheHint::Never);
                        if !leaf {
                            preview_open.push(node);
                        }
                    }
                    if story {
                        self.paint_story_path(doc, edit, node, &mut attrs, quality, &mut b);
                        self.paint_story(doc, node, &mut attrs, quality, &mut b);
                        walk.control(xarast_doc::Descend::Skip);
                        if previewed && preview_xf.is_some() {
                            b.pop_group();
                        }
                    } else if leaf {
                        let over = overrides.get(&node);
                        if let Some((v, fp)) = over {
                            attrs.push_scope();
                            scopes.push(self.scope);
                            attrs.push(Arc::clone(v));
                            self.scope = mix64(self.scope, *fp);
                        }
                        self.draft_ramps = over.is_some();
                        self.paint(doc, edit, node, &attrs, quality, &mut b);
                        self.draft_ramps = false;
                        if over.is_some() {
                            attrs.pop_scope();
                            if let Some(fp) = scopes.pop() {
                                self.scope = fp;
                            }
                        }
                        if previewed && preview_xf.is_some() {
                            b.pop_group();
                        }
                    }
                }
                WalkEvent::EnterScope { parent } => {
                    attrs.push_scope();
                    scopes.push(self.scope);
                    // The override stands first, where the command would
                    // add the attribute; an own attribute of the slot is
                    // replaced at its visit.
                    if let Some((v, fp)) = overrides.get(&parent) {
                        attrs.push(Arc::clone(v));
                        self.scope = mix64(self.scope, *fp);
                    }
                    frames.push(self.open(doc, parent, &attrs, &mut b));
                }
                WalkEvent::LeaveScope { parent } => {
                    self.draft_ramps = overrides.contains_key(&parent);
                    self.paint(doc, edit, parent, &attrs, quality, &mut b);
                    self.draft_ramps = false;
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
                    if let Some(fp) = scopes.pop() {
                        self.scope = fp;
                    }
                    if preview_open.last() == Some(&parent) {
                        preview_open.pop();
                        b.pop_group();
                    }
                }
            }
        }

        let stats = b.finish()?;
        self.scene_stats = stats;
        // Ramps no scene of late has used go past the budget (a fill drag
        // makes one per frame); this frame's are never evicted.
        self.resolver.trim_ramps(RAMP_CACHE_BUDGET);
        Ok(stats)
    }

    /// Drops the caches that a document change invalidated.
    fn sync_caches(&mut self, doc: &Document) {
        if self.attr_epoch != doc.epoch {
            self.attr_cache.clear();
            self.stories.clear();
            self.attr_epoch = doc.epoch;
        }
        self.register_images(doc);
    }

    /// Registers every bitmap with the renderer, decoding it first when
    /// all the document holds is its encoded original.
    ///
    /// The `.xar` importer keeps a bitmap's encoded bytes and leaves
    /// `pixels` empty; decoding happens here, once per walker and bitmap,
    /// on the first frame that sees the resource (`docs/memory/image.md`,
    /// "Walker integration contract"). A failed decode goes into the
    /// negative cache and is never retried by this walker. The seam is
    /// here and not in the walk itself so that the registry is built once
    /// per frame rather than once per object.
    fn register_images(&mut self, doc: &Document) {
        let mut todo: Vec<(BitmapId, &xarast_doc::BitmapResource)> = Vec::new();
        for (id, res) in doc.resources.bitmaps() {
            if self.images.contains_key(&id) || self.failed.contains(&id) {
                continue;
            }
            let (w, h) = (res.info.width, res.info.height);
            let expected = w as usize * h as usize * 4;
            if expected != 0 && res.pixels.pixels.len() == expected {
                let image = ImageRef::new(w, h, res.pixels.pixels.to_vec());
                let rid = self.resolver.images.insert(image);
                self.images.insert(id, rid);
            } else if res.pixels.pixels.is_empty() && res.original.is_some() {
                todo.push((id, res));
            }
        }
        for (id, decoded) in decode_all(&todo) {
            match decoded {
                Some(image) => {
                    let rid = self.resolver.images.insert(image);
                    self.images.insert(id, rid);
                }
                None => {
                    self.failed.insert(id);
                }
            }
        }
    }

    /// Counts an object whose bitmap is not registered, as failed or as
    /// pending.
    fn image_missing(&mut self, id: BitmapId) {
        if self.failed.contains(&id) {
            self.stats.images_failed += 1;
        } else {
            self.stats.images_pending += 1;
        }
    }

    /// Counts the object when the transparency in force is a bitmap whose
    /// image is not registered: the renderer then composites it opaque.
    fn check_transparency_image(&mut self, attrs: &AttrStack) {
        if let AttrValue::TranspFill(t) = attrs.get(AttrSlot::TranspFillGeometry)
            && let Some(image) = t.bitmap()
            && !self.images.contains_key(&image)
        {
            self.image_missing(image);
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
            self.image_missing(*image);
        }
        // A bitmap *transparency* without its image renders opaque, which
        // turns a soft shadow into a black box: count it as well.
        self.check_transparency_image(attrs);
        let mut ctx = PaintCtx {
            colours: &doc.resources.colours,
            ramp_length: if self.draft_ramps {
                xarast_render::RampLength::Short
            } else {
                quality.ramp_length()
            },
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

        b.finish_node(id, content_hash(doc, node, self.scope));
    }

    /// Paints the path a story on a path follows, under its text, as the
    /// original does: the story's first `Path` child, with the story's own
    /// attributes before it and the path's own children in scope. Most
    /// such paths have no line colour; a visible one is part of the
    /// design (`Designs/TextCurve.xar`).
    fn paint_story_path(
        &mut self,
        doc: &Document,
        edit: &EditState,
        node: NodeId,
        attrs: &mut AttrStack,
        quality: RenderQuality,
        b: &mut SceneBuilder<'_>,
    ) {
        let Some(NodeKind::TextStory(story)) = doc.tree.kind(node) else {
            return;
        };
        if !matches!(story.layout, xarast_doc::TextLayout::OnPath { .. }) {
            return;
        }
        let saved = self.scope;
        attrs.push_scope();
        for child in doc.tree.children(node) {
            match doc.tree.kind(child) {
                Some(NodeKind::Attr(a)) => {
                    attrs.push(self.attr_value(child, a));
                    self.scope = mix64(self.scope, node_version(doc, child));
                }
                Some(NodeKind::Path(_)) => {
                    attrs.push_scope();
                    let inner = self.scope;
                    for c in doc.tree.children(child) {
                        if let Some(NodeKind::Attr(a)) = doc.tree.kind(c) {
                            attrs.push(self.attr_value(c, a));
                            self.scope = mix64(self.scope, node_version(doc, c));
                        }
                    }
                    self.paint(doc, edit, child, attrs, quality, b);
                    self.scope = inner;
                    attrs.pop_scope();
                    break;
                }
                _ => {}
            }
        }
        attrs.pop_scope();
        self.scope = saved;
    }

    /// Lays a story out (or takes it from the cache) and paints each
    /// attribute run's glyphs with that run's fill, stroke and transparency.
    fn paint_story(
        &mut self,
        doc: &Document,
        node: NodeId,
        attrs: &mut AttrStack,
        quality: RenderQuality,
        b: &mut SceneBuilder<'_>,
    ) {
        let Some(NodeKind::TextStory(story)) = doc.tree.kind(node) else {
            return;
        };
        let geom = if let Some(g) = self.stories.get(&node) {
            Arc::clone(g)
        } else {
            let cache = &mut self.attr_cache;
            let Some(st) = StoryText::collect(&doc.tree, node, attrs, &mut |id, a| {
                cache
                    .entry(id)
                    .or_insert_with(|| Arc::new(a.value.clone()))
                    .clone()
            }) else {
                return;
            };
            let fonts = self.fonts.get_or_insert_with(crate::fonts::shared).clone();
            let g = Arc::new(crate::text::build_story(&fonts, &doc.tree, &st, story));
            for s in &g.substitutions {
                if !self.substitutions.contains(s) {
                    self.substitutions.push(s.clone());
                }
            }
            self.stories.insert(node, Arc::clone(&g));
            g
        };
        if geom.unrendered {
            self.stats.text_pending += 1;
            return;
        }
        self.stats.text_stories += 1;
        if !geom.bounds.is_empty() {
            self.text_ink = if self.text_ink.is_empty() {
                geom.bounds
            } else {
                self.text_ink.union(geom.bounds)
            };
        }
        if geom.on_path_unfitted {
            self.stats.text_on_path_pending += 1;
        }
        let id = scene_id(doc, node);
        for run in &geom.runs {
            self.painted_text.push(xarast_io::TextRun {
                path: run.path.clone(),
                glyphs: Arc::clone(&run.glyphs),
                decoration: run.decoration.clone(),
            });
            let a = &run.attrs;
            if let AttrValue::Fill(xarast_doc::fill::FillGeometry::Bitmap { image, .. }) =
                a.get(AttrSlot::FillGeometry)
                && !self.images.contains_key(image)
            {
                self.image_missing(*image);
            }
            let mut ctx = PaintCtx {
                colours: &doc.resources.colours,
                ramp_length: quality.ramp_length(),
                filter: quality.image_filter(),
                resolver: &mut self.resolver,
                images: &self.images,
            };
            // Glyph outlines wind by the non-zero rule whatever the winding
            // attribute says: that is how fonts are drawn.
            if let AttrValue::Fill(fill) = a.get(AttrSlot::FillGeometry)
                && let Some(paint) = crate::paint::colour_paint(
                    fill,
                    resolved_tiling(a, AttrSlot::FillMapping),
                    resolved_effect(a),
                    &mut ctx,
                )
            {
                let t = resolved_transparency(a, AttrSlot::TranspFillGeometry, &mut ctx);
                emit(b, t, |b| {
                    b.fill(id, &run.path, FillRule::NonZero, paint.clone())
                });
            }
            if let AttrValue::StrokeColour(stroke) = a.get(AttrSlot::StrokeColour)
                && let Some(paint) =
                    crate::paint::colour_paint(stroke, Tiling::None, resolved_effect(a), &mut ctx)
            {
                let style = stroke_style_of(|s| a.get(s));
                let t = resolved_transparency(a, AttrSlot::StrokeTransp, &mut ctx);
                emit(b, t, |b| {
                    b.stroke(id, &run.path, style.clone(), paint.clone())
                });
            }
        }
        b.finish_node(id, content_hash(doc, node, mix64(self.scope, geom.version)));
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
            self.image_missing(bm.image);
            return;
        };
        self.check_transparency_image(attrs);
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
        b.finish_node(id, content_hash(doc, node, self.scope));
    }
}

/// Decodes a batch of encoded bitmaps, in parallel when there is more
/// than one, and returns them in input order (`None` for a failure).
///
/// Every decode runs under `DecodeLimits::default()`, which bounds its
/// memory and wall clock whatever the bytes claim. Spreading a document's
/// bitmaps over the cores keeps the first frame of a bitmap-heavy file
/// close to the cost of its largest image rather than the sum of all.
fn decode_all(
    todo: &[(BitmapId, &xarast_doc::BitmapResource)],
) -> Vec<(BitmapId, Option<ImageRef>)> {
    let threads = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .min(todo.len());
    if threads <= 1 {
        return todo
            .iter()
            .map(|(id, res)| (*id, decode_resource(res)))
            .collect();
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut out: Vec<(BitmapId, Option<ImageRef>)> =
        todo.iter().map(|(id, _)| (*id, None)).collect();
    let results: Vec<Vec<(usize, Option<ImageRef>)>> = std::thread::scope(|s| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some((_, res)) = todo.get(i) else { break };
                        mine.push((i, decode_resource(res)));
                    }
                    mine
                })
            })
            .collect();
        // A panicking decoder is a decode failure, not a crashed frame.
        workers
            .into_iter()
            .map(|w| w.join().unwrap_or_default())
            .collect()
    });
    for (i, image) in results.into_iter().flatten() {
        out[i].1 = image;
    }
    out
}

/// Decodes one resource's encoded original into straight RGBA.
///
/// The `.xar` wrappings are chosen from what the importer recorded: a
/// JPEG that arrived with a reconstruction palette is tag 71
/// (JPEG8BPP), a `Bmp` may be a headerless DIB (tag 65), and `Unknown` is
/// the importer's name for the zlib-wrapped DIB (tag 69). The decoded
/// dimensions are used, not `res.info`, which the importer leaves zeroed.
fn decode_resource(res: &xarast_doc::BitmapResource) -> Option<ImageRef> {
    use xarast_doc::resources::ImageFormat as F;
    use xarast_image::xar::decode_xar_bitmap;
    let original = res.original.as_ref()?;
    let bytes: &[u8] = &original.bytes;
    let limits = xarast_image::DecodeLimits::default();
    let decoded = match original.format {
        F::Jpeg if !res.pixels.palette.is_empty() => {
            let palette: Vec<[u8; 3]> =
                res.pixels.palette.iter().map(|c| [c.r, c.g, c.b]).collect();
            decode_xar_bitmap(71, bytes, &palette, &limits)
        }
        F::Png | F::Jpeg | F::Gif => xarast_image::decode(bytes, &limits),
        F::Bmp => decode_xar_bitmap(65, bytes, &[], &limits),
        F::Unknown => decode_xar_bitmap(69, bytes, &[], &limits),
    }
    .ok()?;
    let d = decoded.data;
    (d.width > 0 && d.height > 0).then(|| ImageRef::new(d.width, d.height, d.to_straight_rgba8()))
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

fn resolved_transparency(
    attrs: &ResolvedAttrs,
    slot: AttrSlot,
    ctx: &mut PaintCtx<'_>,
) -> Transparency {
    match attrs.get(slot) {
        AttrValue::TranspFill(t) | AttrValue::StrokeTransp(t) => {
            crate::paint::transparency(t, resolved_tiling(attrs, AttrSlot::TranspFillMapping), ctx)
        }
        _ => Transparency::OPAQUE,
    }
}

fn resolved_tiling(attrs: &ResolvedAttrs, slot: AttrSlot) -> Tiling {
    match attrs.get(slot) {
        AttrValue::FillMapping(t) | AttrValue::TranspFillMapping(t) => *t,
        _ => Tiling::None,
    }
}

fn resolved_effect(attrs: &ResolvedAttrs) -> xarast_color::FillEffect {
    match attrs.get(AttrSlot::FillEffect) {
        AttrValue::FillEffect(e) => *e,
        _ => xarast_color::FillEffect::Fade,
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
    stroke_style_of(|s| attrs.get(s))
}

fn stroke_style_of<'a>(get: impl Fn(AttrSlot) -> &'a AttrValue) -> StrokeStyle {
    let width = match get(AttrSlot::LineWidth) {
        AttrValue::LineWidth(w) => *w,
        _ => Mp::ZERO,
    };
    let cap = match get(AttrSlot::StartCap) {
        AttrValue::LineCap(c) => *c,
        _ => Cap::Butt,
    };
    let join = match get(AttrSlot::JoinType) {
        AttrValue::JoinType(j) => *j,
        _ => Join::Mitre,
    };
    let mitre_limit = match get(AttrSlot::MitreLimit) {
        AttrValue::MitreLimit(m) => (m.to_f64() / f64::from(Mp::PER_PT)).max(1.0),
        _ => 4.0,
    };
    let dash = match get(AttrSlot::DashPattern) {
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

/// The scene id of the group a preview wraps a node in: the node's own id
/// with the top bit set, which no tag (a `u32`) can reach.
fn preview_id(doc: &Document, node: NodeId) -> SceneNodeId {
    SceneNodeId(scene_id(doc, node).0 | (1 << 63))
}

/// The per-node content hash the render cache keys on.
///
/// It names one *version* of what a node paints, from three things:
///
/// 1. the node's own version — its [`Tag`](xarast_doc::Tag) and
///    [`content_rev`](xarast_doc::Tree::content_rev), which every action
///    that changes its payload, geometry or flags (undo and redo included)
///    moves to a value the tree never handed out before;
/// 2. the attribute scope it is painted in — `scope`, a fold of the
///    version of every attribute node the walk pushed on the way down, in
///    order, so that recolouring a group's fill attribute changes the hash
///    of every sibling it applies to and of nothing else;
/// 3. the resources' revision, folded into the scope's seed.
///
/// Editing one object therefore changes that object's hash and leaves the
/// rest of the document's alone, which is what lets a per-node cache pay
/// for itself. The first half is the node's version verbatim (collision
/// free within a document); the second is the 64-bit scope fold. Caches
/// keyed on it must be per document: tags and revisions are numbered per
/// tree.
fn content_hash(doc: &Document, node: NodeId, scope: u64) -> ContentHash {
    let mut bytes = [0u8; 16];
    let tag = doc.tree.get(node).map_or(0, |d| d.tag.0);
    let rev = doc.tree.content_rev(node);
    // Tags are u32; revisions stay far below 2^32 in any session, but fold
    // the high half in rather than drop it.
    bytes[..4].copy_from_slice(&tag.to_le_bytes());
    bytes[4..8].copy_from_slice(&((rev as u32) ^ ((rev >> 32) as u32)).to_le_bytes());
    bytes[8..].copy_from_slice(&scope.to_le_bytes());
    ContentHash(bytes)
}

/// The seed of the scope fold.
const SCOPE_SEED: u64 = 0x9e37_79b9_7f4a_7c15;

/// One node's version as a single number: its tag and content revision.
fn node_version(doc: &Document, node: NodeId) -> u64 {
    let tag = u64::from(doc.tree.get(node).map_or(0, |d| d.tag.0));
    (tag << 32) ^ doc.tree.content_rev(node)
}

/// Folds `v` into `h`: order-dependent, and well mixed (the SplitMix64
/// finaliser), so that two scopes differing in one attribute's revision
/// differ in about half their bits.
/// The bytes of gradient tables the walker's ramp cache keeps between
/// frames: 2048 final-quality tables, or 16 384 draft ones.
pub const RAMP_CACHE_BUDGET: usize = 16 << 20;

/// A fingerprint of a previewed attribute value. Only computed for the
/// few nodes a gesture previews, once per scene rebuild.
fn value_fingerprint(v: &AttrValue) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{v:?}").hash(&mut h);
    mix64(0x5052_4556_4945_5721, h.finish())
}

fn mix64(h: u64, v: u64) -> u64 {
    let mut z = h.rotate_left(5) ^ v.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
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
