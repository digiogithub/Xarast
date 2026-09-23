//! The fill tool and the transparency tool (phase 8, W8.4): one generic
//! state machine, [`FillLikeTool`], over two payloads.
//!
//! ```text
//! Idle ──hover handle──▶ HoverHandle ──press──▶ DragHandle ──release──▶ Idle (one step)
//!   │                                                │
//!   │                                          (Esc) cancel → nothing happened
//!   ├──press on an object (or the selection)──▶ DragNewGradient ──release──▶ Idle
//!   ├──double click on an arm──▶ insert a stop there (selected)
//!   └──click a handle──▶ the handle is selected; the infobar rebinds
//! ```
//!
//! # Live feedback without mutation
//!
//! A drag never touches the document before the button comes up. Every
//! frame the tool recomputes the dragged fill from the value it snapshotted
//! at the press and puts it in [`Preview::attrs`](crate::tool::Preview): the
//! walker draws each object with that value standing in for its own fill
//! attribute. The release emits one [`EditCommand::Fill`]; `Esc` drops the
//! preview and the document is exactly as it was — no command to undo, no
//! history entry to drop (`phase-08 §W8.4` asks for "restore the exact
//! pre-drag value"; not having changed it is the strongest form of that).
//!
//! # Several objects, one handle set
//!
//! The selected objects are grouped by the fill in force on them; objects
//! whose fills are equal share one handle set, and dragging it edits them
//! all (T8.3.4). Objects with different fills each show their own.

use xarast_color::{Colour, ColourValue, FillEffect, Stop, TranspMode, Transparency};
use xarast_doc::fill::{FillGeometry, Ramp, RampMapping, Tiling};
use xarast_doc::fill_edit::{
    FillChannel, FillHandle, FillValue, InsertStop, MoveFillControl, MoveStop, PaintSlot,
    RemoveStop, SetFillEffect, SetFillGeometry, SetFillProfile, SetRampMapping, SetStopValue,
    SetTiling, SetTranspMode, StopTarget, StopValue, fill_in_force, move_control, ramp_move,
    stop_value,
};
use xarast_doc::fill_mutate::{FillShape, MutateFill, clear_end, desaturated, mutate_fill};
use xarast_doc::{AttrSlot, AttrValue, Document, NodeId};
use xarast_geom::{BiasGain, Matrix, Point};

use crate::edit::{EditState, SelectMode, ToolId};
use crate::fill_handles::{FILL_PICK_RADIUS_PX, FillHandles, FillHit, fill_handles, hit_handle};
use crate::geometry::DocPoint;
use crate::ops::EditCommand;
use crate::tool::{
    CursorKind, GestureEvent, Infobar, InfobarField, InfobarItem, InfobarValue, InteractionState,
    OverlayShape, Tool, ToolAction, ToolCtx, ToolView,
};
use crate::viewport::Viewport;

/// One phase-8 fill or transparency edit, as `xarast_doc` defines it.
#[derive(Debug, Clone, PartialEq)]
pub enum FillCommand {
    /// Replace the fill.
    SetGeometry(SetFillGeometry),
    /// Move a control point.
    MoveControl(MoveFillControl),
    /// Add a ramp stop.
    InsertStop(InsertStop),
    /// Move a ramp stop.
    MoveStop(MoveStop),
    /// Remove a ramp stop.
    RemoveStop(RemoveStop),
    /// Set one stop's value.
    SetStopValue(SetStopValue),
    /// Set the profile.
    SetProfile(SetFillProfile),
    /// Set the ramp mapping.
    SetMapping(SetRampMapping),
    /// Set the colour effect.
    SetEffect(SetFillEffect),
    /// Set the tiling.
    SetTiling(SetTiling),
    /// Set the transparency mode.
    SetTranspMode(SetTranspMode),
    /// Change the fill's shape.
    Mutate(MutateFill),
}

impl FillCommand {
    /// The object it edits.
    #[must_use]
    pub fn node(&self) -> NodeId {
        match self {
            FillCommand::SetGeometry(c) => c.node,
            FillCommand::MoveControl(c) => c.node,
            FillCommand::InsertStop(c) => c.node,
            FillCommand::MoveStop(c) => c.node,
            FillCommand::RemoveStop(c) => c.node,
            FillCommand::SetStopValue(c) => c.node,
            FillCommand::SetProfile(c) => c.node,
            FillCommand::SetMapping(c) => c.node,
            FillCommand::SetEffect(c) => c.node,
            FillCommand::SetTiling(c) => c.node,
            FillCommand::SetTranspMode(c) => c.node,
            FillCommand::Mutate(c) => c.node,
        }
    }

