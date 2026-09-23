//! The composition root: shell, interface and application core as one
//! running program.
//!
//! Each of the three crates is correct on its own and none of them knows
//! the others' types beyond the contract below it. This module is the only
//! place all three meet, and it is deliberately small:
//!
//! ```text
//!  ShellEvent ──► intents::IntentAdapter ──► Intent ──► AppState::apply ──► Changed
//!                                              ▲                              │
//!  egui frame ──► Workspace::ui ──► UiCommand ─┘            needs_scene: rebuild_scene
//!        │                                                   needs_redraw: frame_job
//!        ▼                                                                    │
//!  UiFrame ──► ShellCtx::show_ui     Canvas::pump (Draft/Final) ◄─────────────┘
//!                                                  │ (render thread, CPU backend)
//!  CanvasFrame ──► ShellCtx::show_canvas ◄─ take_latest ◄─ waker ◄─┘
//! ```
//!
//! What this module does **not** do yet: feed input to `egui`. The panels
//! are drawn and laid out — the canvas region comes from their layout —
//! but they do not respond to the pointer or the keyboard until the `egui`
//! input shim lands (XARA-US-0002). Pan and zoom reach the canvas through
//! [`crate::intents`] directly, which is the path that shim must not
//! duplicate: when it lands, the canvas widget's own navigation and this
//! module's must be reconciled so that a wheel notch is not applied twice.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use xarast_app::schedule::{Backdrop, Canvas};
use xarast_app::{AppState, Changed, Intent, Session};
use xarast_ui::model::{
    DocumentView, LayerInfo, LayerKey, StatusInfo, UiCommand, UiModel, ViewTransform,
};
use xarast_ui::{Scale, Workspace};

use crate::input::event::{ColorScheme, DragEvent, ShellEvent};
use crate::input::keyboard::{Key, KeyState, NamedKey};
use crate::intents::{CanvasRegion, IntentAdapter};
use crate::paint::{CanvasFrame, UiFrame};
use crate::portal::PortalEvent;
use crate::{FrameRequest, ShellApp, ShellCtx};

/// The pasteboard, premultiplied sRGB: a neutral mid grey that reads as
/// "not the page" under both themes.
pub const PASTEBOARD: [u8; 4] = [0x80, 0x80, 0x84, 0xff];

/// The page underneath the drawing.
pub const PAGE: [u8; 4] = [0xff, 0xff, 0xff, 0xff];

/// The running application.
pub struct Viewer {
    app: AppState,
    adapter: IntentAdapter,
    egui: egui::Context,
    workspace: Workspace,
    /// The render thread behind the Draft → Final scheduler.
    render: Option<Canvas>,
    /// When the owed Final frame is due, if one is.
    final_due: Option<Instant>,
    /// Files to open at the next frame, from the command line or a drop.
    to_open: Vec<PathBuf>,
    /// The canvas has not been primed with a size and scale yet.
    primed: bool,
    /// A document was just opened and should be framed once the canvas
    /// has its real size.
    fit_pending: bool,
    scene_stale: bool,
    render_stale: bool,
    /// The generation of the frame on screen, so that a stale frame from
    /// a closed document is never shown.
    shown: u64,
    /// Row index → layer node, rebuilt with the model every frame.
    layer_keys: Vec<xarast_doc::NodeId>,
    scheme: ColorScheme,
    message: Option<String>,
    renderer_label: String,
    /// Write the first settled frame here and quit.
    screenshot: Option<PathBuf>,
}

impl std::fmt::Debug for Viewer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Viewer")
            .field("documents", &self.app.docs.len())
            .field("to_open", &self.to_open)
            .field("render", &self.render)
            .finish_non_exhaustive()
    }
}

impl Viewer {
    /// A viewer that will open `files` once its window exists. The last
    /// one opened is the active document.
    #[must_use]
    pub fn new(files: Vec<PathBuf>) -> Viewer {
        Viewer {
            app: AppState::new(),
            adapter: IntentAdapter::new(),
            egui: egui::Context::default(),
            workspace: Workspace::new(),
            render: None,
            final_due: None,
            to_open: files,
            primed: false,
            fit_pending: false,
            scene_stale: false,
            render_stale: false,
            shown: 0,
            layer_keys: Vec::new(),
            scheme: ColorScheme::NoPreference,
            message: None,
            renderer_label: "CPU".to_owned(),
            screenshot: None,
        }
    }

