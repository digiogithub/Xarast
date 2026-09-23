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
//!  UiFrame ──► ShellCtx::show_ui           RenderThread::submit ◄─────────────┘
//!                                                  │ (render thread, CPU backend)
//!  CanvasFrame ──► ShellCtx::show_canvas ◄─ take_latest ◄─ waker ◄─┘
//! ```
//!
//! # Input ownership
//!
//! Every [`ShellEvent`] goes to `egui` through [`crate::egui_input`], so
//! the panels respond to the pointer, the keyboard, the wheel and the input
//! method. Canvas **navigation** has exactly one owner, the
//! [`IntentAdapter`]: the canvas widget runs with
//! [`xarast_ui::CanvasNavigation::External`] and emits no pan or zoom of its
//! own, so one wheel notch is one zoom step. Two gates keep the adapter out
//! of `egui`'s way:
//!
//! * a press or a wheel at a point where `egui` shows something above the
//!   canvas (a popup, a menu, a floating window) is not the canvas's;
//! * view keys do nothing while a text field has the keyboard.

use std::path::PathBuf;
use std::time::Duration;

use xarast_app::{AppState, Changed, Intent, RenderThread, Session};
use xarast_ui::model::{
    DocumentView, LayerInfo, LayerKey, StatusInfo, UiCommand, UiModel, ViewTransform,
};
use xarast_ui::{Scale, Workspace};

use crate::egui_input::{EguiInput, cursor_shape};
use crate::input::event::{ColorScheme, DragEvent, PointerPhase, ShellEvent};
use crate::input::keyboard::{Key, KeyState, NamedKey};
use crate::intents::{CanvasRegion, IntentAdapter};
use crate::paint::{CanvasFrame, UiFrame};
use crate::portal::PortalEvent;
use crate::scale::{PhysicalPos, PhysicalSize, ScaleFactor};
use crate::{CursorShape, FrameRequest, ShellApp, ShellCtx};

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
    render: Option<RenderThread>,
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
    /// `egui`'s input for the next interface frame.
    input: EguiInput,
    /// The scale of the last event or frame, in pixels per point.
    ppp: f32,
    /// A text field had the keyboard in the last interface frame.
    text_input: bool,
    /// The canvas had keyboard focus in the last interface frame.
    canvas_focused: bool,
    ime_allowed: bool,
    ime_area: Option<[i32; 4]>,
    cursor: CursorShape,
}