    /// The document command.
    #[must_use]
    pub fn command(&self) -> &dyn xarast_doc::Command {
        match self {
            FillCommand::SetGeometry(c) => c,
            FillCommand::MoveControl(c) => c,
            FillCommand::InsertStop(c) => c,
            FillCommand::MoveStop(c) => c,
            FillCommand::RemoveStop(c) => c,
            FillCommand::SetStopValue(c) => c,
            FillCommand::SetProfile(c) => c,
            FillCommand::SetMapping(c) => c,
            FillCommand::SetEffect(c) => c,
            FillCommand::SetTiling(c) => c,
            FillCommand::SetTranspMode(c) => c,
            FillCommand::Mutate(c) => c,
        }
    }
}

/// What distinguishes the fill tool from the transparency tool.
pub trait FillKind: Send + std::fmt::Debug + 'static {
    /// The payload at the stops.
    type S: Stop + Send;
    /// The channel it edits.
    const CHANNEL: FillChannel;
    /// The tool it is.
    const TOOL: ToolId;
    /// The fill of this channel, when `v` carries it.
    fn unwrap(v: FillValue) -> Option<FillGeometry<Self::S>>;
    /// Wraps a fill.
    fn wrap(g: FillGeometry<Self::S>) -> FillValue;
    /// The interior attribute holding `g`.
    fn attr(g: FillGeometry<Self::S>) -> AttrValue;
    /// The two end values a new gradient dragged out over `base` gets.
    fn new_ends(doc: &Document, base: &FillGeometry<Self::S>) -> (Self::S, Self::S);
    /// A stop value in command form.
    fn stop_value(s: &Self::S) -> StopValue;
    /// The blend mode a stop carries (mix for colour stops, and for a
    /// transparency in no mode).
    fn transp_mode(s: &Self::S) -> TranspMode {
        let _ = s;
        TranspMode::Mix
    }
}

/// The colour payload: the fill tool.
#[derive(Debug, Clone, Copy, Default)]
pub struct ColourFill;

/// The transparency payload: the transparency tool.
#[derive(Debug, Clone, Copy, Default)]
pub struct TranspFill;

impl FillKind for ColourFill {
    type S = Colour;
    const CHANNEL: FillChannel = FillChannel::Colour;
    const TOOL: ToolId = ToolId::Fill;

    fn unwrap(v: FillValue) -> Option<FillGeometry<Colour>> {
        match v {
            FillValue::Colour(g) => Some(g),
            FillValue::Transparency(_) => None,
        }
    }

    fn wrap(g: FillGeometry<Colour>) -> FillValue {
        FillValue::Colour(g)
    }

    fn attr(g: FillGeometry<Colour>) -> AttrValue {
        AttrValue::Fill(g)
    }

    /// A flat colour runs to the same colour at zero saturation (the
    /// mutation rule of `phase-08 §W8.2`); a gradient keeps its ends.
    fn new_ends(doc: &Document, base: &FillGeometry<Colour>) -> (Colour, Colour) {
        match base {
            FillGeometry::Flat { value } => (value.clone(), desaturated(doc, value)),
            g => (
                stop_value(g, StopTarget::From)
                    .or_else(|| stop_value(g, StopTarget::Corner(0)))
                    .unwrap_or(Colour::Direct(ColourValue::WHITE)),
                stop_value(g, StopTarget::To)
                    .or_else(|| stop_value(g, StopTarget::Corner(1)))
                    .unwrap_or(Colour::Direct(ColourValue::BLACK)),
            ),
        }
    }

    fn stop_value(s: &Colour) -> StopValue {
        StopValue::Colour(s.clone())
    }
}

impl FillKind for TranspFill {
    type S = Transparency;
    const CHANNEL: FillChannel = FillChannel::Transparency;
    const TOOL: ToolId = ToolId::Transparency;

    fn unwrap(v: FillValue) -> Option<FillGeometry<Transparency>> {
        match v {
            FillValue::Transparency(g) => Some(g),
            FillValue::Colour(_) => None,
        }
    }

    fn wrap(g: FillGeometry<Transparency>) -> FillValue {
        FillValue::Transparency(g)
    }

    fn attr(g: FillGeometry<Transparency>) -> AttrValue {
        AttrValue::TranspFill(g)
    }

    /// Opaque to clear, as the original's new transparency fill
    /// (`Kernel/opgrad.cpp:2796-2802`: start 0, end 255), keeping a mode
    /// the object already has.
    fn new_ends(
        _doc: &Document,
        base: &FillGeometry<Transparency>,
    ) -> (Transparency, Transparency) {
        let first = stop_value(base, StopTarget::From)
            .or_else(|| stop_value(base, StopTarget::Corner(0)))
            .unwrap_or(Transparency::OPAQUE);
        let end = clear_end(&first);
        (
            Transparency {
                level: 0,
                mode: end.mode,
            },
            end,
        )
    }

    fn stop_value(s: &Transparency) -> StopValue {
        StopValue::Transparency(s.level)
    }

    fn transp_mode(s: &Transparency) -> TranspMode {
        match s.mode {
            TranspMode::None => TranspMode::Mix,
            m => m,
        }
    }
}

