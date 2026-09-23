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
use crate::{
    APP_ID, AdapterReport, BackendPreference, FrameRequest, ShellApp, ShellConfig, ShellError,
    record_first_frame,
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
    ui: Option<UiFrame>,
    /// Read the next presented frame back into a PNG, then maybe exit.
    capture: Option<(std::path::PathBuf, bool)>,
}

#[derive(Debug)]
enum CanvasOp {
    Show(CanvasFrame),
    Move((i32, i32)),
    Clear,
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
        }
    };
}

/// The GPU state bound to one window.
#[derive(Debug)]
pub(crate) struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    painter: Painter,
    /// Where `wgpu` reports the errors it would otherwise panic on.
    errors: GpuErrorSink,
    pub(crate) report: AdapterReport,
}

impl Gpu {
    pub(crate) fn new(
        window: Arc<Window>,
        preference: BackendPreference,
    ) -> Result<Self, ShellError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: preference.backends(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| ShellError::Surface(e.to_string()))?;

        // `force_fallback_adapter` stays false: we would rather have the real
        // GPU. A software adapter is still acceptable if it is all there is,
        // which is why the failure below is the only hard stop.
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            ..Default::default()
        }))
        .map_err(|_| ShellError::NoAdapter {
            tried: preference.describe().to_owned(),
        })?;

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

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("xarast device"),
            ..Default::default()
        }))
        .map_err(|e| ShellError::Surface(e.to_string()))?;
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
        surface.configure(&device, &config);
        let painter = Painter::new(&device, config.format);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            painter,
            errors,
            report,
        })
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
            Some(CanvasOp::Show(f)) => self.painter.set_canvas(&self.device, &self.queue, &f),
            Some(CanvasOp::Move(o)) => self.painter.move_canvas(o),
            Some(CanvasOp::Clear) => self.painter.clear_canvas(),
            None => {}
        }
        if let Some(ui) = pending.ui.take() {
            self.painter.set_ui(&self.device, &self.queue, ui);
        }
    }

    /// Presents one frame: the canvas pass, then the interface pass, over
    /// a flat backdrop.
    fn present(&mut self, capture: Option<&std::path::Path>) -> FrameOutcome {
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
    /// What to do about GPU errors, frame by frame.
    recovery: GpuRecovery,
    /// While set, frames are neither drawn nor presented: the GPU is being
    /// left alone after a run of errors.
    backoff_until: Option<std::time::Instant>,
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
            portals: PortalService::start(),
            app,
            pending: PendingFrame::default(),
            waker,
            events: Vec::new(),
            frames: 0,
            consecutive_failures: 0,
            recovery: GpuRecovery::new(),
            backoff_until: None,
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
        match Gpu::new(window.clone(), self.config.backends) {
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
                apply_frame_request(event_loop, self.window.as_ref(), request);

                let Some(gpu) = self.gpu.as_mut() else { return };
                if self.inject_errors > 0 {
                    self.inject_errors -= 1;
                    gpu.inject_error();
                }
                // Applied whether or not this present succeeds: texture
                // deltas must reach the GPU exactly once and in order.
                gpu.apply(&mut self.pending);
                let capture = self.pending.capture.take();
                let outcome = gpu.present(capture.as_ref().map(|(p, _)| p.as_path()));
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
        self.dispatch(event_loop);
        if let Some(until) = self.backoff_until
            && std::time::Instant::now() >= until
            && let Some(w) = &self.window
        {
            w.request_redraw();
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        let mut ctx = ctx_of!(self);
        self.app.on_exit(&mut ctx);
    }
}

/// Turns the application's answer into a control flow and a redraw request.
fn apply_frame_request(
    event_loop: &ActiveEventLoop,
    window: Option<&Arc<Window>>,
    request: FrameRequest,
) {
    match request {
        // Park. `Wait` is what makes an idle window cost nothing; polling
        // here is the difference between 0 % and a busy core.
        FrameRequest::Idle => event_loop.set_control_flow(ControlFlow::Wait),
        FrameRequest::Redraw => {
            event_loop.set_control_flow(ControlFlow::Wait);
            if let Some(w) = window {
                w.request_redraw();
            }
        }
        FrameRequest::RedrawAfter(delay) => {
            event_loop.set_control_flow(ControlFlow::wait_duration(delay));
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
