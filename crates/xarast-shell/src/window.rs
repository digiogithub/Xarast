//! The window, the GPU surface and the event loop.
//!
//! The one place `winit` and `wgpu` are driven. Everything the application
//! sees of them is [`crate::input::event::ShellEvent`] and
//! [`crate::GpuContext`].

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::clipboard::{Clipboard, system_clipboard};
use crate::decorations::DecorationPlan;
use crate::display::{DisplayEnvironment, PlatformCapabilities};
use crate::input::event::ShellEvent;
use crate::input::translate::EventTranslator;
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
    pub(crate) exit: bool,
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

        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| {
                ShellError::Surface("the adapter cannot present to this surface".to_owned())
            })?;
        // Prefer an sRGB swapchain so that the compositor does not apply a
        // second, unwanted transfer function to output we already encoded.
        let caps = surface.get_capabilities(&adapter);
        if let Some(srgb) = caps.formats.iter().copied().find(|f| f.is_srgb()) {
            config.format = srgb;
        }
        surface.configure(&device, &config);

        Ok(Self {
            surface,
            device,
            queue,
            config,
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

    /// Presents one frame. Phase 0 draws a flat clear colour; the scene
    /// arrives with the canvas.
    fn present(&mut self) -> FrameOutcome {
        use wgpu::CurrentSurfaceTexture as Cst;

        let frame = match self.surface.get_current_texture() {
            Cst::Success(frame) => frame,
            // Suboptimal still gives us a usable texture. Draw this frame, then
            // reconfigure so the next one is not suboptimal too.
            Cst::Suboptimal(frame) => {
                self.reconfigure();
                frame
            }
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
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.10,
                            g: 0.10,
                            b: 0.11,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
        record_first_frame();
        FrameOutcome::Presented
    }

    /// Re-applies the current configuration, which is how a lost, outdated or
    /// suboptimal swapchain is recovered.
    fn reconfigure(&mut self) {
        self.surface.configure(&self.device, &self.config);
    }
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
    events: Vec<ShellEvent>,
    frames: u32,
    consecutive_failures: u32,
    pub(crate) report: Option<AdapterReport>,
    pub(crate) error: Option<ShellError>,
}

impl<A: ShellApp> ShellLoop<A> {
    fn new(config: ShellConfig, app: A) -> Self {
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
            events: Vec::new(),
            frames: 0,
            consecutive_failures: 0,
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

                let mut ctx = ctx_of!(self);
                let request = self.app.on_frame(&mut ctx);
                let exit = ctx.exit;
                if exit {
                    event_loop.exit();
                    return;
                }
                apply_frame_request(event_loop, self.window.as_ref(), request);

                let Some(gpu) = self.gpu.as_mut() else { return };
                match gpu.present() {
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

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.collect_portal_answers();
        self.dispatch(event_loop);
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
    let mut shell = ShellLoop::new(config, app);
    event_loop
        .run_app(&mut shell)
        .map_err(|e| ShellError::EventLoop(e.to_string()))?;
    match shell.error.take() {
        Some(e) => Err(e),
        None => Ok(shell),
    }
}
