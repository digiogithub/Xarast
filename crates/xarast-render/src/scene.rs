//! The retained scene and the builder the document walker drives.
//!
//! # Where the document meets the renderer
//!
//! `xarast-render` has no dependency on `xarast-doc` and never sees a node,
//! an attribute or a layer. The walker that turns an arena plus a dirty
//! region into a scene lives in `xarast-app`, which depends on both; this
//! module is the contract it fills. That is what lets every test in this
//! crate build its scene by hand with no document present.
//!
//! # Attribute scope
//!
//! [`SceneBuilder::push_transparency`] follows the original's lexical
//! attribute scoping (`docs/10-architecture.md` §3.6): the transparency
//! applies to everything emitted until the matching pop, and is captured
//! into each primitive as it is emitted. A transparency that has to be
//! composited as a *group* — the whole group flattened first, then blended
//! once — is [`SceneBuilder::push_layer`] instead, because the two are
//! visually different and the file format distinguishes them.

use std::collections::HashMap;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use xarast_geom::{FillRule, StrokeStyle};

use crate::blend::Transparency;
use crate::effect::LayerEffect;
use crate::paint::{GradMapping, ImageId, Paint};
use crate::path::PathRef;
use crate::precision::Transform2D;

/// Identifies a node of the scene. The walker chooses these; the renderer
/// only compares them, so any injective mapping from document node ids
/// works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SceneNodeId(pub u64);

/// A 128-bit content hash: geometry, attributes and the relative matrix of
/// a subtree.
///
/// 128 bits rather than 64 because it keys a render cache, and a collision
/// there is not a slow frame but a wrong picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct ContentHash(pub [u8; 16]);

impl ContentHash {
    /// The hash of nothing.
    pub const EMPTY: ContentHash = ContentHash([0; 16]);

    /// Hashes a byte string.
    #[must_use]
    pub fn of(bytes: &[u8]) -> ContentHash {
        let digest = Sha256::digest(bytes);
        let mut out = [0u8; 16];
        out.copy_from_slice(&digest[..16]);
        ContentHash(out)
    }
}

/// Whether a node is worth a render-cache slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CacheHint {
    /// Never cache: cheap content, or content that changes every frame.
    Never,
    /// Let the admission policy decide.
    #[default]
    Auto,
    /// Always cache: the walker knows this is a live effect or a
    /// transparent group.
    Always,
}

/// What an offscreen layer means for alpha and for destination reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LayerKind {
    /// Composited straight back, with no isolation.
    #[default]
    Plain,
    /// The group is rendered against transparent black and composited as a
    /// unit: what a transparent group means in the file format.
    Isolated,
    /// The layer's blend reads the destination, so the tile planner has to
    /// resolve everything before it and ping-pong.
    DestinationReading,
}

/// How hard the renderer works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub enum RenderQuality {
    /// During a drag, a zoom or a scroll. Antialiasing stays **on** —
    /// turning it off is exactly the regression users notice — but flatness
    /// is multiplied by five, images sample nearest and ramps are 256
    /// entries.
    Draft,
    /// At rest, and always on export: full flatness, high-quality image
    /// filtering, 2048-entry ramps.
    #[default]
    Final,
}

impl RenderQuality {
    /// The multiplier applied to the flatness tolerance.
    #[must_use]
    pub const fn flatness_multiplier(self) -> f64 {
        match self {
            RenderQuality::Draft => 5.0,
            RenderQuality::Final => 1.0,
        }
    }

    /// The ramp table length this quality uses.
    #[must_use]
    pub const fn ramp_length(self) -> crate::ramp::RampLength {
        match self {
            RenderQuality::Draft => crate::ramp::RampLength::Short,
            RenderQuality::Final => crate::ramp::RampLength::Long,
        }
    }

    /// The image filter this quality uses.
    #[must_use]
    pub const fn image_filter(self) -> crate::paint::Filter {
        match self {
            RenderQuality::Draft => crate::paint::Filter::Nearest,
            RenderQuality::Final => crate::paint::Filter::HighQuality,
        }
    }
}