/// One interface frame, before it is handed to the shell.
struct UiStep {
    frame: UiFrame,
    region: Option<CanvasRegion>,
    repaint: Option<Duration>,
    output: egui::PlatformOutput,
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
            input: EguiInput::new(),
            ppp: 1.0,
            text_input: false,
            canvas_focused: false,
            ime_allowed: false,
            ime_area: None,
            cursor: CursorShape::Default,
        }
        .with_external_navigation()
    }

    /// The adapter owns canvas navigation; see the module documentation.
    fn with_external_navigation(mut self) -> Viewer {
        self.workspace
            .set_canvas_navigation(xarast_ui::CanvasNavigation::External);
        self
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

    /// Runs one interface frame: feeds `egui` the input gathered since the
    /// last one, lays out the workspace and applies what it asked for.
    /// Needs no window, which is what lets the input path be tested whole.
    fn ui_step(&mut self, scale: ScaleFactor, size: PhysicalSize) -> UiStep {
        let ppp = scale.pixels_per_point();
        self.ppp = ppp;
        let model = self.ui_model(f64::from(ppp));
        let raw = self.input.take(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(size.width as f32 / ppp, size.height as f32 / ppp),
        ));
        let mut out = None;
        let workspace = &mut self.workspace;
        let full = self.egui.run(raw, |c| {
            out = Some(workspace.ui(c, &model, Scale::new(f64::from(ppp)), &[]));
        });
        let primitives = self.egui.tessellate(full.shapes, full.pixels_per_point);
        let frame = UiFrame {
            primitives,
            textures: full.textures_delta,
            pixels_per_point: full.pixels_per_point,
        };
        let repaint = full
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|v| v.repaint_delay);
        // A focused text field is exactly when egui asks for an input
        // method, so that is the signal that the keyboard is egui's.
        self.text_input = full.platform_output.ime.is_some();

        let mut region = None;
        if let Some(out) = out {
            self.canvas_focused = out.canvas.as_ref().is_some_and(|c| c.focused);
            let intents: Vec<Intent> = out
                .commands
                .into_iter()
                .filter_map(|c| self.ui_intent(c, f64::from(ppp)))
                .collect();
            self.apply(intents);
            region = out.canvas.map(|c| {
                let r = c.rect_device;
                CanvasRegion::new(r.x, r.y, r.width, r.height)
            });
        }
        UiStep {
            frame,
            region,
            repaint,
            output: full.platform_output,
        }
    }

    /// Carries the interface's requests to the platform: the clipboard,
    /// the input method and the pointer shape.
    fn platform_output(&mut self, output: egui::PlatformOutput, ctx: &mut ShellCtx<'_>) {
        for command in output.commands {
            if let egui::OutputCommand::CopyText(text) = command
                && let Err(e) = ctx.clipboard().set_text(&text)
            {
                self.message = Some(format!("Could not copy: {e}"));
            }
        }
        let wants_ime = output.ime.is_some();
        if wants_ime != self.ime_allowed {
            ctx.set_ime_allowed(wants_ime);
            self.ime_allowed = wants_ime;
            self.ime_area = None;
        }
        if let Some(ime) = output.ime {
            let ppp = self.ppp;
            let r = ime.cursor_rect;
            let area = [r.min.x, r.min.y, r.width(), r.height()].map(|v| (v * ppp).round() as i32);
            if self.ime_area != Some(area) {
                self.ime_area = Some(area);
                let [x, y, w, h] = area.map(f64::from);
                ctx.set_ime_cursor_area(x, y, w, h);
            }
        }
        let shape = cursor_shape(output.cursor_icon);
        if shape != self.cursor {
            self.cursor = shape;
            ctx.set_cursor(shape);
        }
    }

    /// Sizes and places the canvas from the interface's layout. Returns
    /// the new origin when the canvas moved without being resized.
    fn place_canvas(&mut self, region: CanvasRegion, scale: ScaleFactor) -> Option<(i32, i32)> {
        let mut intents = Vec::new();
        let mut moved = None;
        if self.primed {
            if region != self.adapter.canvas() {
                moved = Some((region.x, region.y));
            }
            self.adapter.set_canvas(region, &mut intents);
        } else {
            self.adapter.prime(region, scale, &mut intents);
            self.primed = true;
        }
        self.apply(intents);
        if self.fit_pending {
            self.fit_pending = false;
            self.apply(vec![Intent::ZoomTo(xarast_app::ZoomTarget::Page)]);
        }
        moved
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
            match RenderThread::spawn(Box::new(move || waker.wake())) {
                Ok(rt) => self.render = Some(rt),
                Err(e) => {
                    self.message = Some(format!("Could not start the render thread: {e}"));
                    return;
                }
            }
        }
        let Some(session) = self.app.active_mut() else {
            return;
        };
        if self.scene_stale {
            if let Err(e) = session.rebuild_scene(None) {
                self.message = Some(e.to_string());
                return;
            }
            self.scene_stale = false;
        }
        if !self.render_stale || session.viewport.size().is_empty() {
            return;
        }
        let job = session.frame_job(PASTEBOARD, PAGE);
        if let Some(rt) = self.render.as_mut() {
            rt.submit(job);
        }
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
            ShellEvent::GpuError(report) => {
                // Already logged and being recovered from by the shell;
                // the status bar says so rather than the window vanishing.
                self.message = Some(format!(
                    "Graphics error ({} so far), recovering: {}",
                    report.total, report.message
                ));
                redraw = true;
            }
            ShellEvent::ColorSchemeChanged(s)
            | ShellEvent::Portal(PortalEvent::ColorSchemeChanged(s)) => {
                self.scheme = *s;
                redraw = true;
            }
            // A text field has the keyboard: typing "1" into a layer name
            // must not zoom to 100 %.
            ShellEvent::Key(k)
                if k.state == KeyState::Pressed && !k.modifiers.constrain() && !self.text_input =>
            {
                if let Some(pan) = self.arrow_pan(&k.key) {
                    redraw |= self.apply(vec![pan]).needs_redraw();
                }
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
        if self.egui_is_above_the_canvas(event) {
            return redraw;
        }
        let mut intents = Vec::new();
        self.adapter.translate(event, &mut intents);
        if !intents.is_empty() {
            redraw |= self.apply(intents).needs_redraw();
        }
        redraw
    }
}

