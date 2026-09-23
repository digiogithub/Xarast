//! The window, the GPU surface and the event loop.
//!
//! The one place `winit` and `wgpu` are driven. Everything the application
//! sees of them is [`crate::input::event::ShellEvent`] and
//! [`crate::GpuContext`].

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

use crate::clipboard::{Clipboard, system_clipboard};
use crate::decorations::DecorationPlan;
use crate::display::{DisplayEnvironment, PlatformCapabilities};
use crate::gpu_errors::{GpuErrorSink, GpuRecovery, Recovery};
use crate::input::event::ShellEvent;
use crate::input::translate::EventTranslator;
use crate::paint::{CanvasFrame, Painter, UiFrame};
use crate::portal::{PortalHandle, PortalService};
use crate::scale::{PhysicalSize, ScaleFactor};
use crate::tiles::{CanvasTier, CanvasView, Compositor, TiledFrame};
use crate::{
    APP_ID, AdapterReport, BackendPreference, FrameRequest, PresentTiming, RendererPreference,
    ShellApp, ShellConfig, ShellError, record_first_frame,
};

/// What the application is handed on every callback.
///
/// Borrowed, never stored: it is the shell's state as of this instant, and
/// keeping a copy of the scale or the size across a frame is exactly the bug
/// the single-owner rule exists to prevent.
#[derive(Debug)]
pub struct ShellCtx<'a> {
    pub(crate) window: Option<&'a Arc<Window>>,
    pub(crate) scale: ScaleFactor,
    pub(crate) size: PhysicalSize,
    pub(crate) clipboard: &'a mut dyn Clipboard,
    pub(crate) portal: &'a PortalHandle,
    pub(crate) capabilities: PlatformCapabilities,
    pub(crate) frame: &'a mut PendingFrame,
    pub(crate) waker: &'a ShellWaker,
    pub(crate) exit: bool,
    pub(crate) renderer: Option<RendererStatus>,
    pub(crate) last_present: Option<PresentTiming>,
}

/// What presents the canvas, for the status bar and bug reports.
#[derive(Debug, Clone)]
pub struct RendererStatus {
    /// The canvas tier in force.
    pub tier: CanvasTier,
    /// The adapter presenting.
    pub adapter: AdapterReport,
    /// Why the tier is not GPU tiles, when it is not.
    pub reason: Option<String>,
}

impl std::fmt::Display for RendererStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} · {} ({})",
            self.tier.label(),
            self.adapter.name,
            self.adapter.backend
        )?;
        if self.adapter.is_software {
            f.write_str(", software")?;
        }
        Ok(())
    }
}

/// Wakes the event loop from another thread.
///
/// Handed to whatever works off the main thread — the render thread, above
/// all — so that a finished frame is presented as soon as it exists rather
/// than at the next input event. Waking a loop that has exited is a no-op.
#[derive(Debug, Clone)]
pub struct ShellWaker {
    proxy: Option<EventLoopProxy<()>>,
}

impl ShellWaker {
    /// A waker that wakes nothing, for code that runs with no loop.
    #[must_use]
    pub const fn none() -> ShellWaker {
        ShellWaker { proxy: None }
    }

    /// Asks the loop for a redraw.
    pub fn wake(&self) {
        if let Some(p) = &self.proxy {
            // An error means the loop is gone, which is not ours to report.
            let _ = p.send_event(());
        }
    }
}

/// A pointer shape, named by what it means rather than by any toolkit's
/// spelling. The shell maps it onto the platform's cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorShape {
    /// The platform's arrow.
    #[default]
    Default,
    /// No cursor at all.
    Hidden,
    /// A text caret, over editable text.
    Text,
    /// A pointing hand, over a link.
    Hand,
    /// An open hand: something can be dragged.
    Grab,
    /// A closed hand: something is being dragged.
    Grabbing,
    /// Four arrows: something moves freely.
    Move,
    /// Crosshair, for precise placement.
    Crosshair,
    /// The action is not allowed here.
    NotAllowed,
    /// Resize left–right.
    ResizeHorizontal,
    /// Resize up–down.
    ResizeVertical,
    /// Resize along the top-left/bottom-right diagonal.
    ResizeNwSe,
    /// Resize along the top-right/bottom-left diagonal.
    ResizeNeSw,
    /// Busy.
    Wait,
    /// Help is available.
    Help,
    /// Zoom in.
    ZoomIn,
    /// Zoom out.
    ZoomOut,
}

/// What the application handed over for the next present.
#[derive(Debug, Default)]
pub(crate) struct PendingFrame {
    canvas: Option<CanvasOp>,
    tiles: TileOps,
    ui: Option<UiFrame>,
    /// Read the next presented frame back into a PNG, then maybe exit.
    capture: Option<(std::path::PathBuf, bool)>,
    /// The accessibility tree to publish after this frame.
    a11y: Option<egui::accesskit::TreeUpdate>,
}

#[derive(Debug)]
enum CanvasOp {
    Show(CanvasFrame),
    Move((i32, i32)),
    Clear,
}

/// What the application handed the tile compositor since the last
/// present. Frames are applied in order; only the last view matters.
#[derive(Debug, Default)]
struct TileOps {
    frames: Vec<TiledFrame>,
    view: Option<CanvasView>,
}