    /// Writes the composed window to `path` as a PNG as soon as the
    /// document has been rendered at the canvas's size, then quits. This
    /// is how a real window is checked without a screenshot tool, and why
    /// the capture reads back the swapchain rather than the canvas alone:
    /// it proves the composition, not just the rasteriser.
    #[must_use]
    pub fn with_screenshot(mut self, path: PathBuf) -> Viewer {
        self.screenshot = Some(path);
        self
    }

    /// The application state, for tests and for the caller after the loop.
    #[must_use]
    pub const fn app(&self) -> &AppState {
        &self.app
    }

    fn open_queued(&mut self) {
        for path in std::mem::take(&mut self.to_open) {
            match self.app.open(&path) {
                Ok(_) => {
                    tracing::info!(path = %path.display(), "opened");
                    self.message = None;
                    self.fit_pending = true;
                    self.scene_stale = true;
                    self.render_stale = true;
                }
                Err(e) => {
                    tracing::error!(path = %path.display(), error = %e, "could not open");
                    self.message = Some(format!("Could not open {}: {e}", path.display()));
                }
            }
        }
    }

    /// Applies intents to the active document and records what is owed.
    fn apply(&mut self, intents: Vec<Intent>) -> Changed {
        let mut changed = Changed::empty();
        for intent in intents {
            match self.app.apply(intent) {
                Ok(c) => changed |= c,
                Err(e) => self.message = Some(e.to_string()),
            }
        }
        self.scene_stale |= changed.needs_scene();
        self.render_stale |= changed.needs_redraw();
        if let Some(canvas) = self.render.as_mut() {
            canvas.note(Instant::now(), changed);
        }
        changed
    }

    /// A key binding for the view, until the shortcut table (U4.7) exists.
    fn key_intent(key: &Key) -> Option<Intent> {
        use xarast_app::ZoomTarget;
        match key {
            Key::Character(c) => match c.as_str() {
                // The anchor is filled in by the caller, which knows the
                // canvas.
                "+" | "=" => Some(Intent::Zoom {
                    factor: std::f64::consts::SQRT_2,
                    anchor: xarast_app::DevicePoint::new(0.0, 0.0),
                }),
                "-" => Some(Intent::Zoom {
                    factor: std::f64::consts::FRAC_1_SQRT_2,
                    anchor: xarast_app::DevicePoint::new(0.0, 0.0),
                }),
                "1" => Some(Intent::ZoomTo(ZoomTarget::Percent100)),
                "0" => Some(Intent::ZoomTo(ZoomTarget::Page)),
                "d" => Some(Intent::ZoomTo(ZoomTarget::Drawing)),
                _ => None,
            },
            Key::Named(NamedKey::Home) => Some(Intent::ZoomTo(ZoomTarget::Page)),
            _ => None,
        }
    }

    fn ui_model(&mut self, scale: f64) -> UiModel {
        self.layer_keys.clear();
        let document = self
            .app
            .active()
            .map(|s| document_view(s, scale, &mut self.layer_keys));
        UiModel {
            document,
            status: StatusInfo {
                quality: xarast_ui::model::RenderQuality::Final,
                renderer: self.renderer_label.clone(),
                message: self.message.clone(),
                problem_count: self.app.diagnostics.entries().len(),
                ..StatusInfo::default()
            },
            palette: vec![xarast_ui::model::PaletteEntry::none()],
            system_scheme: match self.scheme {
                ColorScheme::NoPreference => xarast_ui::ColorScheme::NoPreference,
                ColorScheme::Dark => xarast_ui::ColorScheme::Dark,
                ColorScheme::Light => xarast_ui::ColorScheme::Light,
            },
            ..UiModel::default()
        }
    }