impl Viewer {
    /// Arrow keys pan the view when the canvas has the keyboard, or when
    /// nothing does; otherwise they belong to the focused widget.
    fn arrow_pan(&self, key: &Key) -> Option<Intent> {
        let nothing_focused = self.egui.memory(|m| m.focused().is_none());
        if !(self.canvas_focused || nothing_focused) {
            return None;
        }
        let step = xarast_ui::canvas::KEY_PAN_STEP * f64::from(self.ppp);
        let (dx, dy) = match key {
            Key::Named(NamedKey::ArrowLeft) => (step, 0.0),
            Key::Named(NamedKey::ArrowRight) => (-step, 0.0),
            Key::Named(NamedKey::ArrowUp) => (0.0, step),
            Key::Named(NamedKey::ArrowDown) => (0.0, -step),
            _ => return None,
        };
        Some(Intent::Pan { dx, dy })
    }

    /// Whether this press or wheel lands on something `egui` shows above
    /// the canvas — a popup, a menu, a floating window — and so is not the
    /// canvas's. Panels share the background layer with the canvas and are
    /// told apart by the adapter's own region test instead. Motion and
    /// releases always reach the adapter, so a drag it owns is never cut.
    fn egui_is_above_the_canvas(&self, event: &ShellEvent) -> bool {
        let ShellEvent::Pointer(p) = event else {
            return false;
        };
        if !matches!(
            p.phase,
            PointerPhase::Pressed(_) | PointerPhase::Scroll { .. }
        ) {
            return false;
        }
        let at = points(p.position, self.ppp);
        self.egui
            .layer_id_at(at)
            .is_some_and(|l| l.order != egui::Order::Background)
    }
}

fn points(p: PhysicalPos, ppp: f32) -> egui::Pos2 {
    egui::pos2(p.x as f32 / ppp, p.y as f32 / ppp)
}

/// Projects a session for the interface.
///
/// The viewport stays the only thing that decides which way up a document
/// is (`app-core.md` §4); the interface is told, not asked. The projection
/// copies the viewport's scale, the device position of the document
/// origin and its `y`-up orientation into [`ViewTransform`], so the page
/// edge, the rulers, the grid and the pointer read-out land on exactly the
/// pixels the renderer used and read document `y` the right way up.
fn document_view(s: &Session, ppp: f64, keys: &mut Vec<xarast_doc::NodeId>) -> DocumentView {
    use xarast_geom::Mp;
    let vp = &s.viewport;
    let origin = vp.doc_to_device_f64(xarast_app::DocPointF::new(0.0, 0.0));
    let view = ViewTransform {
        // Logical points per document point.
        zoom: vp.scale() * f64::from(Mp::PER_PT) / ppp,
        origin_x: origin.x / ppp,
        origin_y: origin.y / ppp,
        y_up: true,
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
        // Left, top, right, bottom: under a y-up view the top is `hi.y`.
        page: (page.lo.x, page.hi.y, page.hi.x, page.lo.y),
        layers,
        active_layer: active,
        view,
        ..DocumentView::default()
    }
}