impl ShellCtx<'_> {
    /// The scale factor in force. The only correct source of it.
    #[must_use]
    pub const fn scale(&self) -> ScaleFactor {
        self.scale
    }

    /// The surface size in device pixels.
    #[must_use]
    pub const fn surface_size(&self) -> PhysicalSize {
        self.size
    }

    /// What this display server can do.
    #[must_use]
    pub const fn capabilities(&self) -> PlatformCapabilities {
        self.capabilities
    }

    /// The system clipboard.
    pub fn clipboard(&mut self) -> &mut dyn Clipboard {
        self.clipboard
    }

    /// Posts a portal request. The answer arrives as a
    /// [`ShellEvent::Portal`].
    #[must_use]
    pub const fn portal(&self) -> &PortalHandle {
        self.portal
    }

    /// Asks for another frame.
    pub fn request_redraw(&self) {
        if let Some(w) = self.window {
            w.request_redraw();
        }
    }

    /// Sets the window title.
    pub fn set_title(&self, title: &str) {
        if let Some(w) = self.window {
            w.set_title(title);
        }
    }

    /// Enables or disables the input method for this window.
    ///
    /// The text tool turns it on when a caret appears and off when it
    /// leaves; with it left on, every keystroke is offered to the input
    /// method first and plain shortcuts stop working on CJK layouts.
    pub fn set_ime_allowed(&self, allowed: bool) {
        if let Some(w) = self.window {
            w.set_ime_allowed(allowed);
        }
    }

    /// Tells the input method where the caret is, in device pixels, so
    /// the candidate window opens next to it rather than at the corner.
    pub fn set_ime_cursor_area(&self, x: f64, y: f64, width: f64, height: f64) {
        if let Some(w) = self.window {
            w.set_ime_cursor_area(
                winit::dpi::PhysicalPosition::new(x, y),
                winit::dpi::PhysicalSize::new(width.max(1.0), height.max(1.0)),
            );
        }
    }

    /// Sets the pointer's shape over the window.
    pub fn set_cursor(&self, shape: CursorShape) {
        use winit::window::CursorIcon as C;
        let Some(w) = self.window else { return };
        let icon = match shape {
            CursorShape::Hidden => {
                w.set_cursor_visible(false);
                return;
            }
            CursorShape::Default => C::Default,
            CursorShape::Text => C::Text,
            CursorShape::Hand => C::Pointer,
            CursorShape::Grab => C::Grab,
            CursorShape::Grabbing => C::Grabbing,
            CursorShape::Move => C::Move,
            CursorShape::Crosshair => C::Crosshair,
            CursorShape::NotAllowed => C::NotAllowed,
            CursorShape::ResizeHorizontal => C::EwResize,
            CursorShape::ResizeVertical => C::NsResize,
            CursorShape::ResizeNwSe => C::NwseResize,
            CursorShape::ResizeNeSw => C::NeswResize,
            CursorShape::Wait => C::Wait,
            CursorShape::Help => C::Help,
            CursorShape::ZoomIn => C::ZoomIn,
            CursorShape::ZoomOut => C::ZoomOut,
        };
        w.set_cursor_visible(true);
        w.set_cursor(icon);
    }

    /// A handle another thread can use to wake the loop.
    #[must_use]
    pub fn waker(&self) -> ShellWaker {
        self.waker.clone()
    }

    /// Shows new canvas pixels from the next present on. The canvas pass
    /// draws them at `origin`, under the interface.
    pub fn show_canvas(&mut self, frame: CanvasFrame) {
        self.frame.canvas = Some(CanvasOp::Show(frame));
    }

    /// Moves the canvas already shown, with no new pixels.
    pub fn move_canvas(&mut self, origin: (i32, i32)) {
        match &mut self.frame.canvas {
            Some(CanvasOp::Show(f)) => f.origin = origin,
            _ => self.frame.canvas = Some(CanvasOp::Move(origin)),
        }
    }

    /// Stops showing a canvas.
    pub fn clear_canvas(&mut self) {
        self.frame.canvas = Some(CanvasOp::Clear);
        self.frame.tiles = TileOps::default();
    }

    /// Hands over a finished canvas frame to be kept as tiles (`tiles.rs`).
    /// The shell uploads only what it does not already hold; what is shown
    /// is decided by [`ShellCtx::set_canvas_view`].
    pub fn show_tiled_frame(&mut self, frame: TiledFrame) {
        // Frames queued between presents are all applied, in order, so a
        // scroll's base is never skipped.
        self.frame.tiles.frames.push(frame);
    }

    /// The view the canvas should show at the next present, composited
    /// from the tiles held. Called every frame: a pan or zoom is on
    /// screen as soon as its input is, before the render thread answers.
    pub fn set_canvas_view(&mut self, view: CanvasView) {
        self.frame.tiles.view = Some(view);
    }

    /// What presents the canvas: the tier, the adapter and why. `None`
    /// before the GPU is up.
    #[must_use]
    pub const fn renderer(&self) -> Option<&RendererStatus> {
        self.renderer.as_ref()
    }

    /// When the previous frame was presented, for latency probes.
    #[must_use]
    pub const fn last_present(&self) -> Option<PresentTiming> {
        self.last_present
    }

    /// Hands over one frame of interface output. Its texture changes are
    /// applied once; its meshes are drawn on every present until the next.
    pub fn show_ui(&mut self, mut frame: UiFrame) {
        // Two interface frames between presents: keep the second's meshes
        // but every texture change of both, in order.
        if let Some(prev) = self.frame.ui.take() {
            let mut textures = prev.textures;
            textures.append(frame.textures);
            frame.textures = textures;
        }
        self.frame.ui = Some(frame);
    }

    /// Reads the next presented frame back and writes it to `path` as a
    /// PNG — exactly the composed window content, canvas and interface,
    /// and nothing of the rest of the desktop. With `exit_after`, the loop
    /// quits once the file is written.
    ///
    /// Needs a swapchain that allows copies; where it does not, the
    /// failure is logged and the loop still exits if asked to.
    pub fn capture(&mut self, path: std::path::PathBuf, exit_after: bool) {
        self.frame.capture = Some((path, exit_after));
    }

    /// Publishes an accessibility tree update to the platform's assistive
    /// technologies. Only the latest update of a frame is kept; after
    /// [`ShellEvent::AccessibilityActivated`] it must be a full tree. A
    /// no-op when nothing is listening or the `accessibility` feature is
    /// off.
    pub fn update_accessibility(&mut self, update: egui::accesskit::TreeUpdate) {
        self.frame.a11y = Some(update);
    }

    /// Asks the shell to quit after this callback.
    pub const fn exit(&mut self) {
        self.exit = true;
    }
}