    /// Runs one interface frame and hands its output to the shell.
    /// Returns the canvas region it laid out and how soon it wants another.
    fn run_ui(&mut self, ctx: &mut ShellCtx<'_>) -> (Option<CanvasRegion>, Option<Duration>) {
        let scale = ctx.scale();
        let ppp = scale.pixels_per_point();
        let size = ctx.surface_size();
        let model = self.ui_model(f64::from(ppp));
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(size.width as f32 / ppp, size.height as f32 / ppp),
            )),
            ..egui::RawInput::default()
        };
        let mut out = None;
        let workspace = &mut self.workspace;
        let full = self.egui.run(raw, |c| {
            out = Some(workspace.ui(c, &model, Scale::new(f64::from(ppp)), &[]));
        });
        let primitives = self.egui.tessellate(full.shapes, full.pixels_per_point);
        ctx.show_ui(UiFrame {
            primitives,
            textures: full.textures_delta,
            pixels_per_point: full.pixels_per_point,
        });
        let repaint = full
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|v| v.repaint_delay);

        let Some(out) = out else {
            return (None, repaint);
        };
        let intents: Vec<Intent> = out
            .commands
            .into_iter()
            .filter_map(|c| self.ui_intent(c, f64::from(ppp)))
            .collect();
        self.apply(intents);
        let region = out.canvas.map(|c| {
            let r = c.rect_device;
            CanvasRegion::new(r.x, r.y, r.width, r.height)
        });
        (region, repaint)
    }

    /// Maps an interface command onto an intent. The interface speaks in
    /// logical points; intents are device pixels.
    fn ui_intent(&self, cmd: UiCommand, ppp: f64) -> Option<Intent> {
        let layer = |k: LayerKey| {
            usize::try_from(k.0)
                .ok()
                .and_then(|i| self.layer_keys.get(i).copied())
        };
        Some(match cmd {
            UiCommand::SetLayerVisible { layer: k, visible } => Intent::SetLayerVisible {
                layer: layer(k)?,
                visible,
            },
            UiCommand::SetLayerLocked { layer: k, locked } => Intent::SetLayerLocked {
                layer: layer(k)?,
                locked,
            },
            UiCommand::SetActiveLayer(k) => Intent::SetActiveLayer(layer(k)?),
            UiCommand::RenameLayer { layer: k, name } => Intent::RenameLayer {
                layer: layer(k)?,
                name,
            },
            UiCommand::Pan { dx, dy } => Intent::Pan {
                dx: dx * ppp,
                dy: dy * ppp,
            },
            UiCommand::ZoomAbout {
                factor,
                anchor_x,
                anchor_y,
            } => Intent::Zoom {
                factor,
                anchor: xarast_app::DevicePoint::new(anchor_x * ppp, anchor_y * ppp),
            },
            UiCommand::ZoomTo(t) => Intent::ZoomTo(zoom_target(t)),
            // Guides, grid, unit, colours, layer order and the theme have
            // no intent yet; they are phase 7/8 commands.
            _ => return None,
        })
    }

    fn submit_render(&mut self, ctx: &ShellCtx<'_>) {
        if self.render.is_none() {
            let waker = ctx.waker();
            let backdrop = Backdrop {
                pasteboard: PASTEBOARD,
                page: PAGE,
            };
            match Canvas::spawn(Box::new(move || waker.wake()), backdrop) {
                Ok(canvas) => self.render = Some(canvas),
                Err(e) => {
                    self.message = Some(format!("Could not start the render thread: {e}"));
                    return;
                }
            }
        }
        let Some(session) = self.app.active_mut() else {
            return;
        };
        let Some(canvas) = self.render.as_mut() else {
            return;
        };
        if self.render_stale || self.scene_stale {
            canvas.invalidate();
        }
        // The canvas rebuilds the scene when the session says it is stale,
        // and decides between a Draft now and a Final after 120 ms idle.
        match canvas.pump(Instant::now(), session) {
            Ok(due) => self.final_due = due,
            Err(e) => {
                self.message = Some(e.to_string());
                return;
            }
        }
        self.scene_stale = false;
        self.render_stale = false;
    }
}

fn zoom_target(t: xarast_ui::model::ZoomTarget) -> xarast_app::ZoomTarget {
    use xarast_app::ZoomTarget as A;
    use xarast_ui::model::ZoomTarget as U;
    match t {
        U::Page => A::Page,
        U::Spread => A::Spread,
        U::Drawing => A::Drawing,
        U::Selection => A::Selection,
        U::Percent100 => A::Percent100,
        U::Previous => A::Previous,
    }
}