/// Selected objects sharing one fill: one handle set.
#[derive(Debug, Clone, PartialEq)]
pub struct FillSet<S: Stop> {
    /// The objects.
    pub nodes: Vec<NodeId>,
    /// Their common fill.
    pub fill: FillGeometry<S>,
}

/// Groups the selected objects by the fill of `channel` in force on each.
#[must_use]
pub fn fill_sets<K: FillKind>(doc: &Document, edit: &EditState) -> Vec<FillSet<K::S>> {
    let mut out: Vec<FillSet<K::S>> = Vec::new();
    for n in edit.selection() {
        if !doc.tree.contains(n) {
            continue;
        }
        let Some(g) = K::unwrap(fill_in_force(doc, n, PaintSlot::Fill, K::CHANNEL)) else {
            continue;
        };
        match out.iter_mut().find(|s| s.fill == g) {
            Some(s) => s.nodes.push(n),
            None => out.push(FillSet {
                nodes: vec![n],
                fill: g,
            }),
        }
    }
    out
}

/// The handle under a document point, over every set, topmost set first.
fn hit_sets<S: Stop>(
    sets: &[FillSet<S>],
    vp: &Viewport,
    at: DocPoint,
) -> Option<(usize, FillHandles, FillHit)> {
    let p = vp.doc_to_device(at);
    sets.iter().enumerate().rev().find_map(|(i, s)| {
        let h = fill_handles(&s.fill, &Matrix::IDENTITY);
        hit_handle(&h, vp, p, FILL_PICK_RADIUS_PX).map(|hit| (i, h, hit))
    })
}

#[derive(Debug, Clone)]
enum Drag<S: Stop> {
    /// A handle of a set.
    Handle {
        nodes: Vec<NodeId>,
        handle: FillHandle,
        start: FillGeometry<S>,
        /// Where the handle was at the press.
        origin: Point,
        /// The last computed target point, for the commit.
        to: Point,
    },
    /// A new gradient being dragged out.
    New {
        nodes: Vec<NodeId>,
        base: FillGeometry<S>,
        shape: FillShape,
        to: Option<FillGeometry<S>>,
    },
}

/// The fill tool and the transparency tool: one machine, two payloads.
#[derive(Debug)]
pub struct FillLikeTool<K: FillKind> {
    /// The handle the infobar edits: the set's objects and the handle.
    selected: Option<(Vec<NodeId>, FillHandle)>,
    drag: Option<Drag<K::S>>,
    /// Whether the pointer is over a handle (for the cursor).
    over_handle: bool,
    /// The shape a new gradient takes: the infobar's type while nothing
    /// is selected. Flat drags out a linear fill, as the original does.
    shape: FillShape,
    _kind: std::marker::PhantomData<K>,
}

impl<K: FillKind> Default for FillLikeTool<K> {
    fn default() -> Self {
        FillLikeTool {
            selected: None,
            drag: None,
            over_handle: false,
            shape: FillShape::Linear,
            _kind: std::marker::PhantomData,
        }
    }
}

/// The fill tool.
pub type GradFillTool = FillLikeTool<ColourFill>;
/// The transparency tool.
pub type TransparencyTool = FillLikeTool<TranspFill>;

/// The transparency modes the infobar offers, in menu order.
pub const TRANSP_MODES: [TranspMode; 9] = [
    TranspMode::Mix,
    TranspMode::StainedGlass,
    TranspMode::Bleach,
    TranspMode::Contrast,
    TranspMode::Saturation,
    TranspMode::Darken,
    TranspMode::Lighten,
    TranspMode::Brightness,
    TranspMode::Luminosity,
];

/// The label of a transparency mode.
#[must_use]
pub const fn transp_mode_label(m: TranspMode) -> &'static str {
    match m {
        TranspMode::None | TranspMode::Mix => "Mix",
        TranspMode::StainedGlass => "Stained glass",
        TranspMode::Bleach => "Bleach",
        TranspMode::Contrast => "Contrast",
        TranspMode::Saturation => "Saturation",
        TranspMode::Darken => "Darken",
        TranspMode::Lighten => "Lighten",
        TranspMode::Brightness => "Brightness",
        TranspMode::Luminosity => "Luminosity",
    }
}

const EFFECTS: [FillEffect; 3] = [
    FillEffect::Fade,
    FillEffect::Rainbow,
    FillEffect::AltRainbow,
];

fn is_graduated<S: Stop>(g: &FillGeometry<S>) -> bool {
    matches!(
        g,
        FillGeometry::Linear { .. }
            | FillGeometry::Radial { .. }
            | FillGeometry::Conical { .. }
            | FillGeometry::Diamond { .. }
    )
}

/// The tiling the "Repeating" choice writes: a graduated fill tiles only
/// under the "extra" mapping (`research/01 §8.3`), a three/four-colour fill
/// under any mapping but `Simple`.
fn repeating<S: Stop>(g: &FillGeometry<S>) -> Tiling {
    if is_graduated(g) {
        Tiling::RepeatExtra
    } else {
        Tiling::Repeat
    }
}

