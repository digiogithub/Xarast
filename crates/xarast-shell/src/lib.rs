//! Windowing, GPU surface and platform integration.
//!
//! Wayland is the primary target: fractional scaling, client-side decorations,
//! XDG portals for file dialogs, and pressure-sensitive tablet input.
//!
//! See `docs/phases/phase-05-shell-and-ui.md`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

/// The Wayland and X11 application identifier.
///
/// This must equal the basename of `packaging/linux/xarast.desktop`. When the
/// two drift apart, GNOME and KDE fall back to a generic icon and group windows
/// wrongly — a failure that is invisible in development and obvious to users,
/// which is why a test pins it.
pub const APP_ID: &str = "xarast";

/// Wall-clock milliseconds from process start to the first presented frame,
/// stored as bits of an `f64`. Zero means "not presented yet".
static COLD_START_MS: AtomicU64 = AtomicU64::new(0);

/// Process start, captured as early as the first call to [`mark_process_start`].
static PROCESS_START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// Records the process start instant. Call this first thing in `main`, before
/// any other initialisation, or the cold-start figure is meaningless.
pub fn mark_process_start() {
    let _ = PROCESS_START.set(Instant::now());
}

/// Milliseconds from process start to the first presented frame.
///
/// `None` before the first frame is presented.
pub fn cold_start_ms() -> Option<f64> {
    match COLD_START_MS.load(Ordering::Relaxed) {
        0 => None,
        bits => Some(f64::from_bits(bits)),
    }
}

fn record_first_frame() {
    if COLD_START_MS.load(Ordering::Relaxed) != 0 {
        return;
    }
    let elapsed = PROCESS_START
        .get()
        .map(|start| start.elapsed().as_secs_f64() * 1000.0)
        .unwrap_or(f64::NAN);
    COLD_START_MS.store(elapsed.to_bits(), Ordering::Relaxed);
    tracing::info!(cold_start_ms = elapsed, "first frame presented");
}

/// Which wgpu backends to try, and in what order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackendPreference {
    /// Vulkan first, then GL. The default: Vulkan where it works, GL where it
    /// does not, and a software adapter rather than a hard failure.
    #[default]
    VulkanThenGl,
    /// GL only. Useful for reproducing driver bugs and for constrained hosts.
    GlOnly,
    /// Whatever wgpu picks, including platform-native backends.
    Auto,
}

impl BackendPreference {
    fn backends(self) -> wgpu::Backends {
        match self {
            Self::VulkanThenGl => wgpu::Backends::VULKAN | wgpu::Backends::GL,
            Self::GlOnly => wgpu::Backends::GL,
            Self::Auto => wgpu::Backends::all(),
        }
    }

    fn describe(self) -> &'static str {
        match self {
            Self::VulkanThenGl => "Vulkan, then GL",
            Self::GlOnly => "GL only",
            Self::Auto => "any",
        }
    }
}

/// How the shell should start up.
#[derive(Debug, Clone)]
pub struct ShellConfig {
    /// Initial logical window size.
    pub size: (u32, u32),
    /// Window title.
    pub title: String,
    /// Preferred backends, tried in order.
    pub backends: BackendPreference,
    /// Exit after presenting this many frames. `None` runs until the window is
    /// closed; `Some(n)` is what makes the shell testable without a human.
    pub exit_after_frames: Option<u32>,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            size: (1280, 800),
            title: "Xarast".to_owned(),
            backends: BackendPreference::default(),
            exit_after_frames: None,
        }
    }
}

/// What the shell actually got from the system.
///
/// Printed by `--selftest-window` and worth asking for in any bug report: most
/// rendering complaints are answered by the adapter line alone.
#[derive(Debug, Clone)]
pub struct AdapterReport {
    /// Driver-reported adapter name.
    pub name: String,
    /// Backend in use.
    pub backend: &'static str,
    /// Discrete, integrated, virtual, CPU or other.
    pub device_type: &'static str,
    /// True for a CPU adapter such as lavapipe or llvmpipe. Correct output,
    /// unusable speed — worth saying out loud rather than letting the user
    /// conclude the program is slow.
    pub is_software: bool,
}

impl std::fmt::Display for AdapterReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} ({}, {}){}",
            self.name,
            self.backend,
            self.device_type,
            if self.is_software {
                ", software rasterisation"
            } else {
                ""
            }
        )
    }
}