impl Viewer {
    /// Routes one platform event: files and the colour scheme to the
    /// viewer, view keys and everything the adapter understands to the
    /// active document. Returns whether a redraw is owed.
    ///
    /// Separate from [`ShellApp::on_event`] so that the whole input path
    /// — shell event, adapter, intent, session, render bookkeeping — runs
    /// in a test with no window.
    pub fn handle(&mut self, event: &ShellEvent) -> bool {
        let mut redraw = false;
        match event {
            ShellEvent::Drag(DragEvent::Dropped { paths, .. }) => {
                self.to_open.extend(paths.iter().cloned());
                redraw = true;
            }
            ShellEvent::ColorSchemeChanged(s)
            | ShellEvent::Portal(PortalEvent::ColorSchemeChanged(s)) => {
                self.scheme = *s;
                redraw = true;
            }
            ShellEvent::Key(k) if k.state == KeyState::Pressed && !k.modifiers.constrain() => {
                if let Some(mut intent) = Self::key_intent(&k.key) {
                    // Keyboard zoom is about the canvas centre.
                    if let Intent::Zoom { anchor, .. } = &mut intent {
                        let c = self.adapter.canvas();
                        *anchor = xarast_app::DevicePoint::new(
                            f64::from(c.width) / 2.0,
                            f64::from(c.height) / 2.0,
                        );
                    }
                    redraw |= self.apply(vec![intent]).needs_redraw();
                }
            }
            _ => {}
        }
        let mut intents = Vec::new();
        self.adapter.translate(event, &mut intents);
        if !intents.is_empty() {
            redraw |= self.apply(intents).needs_redraw();
        }
        redraw
    }
}

/// Projects a session for the interface.
///
/// The interface's [`ViewTransform`] is a scale and an offset with `y`
/// pointing down, and it has no flip. The document's `y` points up, and
/// the flip belongs to the [`xarast_app::Viewport`] alone
/// (`app-core.md` §4). So this projection hands the interface document
/// coordinates with `y` **negated**, and derives the offset from the
/// viewport's own transform: the page edge, rulers and grid then land on
/// exactly the pixels the renderer used, and the viewport stays the only
/// thing that knows which way up a document is. The ruler's vertical
/// labels read downwards as a consequence — recorded in `ui.md` as a gap
/// the `UiModel` convergence closes.
fn document_view(s: &Session, ppp: f64, keys: &mut Vec<xarast_doc::NodeId>) -> DocumentView {
    use xarast_geom::Mp;
    let vp = &s.viewport;
    let origin = vp.doc_to_device_f64(xarast_app::DocPointF::new(0.0, 0.0));
    let view = ViewTransform {
        // Logical points per document point.
        zoom: vp.scale() * f64::from(Mp::PER_PT) / ppp,
        origin_x: origin.x / ppp,
        origin_y: origin.y / ppp,
    };
    let page = xarast_app::viewport::page_rect(&s.doc);
    let doc = &s.doc;
    let spread = doc.active_spread();
    let mut layers = Vec::new();
    let mut active = None;
    for child in doc.tree.children(spread) {
        if let Some(xarast_doc::NodeKind::Layer(l)) = doc.tree.kind(child) {
            let key = LayerKey(keys.len() as u64);
            keys.push(child);
            if l.active {
                active = Some(key);
            }
            layers.push(LayerInfo {
                key,
                name: l.name.to_string(),
                visible: l.visible,
                locked: l.locked,
                printable: l.printable,
                object_count: doc.tree.children(child).count(),
            });
        }
    }
    let title = s.path.as_ref().and_then(|p| p.file_name()).map_or_else(
        || "Untitled".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );
    DocumentView {
        title,
        page: (page.lo.x, -page.hi.y, page.hi.x, -page.lo.y),
        layers,
        active_layer: active,
        view,
        ..DocumentView::default()
    }
}

impl ShellApp for Viewer {
    fn on_event(&mut self, event: ShellEvent, ctx: &mut ShellCtx<'_>) {
        if self.handle(&event) {
            ctx.request_redraw();
        }
    }

