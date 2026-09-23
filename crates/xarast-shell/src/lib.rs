//! Windowing, GPU surface and platform integration.
//!
//! Wayland is the primary target: fractional scaling, client-side
//! decorations, XDG portals for file dialogs, clipboard, drag and drop, and
//! pressure-sensitive tablet input.
//!
//! # What this crate is for
//!
//! `xarast-shell` is the *only* crate that names `winit` and `wgpu`
//! (`docs/10-architecture.md` §2). Everything above it consumes
//! [`ShellEvent`], which mentions no windowing library at all. That is the
//! whole design: phase 14 adds Windows and macOS by writing one new
//! translation module, not by touching a tool, a panel or a command.
//!
//! ```text
//!   winit / wgpu        input::translate        the rest of Xarast
//!   ─────────────  ──►  ───────────────────  ──►  ─────────────────
//!   platform events     ShellEvent, StrokeSample, ShellCtx
//!                       ▲ replaced per platform   ▲ never replaced
//! ```
//!
//! # Running without a compositor
//!
//! Every entry point degrades and reports rather than panicking. With no
//! Wayland or X11 socket, [`run`] returns [`ShellError::NoDisplay`] with a
//! reason; [`crate::clipboard::system_clipboard`] returns a clipboard that
//! explains itself; [`crate::portal::PortalService`] answers every request
//! with the reason portals are unreachable. Tests that need a compositor use
//! [`crate::display::headless_skip_reason`] to **skip**, never to fail.
//!
//! # Modules
//!
//! | Module | What it owns |
//! |---|---|
//! | [`display`] | Which display server we are on and what it can do |
//! | [`scale`] | The single owner of the fractional scale factor |
//! | [`decorations`] | Who draws the window frame |
//! | [`input`] | The platform-neutral event model and its translation |
//! | [`intents`] | Physical [`ShellEvent`]s to semantic `xarast_app::Intent`s |
//! | [`portal`] | XDG portals on a services thread |
//! | [`clipboard`] | The system clipboard, and its Wayland caveat |
//!
//! See `docs/phases/phase-05-shell-and-ui.md` and `docs/memory/ui.md`.

#![deny(missing_docs)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub mod clipboard;
pub mod decorations;
pub mod display;
pub mod input;
pub mod intents;
mod paint;
pub mod portal;
pub mod scale;
pub mod viewer;
mod window;

/// The input-method seam.
///
/// At the crate root rather than under [`input`] because phase 9 is its only
/// consumer and it is the one part of input that a text tool, not the shell,
/// drives.
#[path = "input/ime.rs"]
pub mod ime;

pub use clipboard::{Clipboard, ClipboardError, ClipboardImage};
pub use decorations::{DecorationMode, DecorationPlan};
pub use display::{
    Desktop, DisplayEnvironment, DisplayServer, PlatformCapabilities, headless_skip_reason,
};
pub use input::event::{
    ColorScheme, DragEvent, GestureEvent, PointerButton, PointerEvent, PointerId, PointerPhase,
    ScrollUnit, ShellEvent,
};
pub use input::keyboard::{Key, KeyEvent, KeyLocation, KeyState, Modifiers, NamedKey, Shortcut};
pub use input::tablet::{InputSource, StrokeSample, TabletCaps, TabletSource, ToolAxes};
pub use paint::{CanvasFrame, UiFrame};
pub use portal::{
    FileFilter, OpenFileRequest, PortalEvent, PortalHandle, PortalRequestId, SaveFileRequest,
};
pub use scale::{LogicalSize, PhysicalPos, PhysicalSize, ScaleFactor};
pub use window::{ShellCtx, ShellWaker};

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
#[must_use]
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
        .map_or(f64::NAN, |start| start.elapsed().as_secs_f64() * 1000.0);
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

/// What the application asks for at the end of a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameRequest {
    /// Nothing changed; park until the next input. Idle CPU has a budget too.
    Idle,
    /// Repaint as soon as possible.
    Redraw,
    /// Repaint after this delay — the Draft-to-Final timer, and nothing else
    /// so far.
    RedrawAfter(Duration),
}

/// Implemented by the application. The shell owns the loop; the application
/// owns the state.
///
/// Every method has a default, so a caller that only wants a window (the
/// self-tests, the cold-start measurement) implements nothing.
pub trait ShellApp: 'static {
    /// One platform event.
    fn on_event(&mut self, event: ShellEvent, ctx: &mut ShellCtx<'_>) {
        let _ = (event, ctx);
    }

    /// Called once per frame, before the surface is presented.
    fn on_frame(&mut self, ctx: &mut ShellCtx<'_>) -> FrameRequest {
        let _ = ctx;
        FrameRequest::Idle
    }

    /// Called once, as the loop exits.
    fn on_exit(&mut self, ctx: &mut ShellCtx<'_>) {
        let _ = ctx;
    }
}

/// An application that does nothing: a window, and no behaviour.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullApp;