/// Builds the borrowed view of the shell handed to the application.
///
/// A macro rather than a method: the context borrows four fields of the
/// loop, and the application it is passed to is a fifth. Borrowing them
/// separately at the call site is what keeps that disjoint.
macro_rules! ctx_of {
    ($this:expr) => {
        ShellCtx {
            window: $this.window.as_ref(),
            scale: $this.translator.scale(),
            size: $this.gpu.as_ref().map_or(
                PhysicalSize::new($this.config.size.0, $this.config.size.1),
                Gpu::size,
            ),
            clipboard: $this.clipboard.as_mut(),
            portal: $this.portals.handle(),
            capabilities: $this.env.capabilities(),
            frame: &mut $this.pending,
            waker: &$this.waker,
            exit: false,
            renderer: $this.gpu.as_ref().map(Gpu::status),
            last_present: $this.gpu.as_ref().and_then(|g| g.last_present),
        }
    };
}

/// What the AccessKit handlers leave for the event loop. They run on the
/// adapter's own thread; the loop drains this on the main thread.
#[cfg(feature = "accessibility")]
#[derive(Debug, Default)]
struct A11yInbox {
    activated: bool,
    deactivated: bool,
    actions: Vec<egui::accesskit::ActionRequest>,
}

/// The AccessKit adapter bound to the window: the interface's tree on
/// AT-SPI (and, in phase 14, UIA and NSAccessibility).
///
/// The handlers never touch the interface: they record what was asked in
/// an [`A11yInbox`] and wake the loop, which turns it into
/// [`ShellEvent`]s. The application answers with
/// [`ShellCtx::update_accessibility`], published after the frame.
#[cfg(feature = "accessibility")]
struct A11y {
    adapter: accesskit_winit::Adapter,
    inbox: Arc<std::sync::Mutex<A11yInbox>>,
}

#[cfg(feature = "accessibility")]
impl std::fmt::Debug for A11y {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("A11y").finish_non_exhaustive()
    }
}

#[cfg(feature = "accessibility")]
mod a11y_handlers {
    use std::sync::{Arc, Mutex, PoisonError};

    use egui::accesskit::{
        ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, TreeUpdate,
    };

    use super::{A11yInbox, ShellWaker};

    pub(super) struct Handler {
        pub(super) inbox: Arc<Mutex<A11yInbox>>,
        pub(super) waker: ShellWaker,
    }

    impl Handler {
        fn with(&self, f: impl FnOnce(&mut A11yInbox)) {
            f(&mut self.inbox.lock().unwrap_or_else(PoisonError::into_inner));
            self.waker.wake();
        }
    }

    impl ActivationHandler for Handler {
        fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
            // The tree comes from the next interface frame, which the wake
            // below schedules; AccessKit shows a placeholder until then.
            self.with(|i| {
                i.activated = true;
                i.deactivated = false;
            });
            None
        }
    }

    impl ActionHandler for Handler {
        fn do_action(&mut self, request: ActionRequest) {
            self.with(|i| i.actions.push(request));
        }
    }

    impl DeactivationHandler for Handler {
        fn deactivate_accessibility(&mut self) {
            self.with(|i| {
                i.deactivated = true;
                i.activated = false;
            });
        }
    }
}

#[cfg(feature = "accessibility")]
impl A11y {
    /// Must run before the window is first shown: the adapter refuses a
    /// visible window.
    fn new(event_loop: &ActiveEventLoop, window: &Window, waker: &ShellWaker) -> A11y {
        let inbox = Arc::new(std::sync::Mutex::new(A11yInbox::default()));
        let handler = || a11y_handlers::Handler {
            inbox: inbox.clone(),
            waker: waker.clone(),
        };
        let adapter = accesskit_winit::Adapter::with_direct_handlers(
            event_loop,
            window,
            handler(),
            handler(),
            handler(),
        );
        A11y { adapter, inbox }
    }

    fn drain(&self, out: &mut Vec<ShellEvent>) {
        let mut inbox = self
            .inbox
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if std::mem::take(&mut inbox.deactivated) {
            out.push(ShellEvent::AccessibilityDeactivated);
        }
        if std::mem::take(&mut inbox.activated) {
            tracing::info!("an assistive technology is listening; publishing the interface");
            out.push(ShellEvent::AccessibilityActivated);
        }
        out.extend(inbox.actions.drain(..).map(ShellEvent::AccessibilityAction));
    }
}

/// The GPU state bound to one window.
#[derive(Debug)]
pub(crate) struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    painter: Painter,
    /// The canvas kept as tiles, on the GPU or in memory.
    compositor: Compositor,
    /// The GPU tier has a composite to encode in the next present.
    tiles_pending: bool,
    /// Where `wgpu` reports the errors it would otherwise panic on.
    errors: GpuErrorSink,
    pub(crate) report: AdapterReport,
    last_present: Option<PresentTiming>,
    /// Wait for the GPU after each present (`ShellConfig::probe`).
    probe: bool,
}