    fn on_frame(&mut self, ctx: &mut ShellCtx<'_>) -> FrameRequest {
        if !self.to_open.is_empty() {
            self.open_queued();
            if let Some(s) = self.app.active() {
                let title = s
                    .path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                ctx.set_title(&format!("{title} — Xarast"));
                // A new document has no size yet; give it the canvas's.
                self.primed = false;
            }
        }

        let (region, repaint) = self.run_ui(ctx);

        if let Some(region) = region.filter(|r| r.width > 0 && r.height > 0) {
            let mut intents = Vec::new();
            if self.primed {
                let moved = region != self.adapter.canvas();
                self.adapter.set_canvas(region, &mut intents);
                if moved {
                    ctx.move_canvas((region.x, region.y));
                }
            } else {
                self.adapter.prime(region, ctx.scale(), &mut intents);
                self.primed = true;
            }
            self.apply(intents);
            if self.fit_pending {
                self.fit_pending = false;
                self.apply(vec![Intent::ZoomTo(xarast_app::ZoomTarget::Page)]);
            }
        }

        self.submit_render(ctx);

        if let Some(canvas) = self.render.as_mut()
            && let Some(frame) = canvas.take_latest()
        {
            let active = self.app.active().map(|s| s.id);
            if Some(frame.doc) == active && frame.generation > self.shown {
                self.shown = frame.generation;
                if let Some(e) = &frame.error {
                    self.message = Some(e.to_string());
                }
                tracing::debug!(
                    generation = frame.generation,
                    total_us = frame.timings.total_us(),
                    quality = ?frame.view.quality,
                    reuse = ?frame.reuse,
                    "canvas frame"
                );
                let c = self.adapter.canvas();
                let settled = frame.view.viewport.width() == c.width
                    && frame.view.viewport.height() == c.height
                    && frame.exact
                    && !self.render_stale
                    && canvas.is_settled();
                ctx.show_canvas(CanvasFrame {
                    origin: (c.x, c.y),
                    surface: frame.surface,
                });
                if settled && let Some(path) = self.screenshot.take() {
                    ctx.capture(path, true);
                }
            }
        }
        // Nothing to render (no document, or it failed to open): capture
        // what there is rather than wait forever.
        if self.app.active().is_none()
            && self.to_open.is_empty()
            && let Some(path) = self.screenshot.take()
        {
            ctx.capture(path, true);
        }

        // Wake for the owed Final even when nothing else is animating.
        let due = self
            .final_due
            .map(|t| t.saturating_duration_since(Instant::now()));
        let repaint = match (repaint, due) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        match repaint {
            Some(d) if d.is_zero() => FrameRequest::Redraw,
            Some(d) if d < Duration::from_secs(3600) => FrameRequest::RedrawAfter(d),
            // The render thread wakes the loop itself when a frame lands.
            _ => FrameRequest::Idle,
        }
    }