/// One recorded operation. The scene keeps these in draw order; the display
/// list resolves them against a view.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SceneOp {
    PushGroup {
        id: SceneNodeId,
        xf: Transform2D,
        hint: CacheHint,
    },
    PopGroup,
    PushClip {
        path: PathRef,
        rule: FillRule,
    },
    PopClip,
    PushTransparency(Transparency),
    PopTransparency,
    PushLayer {
        kind: LayerKind,
        transparency: Transparency,
    },
    PopLayer,
    PushEffect(LayerEffect),
    PopEffect,
    Fill {
        id: SceneNodeId,
        path: PathRef,
        rule: FillRule,
        paint: Paint,
        transparency: Transparency,
    },
    Stroke {
        id: SceneNodeId,
        path: PathRef,
        style: StrokeStyle,
        paint: Paint,
        transparency: Transparency,
    },
    Image {
        id: SceneNodeId,
        image: ImageId,
        mapping: GradMapping,
        paint: Paint,
        transparency: Transparency,
    },
}

/// What the renderer remembers about one scene node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NodeInfo {
    /// The hash of the node's content, which keys its cache slot.
    pub content: ContentHash,
    /// Whether the walker wants it cached.
    pub hint: CacheHint,
    /// Index of the op that opened the node, for invalidation.
    pub first_op: usize,
    /// Index one past the last op of the node.
    pub last_op: usize,
}

/// The retained scene: what the walker produces and the display list
/// consumes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scene {
    /// Shared with every display list built from it, which is what lets a
    /// build reference paths, paints and styles instead of copying them.
    pub(crate) ops: Arc<Vec<SceneOp>>,
    /// One entry per op: what a culled build needs to reject it without
    /// reading the op itself. See [`Cull`].
    pub(crate) cull: Vec<Cull>,
    pub(crate) nodes: HashMap<SceneNodeId, NodeInfo>,
    pub(crate) quality: RenderQuality,
}

/// A culling entry, kept apart from its op.
///
/// A culled build (a pan strip, a dirty rectangle) rejects almost every
/// primitive. Reading the rejection data out of 320-byte ops streamed the
/// whole scene through the cache; these entries are 40 bytes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Cull {
    /// Structural, or bounded by something other than a path: read the op.
    Visit,
    /// A fill whose path has these document-space control bounds.
    Fill(kurbo::Rect),
    /// A stroke: its path's control bounds, already grown by the stroke's
    /// pad in document units.
    Stroke(kurbo::Rect),
    /// A fill or stroke whose path bounds are inverted: it draws nothing.
    Never,
}

impl Cull {
    /// The entry for a path, padded by `pad` document units, exactly as
    /// `display_list::device_bounds_of` pads it.
    fn of(path: &PathRef, pad: f64, stroke: bool) -> Cull {
        let r = path.bounds();
        if r.x1 < r.x0 || r.y1 < r.y0 {
            return Cull::Never;
        }
        let r = kurbo::Rect::new(r.x0 - pad, r.y0 - pad, r.x1 + pad, r.y1 + pad);
        if stroke {
            Cull::Stroke(r)
        } else {
            Cull::Fill(r)
        }
    }
}

/// How far a stroke spreads beyond its path, in document units: half its
/// width times the mitre allowance, or nothing for a hairline, whose one
/// pixel the build adds in device space.
pub(crate) fn stroke_pad(style: &StrokeStyle) -> f64 {
    if style.width == xarast_geom::Mp::ZERO {
        0.0
    } else {
        style.width.to_f64() * 0.5 * style.mitre_limit.max(1.0)
    }
}

impl Scene {
    /// An empty scene.
    #[must_use]
    pub fn new() -> Scene {
        Scene::default()
    }

    /// Drops everything recorded, keeping the allocation.
    pub fn clear(&mut self) {
        // A display list still in flight keeps the old ops alive; only then
        // is a fresh vector needed.
        match Arc::get_mut(&mut self.ops) {
            Some(ops) => ops.clear(),
            None => self.ops = Arc::new(Vec::with_capacity(self.ops.len())),
        }
        self.cull.clear();
        self.nodes.clear();
    }

    /// How many operations are recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// Whether nothing is recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// The quality the scene was recorded at.
    #[must_use]
    pub const fn quality(&self) -> RenderQuality {
        self.quality
    }