/// Anything that can stop the shell from starting.
#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    /// No adapter at all, not even a software one.
    #[error("no compatible GPU adapter (tried {tried})")]
    NoAdapter {
        /// Human-readable description of the backends that were tried.
        tried: String,
    },
    /// The compositor refused to give us a window.
    #[error("failed to create a window: {0}")]
    Window(String),
    /// We have a window but cannot draw to it.
    #[error("surface configuration failed: {0}")]
    Surface(String),
    /// The event loop itself failed.
    #[error("event loop error: {0}")]
    EventLoop(String),
}

/// Initialises tracing from the `XARAST_LOG` environment variable.
///
/// Idempotent: calling it twice is harmless, which matters because both the
/// binary and the self-tests want logging.
pub fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_env("XARAST_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}

/// The GPU state bound to one window.
struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    report: AdapterReport,
}

impl Gpu {
    fn new(window: Arc<Window>, preference: BackendPreference) -> Result<Self, ShellError> {
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

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    /// Presents one frame. Phase 0 draws a flat clear colour; the scene arrives
    /// in phase 4.
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

/// The winit application. Holds everything that only exists once the event loop
/// has resumed, which on Wayland is the only point at which a surface is legal.
struct Shell {
    config: ShellConfig,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    frames: u32,
    consecutive_failures: u32,
    report: Option<AdapterReport>,
    error: Option<ShellError>,
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

impl Shell {
    fn new(config: ShellConfig) -> Self {
        Self {
            config,
            window: None,
            gpu: None,
            frames: 0,
            consecutive_failures: 0,
            report: None,
            error: None,
        }
    }

    fn window_attributes(&self) -> winit::window::WindowAttributes {
        let attrs = Window::default_attributes()
            .with_title(self.config.title.clone())
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
}

impl ApplicationHandler for Shell {
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
        window.request_redraw();
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.resize(size.width, size.height);
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                // Fractional scaling on Wayland arrives here; the surface is
                // resized by the Resized event that follows.
                tracing::debug!(scale_factor, "scale factor changed");
            }
            WindowEvent::RedrawRequested => {
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
            _ => {}
        }
    }
}

fn run_shell(config: ShellConfig) -> Result<Shell, ShellError> {
    let event_loop = EventLoop::new().map_err(|e| ShellError::EventLoop(e.to_string()))?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut shell = Shell::new(config);
    event_loop
        .run_app(&mut shell)
        .map_err(|e| ShellError::EventLoop(e.to_string()))?;
    match shell.error.take() {
        Some(e) => Err(e),
        None => Ok(shell),
    }
}

/// Runs the application to completion. Blocks the calling thread.
pub fn run(config: ShellConfig) -> Result<(), ShellError> {
    run_shell(config).map(|_| ())
}

/// Creates a window and a surface, presents one frame, tears it all down and
/// reports what it got.
///
/// This is the CI liveness check. It needs a compositor socket, not a
/// human-visible display.
pub fn selftest_window(config: &ShellConfig) -> Result<AdapterReport, ShellError> {
    let mut config = config.clone();
    config.exit_after_frames = Some(config.exit_after_frames.unwrap_or(1));
    let shell = run_shell(config)?;
    shell.report.ok_or(ShellError::NoAdapter {
        tried: "no adapter was reached".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The app id and the desktop entry must not drift apart. See [`APP_ID`].
    #[test]
    fn app_id_matches_the_desktop_entry() {
        let entry = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../packaging/linux/xarast.desktop"
        );
        let path = std::path::Path::new(entry);
        assert!(path.exists(), "packaging/linux/xarast.desktop is missing");
        let basename = path.file_stem().unwrap().to_str().unwrap();
        assert_eq!(basename, APP_ID, "desktop entry basename must equal APP_ID");

        // The entry's own StartupWMClass has to agree too, or the association
        // fails on X11 even when the filename is right.
        let text = std::fs::read_to_string(path).unwrap();
        assert!(
            text.lines()
                .any(|l| l.trim() == format!("StartupWMClass={APP_ID}")),
            "desktop entry must declare StartupWMClass={APP_ID}"
        );
    }

    #[test]
    fn cold_start_is_none_before_any_frame() {
        assert!(cold_start_ms().is_none());
    }
}
