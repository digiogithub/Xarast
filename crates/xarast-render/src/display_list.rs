//! The immutable, per-frame display list: what crosses to the render thread.
//!
//! The scene is retained and mutable; the display list is neither. It is a
//! flat, ordered stream of [`DrawCmd`] with every transform already resolved
//! into device space, every paint mapping already moved with its shape, and
//! every command's device bounding box already computed, so that the tile
//! planner never has to touch a path again.
//!
//! # Ordering is a correctness property, not a performance one
//!
//! Xara's exotic blends read the destination (`research/03 §3.4`), so they
//! cannot use fixed blend state and cannot be reordered. A command whose
//! family reads the destination is a **barrier** inside its tile: everything
//! before it is resolved, then the ping-pong swap happens. Getting this
//! wrong produces output that is nearly right, which is the worst kind of
//! bug; the GPU-versus-CPU parity test is the detector.

use std::sync::Arc;

use xarast_geom::{FillRule, StrokeStyle};

use crate::blend::{BlendFamily, Transparency};
use crate::cache::CacheKey;
use crate::paint::{GradMapping, ImageId, Paint};
use crate::path::PathRef;
use crate::precision::Transform2D;
use crate::scene::{LayerKind, RenderQuality, Scene, SceneNodeId, SceneOp};
use crate::surface::{DeviceRect, DirtyRect};

/// Everything the renderer needs to know about the view being drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewParams {
    /// Document to device, in `f64`. Device units are pixels.
    pub transform: Transform2D,
    /// The pixels that exist.
    pub viewport: DeviceRect,
    /// How hard to work.
    pub quality: RenderQuality,
    /// Device resolution, for flatness and for hairline width.
    pub dpi: f64,
}

impl Default for ViewParams {
    fn default() -> ViewParams {
        ViewParams {
            transform: Transform2D::IDENTITY,
            viewport: DeviceRect::EMPTY,
            quality: RenderQuality::Final,
            dpi: 96.0,
        }
    }
}

impl ViewParams {
    /// A view of a whole surface at 96 dpi with the given document scale.
    #[must_use]
    pub fn new(width: u32, height: u32, transform: Transform2D, quality: RenderQuality) -> ViewParams {
        ViewParams {
            transform,
            viewport: DeviceRect::from_size(width, height),
            quality,
            dpi: 96.0,
        }
    }

    /// The flattening tolerance in **document** units for this view.
    ///
    /// A quarter of a device pixel at the current scale, multiplied by the
    /// quality's flatness factor. The original computes the same quantity
    /// and divides it by five when antialiasing is on; Xarast is always
    /// antialiased, so the tight value is the baseline and Draft multiplies
    /// it back up.
    #[must_use]
    pub fn tolerance(&self) -> f64 {
        let scale = self.transform.max_scale().max(1e-9);
        0.25 / scale * self.quality.flatness_multiplier()
    }
}

/// One drawing command, in device space.
#[derive(Debug, Clone, PartialEq)]
pub enum DrawCmd {
    /// Fill a path.
    Fill {
        /// Which scene node it came from.
        node: SceneNodeId,
        /// The path, still in document units.
        path: PathRef,
        /// Which regions count as inside.
        rule: FillRule,
        /// What to fill it with, already mapped into device space.
        paint: Paint,
        /// Document to device for this command.
        xf: Transform2D,
        /// How it composites.
        transparency: Transparency,
        /// Device-space bounds, already computed.
        bounds: DeviceRect,
    },
    /// Stroke a path.
    Stroke {
        /// Which scene node it came from.
        node: SceneNodeId,
        /// The path, still in document units.
        path: PathRef,
        /// Width, caps, join, dashes. A zero width is a hairline.
        style: StrokeStyle,
        /// What to stroke it with.
        paint: Paint,
        /// Document to device for this command.
        xf: Transform2D,
        /// How it composites.
        transparency: Transparency,
        /// Device-space bounds.
        bounds: DeviceRect,
    },
    /// Draw an image into a parallelogram or quadrilateral.
    Image {
        /// Which scene node it came from.
        node: SceneNodeId,
        /// The image.
        image: ImageId,
        /// Its placement, already in device space.
        mapping: GradMapping,
        /// Filtering, contone and adjustment.
        paint: Paint,
        /// How it composites.
        transparency: Transparency,
        /// Device-space bounds.
        bounds: DeviceRect,
    },
    /// Start clipping to a path.
    PushClip {
        /// The clip path, in document units.
        path: PathRef,
        /// Which regions count as inside.
        rule: FillRule,
        /// Document to device for the clip path.
        xf: Transform2D,
    },
    /// Stop clipping to the innermost clip path.
    PopClip,
    /// Begin an offscreen layer: the equivalent of a Xara capture.
    PushLayer {
        /// What the layer means for alpha.
        kind: LayerKind,
        /// The device area the layer needs.
        bounds: DeviceRect,
        /// Whether its blend reads the destination.
        needs_dst_read: bool,
    },
    /// Composite the innermost offscreen layer back.
    PopLayer {
        /// Which family composites it.
        blend: BlendFamily,
        /// With what transparency.
        opacity: Transparency,
    },
    /// Blit an already rendered node from the render cache.
    CachedSurface {
        /// Which node.
        node: SceneNodeId,
        /// Its cache key.
        key: CacheKey,
        /// Where it goes.
        bounds: DeviceRect,
    },
}