fn is_repeating<S: Stop>(g: &FillGeometry<S>, t: Tiling) -> bool {
    if is_graduated(g) {
        t == Tiling::RepeatExtra
    } else {
        t != Tiling::Simple
    }
}

/// The node's own-or-inherited value of an attribute slot.
fn attr_in_force(doc: &Document, node: NodeId, slot: AttrSlot) -> AttrValue {
    xarast_doc::attr::resolve_uncached(&doc.tree, node, &doc.defaults)
        .get(slot)
        .clone()
}

impl<K: FillKind> FillLikeTool<K> {
    fn sets(doc: &Document, edit: &EditState) -> Vec<FillSet<K::S>> {
        fill_sets::<K>(doc, edit)
    }

    /// The set the selected handle belongs to, if it is still shown.
    fn selected_set<'a>(
        &self,
        sets: &'a [FillSet<K::S>],
    ) -> Option<(&'a FillSet<K::S>, FillHandle)> {
        let (nodes, h) = self.selected.as_ref()?;
        let set = sets
            .iter()
            .find(|s| s.nodes.iter().any(|n| nodes.contains(n)))?;
        Some((set, *h))
    }

    fn emit(cx: &mut ToolCtx<'_>, edits: Vec<FillCommand>) {
        if !edits.is_empty() {
            cx.commands.emit(EditCommand::Fill { edits });
        }
    }

    fn slot_cmds(nodes: &[NodeId], f: impl Fn(NodeId) -> FillCommand) -> Vec<FillCommand> {
        nodes.iter().map(|n| f(*n)).collect()
    }

    /// The handle's new point: snapped, and with Constrain held, turned to
    /// the nearest 15° about the arm's other end.
    fn target(
        cx: &mut ToolCtx<'_>,
        g: &FillGeometry<K::S>,
        handle: FillHandle,
        to: Point,
    ) -> Point {
        let to = cx.snap_point(to);
        if !cx.modifiers.constrain || matches!(handle, FillHandle::Stop(_)) {
            return to;
        }
        let anchor = match (g, handle) {
            (FillGeometry::Linear { start, .. }, FillHandle::End) => *start,
            (FillGeometry::Linear { end, .. }, FillHandle::Start) => *end,
            (
                FillGeometry::Radial { centre, .. }
                | FillGeometry::Conical { centre, .. }
                | FillGeometry::Diamond { centre, .. },
                FillHandle::Major
                | FillHandle::Minor
                | FillHandle::End
                | FillHandle::Corner1
                | FillHandle::Corner2,
            ) => *centre,
            (
                FillGeometry::ThreeColour { origin, .. } | FillGeometry::FourColour { origin, .. },
                FillHandle::End | FillHandle::End2 | FillHandle::End3,
            ) => *origin,
            _ => return to,
        };
        let (ax, ay) = anchor.to_f64();
        let (px, py) = to.to_f64();
        let (dx, dy) = (px - ax, py - ay);
        let len = dx.hypot(dy);
        if len <= 0.0 {
            return to;
        }
        let step = 15f64.to_radians();
        let a = (dy.atan2(dx) / step).round() * step;
        Point::from_f64_round(ax + len * a.cos(), ay + len * a.sin())
    }

    /// The fill a new drag from `a` to `b` makes.
    fn new_fill(
        doc: &Document,
        base: &FillGeometry<K::S>,
        shape: FillShape,
        a: Point,
        b: Point,
    ) -> Option<FillGeometry<K::S>> {
        if a == b {
            return None;
        }
        let (from, to) = K::new_ends(doc, base);
        let ramp = match base {
            FillGeometry::Linear { ramp, .. }
            | FillGeometry::Radial { ramp, .. }
            | FillGeometry::Conical { ramp, .. }
            | FillGeometry::Diamond { ramp, .. } => ramp.clone(),
            _ => Ramp::new(),
        };
        let linear = FillGeometry::Linear {
            start: a,
            end: b,
            persp: None,
            from,
            to: to.clone(),
            ramp,
        };
        let shape = if shape == FillShape::Flat {
            FillShape::Linear
        } else {
            shape
        };
        mutate_fill(&linear, shape, xarast_geom::Rect::new(a, b), to)
            .ok()
            .map(|(g, _)| g)
    }

    fn preview(cx: &mut ToolCtx<'_>, nodes: &[NodeId], g: &FillGeometry<K::S>) {
        cx.preview.attrs = nodes.iter().map(|n| (*n, K::attr(g.clone()))).collect();
    }

    fn on_drag_start(
        &mut self,
        from: DocPoint,
        hit: Option<crate::tool::HitResult>,
        cx: &mut ToolCtx<'_>,
    ) {
        let sets = Self::sets(cx.doc, cx.edit);
        if let Some((i, h, FillHit::Handle(handle))) = hit_sets(&sets, cx.viewport, from) {
            let set = &sets[i];
            let origin = h
                .handles
                .iter()
                .find(|x| x.id == handle)
                .map_or(from, |x| x.pos);
            self.selected = Some((set.nodes.clone(), handle));
            self.drag = Some(Drag::Handle {
                nodes: set.nodes.clone(),
                handle,
                start: set.fill.clone(),
                origin,
                to: origin,
            });
            cx.requests.overlay_changed = true;
            return;
        }
        // A new gradient, over the object under the press — or over the
        // whole selection when the press is on a selected object or on
        // nothing at all.
        let nodes: Vec<NodeId> = match hit {
            Some(h) if !cx.edit.is_selected(h.top_group) => {
                cx.requests.select(vec![h.top_group], SelectMode::Replace);
                vec![h.top_group]
            }
            _ => cx.edit.selection().collect(),
        };
        let Some(&first) = nodes.first() else {
            return;
        };
        let Some(base) = K::unwrap(fill_in_force(cx.doc, first, PaintSlot::Fill, K::CHANNEL))
        else {
            return;
        };
        let shape = if cx.modifiers.adjust {
            FillShape::Circular
        } else {
            FillShape::of(&base)
                .filter(|s| *s != FillShape::Flat)
                .unwrap_or(self.shape)
        };
        self.selected = None;
        self.drag = Some(Drag::New {
            nodes,
            base,
            shape,
            to: None,
        });
    }

    fn on_drag_update(&mut self, from: DocPoint, to: DocPoint, cx: &mut ToolCtx<'_>) {
        let doc = cx.doc;
        match &mut self.drag {
            Some(Drag::Handle {
                nodes,
                handle,
                start,
                origin,
                to: last,
            }) => {
                let target = *origin + (to - from);
                let target = Self::target(cx, start, *handle, target);
                *last = target;
                let mut g = start.clone();
                if move_control(&mut g, *handle, target).is_ok() {
                    let nodes = nodes.clone();
                    Self::preview(cx, &nodes, &g);
                }
                cx.requests.overlay_changed = true;
            }
            Some(Drag::New {
                nodes,
                base,
                shape,
                to: out,
            }) => {
                let b = cx.snap_point(to);
                let shape = if cx.modifiers.adjust {
                    FillShape::Circular
                } else {
                    *shape
                };
                *out = Self::new_fill(doc, base, shape, from, b);
                if let Some(g) = out.clone() {
                    let nodes = nodes.clone();
                    Self::preview(cx, &nodes, &g);
                }
                cx.requests.overlay_changed = true;
            }
            None => {}
        }
    }

    fn on_drag_end(&mut self, cx: &mut ToolCtx<'_>) {
        match self.drag.take() {
            Some(Drag::Handle {
                nodes,
                handle,
                start,
                to,
                ..
            }) => {
                let edits = match handle {
                    FillHandle::Stop(i) => {
                        let Some((a, b)) = xarast_doc::fill_edit::fill_arm(&start) else {
                            return;
                        };
                        let pos = xarast_doc::fill_edit::arm_position(a, b, to);
                        // Keep hold of the stop across a re-sort.
                        if let Some(r) = ramp_ref(&start)
                            && let Ok((_, j)) = ramp_move(r, usize::from(i), pos)
                            && let Ok(j) = u16::try_from(j)
                        {
                            self.selected = Some((nodes.clone(), FillHandle::Stop(j)));
                        }
                        Self::slot_cmds(&nodes, |node| {
                            FillCommand::MoveStop(MoveStop {
                                node,
                                slot: PaintSlot::Fill,
                                channel: K::CHANNEL,
                                index: i,
                                pos,
                                drag: None,
                            })
                        })
                    }
                    _ => Self::slot_cmds(&nodes, |node| {
                        FillCommand::MoveControl(MoveFillControl {
                            node,
                            slot: PaintSlot::Fill,
                            channel: K::CHANNEL,
                            handle,
                            to,
                            drag: None,
                        })
                    }),
                };
                Self::emit(cx, edits);
            }
            Some(Drag::New {
                nodes, to: Some(g), ..
            }) => {
                // The end blob is selected, as the original leaves it.
                let end = match &g {
                    FillGeometry::Radial { .. } => FillHandle::Major,
                    FillGeometry::Diamond { .. } => FillHandle::Corner1,
                    _ => FillHandle::End,
                };
                self.selected = Some((nodes.clone(), end));
                let edits = Self::slot_cmds(&nodes, |node| {
                    FillCommand::SetGeometry(SetFillGeometry {
                        node,
                        slot: PaintSlot::Fill,
                        value: K::wrap(g.clone()),
                    })
                });
                Self::emit(cx, edits);
            }
            Some(Drag::New { to: None, .. }) | None => {}
        }
        cx.requests.overlay_changed = true;
    }

    fn on_click(
        &mut self,
        at: DocPoint,
        hit: Option<crate::tool::HitResult>,
        count: u8,
        cx: &mut ToolCtx<'_>,
    ) {
        let sets = Self::sets(cx.doc, cx.edit);
        match hit_sets(&sets, cx.viewport, at) {
            Some((i, _, FillHit::Handle(h))) => {
                self.selected = Some((sets[i].nodes.clone(), h));
                cx.requests.overlay_changed = true;
                return;
            }
            Some((i, _, FillHit::Arm(pos))) if count >= 2 => {
                let set = &sets[i];
                let Some(r) = ramp_ref(&set.fill) else { return };
                let (from, to) = match (
                    stop_value(&set.fill, StopTarget::From),
                    stop_value(&set.fill, StopTarget::To),
                ) {
                    (Some(a), Some(b)) => (a, b),
                    _ => return,
                };
                let value = r.sample(&from, &to, pos, FillEffect::Fade);
                let index = r.stops().partition_point(|s| s.pos <= pos);
                if let Ok(index) = u16::try_from(index) {
                    self.selected = Some((set.nodes.clone(), FillHandle::Stop(index)));
                }
                let value = K::stop_value(&value);
                let edits = Self::slot_cmds(&set.nodes, |node| {
                    FillCommand::InsertStop(InsertStop {
                        node,
                        slot: PaintSlot::Fill,
                        channel: K::CHANNEL,
                        pos,
                        value: value.clone(),
                    })
                });
                Self::emit(cx, edits);
                cx.requests.overlay_changed = true;
                return;
            }
            _ => {}
        }
        // Not on a handle: select as the selector does.
        let adjust = cx.modifiers.adjust;
        match hit {
            Some(h) => cx.requests.select(
                vec![h.top_group],
                if adjust {
                    SelectMode::Toggle
                } else {
                    SelectMode::Replace
                },
            ),
            None if !adjust => cx.requests.select(Vec::new(), SelectMode::Replace),
            None => {}
        }
        self.selected = None;
        cx.requests.overlay_changed = true;
    }
}

