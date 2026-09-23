//! The built-in tools.
//!
//! This round implements the selector (click to select, drag to move,
//! marquee), the push tool and the zoom tool. The drawing tools of the
//! phase are registered as [`PendingTool`]s: they can be chosen, say so in
//! their infobar, and do nothing on the canvas.

use xarast_doc::NodeId;
use xarast_geom::{Mp, Vector};

use crate::edit::{SelectMode, ToolId};
use crate::geometry::{DocPoint, DocRect};
use crate::ops::EditCommand;
use crate::tool::{
    CursorKind, GestureEvent, HandleShape, Infobar, InfobarField, InfobarItem, InteractionState,
    OverlayShape, Tool, ToolCtx, ToolView, ViewRequest, pick_enclosed,
};

/// Every tool this build implements or announces, one per [`ToolId`] that
/// [`ToolId::is_available`].
#[must_use]
pub fn builtin() -> Vec<Box<dyn Tool>> {
    let mut tools: Vec<Box<dyn Tool>> = vec![
        Box::new(SelectorTool::default()),
        Box::new(PushTool::default()),
        Box::new(ZoomTool::default()),
    ];
    for id in ToolId::ALL {
        if id.is_available() && !id.is_implemented() {
            tools.push(Box::new(PendingTool { id }));
        }
    }
    tools
}

/// Snaps a displacement to the nearest multiple of 45°, keeping its
/// projection on that direction: the constrained move.
#[must_use]
pub fn constrain_45(v: Vector) -> Vector {
    let (x, y) = (f64::from(v.dx.raw()), f64::from(v.dy.raw()));
    if x == 0.0 && y == 0.0 {
        return v;
    }
    let step = std::f64::consts::FRAC_PI_4;
    let angle = (y.atan2(x) / step).round() * step;
    let (s, c) = angle.sin_cos();
    let len = x * c + y * s;
    let q = |f: f64| Mp::new(f.round().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32);
    Vector::new(q(len * c), q(len * s))
}

fn rect_of(a: DocPoint, b: DocPoint) -> DocRect {
    DocRect::new(a, b)
}