impl DrawCmd {
    /// The device bounds this command touches, or the whole layer for the
    /// structural commands.
    #[must_use]
    pub fn bounds(&self) -> Option<DeviceRect> {
        match self {
            DrawCmd::Fill { bounds, .. }
            | DrawCmd::Stroke { bounds, .. }
            | DrawCmd::Image { bounds, .. }
            | DrawCmd::PushLayer { bounds, .. }
            | DrawCmd::CachedSurface { bounds, .. } => Some(*bounds),
            DrawCmd::PushClip { .. } | DrawCmd::PopClip | DrawCmd::PopLayer { .. } => None,
        }
    }

    /// Whether this command has to read the destination, which makes it a
    /// barrier for the tile planner.
    #[must_use]
    pub fn needs_dst_read(&self) -> bool {
        match self {
            DrawCmd::Fill { transparency, .. }
            | DrawCmd::Stroke { transparency, .. }
            | DrawCmd::Image { transparency, .. } => transparency.needs_dst_read(),
            DrawCmd::PushLayer { needs_dst_read, .. } => *needs_dst_read,
            DrawCmd::PopLayer { blend, .. } => blend.needs_dst_read(),
            _ => false,
        }
    }
}

/// An immutable, thread-safe frame's worth of drawing.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayList {
    cmds: Vec<DrawCmd>,
    bounds: DeviceRect,
    needs_dst_read: bool,
    view: ViewParams,
}