fn ramp_ref<S: Stop>(g: &FillGeometry<S>) -> Option<&Ramp<S>> {
    match g {
        FillGeometry::Linear { ramp, .. }
        | FillGeometry::Radial { ramp, .. }
        | FillGeometry::Conical { ramp, .. }
        | FillGeometry::Diamond { ramp, .. } => Some(ramp),
        _ => None,
    }
}

impl<K: FillKind> Tool for FillLikeTool<K> {
    fn id(&self) -> ToolId {
        K::TOOL
    }

    fn on_deactivate(&mut self, _cx: &mut ToolCtx<'_>) {
        self.selected = None;
        self.drag = None;
        self.over_handle = false;
    }

    fn on_gesture(&mut self, ev: &GestureEvent, cx: &mut ToolCtx<'_>) {
        match ev {
            GestureEvent::Hover { at } => {
                let sets = Self::sets(cx.doc, cx.edit);
                self.over_handle = matches!(
                    hit_sets(&sets, cx.viewport, *at),
                    Some((_, _, FillHit::Handle(_)))
                );
            }
            GestureEvent::Click { at, hit, count } => self.on_click(*at, *hit, *count, cx),
            GestureEvent::DragStart { from, hit } => self.on_drag_start(*from, *hit, cx),
            GestureEvent::DragUpdate { from, to, .. } => self.on_drag_update(*from, *to, cx),
            GestureEvent::DragEnd { .. } => self.on_drag_end(cx),
            GestureEvent::Cancel => {
                self.drag = None;
                cx.requests.overlay_changed = true;
            }
            GestureEvent::ModifiersChanged { .. } => {}
        }
    }