impl ShellApp for Viewer {
    fn on_event(&mut self, event: ShellEvent, ctx: &mut ShellCtx<'_>) {
        self.ppp = ctx.scale().pixels_per_point();
        self.input.push(&event, self.ppp, Some(ctx.clipboard()));
        // Anything egui was told about needs an interface frame to act on.
        let for_egui = !self.input.is_empty();
        if self.handle(&event) || for_egui {
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

        let step = self.ui_step(ctx.scale(), ctx.surface_size());
        ctx.show_ui(step.frame);
        self.platform_output(step.output, ctx);
        let repaint = step.repaint;

        if let Some(region) = step.region.filter(|r| r.width > 0 && r.height > 0)
            && let Some(origin) = self.place_canvas(region, ctx.scale())
        {
            ctx.move_canvas(origin);
        }

        self.submit_render(ctx);

        if let Some(rt) = &self.render
            && let Some(frame) = rt.take_latest()
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
                    "canvas frame"
                );
                let c = self.adapter.canvas();
                let settled = frame.view.viewport.width() == c.width
                    && frame.view.viewport.height() == c.height
                    && !self.render_stale
                    && !rt.has_pending();
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

        match repaint {
            Some(d) if d.is_zero() => FrameRequest::Redraw,
            Some(d) if d < Duration::from_secs(3600) => FrameRequest::RedrawAfter(d),
            // The render thread wakes the loop itself when a frame lands.
            _ => FrameRequest::Idle,
        }
    }