impl DisplayList {
    /// Resolves a scene against a view and a dirty region.
    ///
    /// Commands whose bounds miss the dirty region are dropped, which is
    /// what makes an incremental redraw cheap. Structural commands are
    /// always kept, because dropping a `PopLayer` would unbalance the
    /// stream.
    #[must_use]
    pub fn build(scene: &Scene, view: &ViewParams, dirty: &DirtyRect) -> Arc<DisplayList> {
        let clip_to = match dirty.0 {
            Some(d) => d.intersection(view.viewport),
            None => view.viewport,
        };
        let mut cmds: Vec<DrawCmd> = Vec::with_capacity(scene.ops.len());
        let mut xf_stack: Vec<Transform2D> = vec![view.transform];
        let mut bounds = DeviceRect::EMPTY;
        let mut needs_dst_read = false;

        for op in &scene.ops {
            let xf = *xf_stack.last().unwrap_or(&view.transform);
            match op {
                SceneOp::PushGroup { xf: g, .. } => xf_stack.push(g.then(xf)),
                SceneOp::PopGroup => {
                    xf_stack.pop();
                }
                SceneOp::PushClip { path, rule } => cmds.push(DrawCmd::PushClip {
                    path: path.clone(),
                    rule: *rule,
                    xf,
                }),
                SceneOp::PopClip => cmds.push(DrawCmd::PopClip),
                // Attribute-scope transparency is captured into each
                // primitive as it is recorded, so these markers carry no
                // work into the display list.
                SceneOp::PushTransparency(_) | SceneOp::PopTransparency => {}
                SceneOp::PushLayer { kind, transparency } => {
                    let dst = transparency.needs_dst_read();
                    needs_dst_read |= dst;
                    cmds.push(DrawCmd::PushLayer {
                        kind: *kind,
                        bounds: clip_to,
                        needs_dst_read: dst,
                    });
                }
                SceneOp::PopLayer => {
                    // The transparency of the matching push is repeated on
                    // the pop so that a backend never has to look backwards.
                    let (blend, opacity) = last_layer_transparency(&scene.ops, op);
                    cmds.push(DrawCmd::PopLayer { blend, opacity });
                }
                SceneOp::Fill {
                    id,
                    path,
                    rule,
                    paint,
                    transparency,
                } => {
                    let b = device_bounds_of(path, xf, 0.0);
                    if !b.intersects(clip_to) {
                        continue;
                    }
                    bounds = bounds.union(b);
                    needs_dst_read |= transparency.needs_dst_read();
                    cmds.push(DrawCmd::Fill {
                        node: *id,
                        path: path.clone(),
                        rule: *rule,
                        paint: map_paint(paint, xf),
                        xf,
                        transparency: transparency.clone(),
                        bounds: b,
                    });
                }
                SceneOp::Stroke {
                    id,
                    path,
                    style,
                    paint,
                    transparency,
                } => {
                    // A hairline is one device pixel; anything else spreads
                    // by half its width, plus the mitre allowance.
                    let pad = if style.width == xarast_geom::Mp::ZERO {
                        0.0
                    } else {
                        style.width.to_f64() * 0.5 * style.mitre_limit.max(1.0)
                    };
                    let b = device_bounds_of(path, xf, pad).inflated(1);
                    if !b.intersects(clip_to) {
                        continue;
                    }
                    bounds = bounds.union(b);
                    needs_dst_read |= transparency.needs_dst_read();
                    cmds.push(DrawCmd::Stroke {
                        node: *id,
                        path: path.clone(),
                        style: style.clone(),
                        paint: map_paint(paint, xf),
                        xf,
                        transparency: transparency.clone(),
                        bounds: b,
                    });
                }
                SceneOp::Image {
                    id,
                    image,
                    mapping,
                    paint,
                    transparency,
                } => {
                    let dev = mapping.transformed(xf);
                    let b = mapping_bounds(dev);
                    if !b.intersects(clip_to) {
                        continue;
                    }
                    bounds = bounds.union(b);
                    needs_dst_read |= transparency.needs_dst_read();
                    cmds.push(DrawCmd::Image {
                        node: *id,
                        image: *image,
                        mapping: dev,
                        paint: map_paint(paint, xf),
                        transparency: transparency.clone(),
                        bounds: b,
                    });
                }
            }
        }

        Arc::new(DisplayList {
            cmds,
            bounds: bounds.intersection(clip_to),
            needs_dst_read,
            view: *view,
        })
    }

    /// Wraps an already built command stream, for the immediate-mode
    /// facade, which records commands rather than resolving a scene.
    #[must_use]
    pub fn from_commands(
        cmds: Vec<DrawCmd>,
        clip: DeviceRect,
        view: ViewParams,
    ) -> Arc<DisplayList> {
        let mut bounds = DeviceRect::EMPTY;
        let mut needs_dst_read = false;
        for c in &cmds {
            if let Some(b) = c.bounds() {
                bounds = bounds.union(b);
            }
            needs_dst_read |= c.needs_dst_read();
        }
        Arc::new(DisplayList {
            cmds,
            bounds: bounds.intersection(clip),
            needs_dst_read,
            view,
        })
    }

    /// The commands, in order. Order is load-bearing.
    #[must_use]
    pub fn commands(&self) -> &[DrawCmd] {
        &self.cmds
    }

    /// The union of every command's device bounds.
    #[must_use]
    pub fn bounds(&self) -> DeviceRect {
        self.bounds
    }

    /// Whether any command reads the destination.
    #[must_use]
    pub fn needs_dst_read(&self) -> bool {
        self.needs_dst_read
    }

    /// The view this list was built for.
    #[must_use]
    pub fn view(&self) -> &ViewParams {
        &self.view
    }

    /// How many commands there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.cmds.len()
    }

    /// Whether the list draws nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cmds.is_empty()
    }
}