impl Gpu {
    pub(crate) fn new(
        window: Arc<Window>,
        preference: BackendPreference,
        renderer: RendererPreference,
        probe: bool,
    ) -> Result<Self, ShellError> {
        // The window's display handle goes to the instance: the GL backend
        // needs it to find an EGL display on Wayland, and without it
        // `WGPU_BACKEND=gl` found no adapter at all.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: preference.backends(),
            ..wgpu::InstanceDescriptor::new_with_display_handle(Box::new(window.clone()))
        });

        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| ShellError::Surface(e.to_string()))?;

        // The adapter ladder: the one `WGPU_ADAPTER_NAME` names, the
        // high-performance one, then the software fallback (lavapipe,
        // llvmpipe, WARP). The first that yields a device wins; only when
        // none does is it an error, and a diagnosis rather than a panic.
        let mut chosen = None;
        let mut failures = Vec::new();
        for adapter in candidate_adapters(&instance, &surface) {
            let name = adapter.get_info().name;
            match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("xarast device"),
                ..Default::default()
            })) {
                Ok(dq) => {
                    chosen = Some((adapter, dq));
                    break;
                }
                Err(e) => {
                    tracing::warn!(adapter = %name, error = %e, "device creation failed; trying the next adapter");
                    failures.push(format!("{name}: {e}"));
                }
            }
        }
        let Some((adapter, (device, queue))) = chosen else {
            let mut tried = preference.describe().to_owned();
            if !failures.is_empty() {
                tried = format!("{tried}; {}", failures.join("; "));
            }
            return Err(ShellError::NoAdapter { tried });
        };

        let info = adapter.get_info();
        let report = AdapterReport {
            name: info.name.clone(),
            backend: match info.backend {
                wgpu::Backend::Vulkan => "Vulkan",
                wgpu::Backend::Gl => "GL",
                wgpu::Backend::Metal => "Metal",
                wgpu::Backend::Dx12 => "DX12",
                wgpu::Backend::BrowserWebGpu => "WebGPU",
                wgpu::Backend::Noop => "none",
            },
            device_type: match info.device_type {
                wgpu::DeviceType::DiscreteGpu => "discrete GPU",
                wgpu::DeviceType::IntegratedGpu => "integrated GPU",
                wgpu::DeviceType::VirtualGpu => "virtual GPU",
                wgpu::DeviceType::Cpu => "CPU",
                wgpu::DeviceType::Other => "other",
            },
            is_software: info.device_type == wgpu::DeviceType::Cpu,
        };
        tracing::info!(adapter = %report, "selected adapter");

        // Before anything else touches the device: from here on a
        // validation error is logged and recovered from, never a panic
        // (`ui.md` shell invariant 6).
        let errors = GpuErrorSink::new();
        errors.install(&device);
        let lost = errors.clone();
        device.set_device_lost_callback(move |reason, message| {
            // Dropping the device at exit reports `Destroyed`; that is not
            // a fault.
            if reason != wgpu::DeviceLostReason::Destroyed {
                lost.record("device lost", &message);
            }
        });

        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| {
                ShellError::Surface("the adapter cannot present to this surface".to_owned())
            })?;
        // A non-sRGB swapchain: the canvas and the interface both hand over
        // bytes that are already sRGB-encoded (`xarast-render` invariant 2),
        // and an sRGB target would encode them a second time. See `paint`.
        let caps = surface.get_capabilities(&adapter);
        // Exactly eight-bit BGRA or RGBA: "the first non-sRGB format" can
        // be a 10-bit or half-float one, which is neither what the passes
        // write nor four bytes a pixel.
        if let Some(unorm) = caps.formats.iter().copied().find(|f| {
            matches!(
                f,
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
            )
        }) {
            config.format = unorm;
        } else {
            tracing::warn!(format = ?config.format, "only sRGB swapchain formats; colours will be too light");
        }
        // Allow reading the frame back for `--screenshot`, where the
        // swapchain permits it; nothing else depends on it.
        if caps.usages.contains(wgpu::TextureUsages::COPY_SRC) {
            config.usage |= wgpu::TextureUsages::COPY_SRC;
        }
        if probe {
            // Latency is measured to the present, not to the next vblank.
            config.present_mode = wgpu::PresentMode::AutoNoVsync;
        }
        surface.configure(&device, &config);
        let painter = Painter::new(&device, config.format);
        let compositor = Compositor::new(
            Arc::new(device.clone()),
            Arc::new(queue.clone()),
            renderer.wants_gpu_tiles(),
        );

        Ok(Self {
            surface,
            device,
            queue,
            config,
            painter,
            compositor,
            tiles_pending: false,
            errors,
            report,
            last_present: None,
            probe,
        })
    }

    /// The renderer in force, for the status bar.
    pub(crate) fn status(&self) -> RendererStatus {
        RendererStatus {
            tier: self.compositor.tier(),
            adapter: self.report.clone(),
            reason: self.compositor.reason.clone(),
        }
    }

    /// Falls back from GPU tiles to CPU composition, for good.
    pub(crate) fn demote_canvas(&mut self, why: &str) {
        self.compositor.demote(why);
    }

    pub(crate) fn size(&self) -> PhysicalSize {
        PhysicalSize::new(self.config.width, self.config.height)
    }

    fn resize(&mut self, width: u32, height: u32) {
        if PhysicalSize::is_degenerate(width, height) {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    /// Takes over what the application handed in this frame.
    fn apply(&mut self, pending: &mut PendingFrame) {
        match pending.canvas.take() {
            Some(CanvasOp::Show(f)) => {
                // A whole image replaces the compositor's until a view is
                // set again.
                self.compositor.deactivate();
                self.painter.set_canvas(&self.device, &self.queue, &f);
            }
            Some(CanvasOp::Move(o)) => {
                self.painter.move_canvas(o);
                self.compositor.move_to(o);
            }
            Some(CanvasOp::Clear) => {
                self.compositor.deactivate();
                self.painter.clear_canvas();
            }
            None => {}
        }
        let tiles = std::mem::take(&mut pending.tiles);
        for f in tiles.frames {
            let stats = self.compositor.accept(f);
            tracing::trace!(?stats, "canvas frame kept as tiles");
        }
        if let Some(view) = tiles.view {
            self.compositor.set_view(view);
        }
        if self.compositor.is_active() {
            self.tiles_pending |= self.compositor.prepare(&mut self.painter);
        }
        if let Some(ui) = pending.ui.take() {
            self.painter.set_ui(&self.device, &self.queue, ui);
        }
    }

    /// Presents one frame: the canvas pass, then the interface pass, over
    /// a flat backdrop.
    fn present(
        &mut self,
        capture: Option<&std::path::Path>,
        started: std::time::Instant,
    ) -> FrameOutcome {
        use wgpu::CurrentSurfaceTexture as Cst;

        let (frame, suboptimal) = match self.surface.get_current_texture() {
            Cst::Success(frame) => (frame, false),
            // Suboptimal still gives us a usable texture. Draw this frame, then
            // reconfigure so the next one is not suboptimal too — *after*
            // presenting: configuring while a texture is held is a
            // validation error, and wgpu treats those as fatal.
            Cst::Suboptimal(frame) => (frame, true),
            Cst::Outdated | Cst::Lost => {
                self.reconfigure();
                return FrameOutcome::Skipped;
            }
            Cst::Timeout | Cst::Occluded => return FrameOutcome::Skipped,
            Cst::Validation => return FrameOutcome::Failed,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        if std::mem::take(&mut self.tiles_pending) {
            self.compositor.encode(&mut encoder, &self.painter);
        }
        self.painter.draw(
            &self.device,
            &mut encoder,
            &view,
            [self.config.width, self.config.height],
            wgpu::Color {
                r: 0.10,
                g: 0.10,
                b: 0.11,
                a: 1.0,
            },
        );
        let readback = capture.map(|path| (path, self.copy_out(&mut encoder, &frame.texture)));
        self.queue.submit(Some(encoder.finish()));
        if let Some((path, buffer)) = readback {
            match buffer.and_then(|b| self.write_capture(&b, path)) {
                Ok(()) => tracing::info!(path = %path.display(), "frame captured"),
                Err(e) => tracing::error!(error = %e, "frame capture failed"),
            }
        }
        self.queue.present(frame);
        let presented = std::time::Instant::now();
        let gpu_done = self.probe.then(|| {
            let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
            std::time::Instant::now()
        });
        self.last_present = Some(PresentTiming {
            started,
            presented,
            gpu_done,
            tier: self.compositor.tier(),
        });
        if suboptimal {
            self.reconfigure();
        }
        record_first_frame();
        FrameOutcome::Presented
    }

    /// Records a copy of the frame into a mappable buffer. Sizes come from
    /// the texture itself, never from the configuration, which a resize
    /// may already have moved on from.
    fn copy_out(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
    ) -> Result<Readback, String> {
        if !texture.usage().contains(wgpu::TextureUsages::COPY_SRC) {
            return Err("this swapchain cannot be read back".to_owned());
        }
        let (width, height) = (texture.width(), texture.height());
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let row = (width * 4).div_ceil(align) * align;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture"),
            size: u64::from(row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        Ok(Readback {
            buffer,
            width,
            height,
            row,
            bgra: matches!(
                texture.format(),
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
            ),
        })
    }

    /// Maps a read-back buffer and writes it as a PNG.
    fn write_capture(&self, rb: &Readback, path: &std::path::Path) -> Result<(), String> {
        let (tx, rx) = std::sync::mpsc::channel();
        rb.buffer.map_async(wgpu::MapMode::Read, .., move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| e.to_string())?;
        rx.recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let (w, h, row) = (rb.width as usize, rb.height as usize, rb.row as usize);
        let mut surface = xarast_render::Surface::new(rb.width, rb.height);
        {
            let mapped = rb.buffer.get_mapped_range(..).map_err(|e| e.to_string())?;
            let out = surface.data_mut();
            for y in 0..h {
                let src = &mapped[y * row..y * row + w * 4];
                let dst = &mut out[y * w * 4..(y + 1) * w * 4];
                for (d, s) in dst
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(src.as_chunks::<4>().0)
                {
                    // The window is opaque; alpha from the swapchain is
                    // whatever the compositor left there.
                    *d = if rb.bgra {
                        [s[2], s[1], s[0], 255]
                    } else {
                        [s[0], s[1], s[2], 255]
                    };
                }
            }
        }
        rb.buffer.unmap();
        xarast_render::golden::write_png(&surface, path).map_err(|e| e.to_string())
    }

    /// Raises a validation error on purpose, for `XARAST_INJECT_GPU_ERRORS`:
    /// the only way to watch the recovery path in a real window.
    fn inject_error(&self) {
        let _ = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("injected error"),
            size: 16,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::MAP_WRITE,
            mapped_at_creation: false,
        });
    }

    /// Re-applies the current configuration, which is how a lost, outdated or
    /// suboptimal swapchain is recovered.
    fn reconfigure(&mut self) {
        self.surface.configure(&self.device, &self.config);
    }
}

/// The adapters to try, best first: the one `WGPU_ADAPTER_NAME` names (a
/// case-insensitive substring, as `wgpu`'s own helper reads it, but a
/// missing match is a warning here, not a panic), the high-performance
/// default, then the software fallback.
fn candidate_adapters(
    instance: &wgpu::Instance,
    surface: &wgpu::Surface<'_>,
) -> Vec<wgpu::Adapter> {
    let mut out = Vec::new();
    if let Ok(want) = std::env::var("WGPU_ADAPTER_NAME") {
        let want = want.to_lowercase();
        let named = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()))
            .into_iter()
            .find(|a| {
                a.is_surface_supported(surface) && a.get_info().name.to_lowercase().contains(&want)
            });
        match named {
            Some(a) => out.push(a),
            None => {
                tracing::warn!(wanted = %want, "WGPU_ADAPTER_NAME matches no adapter that can present here")
            }
        }
    }
    for fallback in [false, true] {
        if let Ok(a) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(surface),
            force_fallback_adapter: fallback,
            ..Default::default()
        })) {
            let info = a.get_info();
            if !out.iter().any(|o: &wgpu::Adapter| o.get_info() == info) {
                out.push(a);
            }
        }
    }
    out
}