    fn on_exit(&mut self, _ctx: &mut ShellCtx<'_>) {
        if let Some(mut rt) = self.render.take() {
            rt.shutdown();
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
    fn the_vertical_ruler_reads_document_y_over_the_page() {
        // XARA-T-0026: the ruler used to read negative because the
        // projection negated y. Every tick drawn over the rendered page must
        // now carry a value inside the page's own y range, larger nearer
        // the top, and the pointer read-out must agree with the renderer.
        let s = session();
        let ppp = 1.25;
        let mut keys = Vec::new();
        let dv = document_view(&s, ppp, &mut keys);
        let page = xarast_app::viewport::page_rect(&s.doc);
        let rendered = xarast_app::viewport::device_rect_of(&s.viewport, page);
        let (top, bottom) = (f64::from(rendered.y0) / ppp, f64::from(rendered.y1) / ppp);
        let ticks = xarast_ui::rulers::ticks(
            xarast_ui::Axis::Vertical,
            xarast_ui::Unit::Point,
            &dv.view,
            f64::from(s.viewport.size().height) / ppp,
        );
        let over_page: Vec<_> = ticks
            .iter()
            .filter(|t| t.position > top + 1.0 && t.position < bottom - 1.0)
            .collect();
        assert!(!over_page.is_empty());
        for t in &over_page {
            assert!(
                t.value >= page.lo.y && t.value <= page.hi.y,
                "tick {t:?} outside the page's y range {:?}..{:?}",
                page.lo.y,
                page.hi.y
            );
        }
        assert!(over_page.windows(2).all(|w| w[0].value > w[1].value));
        // The top edge of the rendered page reads as the page's top.
        let read = dv.view.view_to_doc_y(top);
        let tolerance = xarast_geom::Mp::from_pt(1.0 / dv.view.zoom).raw();
        assert!(
            (read.raw() - page.hi.y.raw()).abs() <= tolerance,
            "{read:?} vs {:?}",
            page.hi.y
        );
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

    // ---- The egui input shim, end to end (XARA-US-0002) ----------------
    //
    // These drive the real path with no window: ShellEvent → EguiInput and
    // IntentAdapter → egui frame → UiCommand/Intent → Session.

    const SIZE: (u32, u32) = (1280, 800);

    fn one() -> ScaleFactor {
        ScaleFactor::new(1.0)
    }

    /// Runs `n` interface frames and returns the last accessibility tree.
    fn ui_frames(v: &mut Viewer, n: usize) -> Option<egui::accesskit::TreeUpdate> {
        let mut tree = None;
        for _ in 0..n {
            let step = v.ui_step(one(), PhysicalSize::new(SIZE.0, SIZE.1));
            if let Some(r) = step.region.filter(|r| r.width > 0 && r.height > 0) {
                v.place_canvas(r, one());
            }
            if step.output.accesskit_update.is_some() {
                tree = step.output.accesskit_update;
            }
        }
        tree
    }

    /// A viewer with a document, laid out by real interface frames.
    fn live_viewer() -> Viewer {
        let mut v = Viewer::new(Vec::new());
        v.app.new_document();
        v.egui.enable_accesskit();
        ui_frames(&mut v, 3);
        v
    }

    /// Delivers events the way `on_event` does.
    fn send(v: &mut Viewer, events: &[ShellEvent]) {
        for e in events {
            v.input.push(e, 1.0, None);
            v.handle(e);
        }
    }

    fn ptr_mod(
        phase: crate::input::event::PointerPhase,
        x: f64,
        y: f64,
        modifiers: crate::input::keyboard::Modifiers,
    ) -> ShellEvent {
        let ShellEvent::Pointer(mut p) = pointer(phase, x, y) else {
            unreachable!()
        };
        p.modifiers = modifiers;
        ShellEvent::Pointer(p)
    }

    fn click(v: &mut Viewer, x: f64, y: f64) {
        use crate::input::event::{PointerButton, PointerPhase};
        send(v, &[pointer(PointerPhase::Moved, x, y)]);
        ui_frames(v, 1);
        send(
            v,
            &[pointer(PointerPhase::Pressed(PointerButton::Primary), x, y)],
        );
        ui_frames(v, 1);
        send(
            v,
            &[pointer(
                PointerPhase::Released(PointerButton::Primary),
                x,
                y,
            )],
        );
        ui_frames(v, 1);
    }

    fn type_key(v: &mut Viewer, key: Key, text: Option<&str>) {
        use crate::input::keyboard::{KeyEvent, KeyLocation, Modifiers};
        for state in [KeyState::Pressed, KeyState::Released] {
            send(
                v,
                &[ShellEvent::Key(KeyEvent {
                    key: key.clone(),
                    location: KeyLocation::Standard,
                    state,
                    repeat: false,
                    text: text
                        .filter(|_| state == KeyState::Pressed)
                        .map(str::to_owned),
                    modifiers: Modifiers::NONE,
                })],
            );
        }
    }

    /// The centre of the first accessible node whose name matches.
    fn centre_of(tree: &egui::accesskit::TreeUpdate, want: impl Fn(&str) -> bool) -> (f64, f64) {
        tree.nodes
            .iter()
            .find_map(|(_, n)| {
                let label = n.label()?;
                if !want(label) {
                    return None;
                }
                let b = n.bounds()?;
                Some(((b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0))
            })
            .expect("the node is in the accessibility tree")
    }

    fn canvas_centre(v: &Viewer) -> (f64, f64) {
        let c = v.adapter.canvas();
        (
            f64::from(c.x) + f64::from(c.width) / 2.0,
            f64::from(c.y) + f64::from(c.height) / 2.0,
        )
    }

    fn zoom(v: &Viewer) -> f64 {
        v.app.active().unwrap().viewport.zoom()
    }

    #[test]
    fn one_ctrl_wheel_notch_over_the_canvas_zooms_exactly_one_step() {
        use crate::input::event::{PointerPhase, ScrollUnit};
        let mut v = live_viewer();
        let (x, y) = canvas_centre(&v);
        let ctrl = crate::input::keyboard::Modifiers::NONE.with_ctrl();
        send(
            &mut v,
            &[
                ShellEvent::ModifiersChanged(ctrl),
                ptr_mod(PointerPhase::Moved, x, y, ctrl),
            ],
        );
        ui_frames(&mut v, 2);
        let before = zoom(&v);
        send(
            &mut v,
            &[ptr_mod(
                PointerPhase::Scroll {
                    dx: 0.0,
                    dy: 1.0,
                    unit: ScrollUnit::Lines,
                },
                x,
                y,
                ctrl,
            )],
        );
        // egui spreads one wheel notch over about ten frames; run past it,
        // so a second, smoothed zoom would have had time to land.
        ui_frames(&mut v, 20);
        let ratio = zoom(&v) / before;
        assert!(
            (ratio - std::f64::consts::SQRT_2).abs() < 1e-9,
            "one notch zoomed by {ratio}, not √2"
        );
    }

    #[test]
    fn a_plain_wheel_notch_pans_exactly_once() {
        use crate::input::event::{PointerPhase, ScrollUnit};
        let mut v = live_viewer();
        let (x, y) = canvas_centre(&v);
        send(&mut v, &[pointer(PointerPhase::Moved, x, y)]);
        ui_frames(&mut v, 2);
        let before = v.app.active().unwrap().viewport.centre();
        send(
            &mut v,
            &[pointer(
                PointerPhase::Scroll {
                    dx: 0.0,
                    dy: 1.0,
                    unit: ScrollUnit::Lines,
                },
                x,
                y,
            )],
        );
        let after_adapter = v.app.active().unwrap().viewport.centre();
        ui_frames(&mut v, 20);
        let after_egui = v.app.active().unwrap().viewport.centre();
        assert_ne!(before, after_adapter, "the adapter pans");
        assert_eq!(after_adapter, after_egui, "and nothing pans a second time");
    }

    #[test]
    fn a_wheel_over_the_dock_does_not_move_the_document() {
        use crate::input::event::{PointerPhase, ScrollUnit};
        let mut v = live_viewer();
        let at = (f64::from(SIZE.0) - 40.0, 400.0);
        assert!(
            !v.adapter
                .canvas()
                .contains(crate::scale::PhysicalPos::new(at.0, at.1))
        );
        send(&mut v, &[pointer(PointerPhase::Moved, at.0, at.1)]);
        ui_frames(&mut v, 2);
        let before = v.app.active().unwrap().viewport.centre();
        send(
            &mut v,
            &[pointer(
                PointerPhase::Scroll {
                    dx: 0.0,
                    dy: 3.0,
                    unit: ScrollUnit::Lines,
                },
                at.0,
                at.1,
            )],
        );
        ui_frames(&mut v, 20);
        assert_eq!(before, v.app.active().unwrap().viewport.centre());
    }

    #[test]
    fn clicking_a_layer_toggle_in_the_panel_hides_the_layer() {
        let mut v = live_viewer();
        let tree = ui_frames(&mut v, 1).expect("accessibility is on");
        let (x, y) = centre_of(&tree, |l| l.starts_with("Hide layer"));
        click(&mut v, x, y);
        let layers = v.ui_model(1.0).document.unwrap().layers;
        assert!(
            layers.iter().any(|l| !l.visible),
            "a click on the toggle reached the document: {layers:?}"
        );
    }

    #[test]
    fn typing_in_a_rename_field_does_not_trigger_view_keys() {
        let mut v = live_viewer();
        let tree = ui_frames(&mut v, 1).expect("accessibility is on");
        let original = v.ui_model(1.0).document.unwrap().layers[0].name.clone();
        let (x, y) = centre_of(&tree, |l| l.starts_with(&format!("{original}, ")));
        // Double-click the name to rename it.
        click(&mut v, x, y);
        click(&mut v, x, y);
        ui_frames(&mut v, 2);
        assert!(v.text_input, "the rename field has the keyboard");

        let before = zoom(&v);
        type_key(&mut v, Key::char('1'), Some("1"));
        ui_frames(&mut v, 2);
        assert!(
            (zoom(&v) - before).abs() < 1e-12,
            "'1' typed into a text field zoomed the view"
        );
        type_key(&mut v, Key::Named(NamedKey::Enter), None);
        ui_frames(&mut v, 3);
        let renamed = v.ui_model(1.0).document.unwrap().layers[0].name.clone();
        assert!(
            renamed.contains('1') && renamed != original,
            "{original} -> {renamed}"
        );
        assert!(!v.text_input, "Enter gave the keyboard back");

        // And with no text field, the same key is a view key again.
        type_key(&mut v, Key::char('1'), Some("1"));
        assert!(
            (zoom(&v) - before).abs() > 1e-9 || (before - 1.0).abs() < 1e-9,
            "'1' outside a text field zooms to 100 %"
        );
    }
}