/// Finds the transparency of the `PushLayer` matching this `PopLayer`.
fn last_layer_transparency(ops: &[SceneOp], this: &SceneOp) -> (BlendFamily, Transparency) {
    // Walk backwards from this pop, counting nesting.
    let Some(pos) = ops.iter().position(|o| std::ptr::eq(o, this)) else {
        return (BlendFamily::Mix, Transparency::OPAQUE);
    };
    let mut depth = 0i32;
    for op in ops[..pos].iter().rev() {
        match op {
            SceneOp::PopLayer => depth += 1,
            SceneOp::PushLayer { transparency, .. } => {
                if depth == 0 {
                    return (transparency.family, transparency.clone());
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    (BlendFamily::Mix, Transparency::OPAQUE)
}

/// The device bounds of a path, padded in document units before transform.
///
/// Control-point bounds plus one pixel of antialiasing slack: conservative,
/// cheap, and a superset, which is all culling needs.
#[must_use]
pub fn device_bounds_of(path: &PathRef, xf: Transform2D, pad_doc: f64) -> DeviceRect {
    let r = path.bounds();
    if r.x1 < r.x0 || r.y1 < r.y0 {
        return DeviceRect::EMPTY;
    }
    let r = kurbo::Rect::new(r.x0 - pad_doc, r.y0 - pad_doc, r.x1 + pad_doc, r.y1 + pad_doc);
    let a = xf.to_affine();
    let corners = [
        a * kurbo::Point::new(r.x0, r.y0),
        a * kurbo::Point::new(r.x1, r.y0),
        a * kurbo::Point::new(r.x0, r.y1),
        a * kurbo::Point::new(r.x1, r.y1),
    ];
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for c in corners {
        x0 = x0.min(c.x);
        y0 = y0.min(c.y);
        x1 = x1.max(c.x);
        y1 = y1.max(c.y);
    }
    // One pixel of slack for antialiasing spread.
    DeviceRect::enclosing(kurbo::Rect::new(x0, y0, x1, y1)).inflated(1)
}

/// The device bounds of a gradient or image mapping.
fn mapping_bounds(m: GradMapping) -> DeviceRect {
    let pts = match m {
        GradMapping::Affine { a, b, c } => {
            // The parallelogram's fourth corner is b + c - a.
            vec![a, b, c, kurbo::Point::new(b.x + c.x - a.x, b.y + c.y - a.y)]
        }
        GradMapping::Perspective { a, b, c, d } => vec![a, b, c, d],
    };
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in pts {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    DeviceRect::enclosing(kurbo::Rect::new(x0, y0, x1, y1)).inflated(1)
}

/// Moves a paint's control points into device space, so that the backends
/// evaluate every paint in the same frame the geometry is rasterised in.
fn map_paint(paint: &Paint, xf: Transform2D) -> Paint {
    match paint {
        Paint::Solid(_) | Paint::Fractal(_) => paint.clone(),
        Paint::Gradient {
            shape,
            mapping,
            repeat,
            ramp,
        } => Paint::Gradient {
            shape: *shape,
            mapping: mapping.transformed(xf),
            repeat: *repeat,
            ramp: ramp.clone(),
        },
        Paint::Image {
            image,
            mapping,
            repeat,
            filter,
            contone,
            adjust,
        } => Paint::Image {
            image: *image,
            mapping: mapping.transformed(xf),
            repeat: *repeat,
            filter: *filter,
            contone: *contone,
            adjust: *adjust,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{CacheHint, SceneBuilder};
    use xarast_color::Rgba8;
    use xarast_geom::{Mp, Path, Point, Rect};

    fn rect_path(x0: f64, y0: f64, x1: f64, y1: f64) -> PathRef {
        let mut b = Path::builder();
        b.rect(Rect::new(
            Point::new(Mp::from_pt(x0), Mp::from_pt(y0)),
            Point::new(Mp::from_pt(x1), Mp::from_pt(y1)),
        ));
        PathRef::new(b.build())
    }

    fn view() -> ViewParams {
        // One point per pixel keeps the arithmetic readable in tests.
        ViewParams::new(
            200,
            200,
            Transform2D::scale(1.0 / 1000.0),
            RenderQuality::Final,
        )
    }

    #[test]
    fn a_fill_survives_with_its_device_bounds() {
        let mut scene = Scene::new();
        let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
        b.fill(
            SceneNodeId(1),
            &rect_path(10.0, 10.0, 20.0, 20.0),
            FillRule::NonZero,
            Paint::Solid(Rgba8::BLACK),
        );
        b.finish().unwrap();
        let dl = DisplayList::build(&scene, &view(), &DirtyRect::NONE);
        assert_eq!(dl.len(), 1);
        let bounds = dl.commands()[0].bounds().unwrap();
        assert!(bounds.x0 <= 10 && bounds.x1 >= 20);
        assert!(!dl.needs_dst_read());
    }

    #[test]
    fn a_dirty_rect_culls_what_it_does_not_touch() {
        let mut scene = Scene::new();
        let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
        b.fill(
            SceneNodeId(1),
            &rect_path(0.0, 0.0, 5.0, 5.0),
            FillRule::NonZero,
            Paint::Solid(Rgba8::BLACK),
        );
        b.fill(
            SceneNodeId(2),
            &rect_path(100.0, 100.0, 150.0, 150.0),
            FillRule::NonZero,
            Paint::Solid(Rgba8::WHITE),
        );
        b.finish().unwrap();
        let dirty = DirtyRect::of(DeviceRect::new(0, 0, 20, 20));
        let dl = DisplayList::build(&scene, &view(), &dirty);
        assert_eq!(dl.len(), 1, "only the first fill is in the dirty rect");
    }

    #[test]
    fn group_transforms_compose_into_each_command() {
        let mut scene = Scene::new();
        let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
        b.push_group(
            SceneNodeId(1),
            Transform2D::translate(50_000.0, 0.0),
            CacheHint::Auto,
        );
        b.fill(
            SceneNodeId(2),
            &rect_path(0.0, 0.0, 10.0, 10.0),
            FillRule::NonZero,
            Paint::Solid(Rgba8::BLACK),
        );
        b.pop_group();
        b.finish().unwrap();
        let dl = DisplayList::build(&scene, &view(), &DirtyRect::NONE);
        let bounds = dl.commands()[0].bounds().unwrap();
        assert!(bounds.x0 >= 48, "the group translation moved it: {bounds:?}");
    }

    #[test]
    fn a_destination_reading_transparency_marks_the_list() {
        let mut scene = Scene::new();
        let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
        b.push_transparency(Transparency::flat(BlendFamily::StainedGlass, 128));
        b.fill(
            SceneNodeId(1),
            &rect_path(0.0, 0.0, 10.0, 10.0),
            FillRule::NonZero,
            Paint::Solid(Rgba8::BLACK),
        );
        b.pop_transparency();
        b.finish().unwrap();
        let dl = DisplayList::build(&scene, &view(), &DirtyRect::NONE);
        assert!(dl.needs_dst_read());
        assert!(dl.commands()[0].needs_dst_read());
    }

    #[test]
    fn a_pop_layer_carries_the_matching_pushs_transparency() {
        let mut scene = Scene::new();
        let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
        b.push_layer(
            LayerKind::Isolated,
            Transparency::flat(BlendFamily::Bleach, 64),
        );
        b.fill(
            SceneNodeId(1),
            &rect_path(0.0, 0.0, 10.0, 10.0),
            FillRule::NonZero,
            Paint::Solid(Rgba8::BLACK),
        );
        b.pop_layer();
        b.finish().unwrap();
        let dl = DisplayList::build(&scene, &view(), &DirtyRect::NONE);
        let last = dl.commands().last().unwrap();
        match last {
            DrawCmd::PopLayer { blend, opacity } => {
                assert_eq!(*blend, BlendFamily::Bleach);
                assert!(matches!(
                    opacity.source,
                    crate::blend::TranspSource::Flat(64)
                ));
            }
            other => panic!("expected a PopLayer, got {other:?}"),
        }
    }

    #[test]
    fn draft_tolerance_is_five_times_final() {
        let mut v = view();
        v.quality = RenderQuality::Final;
        let fine = v.tolerance();
        v.quality = RenderQuality::Draft;
        assert!((v.tolerance() - fine * 5.0).abs() < 1e-9);
    }
}