/// A frame copied out for `--screenshot`.
#[derive(Debug)]
struct Readback {
    buffer: wgpu::Buffer,
    width: u32,
    height: u32,
    row: u32,
    bgra: bool,
}

/// How far a redraw got. Distinguishing "skipped" from "failed" is what keeps
/// an occluded window cheap and a dead surface from spinning forever.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameOutcome {
    Presented,
    Skipped,
    Failed,
}

/// Consecutive acquisition failures tolerated before giving up on the surface.
const MAX_CONSECUTIVE_FRAME_FAILURES: u32 = 32;

/// The winit application. Holds everything that only exists once the event
/// loop has resumed, which on Wayland is the only point at which a surface is
/// legal.
#[derive(Debug)]
pub(crate) struct ShellLoop<A: ShellApp> {
    config: ShellConfig,
    env: DisplayEnvironment,
    decorations: DecorationPlan,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    translator: EventTranslator,
    clipboard: Box<dyn Clipboard>,
    portals: PortalService,
    app: A,
    pending: PendingFrame,
    waker: ShellWaker,
    events: Vec<ShellEvent>,
    frames: u32,
    consecutive_failures: u32,
    #[cfg(feature = "accessibility")]
    a11y: Option<A11y>,
    /// Drag and drop on Wayland, which `winit` 0.30 does not provide.
    /// Dropped in `exiting`, before the display it borrows.
    #[cfg(all(
        unix,
        not(any(target_os = "macos", target_os = "ios", target_os = "android"))
    ))]
    drops: Option<crate::wayland_dnd::WaylandDrops>,
    /// What to do about GPU errors, frame by frame.
    recovery: GpuRecovery,
    /// While set, frames are neither drawn nor presented: the GPU is being
    /// left alone after a run of errors.
    backoff_until: Option<std::time::Instant>,
    /// When the application asked to be redrawn (`FrameRequest::RedrawAfter`).
    redraw_at: Option<std::time::Instant>,
    /// Frames still to raise a deliberate GPU error in
    /// (`XARAST_INJECT_GPU_ERRORS`); zero in normal use.
    inject_errors: u32,
    pub(crate) report: Option<AdapterReport>,
    pub(crate) error: Option<ShellError>,
}