    fn on_exit(&mut self, _ctx: &mut ShellCtx<'_>) {
        if let Some(mut canvas) = self.render.take() {
            canvas.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xarast_app::{DeviceSize, DocumentId};

    fn session() -> Session {
        let mut s = Session::new_empty(DocumentId(1));
        s.apply(Intent::Resize(DeviceSize::new(900, 700))).unwrap();
        s.apply(Intent::SetDpi(120.0)).unwrap();
        s.apply(Intent::ZoomTo(xarast_app::ZoomTarget::Page))
            .unwrap();
        s.apply(Intent::Pan { dx: 13.0, dy: -7.0 }).unwrap();
        s
    }

    #[test]
    fn the_interface_page_edge_lands_on_the_rendered_page() {
        // The interface draws the page edge from the projection; the render
        // thread fills the page from the viewport. They must agree to a
        // pixel at a fractional scale, or the edge floats off the page.
        let s = session();
        let ppp = 1.25;
        let mut keys = Vec::new();
        let dv = document_view(&s, ppp, &mut keys);
        let rendered = xarast_app::viewport::device_rect_of(
            &s.viewport,
            xarast_app::viewport::page_rect(&s.doc),
        );
        let (l, t, r, b) = dv.page;
        let edge = [
            dv.view.doc_to_view_x(l) * ppp,
            dv.view.doc_to_view_y(t) * ppp,
            dv.view.doc_to_view_x(r) * ppp,
            dv.view.doc_to_view_y(b) * ppp,
        ];
        let want = [rendered.x0, rendered.y0, rendered.x1, rendered.y1].map(f64::from);
        for (e, w) in edge.iter().zip(want) {
            assert!((e - w).abs() <= 1.0, "edge {edge:?} vs rendered {want:?}");
        }
        assert!(edge[1] < edge[3], "top above bottom: {edge:?}");
    }

    #[test]
    fn layers_are_listed_with_keys_that_map_back_to_nodes() {
        let s = session();
        let mut keys = Vec::new();
        let dv = document_view(&s, 1.0, &mut keys);
        assert_eq!(dv.layers.len(), keys.len());
        assert!(!keys.is_empty(), "an empty document still has a layer");
        let mut v = Viewer::new(Vec::new());
        v.layer_keys = keys.clone();
        let intent = v.ui_intent(
            UiCommand::SetLayerVisible {
                layer: dv.layers[0].key,
                visible: false,
            },
            1.0,
        );
        assert_eq!(
            intent,
            Some(Intent::SetLayerVisible {
                layer: keys[0],
                visible: false
            })
        );
    }

    #[test]
    fn interface_navigation_is_scaled_from_points_to_pixels() {
        let v = Viewer::new(Vec::new());
        assert_eq!(
            v.ui_intent(UiCommand::Pan { dx: 2.0, dy: -4.0 }, 1.5),
            Some(Intent::Pan { dx: 3.0, dy: -6.0 })
        );
        assert_eq!(
            v.ui_intent(
                UiCommand::ZoomTo(xarast_ui::model::ZoomTarget::Drawing),
                1.0
            ),
            Some(Intent::ZoomTo(xarast_app::ZoomTarget::Drawing))
        );
        assert_eq!(v.ui_intent(UiCommand::RequestRedraw, 1.0), None);
    }

    /// A viewer with one open document and a primed 800×600 canvas, as
    /// the first frame leaves it.
    fn primed_viewer() -> Viewer {
        let mut v = Viewer::new(Vec::new());
        v.app.new_document();
        let mut intents = Vec::new();
        v.adapter.prime(
            CanvasRegion::new(20, 20, 800, 600),
            crate::scale::ScaleFactor::new(1.0),
            &mut intents,
        );
        v.apply(intents);
        v.primed = true;
        v.render_stale = false;
        v.scene_stale = false;
        v
    }

    fn pointer(phase: crate::input::event::PointerPhase, x: f64, y: f64) -> ShellEvent {
        ShellEvent::Pointer(crate::input::event::PointerEvent {
            id: crate::input::event::PointerId(0),
            phase,
            position: crate::scale::PhysicalPos::new(x, y),
            source: crate::input::tablet::InputSource::Mouse,
            primary: true,
            modifiers: crate::input::keyboard::Modifiers::NONE,
        })
    }

    #[test]
    fn constrain_wheel_over_the_canvas_zooms_the_document_and_owes_a_frame() {
        use crate::input::event::{PointerPhase, ScrollUnit};
        let mut v = primed_viewer();
        let before = v.app.active().unwrap().viewport.zoom();
        v.handle(&ShellEvent::ModifiersChanged(
            crate::input::keyboard::Modifiers::NONE.with_ctrl(),
        ));
        let redraw = v.handle(&pointer(
            PointerPhase::Scroll {
                dx: 0.0,
                dy: 1.0,
                unit: ScrollUnit::Lines,
            },
            400.0,
            300.0,
        ));
        assert!(redraw);
        assert!(v.render_stale, "a zoom owes a canvas frame");
        assert!(!v.scene_stale, "a zoom does not rebuild the scene");
        let after = v.app.active().unwrap().viewport.zoom();
        assert!((after / before - std::f64::consts::SQRT_2).abs() < 1e-9);
    }

    #[test]
    fn a_middle_drag_pans_the_document() {
        use crate::input::event::{PointerButton, PointerPhase};
        let mut v = primed_viewer();
        let c0 = v.app.active().unwrap().viewport.centre();
        v.handle(&pointer(
            PointerPhase::Pressed(PointerButton::Middle),
            300.0,
            300.0,
        ));
        assert!(v.handle(&pointer(PointerPhase::Moved, 340.0, 280.0)));
        v.handle(&pointer(
            PointerPhase::Released(PointerButton::Middle),
            340.0,
            280.0,
        ));
        let c1 = v.app.active().unwrap().viewport.centre();
        // The drawing follows the pointer: right and up on screen moves the
        // centre left and down in document space (y up).
        assert!(c1.x < c0.x && c1.y < c0.y, "{c0:?} -> {c1:?}");
        assert!(v.render_stale);
    }

    #[test]
    fn a_dropped_file_is_queued_for_the_next_frame() {
        let mut v = primed_viewer();
        let dropped = ShellEvent::Drag(DragEvent::Dropped {
            paths: vec![PathBuf::from("/nonexistent/a.xar")],
            at: None,
        });
        assert!(v.handle(&dropped));
        assert_eq!(v.to_open.len(), 1);
        v.open_queued();
        assert!(
            v.message
                .as_deref()
                .is_some_and(|m| m.contains("Could not open"))
        );
    }

    #[test]
    fn view_keys_map_to_view_intents() {
        assert!(matches!(
            Viewer::key_intent(&Key::char('1')),
            Some(Intent::ZoomTo(xarast_app::ZoomTarget::Percent100))
        ));
        assert!(matches!(
            Viewer::key_intent(&Key::char('+')),
            Some(Intent::Zoom { factor, .. }) if factor > 1.0
        ));
        assert!(Viewer::key_intent(&Key::char('q')).is_none());
    }
}