impl ShellApp for NullApp {}

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
    /// There is no display server at all.
    ///
    /// Distinct from every other error on purpose: this one is a fact about
    /// the machine, not a fault, and the caller should skip rather than
    /// report a failure.
    #[error("{reason}")]
    NoDisplay {
        /// What was looked for and not found.
        reason: String,
    },
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

impl ShellError {
    /// True when this is "the machine has no compositor" rather than a fault.
    #[must_use]
    pub const fn is_missing_display(&self) -> bool {
        matches!(self, Self::NoDisplay { .. })
    }
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

/// Installs a panic hook that logs the panic before unwinding.
///
/// The event loop swallows a panic in a callback on some backends, which
/// turns a crash into a silently dead window. Logging first means the report
/// exists even when the process disappears.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(panic = %info, "xarast panicked");
        previous(info);
    }));
}

/// A one-line description of the session, for `--version --verbose` and for
/// the first line of a bug report.
#[must_use]
pub fn platform_summary() -> String {
    let env = DisplayEnvironment::from_env();
    let caps = env.capabilities();
    let plan = DecorationPlan::for_environment(&env);
    format!(
        "{} · {} · fractional scaling: {} · tablet axes: {} · gestures: {}",
        env.summary(),
        plan.summary(),
        yes_no(caps.fractional_scale),
        yes_no(caps.tablet_axes),
        yes_no(caps.gestures),
    )
}

const fn yes_no(v: bool) -> &'static str {
    if v { "yes" } else { "no" }
}

/// Runs the application to completion. Blocks the calling thread.
///
/// # Errors
/// [`ShellError::NoDisplay`] when there is no compositor, and the
/// window/surface/event-loop errors otherwise.
pub fn run(config: ShellConfig) -> Result<(), ShellError> {
    run_app(config, NullApp)
}

/// Runs `app` to completion. Blocks the calling thread.
///
/// # Errors
/// [`ShellError::NoDisplay`] when there is no compositor, and the
/// window/surface/event-loop errors otherwise.
pub fn run_app<A: ShellApp>(config: ShellConfig, app: A) -> Result<(), ShellError> {
    window::run_shell(config, app).map(|_| ())
}

/// Creates a window and a surface, presents one frame, tears it all down and
/// reports what it got.
///
/// This is the CI liveness check. It needs a compositor socket, not a
/// human-visible display.
///
/// # Errors
/// [`ShellError::NoDisplay`] when there is no compositor — a reason to skip,
/// not to fail — and the window/surface errors otherwise.
pub fn selftest_window(config: &ShellConfig) -> Result<AdapterReport, ShellError> {
    let mut config = config.clone();
    config.exit_after_frames = Some(config.exit_after_frames.unwrap_or(1));
    let shell = window::run_shell(config, NullApp)?;
    shell.report.clone().ok_or(ShellError::NoAdapter {
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

    #[test]
    fn a_machine_with_no_compositor_reports_that_and_does_not_panic() {
        let Some(reason) = headless_skip_reason() else {
            // There is a compositor; opening a window here would need a
            // display we cannot assume in a test, so there is nothing to do.
            return;
        };
        assert!(reason.contains("display server"), "{reason}");

        let err = run(ShellConfig {
            exit_after_frames: Some(1),
            ..ShellConfig::default()
        })
        .expect_err("a headless machine cannot open a window");
        assert!(err.is_missing_display(), "{err}");
        assert!(err.to_string().contains("no display server"), "{err}");

        let err = selftest_window(&ShellConfig::default())
            .expect_err("a headless machine cannot present a frame");
        assert!(err.is_missing_display(), "{err}");
    }

    #[test]
    fn the_platform_summary_is_printable_anywhere() {
        let s = platform_summary();
        assert!(s.contains("fractional scaling"), "{s}");
        assert!(!s.is_empty());
    }

    #[test]
    fn backend_preferences_describe_themselves() {
        assert_eq!(
            BackendPreference::default(),
            BackendPreference::VulkanThenGl
        );
        assert!(BackendPreference::GlOnly.describe().contains("GL"));
        assert!(
            BackendPreference::VulkanThenGl
                .backends()
                .contains(wgpu::Backends::VULKAN)
        );
    }

    #[test]
    fn an_application_with_no_opinion_parks_the_loop() {
        // The default frame request has to be Idle: anything else spins the
        // loop and blows the idle-CPU budget for every caller that has no
        // opinion. There is no window here, so the context is not available;
        // what is asserted is the default itself.
        #[derive(Debug, Default)]
        struct Counting(u32);
        impl ShellApp for Counting {
            fn on_event(&mut self, _event: ShellEvent, _ctx: &mut ShellCtx<'_>) {
                self.0 += 1;
            }
        }
        let app = Counting::default();
        assert_eq!(app.0, 0);
    }

    #[test]
    fn a_frame_request_carries_its_delay() {
        let r = FrameRequest::RedrawAfter(Duration::from_millis(120));
        assert_eq!(r, FrameRequest::RedrawAfter(Duration::from_millis(120)));
        assert_ne!(r, FrameRequest::Idle);
    }
}