impl<A: ShellApp> ShellLoop<A> {
    fn new(config: ShellConfig, app: A, waker: ShellWaker) -> Self {
        let env = DisplayEnvironment::from_env();
        let decorations = DecorationPlan::for_environment(&env);
        tracing::info!(session = %env.summary(), decorations = decorations.summary(), "shell starting");
        Self {
            config,
            env,
            decorations,
            window: None,
            gpu: None,
            translator: EventTranslator::new(),
            clipboard: system_clipboard(),
            portals: {
                // A portal answer, or the desktop changing colour scheme,
                // must reach a loop that is parked with nothing to do.
                let waker = waker.clone();
                PortalService::start_with_waker(move || waker.wake())
            },
            app,
            pending: PendingFrame::default(),
            waker,
            events: Vec::new(),
            frames: 0,
            consecutive_failures: 0,
            #[cfg(feature = "accessibility")]
            a11y: None,
            #[cfg(all(
                unix,
                not(any(target_os = "macos", target_os = "ios", target_os = "android"))
            ))]
            drops: None,
            recovery: GpuRecovery::new(),
            backoff_until: None,
            redraw_at: None,
            inject_errors: std::env::var("XARAST_INJECT_GPU_ERRORS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            report: None,
            error: None,
        }
    }

    fn window_attributes(&self) -> winit::window::WindowAttributes {
        let attrs = Window::default_attributes()
            .with_title(self.config.title.clone())
            // Shown once the accessibility adapter is attached, which it
            // must be before the first map (`A11y::new`).
            .with_visible(false)
            .with_decorations(self.decorations.request_decorations)
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.config.size.0,
                self.config.size.1,
            ));

        // The app id has to match the desktop entry on both Wayland and X11, or
        // the desktop environment cannot associate the window with the
        // installed application. Both platform traits spell the setter
        // `with_name`, so each call is fully qualified to say which one it is.
        #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
        let attrs = {
            use winit::platform::wayland::WindowAttributesExtWayland;
            use winit::platform::x11::WindowAttributesExtX11;
            let attrs = WindowAttributesExtWayland::with_name(attrs, APP_ID, "");
            WindowAttributesExtX11::with_name(attrs, APP_ID, APP_ID)
        };

        attrs
    }

    /// Hands the queued events to the application.
    fn dispatch(&mut self, event_loop: &ActiveEventLoop) {
        if self.events.is_empty() {
            return;
        }
        let queued = std::mem::take(&mut self.events);
        let mut ctx = ctx_of!(self);
        for ev in queued {
            self.app.on_event(ev, &mut ctx);
        }
        if ctx.exit {
            event_loop.exit();
        }
    }

    /// Acts on the GPU errors the last frame raised, if any: reconfigure,
    /// or back off. Never exits — a failing GPU leaves the document open
    /// (`ui.md` shell invariant 6). The application hears of it as a
    /// [`ShellEvent::GpuError`].
    fn recover_from_gpu_errors(&mut self, event_loop: &ActiveEventLoop) {
        let Some(gpu) = self.gpu.as_mut() else { return };
        let report = gpu.errors.take_report();
        match self.recovery.after_frame(report.is_some()) {
            Recovery::Continue => {}
            Recovery::Reconfigure => {
                tracing::warn!(
                    streak = self.recovery.streak(),
                    "GPU error; reconfiguring the surface"
                );
                gpu.reconfigure();
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            Recovery::Backoff(delay) => {
                // Errors that survive reconfiguring: stop asking the GPU
                // to do more than present (the capability ladder's last
                // rung before giving up on frames altogether).
                gpu.demote_canvas("repeated GPU errors");
                tracing::warn!(
                    streak = self.recovery.streak(),
                    delay_ms = delay.as_millis(),
                    "GPU errors persist; pausing presentation"
                );
                let until = std::time::Instant::now() + delay;
                self.backoff_until = Some(until);
                event_loop.set_control_flow(ControlFlow::WaitUntil(until));
            }
        }
        if let Some(report) = report {
            self.events.push(ShellEvent::GpuError(report));
        }
    }

    fn collect_portal_answers(&mut self) {
        let mut answers = Vec::new();
        self.portals.poll(&mut answers);
        self.events
            .extend(answers.into_iter().map(ShellEvent::Portal));
        #[cfg(feature = "accessibility")]
        if let Some(a11y) = &self.a11y {
            a11y.drain(&mut self.events);
        }
    }

    /// Hands the application's latest tree to the adapter, which forwards
    /// it only while an assistive technology is listening.
    fn publish_accessibility(&mut self) {
        let update = self.pending.a11y.take();
        #[cfg(feature = "accessibility")]
        if let (Some(update), Some(a11y)) = (update, self.a11y.as_mut()) {
            a11y.adapter.update_if_active(|| update);
        }
        #[cfg(not(feature = "accessibility"))]
        drop(update);
    }
}