    fn overlay(&self, view: ToolView<'_>, out: &mut Vec<OverlayShape>) {
        // While a handle is dragged, draw the handles of the previewed
        // value so they move with the pointer.
        let live: Option<(Vec<NodeId>, FillGeometry<K::S>)> = match &self.drag {
            Some(Drag::Handle { nodes, .. } | Drag::New { nodes, .. }) => view
                .preview
                .attrs
                .iter()
                .find(|(n, _)| nodes.contains(n))
                .and_then(|(_, v)| match v {
                    AttrValue::Fill(g) => K::unwrap(FillValue::Colour(g.clone())),
                    AttrValue::TranspFill(g) => K::unwrap(FillValue::Transparency(g.clone())),
                    _ => None,
                })
                .map(|g| (nodes.clone(), g)),
            None => None,
        };
        let sets = Self::sets(view.doc, view.edit);
        for s in &sets {
            let fill = match &live {
                Some((nodes, g)) if s.nodes.iter().any(|n| nodes.contains(n)) => g,
                _ => &s.fill,
            };
            let selected = self
                .selected
                .as_ref()
                .filter(|(nodes, _)| s.nodes.iter().any(|n| nodes.contains(n)))
                .map(|(_, h)| *h);
            crate::fill_handles::overlay_of(&fill_handles(fill, &Matrix::IDENTITY), selected, out);
        }
    }