    /// What is known about a node.
    #[must_use]
    pub fn node(&self, id: SceneNodeId) -> Option<&NodeInfo> {
        self.nodes.get(&id)
    }

    /// Every node the scene knows about.
    pub fn nodes(&self) -> impl Iterator<Item = (&SceneNodeId, &NodeInfo)> {
        self.nodes.iter()
    }
}

/// Why a scene could not be closed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SceneError {
    /// A `push_*` was never matched by its `pop_*`.
    #[error("{count} unclosed {kind} at the end of the scene")]
    Unbalanced {
        /// What was left open.
        kind: &'static str,
        /// How many.
        count: usize,
    },
    /// A `pop_*` had nothing to pop.
    #[error("a pop_{kind} had no matching push")]
    Underflow {
        /// What was popped.
        kind: &'static str,
    },
}

/// Counts of what a scene contains, returned by [`SceneBuilder::finish`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SceneStats {
    /// Number of fill commands.
    pub fills: usize,
    /// Number of stroke commands.
    pub strokes: usize,
    /// Number of image commands.
    pub images: usize,
    /// Number of groups.
    pub groups: usize,
    /// Number of offscreen layers.
    pub layers: usize,
    /// Number of clips.
    pub clips: usize,
    /// Number of live effects ([`SceneBuilder::push_effect`]).
    pub effects: usize,
}