impl<A: ShellApp> ApplicationHandler for ShellLoop<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = match event_loop.create_window(self.window_attributes()) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                self.error = Some(ShellError::Window(e.to_string()));
                event_loop.exit();
                return;
            }
        };
        #[cfg(feature = "accessibility")]
        {
            self.a11y = Some(A11y::new(event_loop, &window, &self.waker));
        }
        window.set_visible(true);
        #[cfg(all(
            unix,
            not(any(target_os = "macos", target_os = "ios", target_os = "android"))
        ))]
        {
            use raw_window_handle::HasDisplayHandle;
            self.drops = window.display_handle().ok().and_then(|h| {
                crate::wayland_dnd::WaylandDrops::new(h.as_raw(), self.waker.clone())
            });
        }
        match Gpu::new(
            window.clone(),
            self.config.backends,
            self.config.renderer,
            self.config.probe,
        ) {
            Ok(gpu) => {
                self.report = Some(gpu.report.clone());
                self.gpu = Some(gpu);
            }
            Err(e) => {
                self.error = Some(e);
                event_loop.exit();
                return;
            }
        }
        // The compositor's own scale is authoritative from the first frame;
        // reading it here is what stops the first frame being drawn at 1×
        // and snapping a frame later.
        self.translator
            .set_scale(ScaleFactor::new(window.scale_factor()));
        window.request_redraw();
        self.window = Some(window);
        self.portals.handle().query_color_scheme();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // AccessKit sees every window event first (focus, above all).
        #[cfg(feature = "accessibility")]
        if let (Some(a11y), Some(window)) = (self.a11y.as_mut(), self.window.as_ref()) {
            a11y.adapter.process_event(window, &event);
        }
        // Translate first, so that the application sees the event before the
        // shell acts on it and there is exactly one interpretation of each.
        self.translator.translate(&event, &mut self.events);

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.resize(size.width, size.height);
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                // Fractional scaling on Wayland arrives here; the surface is
                // resized by the Resized event that follows. The translator
                // already holds the new factor and is the only owner of it.
                tracing::debug!(scale_factor, "scale factor changed");
            }
            WindowEvent::RedrawRequested => {
                let started = std::time::Instant::now();
                self.collect_portal_answers();
                self.translator.drain_samples(&mut self.events);
                self.dispatch(event_loop);
                if event_loop.exiting() {
                    return;
                }
                // Backing off after GPU errors: the application's state
                // waits in `pending` (texture deltas merge there), and the
                // loop sleeps until the deadline instead of spinning.
                if let Some(until) = self.backoff_until {
                    if std::time::Instant::now() < until {
                        event_loop.set_control_flow(ControlFlow::WaitUntil(until));
                        return;
                    }
                    self.backoff_until = None;
                }

                let mut ctx = ctx_of!(self);
                let request = self.app.on_frame(&mut ctx);
                let exit = ctx.exit;
                if exit {
                    event_loop.exit();
                    return;
                }
                self.redraw_at = apply_frame_request(event_loop, self.window.as_ref(), request);
                self.publish_accessibility();

                let Some(gpu) = self.gpu.as_mut() else { return };
                if self.inject_errors > 0 {
                    self.inject_errors -= 1;
                    gpu.inject_error();
                }
                // Applied whether or not this present succeeds: texture
                // deltas must reach the GPU exactly once and in order.
                gpu.apply(&mut self.pending);
                let capture = self.pending.capture.take();
                let outcome = gpu.present(capture.as_ref().map(|(p, _)| p.as_path()), started);
                if outcome != FrameOutcome::Presented {
                    // Not presented, not captured: try again next frame.
                    self.pending.capture = capture;
                } else if let Some((_, true)) = capture {
                    event_loop.exit();
                }
                self.recover_from_gpu_errors(event_loop);
                match outcome {
                    FrameOutcome::Presented => self.frames += 1,
                    // A skipped frame is normal: the window is occluded, or the
                    // swapchain went stale and has just been rebuilt.
                    FrameOutcome::Skipped => {
                        self.consecutive_failures = 0;
                        return;
                    }
                    FrameOutcome::Failed => {
                        self.consecutive_failures += 1;
                        tracing::warn!(
                            failures = self.consecutive_failures,
                            "failed to acquire a surface texture"
                        );
                        // One failure is a hiccup; a run of them means the
                        // surface will not come back, and spinning on it would
                        // burn a core forever.
                        if self.consecutive_failures >= MAX_CONSECUTIVE_FRAME_FAILURES {
                            self.error = Some(ShellError::Surface(
                                "the surface stopped producing textures".to_owned(),
                            ));
                            event_loop.exit();
                        }
                        return;
                    }
                }
                self.consecutive_failures = 0;
                if let Some(limit) = self.config.exit_after_frames
                    && self.frames >= limit
                {
                    event_loop.exit();
                }
            }
            _ => {
                self.dispatch(event_loop);
            }
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, (): ()) {
        // Another thread has something to show: a finished render.
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.collect_portal_answers();
        #[cfg(all(
            unix,
            not(any(target_os = "macos", target_os = "ios", target_os = "android"))
        ))]
        if let Some(drops) = self.drops.as_mut() {
            drops.dispatch(self.translator.scale(), &mut self.events);
        }
        self.dispatch(event_loop);
        if let Some(until) = self.backoff_until
            && std::time::Instant::now() >= until
            && let Some(w) = &self.window
        {
            w.request_redraw();
        }
        // A timed redraw (the owed Final frame): a `WaitUntil` only wakes
        // the loop, it draws nothing. Before this, the Final after a
        // gesture appeared only if something else asked for a frame.
        if let Some(at) = self.redraw_at {
            if std::time::Instant::now() >= at {
                self.redraw_at = None;
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            } else {
                let until = self.backoff_until.map_or(at, |b| b.min(at));
                event_loop.set_control_flow(ControlFlow::WaitUntil(until));
            }
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        let mut ctx = ctx_of!(self);
        self.app.on_exit(&mut ctx);
        // It borrows the event loop's `wl_display`, which dies with the loop.
        #[cfg(all(
            unix,
            not(any(target_os = "macos", target_os = "ios", target_os = "android"))
        ))]
        {
            self.drops = None;
        }
    }
}