    fn infobar(&self, view: ToolView<'_>) -> Infobar {
        let sets = Self::sets(view.doc, view.edit);
        let first = sets.first();
        let mut items = Vec::new();
        let shape = first.map_or(Some(self.shape), |s| FillShape::of(&s.fill));
        let all_same = sets.len() <= 1;
        items.push(InfobarItem::Choice {
            field: InfobarField::FillType,
            options: FillShape::ALL.iter().map(|s| s.label()).collect(),
            selected: shape
                .filter(|_| all_same)
                .and_then(|s| FillShape::ALL.iter().position(|x| *x == s)),
        });
        let Some(set) = first else {
            items.push(InfobarItem::Note(
                "Select an object, then drag across it to make a graduated fill.".to_owned(),
            ));
            return Infobar { items };
        };
        let node = set.nodes[0];
        if K::CHANNEL == FillChannel::Colour {
            let effect = match attr_in_force(view.doc, node, AttrSlot::FillEffect) {
                AttrValue::FillEffect(e) => Some(e),
                _ => None,
            };
            items.push(InfobarItem::Choice {
                field: InfobarField::FillEffect,
                options: vec!["Fade", "Rainbow", "Alt rainbow"],
                selected: effect.and_then(|e| EFFECTS.iter().position(|x| *x == e)),
            });
        } else {
            let mode = stop_value(&set.fill, StopTarget::From)
                .or_else(|| stop_value(&set.fill, StopTarget::Corner(0)))
                .map(|t| K::transp_mode(&t));
            items.push(InfobarItem::Choice {
                field: InfobarField::TranspMode,
                options: TRANSP_MODES.iter().map(|m| transp_mode_label(*m)).collect(),
                selected: mode.and_then(|m| TRANSP_MODES.iter().position(|x| *x == m)),
            });
        }
        let has_points = set.fill.has_control_points();
        if has_points {
            let slot = match K::CHANNEL {
                FillChannel::Colour => AttrSlot::FillMapping,
                FillChannel::Transparency => AttrSlot::TranspFillMapping,
            };
            let tiling = match attr_in_force(view.doc, node, slot) {
                AttrValue::FillMapping(t) | AttrValue::TranspFillMapping(t) => Some(t),
                _ => None,
            };
            items.push(InfobarItem::Choice {
                field: InfobarField::FillTiling,
                options: vec!["Simple", "Repeating"],
                selected: tiling.map(|t| usize::from(is_repeating(&set.fill, t))),
            });
        }
        if is_graduated(&set.fill) {
            items.push(InfobarItem::Choice {
                field: InfobarField::RampMapping,
                options: vec!["Linear", "Sine"],
                selected: Some(usize::from(set.fill.mapping() == RampMapping::Sin)),
            });
            let p = set.fill.profile();
            items.push(InfobarItem::Real {
                field: InfobarField::ProfileBias,
                value: Some(p.bias),
                min: -1.0,
                max: 1.0,
            });
            items.push(InfobarItem::Real {
                field: InfobarField::ProfileGain,
                value: Some(p.gain),
                min: -1.0,
                max: 1.0,
            });
        }
        if let Some((sel, h)) = self.selected_set(&sets) {
            if let FillHandle::Stop(i) = h {
                let pos = ramp_ref(&sel.fill)
                    .and_then(|r| r.stops().get(usize::from(i)))
                    .map(|s| f64::from(s.pos) * 100.0);
                items.push(InfobarItem::Real {
                    field: InfobarField::StopPosition,
                    value: pos,
                    min: 0.0,
                    max: 100.0,
                });
            }
            if K::CHANNEL == FillChannel::Transparency {
                let level = crate::fill_handles::stop_target(&sel.fill, h)
                    .and_then(|t| stop_value(&sel.fill, t))
                    .map(|v| match K::stop_value(&v) {
                        StopValue::Transparency(l) => f64::from(l) * 100.0 / 255.0,
                        StopValue::Colour(_) => 0.0,
                    });
                items.push(InfobarItem::Real {
                    field: InfobarField::StopLevel,
                    value: level,
                    min: 0.0,
                    max: 100.0,
                });
            }
        }
        Infobar { items }
    }