/// The eight bounding-box handles of a rectangle.
fn bounds_handles(r: DocRect, out: &mut Vec<OverlayShape>) {
    let (x0, y0, x1, y1) = (r.lo.x, r.lo.y, r.hi.x, r.hi.y);
    let xm = Mp::new(((i64::from(x0.raw()) + i64::from(x1.raw())) / 2) as i32);
    let ym = Mp::new(((i64::from(y0.raw()) + i64::from(y1.raw())) / 2) as i32);
    for (x, y) in [
        (x0, y0),
        (xm, y0),
        (x1, y0),
        (x0, ym),
        (x1, ym),
        (x0, y1),
        (xm, y1),
        (x1, y1),
    ] {
        out.push(OverlayShape::Handle {
            at: DocPoint::new(x, y),
            shape: HandleShape::Bounds,
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
enum SelectorDrag {
    /// Moving the selection by `delta`.
    Move { nodes: Vec<NodeId>, delta: Vector },
    /// A rubber band from `from` to `to`.
    Marquee { from: DocPoint, to: DocPoint },
}

/// The selector: click selects, drag moves, drag on empty space marquees.
///
/// Implemented this round: click-select (Adjust toggles), move-drag with
/// Constrain snapping to 45°, marquee (Adjust adds), the X/Y fields of the
/// infobar. The scale ⇄ rotate dual state and the handles' own drags are
/// W4.
#[derive(Debug, Default)]
pub struct SelectorTool {
    drag: Option<SelectorDrag>,
}

impl SelectorTool {
    fn delta(cx: &ToolCtx<'_>, from: DocPoint, to: DocPoint) -> Vector {
        let d = to - from;
        if cx.modifiers.constrain {
            constrain_45(d)
        } else {
            d
        }
    }
}

impl Tool for SelectorTool {
    fn id(&self) -> ToolId {
        ToolId::Selector
    }

    fn on_gesture(&mut self, ev: &GestureEvent, cx: &mut ToolCtx<'_>) {
        match ev {
            GestureEvent::Click { hit, .. } => {
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
            }
            GestureEvent::DragStart { from, hit } => {
                self.drag = Some(match hit {
                    Some(h) => {
                        let mut nodes: Vec<NodeId> = if cx.edit.is_selected(h.top_group) {
                            cx.edit.selection().collect()
                        } else if cx.modifiers.adjust {
                            cx.requests.select(vec![h.top_group], SelectMode::Add);
                            cx.edit.selection().chain([h.top_group]).collect()
                        } else {
                            cx.requests.select(vec![h.top_group], SelectMode::Replace);
                            vec![h.top_group]
                        };
                        nodes.dedup();
                        SelectorDrag::Move {
                            nodes,
                            delta: Vector::ZERO,
                        }
                    }
                    None => SelectorDrag::Marquee {
                        from: *from,
                        to: *from,
                    },
                });
                cx.requests.overlay_changed = true;
            }
            GestureEvent::DragUpdate { from, to, .. } => {
                let delta = Self::delta(cx, *from, *to);
                match &mut self.drag {
                    Some(SelectorDrag::Move { nodes, delta: d }) => {
                        *d = delta;
                        cx.preview.transform =
                            Some((nodes.clone(), xarast_geom::Matrix::translate(delta)));
                    }
                    Some(SelectorDrag::Marquee { to: t, .. }) => *t = *to,
                    None => return,
                }
                cx.requests.overlay_changed = true;
            }
            GestureEvent::DragEnd { from, to } => {
                let delta = Self::delta(cx, *from, *to);
                match self.drag.take() {
                    Some(SelectorDrag::Move { nodes, .. }) => {
                        let cmd = EditCommand::translate(nodes, delta);
                        if !cmd.is_noop() {
                            cx.commands.emit(cmd);
                        }
                    }
                    Some(SelectorDrag::Marquee { from, .. }) => {
                        let hits = pick_enclosed(cx.doc, rect_of(from, *to));
                        let mode = if cx.modifiers.adjust {
                            SelectMode::Add
                        } else {
                            SelectMode::Replace
                        };
                        cx.requests.select(hits, mode);
                    }
                    None => {}
                }
                cx.requests.overlay_changed = true;
            }
            GestureEvent::Cancel => {
                self.drag = None;
                cx.requests.overlay_changed = true;
            }
            GestureEvent::Hover { .. } | GestureEvent::ModifiersChanged { .. } => {}
        }
    }

    fn overlay(&self, view: ToolView<'_>, out: &mut Vec<OverlayShape>) {
        if let Some(SelectorDrag::Marquee { from, to }) = &self.drag {
            out.push(OverlayShape::Rect {
                rect: rect_of(*from, *to),
                dashed: true,
            });
        }
        let mut r = view.edit.selection_bounds(view.doc);
        if r.is_empty() {
            return;
        }
        if let Some(SelectorDrag::Move { delta, .. }) = &self.drag {
            r = r.translated(*delta);
        }
        out.push(OverlayShape::Rect {
            rect: r,
            dashed: false,
        });
        bounds_handles(r, out);
    }

    fn infobar(&self, view: ToolView<'_>) -> Infobar {
        let r = view.edit.selection_bounds(view.doc);
        let (x, y, w, h) = if r.is_empty() {
            (None, None, None, None)
        } else {
            (
                Some(r.lo.x),
                Some(r.lo.y),
                Some(r.width()),
                Some(r.height()),
            )
        };
        let editable = !r.is_empty();
        Infobar {
            items: vec![
                InfobarItem::Measure {
                    field: InfobarField::X,
                    value: x,
                    editable,
                },
                InfobarItem::Measure {
                    field: InfobarField::Y,
                    value: y,
                    editable,
                },
                InfobarItem::Measure {
                    field: InfobarField::W,
                    value: w,
                    editable: false,
                },
                InfobarItem::Measure {
                    field: InfobarField::H,
                    value: h,
                    editable: false,
                },
            ],
        }
    }

    fn infobar_edit(&mut self, field: InfobarField, value: Mp, cx: &mut ToolCtx<'_>) {
        let r = cx.edit.selection_bounds(cx.doc);
        if r.is_empty() {
            return;
        }
        let by = match field {
            InfobarField::X => Vector::new(value - r.lo.x, Mp::ZERO),
            InfobarField::Y => Vector::new(Mp::ZERO, value - r.lo.y),
            // Scaling from the bar is W4 (the 9-anchor grid).
            InfobarField::W | InfobarField::H => return,
        };
        let cmd = EditCommand::translate(cx.edit.selection().collect(), by);
        if !cmd.is_noop() {
            cx.commands.emit(cmd);
        }
    }

    fn cursor(&self, state: InteractionState) -> CursorKind {
        match (state, &self.drag) {
            (InteractionState::Dragging, Some(SelectorDrag::Move { .. })) => CursorKind::Move,
            _ => CursorKind::Default,
        }
    }
}

/// The push tool: drag the view.
#[derive(Debug, Default)]
pub struct PushTool {
    last: Option<crate::geometry::DevicePoint>,
}

impl Tool for PushTool {
    fn id(&self) -> ToolId {
        ToolId::Pan
    }

    fn on_gesture(&mut self, ev: &GestureEvent, cx: &mut ToolCtx<'_>) {
        match ev {
            GestureEvent::DragStart { from, .. } => {
                self.last = Some(cx.viewport.doc_to_device(*from));
            }
            GestureEvent::DragUpdate { to_device, .. } => {
                if let Some(l) = self.last {
                    let (dx, dy) = (to_device.x - l.x, to_device.y - l.y);
                    if dx != 0.0 || dy != 0.0 {
                        cx.requests.view.push(ViewRequest::Pan { dx, dy });
                    }
                }
                self.last = Some(*to_device);
            }
            GestureEvent::DragEnd { .. } | GestureEvent::Cancel => self.last = None,
            _ => {}
        }
    }

    fn infobar(&self, _view: ToolView<'_>) -> Infobar {
        Infobar {
            items: vec![InfobarItem::Note("Drag to move the view.".to_owned())],
        }
    }

    fn cursor(&self, state: InteractionState) -> CursorKind {
        if state == InteractionState::Dragging {
            CursorKind::Grabbing
        } else {
            CursorKind::Grab
        }
    }
}

/// The zoom tool: click zooms in, Adjust-click zooms out, drag frames a
/// rectangle.
#[derive(Debug, Default)]
pub struct ZoomTool {
    band: Option<(DocPoint, DocPoint)>,
}

impl Tool for ZoomTool {
    fn id(&self) -> ToolId {
        ToolId::Zoom
    }

    fn on_gesture(&mut self, ev: &GestureEvent, cx: &mut ToolCtx<'_>) {
        match ev {
            GestureEvent::Click { at, .. } => {
                let factor = if cx.modifiers.adjust {
                    1.0 / crate::command::ZOOM_STEP
                } else {
                    crate::command::ZOOM_STEP
                };
                cx.requests.view.push(ViewRequest::ZoomAbout {
                    factor,
                    anchor: cx.viewport.doc_to_device(*at),
                });
            }
            GestureEvent::DragStart { from, .. } => self.band = Some((*from, *from)),
            GestureEvent::DragUpdate { to, .. } => {
                if let Some((_, t)) = &mut self.band {
                    *t = *to;
                    cx.requests.overlay_changed = true;
                }
            }
            GestureEvent::DragEnd { to, .. } => {
                if let Some((from, _)) = self.band.take() {
                    let r = rect_of(from, *to);
                    if !r.is_empty() && r.width() > Mp::ZERO && r.height() > Mp::ZERO {
                        cx.requests.view.push(ViewRequest::ZoomToRect(r));
                    }
                    cx.requests.overlay_changed = true;
                }
            }
            GestureEvent::Cancel => {
                self.band = None;
                cx.requests.overlay_changed = true;
            }
            _ => {}
        }
    }

    fn overlay(&self, _view: ToolView<'_>, out: &mut Vec<OverlayShape>) {
        if let Some((a, b)) = self.band {
            out.push(OverlayShape::Rect {
                rect: rect_of(a, b),
                dashed: true,
            });
        }
    }

    fn infobar(&self, _view: ToolView<'_>) -> Infobar {
        Infobar {
            items: vec![InfobarItem::Note(
                "Click to zoom in, Shift+click to zoom out, drag to zoom to an area.".to_owned(),
            )],
        }
    }

    fn cursor(&self, _state: InteractionState) -> CursorKind {
        CursorKind::ZoomIn
    }
}

/// A tool of this phase that is not implemented yet: it can be chosen,
/// says so, and ignores the canvas.
#[derive(Debug)]
pub struct PendingTool {
    id: ToolId,
}

impl Tool for PendingTool {
    fn id(&self) -> ToolId {
        self.id
    }

    fn on_gesture(&mut self, _ev: &GestureEvent, _cx: &mut ToolCtx<'_>) {}

    fn infobar(&self, _view: ToolView<'_>) -> Infobar {
        Infobar {
            items: vec![InfobarItem::Note(format!(
                "The {} tool is coming soon.",
                self.id.label()
            ))],
        }
    }

    fn cursor(&self, _state: InteractionState) -> CursorKind {
        CursorKind::NotAllowed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constrain_snaps_to_the_nearest_eighth_turn() {
        let v = constrain_45(Vector::raw(1000, 100));
        assert_eq!(v, Vector::raw(1000, 0));
        let v = constrain_45(Vector::raw(100, -1000));
        assert_eq!(v, Vector::raw(0, -1000));
        let v = constrain_45(Vector::raw(1000, 900));
        assert_eq!(v.dx, v.dy, "{v:?}");
        assert_eq!(constrain_45(Vector::ZERO), Vector::ZERO);
    }

    #[test]
    fn every_available_tool_is_registered_once() {
        let tools = builtin();
        for id in ToolId::ALL {
            let n = tools.iter().filter(|t| t.id() == id).count();
            assert_eq!(n, usize::from(id.is_available()), "{id:?}");
        }
    }
}