/// Turns the application's answer into a control flow and a redraw request.
///
/// Returns when a [`FrameRequest::RedrawAfter`] is due: the loop only wakes
/// at that instant, and `about_to_wait` is what turns the wake-up into a
/// redraw.
fn apply_frame_request(
    event_loop: &ActiveEventLoop,
    window: Option<&Arc<Window>>,
    request: FrameRequest,
) -> Option<std::time::Instant> {
    match request {
        // Park. `Wait` is what makes an idle window cost nothing; polling
        // here is the difference between 0 % and a busy core.
        FrameRequest::Idle => {
            event_loop.set_control_flow(ControlFlow::Wait);
            None
        }
        FrameRequest::Redraw => {
            event_loop.set_control_flow(ControlFlow::Wait);
            if let Some(w) = window {
                w.request_redraw();
            }
            None
        }
        FrameRequest::RedrawAfter(delay) => {
            let at = std::time::Instant::now() + delay;
            event_loop.set_control_flow(ControlFlow::WaitUntil(at));
            Some(at)
        }
    }
}

pub(crate) fn run_shell<A: ShellApp>(
    config: ShellConfig,
    app: A,
) -> Result<ShellLoop<A>, ShellError> {
    if let Some(reason) = crate::display::headless_skip_reason() {
        return Err(ShellError::NoDisplay { reason });
    }
    let event_loop = EventLoop::new().map_err(|e| ShellError::EventLoop(e.to_string()))?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let waker = ShellWaker {
        proxy: Some(event_loop.create_proxy()),
    };
    let mut shell = ShellLoop::new(config, app, waker);
    event_loop
        .run_app(&mut shell)
        .map_err(|e| ShellError::EventLoop(e.to_string()))?;
    match shell.error.take() {
        Some(e) => Err(e),
        None => Ok(shell),
    }
}