    fn infobar_edit(&mut self, field: InfobarField, value: InfobarValue, cx: &mut ToolCtx<'_>) {
        let sets = Self::sets(cx.doc, cx.edit);
        let all: Vec<NodeId> = sets.iter().flat_map(|s| s.nodes.iter().copied()).collect();
        let channel = K::CHANNEL;
        let slot = PaintSlot::Fill;
        let edits: Vec<FillCommand> = match (field, value) {
            (InfobarField::FillType, InfobarValue::Choice(i)) => {
                let Some(&to) = FillShape::ALL.get(i) else {
                    return;
                };
                self.shape = to;
                Self::slot_cmds(&all, |node| {
                    FillCommand::Mutate(MutateFill {
                        node,
                        slot,
                        channel,
                        to,
                    })
                })
            }
            (InfobarField::FillEffect, InfobarValue::Choice(i)) => {
                let Some(&effect) = EFFECTS.get(i) else {
                    return;
                };
                Self::slot_cmds(&all, |node| {
                    FillCommand::SetEffect(SetFillEffect { node, effect })
                })
            }
            (InfobarField::TranspMode, InfobarValue::Choice(i)) => {
                let Some(&mode) = TRANSP_MODES.get(i) else {
                    return;
                };
                Self::slot_cmds(&all, |node| {
                    FillCommand::SetTranspMode(SetTranspMode { node, slot, mode })
                })
            }
            (InfobarField::FillTiling, InfobarValue::Choice(i)) => sets
                .iter()
                .filter(|s| s.fill.has_control_points())
                .flat_map(|s| {
                    let tiling = if i == 1 {
                        repeating(&s.fill)
                    } else {
                        Tiling::Simple
                    };
                    Self::slot_cmds(&s.nodes, |node| {
                        FillCommand::SetTiling(SetTiling {
                            node,
                            channel,
                            tiling,
                        })
                    })
                })
                .collect(),
            (InfobarField::RampMapping, InfobarValue::Choice(i)) => {
                let mapping = if i == 1 {
                    RampMapping::Sin
                } else {
                    RampMapping::Linear
                };
                sets.iter()
                    .filter(|s| is_graduated(&s.fill))
                    .flat_map(|s| {
                        Self::slot_cmds(&s.nodes, |node| {
                            FillCommand::SetMapping(SetRampMapping {
                                node,
                                slot,
                                channel,
                                mapping,
                            })
                        })
                    })
                    .collect()
            }
            (InfobarField::ProfileBias | InfobarField::ProfileGain, InfobarValue::Real(v)) => sets
                .iter()
                .filter(|s| is_graduated(&s.fill))
                .flat_map(|s| {
                    let old = s.fill.profile();
                    let v = v.clamp(-1.0, 1.0);
                    let profile = if field == InfobarField::ProfileBias {
                        BiasGain { bias: v, ..old }
                    } else {
                        BiasGain { gain: v, ..old }
                    };
                    Self::slot_cmds(&s.nodes, |node| {
                        FillCommand::SetProfile(SetFillProfile {
                            node,
                            slot,
                            channel,
                            profile,
                        })
                    })
                })
                .collect(),
            (InfobarField::StopPosition, InfobarValue::Real(v)) => {
                let Some((set, FillHandle::Stop(i))) = self.selected_set(&sets) else {
                    return;
                };
                let pos = (v / 100.0).clamp(0.0, 1.0) as f32;
                if let Some(r) = ramp_ref(&set.fill)
                    && let Ok((_, j)) = ramp_move(r, usize::from(i), pos)
                    && let Ok(j) = u16::try_from(j)
                {
                    self.selected = Some((set.nodes.clone(), FillHandle::Stop(j)));
                }
                Self::slot_cmds(&set.nodes, |node| {
                    FillCommand::MoveStop(MoveStop {
                        node,
                        slot,
                        channel,
                        index: i,
                        pos,
                        drag: None,
                    })
                })
            }
            (InfobarField::StopLevel, InfobarValue::Real(v)) => {
                let Some((set, h)) = self.selected_set(&sets) else {
                    return;
                };
                let Some(target) = crate::fill_handles::stop_target(&set.fill, h) else {
                    return;
                };
                let level = (v.clamp(0.0, 100.0) * 255.0 / 100.0).round() as u8;
                Self::slot_cmds(&set.nodes, |node| {
                    FillCommand::SetStopValue(SetStopValue {
                        node,
                        slot,
                        channel,
                        target,
                        value: StopValue::Transparency(level),
                    })
                })
            }
            _ => return,
        };
        Self::emit(cx, edits);
        cx.requests.overlay_changed = true;
    }

    fn cursor(&self, state: InteractionState) -> CursorKind {
        match state {
            InteractionState::Dragging if matches!(self.drag, Some(Drag::Handle { .. })) => {
                CursorKind::Move
            }
            InteractionState::Dragging | InteractionState::ArmedDrag => CursorKind::Crosshair,
            _ if self.over_handle => CursorKind::Move,
            _ => CursorKind::Crosshair,
        }
    }

    fn action(&mut self, action: ToolAction, cx: &mut ToolCtx<'_>) -> bool {
        match action {
            ToolAction::Delete => {
                let sets = Self::sets(cx.doc, cx.edit);
                let Some((set, FillHandle::Stop(index))) = self.selected_set(&sets) else {
                    return false;
                };
                let edits = Self::slot_cmds(&set.nodes, |node| {
                    FillCommand::RemoveStop(RemoveStop {
                        node,
                        slot: PaintSlot::Fill,
                        channel: K::CHANNEL,
                        index,
                    })
                });
                self.selected = None;
                Self::emit(cx, edits);
                cx.requests.overlay_changed = true;
                true
            }
            ToolAction::Cancel if self.selected.is_some() => {
                self.selected = None;
                cx.requests.overlay_changed = true;
                true
            }
            _ => false,
        }
    }
}