impl SceneStats {
    /// Total drawing primitives.
    #[must_use]
    pub const fn primitives(&self) -> usize {
        self.fills + self.strokes + self.images
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frame {
    Group,
    Clip,
    Transparency,
    Layer,
    Effect,
}

/// Records a scene. The walker in `xarast-app` drives this.
#[derive(Debug)]
pub struct SceneBuilder<'a> {
    scene: &'a mut Scene,
    stack: Vec<Frame>,
    transparency: Vec<Transparency>,
    open: Vec<(SceneNodeId, usize)>,
    stats: SceneStats,
    error: Option<SceneError>,
}

impl<'a> SceneBuilder<'a> {
    /// Starts recording into a scene, clearing whatever was there.
    pub fn begin(scene: &'a mut Scene, quality: RenderQuality) -> SceneBuilder<'a> {
        scene.clear();
        scene.quality = quality;
        SceneBuilder {
            scene,
            stack: Vec::new(),
            transparency: vec![Transparency::OPAQUE],
            open: Vec::new(),
            stats: SceneStats::default(),
            error: None,
        }
    }

    /// The op list, unshared: `begin` cleared the scene, so a display list
    /// built from the previous recording no longer holds this vector and
    /// `make_mut` never copies.
    fn ops(&mut self) -> &mut Vec<SceneOp> {
        Arc::make_mut(&mut self.scene.ops)
    }

    /// Records an op with its culling entry; every op goes through here so
    /// that the two lists stay the same length.
    fn push(&mut self, op: SceneOp, cull: Cull) {
        self.ops().push(op);
        self.scene.cull.push(cull);
    }

    fn current_transparency(&self) -> Transparency {
        self.transparency
            .last()
            .cloned()
            .unwrap_or(Transparency::OPAQUE)
    }

    /// Opens a group with its own transform.
    pub fn push_group(&mut self, id: SceneNodeId, xf: Transform2D, hint: CacheHint) -> SceneNodeId {
        self.open.push((id, self.scene.ops.len()));
        self.push(SceneOp::PushGroup { id, xf, hint }, Cull::Visit);
        self.stack.push(Frame::Group);
        self.stats.groups += 1;
        self.scene.nodes.insert(
            id,
            NodeInfo {
                content: ContentHash::EMPTY,
                hint,
                first_op: self.scene.ops.len() - 1,
                last_op: self.scene.ops.len(),
            },
        );
        id
    }

    /// Closes the innermost group.
    pub fn pop_group(&mut self) {
        if self.stack.pop() != Some(Frame::Group) {
            self.error
                .get_or_insert(SceneError::Underflow { kind: "group" });
            return;
        }
        self.push(SceneOp::PopGroup, Cull::Visit);
        if let Some((id, _)) = self.open.pop()
            && let Some(info) = self.scene.nodes.get_mut(&id)
        {
            info.last_op = self.scene.ops.len();
        }
    }

    /// Clips everything until the matching pop to a path.
    pub fn push_clip(&mut self, path: &PathRef, rule: FillRule) {
        self.push(
            SceneOp::PushClip {
                path: path.clone(),
                rule,
            },
            Cull::Visit,
        );
        self.stack.push(Frame::Clip);
        self.stats.clips += 1;
    }

    /// Removes the innermost clip.
    pub fn pop_clip(&mut self) {
        if self.stack.pop() != Some(Frame::Clip) {
            self.error
                .get_or_insert(SceneError::Underflow { kind: "clip" });
            return;
        }
        self.push(SceneOp::PopClip, Cull::Visit);
    }

    /// Applies a transparency to everything emitted until the matching pop,
    /// per object, following the original's lexical attribute scoping.
    pub fn push_transparency(&mut self, t: Transparency) {
        self.push(SceneOp::PushTransparency(t.clone()), Cull::Visit);
        self.transparency.push(t);
        self.stack.push(Frame::Transparency);
    }

    /// Restores the previous transparency.
    pub fn pop_transparency(&mut self) {
        if self.stack.pop() != Some(Frame::Transparency) {
            self.error.get_or_insert(SceneError::Underflow {
                kind: "transparency",
            });
            return;
        }
        self.transparency.pop();
        self.push(SceneOp::PopTransparency, Cull::Visit);
    }

    /// Opens an offscreen layer: everything until the matching pop is
    /// rendered apart and composited once. This is the equivalent of a Xara
    /// capture, and it is what a transparent *group* means, as opposed to a
    /// transparency applied per object.
    pub fn push_layer(&mut self, kind: LayerKind, transparency: Transparency) {
        self.push(SceneOp::PushLayer { kind, transparency }, Cull::Visit);
        self.stack.push(Frame::Layer);
        self.stats.layers += 1;
    }

    /// Closes the innermost offscreen layer.
    pub fn pop_layer(&mut self) {
        if self.stack.pop() != Some(Frame::Layer) {
            self.error
                .get_or_insert(SceneError::Underflow { kind: "layer" });
            return;
        }
        self.push(SceneOp::PopLayer, Cull::Visit);
    }

    /// Opens a live effect: everything until the matching pop is rendered
    /// offscreen at the view's resolution, the effect is applied to it,
    /// and the result is composited in its place ([`crate::effect`]).
    pub fn push_effect(&mut self, effect: LayerEffect) {
        self.push(SceneOp::PushEffect(effect), Cull::Visit);
        self.stack.push(Frame::Effect);
        self.stats.effects += 1;
    }

    /// Closes the innermost live effect.
    pub fn pop_effect(&mut self) {
        if self.stack.pop() != Some(Frame::Effect) {
            self.error
                .get_or_insert(SceneError::Underflow { kind: "effect" });
            return;
        }
        self.push(SceneOp::PopEffect, Cull::Visit);
    }

    /// Emits a filled path.
    pub fn fill(&mut self, id: SceneNodeId, path: &PathRef, rule: FillRule, paint: Paint) {
        let transparency = self.current_transparency();
        let cull = Cull::of(path, 0.0, false);
        self.push(
            SceneOp::Fill {
                id,
                path: path.clone(),
                rule,
                paint,
                transparency,
            },
            cull,
        );
        self.stats.fills += 1;
    }

    /// Emits a stroked path. A zero width is a hairline: one device pixel
    /// at any zoom, which the backend draws directly because it has no
    /// document-space outline.
    pub fn stroke(&mut self, id: SceneNodeId, path: &PathRef, style: StrokeStyle, paint: Paint) {
        let transparency = self.current_transparency();
        let cull = Cull::of(path, stroke_pad(&style), true);
        self.push(
            SceneOp::Stroke {
                id,
                path: path.clone(),
                style,
                paint,
                transparency,
            },
            cull,
        );
        self.stats.strokes += 1;
    }

    /// Emits an image.
    pub fn image(&mut self, id: SceneNodeId, image: ImageId, mapping: GradMapping, paint: Paint) {
        let transparency = self.current_transparency();
        self.push(
            SceneOp::Image {
                id,
                image,
                mapping,
                paint,
                transparency,
            },
            Cull::Visit,
        );
        self.stats.images += 1;
    }

    /// Records the content hash of the subtree just emitted, which is what
    /// the render cache is keyed on.
    pub fn finish_node(&mut self, id: SceneNodeId, hash: ContentHash) -> ContentHash {
        let ops = self.scene.ops.len();
        self.scene
            .nodes
            .entry(id)
            .and_modify(|n| {
                n.content = hash;
                n.last_op = ops;
            })
            .or_insert(NodeInfo {
                content: hash,
                hint: CacheHint::Auto,
                first_op: ops,
                last_op: ops,
            });
        hash
    }

    /// Closes the scene.
    ///
    /// # Errors
    ///
    /// Returns [`SceneError`] if any push was left unmatched, or if a pop
    /// had no matching push. Rejecting an unbalanced scene here rather than
    /// at render time is what keeps the backends free of recovery paths.
    pub fn finish(self) -> Result<SceneStats, SceneError> {
        if let Some(e) = self.error {
            return Err(e);
        }
        if let Some(frame) = self.stack.first() {
            return Err(SceneError::Unbalanced {
                kind: match frame {
                    Frame::Group => "group",
                    Frame::Clip => "clip",
                    Frame::Transparency => "transparency",
                    Frame::Layer => "layer",
                    Frame::Effect => "effect",
                },
                count: self.stack.len(),
            });
        }
        Ok(self.stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xarast_color::Rgba8;
    use xarast_geom::{Mp, Point, Rect};

    fn square() -> PathRef {
        let mut b = xarast_geom::Path::builder();
        b.rect(Rect::new(
            Point::new(Mp::ZERO, Mp::ZERO),
            Point::new(Mp::from_pt(10.0), Mp::from_pt(10.0)),
        ));
        PathRef::new(b.build())
    }

    #[test]
    fn a_balanced_scene_closes_and_counts() {
        let mut scene = Scene::new();
        let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
        b.push_group(SceneNodeId(1), Transform2D::IDENTITY, CacheHint::Auto);
        b.fill(
            SceneNodeId(2),
            &square(),
            FillRule::NonZero,
            Paint::Solid(Rgba8::BLACK),
        );
        b.pop_group();
        let stats = b.finish().unwrap();
        assert_eq!(stats.fills, 1);
        assert_eq!(stats.groups, 1);
        assert_eq!(stats.primitives(), 1);
        assert_eq!(scene.quality(), RenderQuality::Final);
    }

    #[test]
    fn an_unclosed_group_is_rejected_at_build_time() {
        let mut scene = Scene::new();
        let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Draft);
        b.push_group(SceneNodeId(1), Transform2D::IDENTITY, CacheHint::Auto);
        assert!(matches!(
            b.finish(),
            Err(SceneError::Unbalanced { kind: "group", .. })
        ));
    }

    #[test]
    fn a_mismatched_pop_is_rejected() {
        let mut scene = Scene::new();
        let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Draft);
        b.push_group(SceneNodeId(1), Transform2D::IDENTITY, CacheHint::Auto);
        b.pop_clip();
        assert!(matches!(b.finish(), Err(SceneError::Underflow { .. })));
    }

    #[test]
    fn quality_drives_flatness_ramp_length_and_filter() {
        assert_eq!(RenderQuality::Draft.flatness_multiplier(), 5.0);
        assert_eq!(RenderQuality::Final.flatness_multiplier(), 1.0);
        assert_eq!(RenderQuality::Draft.ramp_length().len(), 256);
        assert_eq!(RenderQuality::Final.ramp_length().len(), 2048);
        assert_eq!(
            RenderQuality::Draft.image_filter(),
            crate::paint::Filter::Nearest
        );
    }

    #[test]
    fn content_hashes_are_stable_and_distinct() {
        assert_eq!(ContentHash::of(b"abc"), ContentHash::of(b"abc"));
        assert_ne!(ContentHash::of(b"abc"), ContentHash::of(b"abd"));
    }
}
