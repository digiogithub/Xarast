//! GPU errors as a diagnosis, never a panic.
//!
//! `wgpu` turns every error nobody captured into a panic by default. For an
//! editor that is the worst possible policy: a driver quirk or a bug in one
//! pass would take the open documents down with it. This module replaces
//! it with two small pieces:
//!
//! * [`GpuErrorSink`] — installed with `Device::on_uncaptured_error`. It
//!   logs the error, counts it and keeps the latest description. It is the
//!   only part that runs inside `wgpu`'s callback, so it does nothing but
//!   record: no GPU call, no allocation beyond the message.
//! * [`GpuRecovery`] — a pure policy the event loop consults after every
//!   frame: nothing went wrong, reconfigure the surface, or stop presenting
//!   for a while and try again. Pure, so the escalation is tested without
//!   an adapter.
//!
//! * [`DeviceLoss`] — the policy for a device that is gone (a driver
//!   reset, a GPU hang, a suspend cycle): rebuild the device and carry the
//!   picture over; after a second loss, rebuild without GPU tiles. Also
//!   pure, so the ladder is tested without losing a real device.
//!
//! The loop also turns each new error into a [`crate::ShellEvent::GpuError`]
//! so the application can say so in its status bar.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// What the event loop is told after a frame that raised GPU errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuErrorReport {
    /// The description of the most recent error.
    pub message: String,
    /// Errors raised since the device was created, this one included.
    pub total: u64,
    /// Errors raised since the previous report.
    pub new: u64,
}

#[derive(Debug, Default)]
struct Shared {
    total: AtomicU64,
    last: Mutex<Option<String>>,
    /// The device was lost and has not been rebuilt yet.
    lost: AtomicBool,
}

/// Collects the errors `wgpu` did not route to an error scope.
///
/// Cheap to clone; every clone sees the same counters.
#[derive(Debug, Clone, Default)]
pub struct GpuErrorSink {
    shared: Arc<Shared>,
    /// What [`GpuErrorSink::take_report`] has already reported.
    seen: u64,
}

impl GpuErrorSink {
    /// An empty sink.
    #[must_use]
    pub fn new() -> GpuErrorSink {
        GpuErrorSink::default()
    }

    /// Records one error. Called from `wgpu`'s uncaptured-error callback,
    /// possibly on another thread.
    pub fn record(&self, kind: &str, description: &str) {
        let total = self.shared.total.fetch_add(1, Ordering::AcqRel) + 1;
        tracing::error!(kind, total, error = %description, "GPU error (recovering, not fatal)");
        *self
            .shared
            .last
            .lock()
            .unwrap_or_else(PoisonError::into_inner) =
            Some(format!("{kind}: {}", summary(description)));
    }

    /// Records that the device is lost: an error like any other, plus a
    /// flag [`GpuErrorSink::take_lost`] hands to the event loop once. Called
    /// from `wgpu`'s device-lost callback, and by the loop itself to
    /// simulate a loss (`XARAST_INJECT_DEVICE_LOSS`, the tests).
    pub fn record_lost(&self, description: &str) {
        self.record("device lost", description);
        self.shared.lost.store(true, Ordering::Release);
    }

    /// Whether the device was lost since the last call.
    pub fn take_lost(&self) -> bool {
        self.shared.lost.swap(false, Ordering::AcqRel)
    }

    /// Installs this sink as `device`'s uncaptured-error handler.
    pub fn install(&self, device: &wgpu::Device) {
        let sink = self.clone();
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            let kind = match &e {
                wgpu::Error::OutOfMemory { .. } => "out of memory",
                wgpu::Error::Validation { .. } => "validation",
                wgpu::Error::Internal { .. } => "internal",
            };
            sink.record(kind, &e.to_string());
        }));
    }

    /// Errors raised since the sink was created.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.shared.total.load(Ordering::Acquire)
    }

    /// The errors raised since the previous call, if any.
    pub fn take_report(&mut self) -> Option<GpuErrorReport> {
        let total = self.total();
        if total == self.seen {
            return None;
        }
        let new = total - self.seen;
        self.seen = total;
        let message = self
            .shared
            .last
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .unwrap_or_default();
        Some(GpuErrorReport {
            message,
            total,
            new,
        })
    }
}

/// The one line of a `wgpu` error worth showing in a status bar: the
/// innermost cause, which is the last non-empty line of its report
/// ("Validation Error / Caused by: / … / the actual reason").
fn summary(description: &str) -> &str {
    description
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .unwrap_or("")
}

/// What the loop does after a frame, given whether it raised errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recovery {
    /// Nothing to do.
    Continue,
    /// Re-apply the surface configuration: the cheap fix, and the right one
    /// for the whole family of stale-swapchain errors.
    Reconfigure,
    /// Stop presenting for this long, then try again. The application keeps
    /// running and its state keeps advancing; only the GPU is left alone.
    Backoff(Duration),
}

/// Frames in a row that may fail before reconfiguring stops being the
/// answer and the loop starts backing off.
pub const RECONFIGURE_ATTEMPTS: u32 = 3;

/// The first back-off: about one frame at 60 Hz.
const FIRST_BACKOFF: Duration = Duration::from_millis(16);

/// The longest back-off. Long enough not to burn a core on a broken
/// device, short enough that recovery is noticed promptly.
pub const MAX_BACKOFF: Duration = Duration::from_secs(2);

/// The escalation policy: reconfigure a few times, then back off
/// exponentially, and forget everything after one clean frame.
///
/// It never gives up and never asks the process to exit — a GPU that keeps
/// failing leaves the document open, saveable and at most a few seconds
/// away from another attempt.
#[derive(Debug, Clone, Copy, Default)]
pub struct GpuRecovery {
    streak: u32,
}

impl GpuRecovery {
    /// A policy with no failures recorded.
    #[must_use]
    pub const fn new() -> GpuRecovery {
        GpuRecovery { streak: 0 }
    }

    /// Frames in a row that raised errors.
    #[must_use]
    pub const fn streak(&self) -> u32 {
        self.streak
    }

    /// Decides what to do after a frame. `failed` is whether the frame
    /// raised any GPU error.
    pub fn after_frame(&mut self, failed: bool) -> Recovery {
        if !failed {
            self.streak = 0;
            return Recovery::Continue;
        }
        self.streak = self.streak.saturating_add(1);
        if self.streak <= RECONFIGURE_ATTEMPTS {
            return Recovery::Reconfigure;
        }
        let doublings = (self.streak - RECONFIGURE_ATTEMPTS - 1).min(16);
        Recovery::Backoff(
            FIRST_BACKOFF
                .saturating_mul(1 << doublings)
                .min(MAX_BACKOFF),
        )
    }
}

/// Why the shell replaced its device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebuildCause {
    /// The device was lost; `losses` so far this session.
    Lost {
        /// Devices lost so far, this one included.
        losses: u32,
    },
    /// The application asked for safe mode.
    SafeMode,
}

/// A device replaced, for [`crate::ShellEvent::GpuRebuilt`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRebuilt {
    /// What presents now, as the status bar names it
    /// ([`crate::RendererStatus`]'s display).
    pub renderer: String,
    /// Why.
    pub cause: RebuildCause,
}

/// What to rebuild after a device loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RebuildPlan {
    /// Whether the new device may hold the canvas as GPU tiles. `false`
    /// from the second loss on: a device that keeps getting lost gets as
    /// little to do as possible, the CPU tier (the GPU only presents).
    pub gpu_tiles: bool,
}

/// The first wait before trying again when a device cannot be rebuilt.
const FIRST_REBUILD_WAIT: Duration = Duration::from_millis(250);

/// The device-loss ladder (phase 12 F2): rebuild the device, keep the
/// picture, and on a second loss fall back to CPU composition. A rebuild
/// that fails is retried with an exponential wait capped at
/// [`MAX_BACKOFF`]; nothing here ever gives up or exits — the document
/// lives in the application, which a lost device never touches.
#[derive(Debug, Clone, Copy, Default)]
pub struct DeviceLoss {
    losses: u32,
    failures: u32,
}

impl DeviceLoss {
    /// No loss so far.
    #[must_use]
    pub const fn new() -> DeviceLoss {
        DeviceLoss {
            losses: 0,
            failures: 0,
        }
    }

    /// Devices lost so far.
    #[must_use]
    pub const fn losses(&self) -> u32 {
        self.losses
    }

    /// Records a loss and says what to rebuild.
    pub fn on_lost(&mut self) -> RebuildPlan {
        self.losses = self.losses.saturating_add(1);
        self.plan()
    }

    /// What a rebuild now should be.
    #[must_use]
    pub const fn plan(&self) -> RebuildPlan {
        RebuildPlan {
            gpu_tiles: self.losses <= 1,
        }
    }

    /// A rebuild failed: how long to wait before the next attempt.
    pub fn on_rebuild_failed(&mut self) -> Duration {
        let doublings = self.failures.min(16);
        self.failures = self.failures.saturating_add(1);
        FIRST_REBUILD_WAIT
            .saturating_mul(1 << doublings)
            .min(MAX_BACKOFF)
    }

    /// A rebuild succeeded.
    pub fn on_rebuilt(&mut self) {
        self.failures = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_loss_rebuilds_as_it_was_and_the_second_drops_gpu_tiles() {
        let mut d = DeviceLoss::new();
        assert!(d.plan().gpu_tiles);
        assert_eq!(d.on_lost(), RebuildPlan { gpu_tiles: true });
        d.on_rebuilt();
        assert_eq!(d.on_lost(), RebuildPlan { gpu_tiles: false });
        assert_eq!(d.on_lost(), RebuildPlan { gpu_tiles: false });
        assert_eq!(d.losses(), 3);
    }

    #[test]
    fn failed_rebuilds_wait_longer_up_to_a_cap_and_success_resets_the_wait() {
        let mut d = DeviceLoss::new();
        d.on_lost();
        let first = d.on_rebuild_failed();
        assert_eq!(first, FIRST_REBUILD_WAIT);
        let mut last = first;
        for _ in 0..30 {
            let w = d.on_rebuild_failed();
            assert!(w >= last && w <= MAX_BACKOFF);
            last = w;
        }
        assert_eq!(last, MAX_BACKOFF);
        d.on_rebuilt();
        assert_eq!(d.on_rebuild_failed(), FIRST_REBUILD_WAIT);
    }

    #[test]
    fn a_loss_is_reported_once_and_counted_as_an_error() {
        let mut sink = GpuErrorSink::new();
        assert!(!sink.take_lost());
        sink.clone().record_lost("injected");
        assert!(sink.take_lost());
        assert!(!sink.take_lost(), "once");
        let r = sink.take_report().expect("an error too");
        assert_eq!(r.message, "device lost: injected");
    }

    #[test]
    fn a_clean_frame_needs_nothing() {
        let mut r = GpuRecovery::new();
        assert_eq!(r.after_frame(false), Recovery::Continue);
        assert_eq!(r.streak(), 0);
    }

    #[test]
    fn errors_reconfigure_first_then_back_off_up_to_a_cap() {
        let mut r = GpuRecovery::new();
        for _ in 0..RECONFIGURE_ATTEMPTS {
            assert_eq!(r.after_frame(true), Recovery::Reconfigure);
        }
        let mut last = Duration::ZERO;
        for _ in 0..40 {
            let Recovery::Backoff(d) = r.after_frame(true) else {
                panic!("expected a back-off after {RECONFIGURE_ATTEMPTS} reconfigures");
            };
            assert!(d >= last && d <= MAX_BACKOFF, "{d:?} after {last:?}");
            last = d;
        }
        assert_eq!(last, MAX_BACKOFF);
    }

    #[test]
    fn one_clean_frame_forgets_the_streak() {
        let mut r = GpuRecovery::new();
        for _ in 0..10 {
            r.after_frame(true);
        }
        assert_eq!(r.after_frame(false), Recovery::Continue);
        assert_eq!(r.after_frame(true), Recovery::Reconfigure);
    }

    #[test]
    fn the_sink_reports_each_error_once() {
        let mut sink = GpuErrorSink::new();
        assert_eq!(sink.take_report(), None);
        let clone = sink.clone();
        clone.record("validation", "first");
        clone.record("validation", "second");
        let report = sink.take_report().expect("two errors were recorded");
        assert_eq!(report.new, 2);
        assert_eq!(report.total, 2);
        assert_eq!(report.message, "validation: second");
        assert_eq!(sink.take_report(), None);
    }

    #[test]
    fn the_summary_is_the_innermost_cause() {
        let wgpu_style =
            "Validation Error\n\nCaused by:\n  In create_buffer\n    MAP usage is wrong\n";
        assert_eq!(summary(wgpu_style), "MAP usage is wrong");
        assert_eq!(summary(""), "");
    }

    /// A real device, a real validation error, and no panic. Skips when
    /// the machine has no adapter, as every GPU test in this crate does.
    #[test]
    fn a_validation_error_on_a_real_device_is_counted_not_fatal() {
        // The machine-wide GPU lock, bound first so that it drops last.
        let Some(_gpu) = xarast_render::gpu_test_lock::acquire("shell::gpu_errors") else {
            return;
        };
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN | wgpu::Backends::GL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let Ok(adapter) =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        else {
            eprintln!("skipped: no GPU adapter");
            return;
        };
        let info = adapter.get_info();
        eprintln!("GPU error sink on {} ({:?})", info.name, info.backend);
        let Ok((device, _queue)) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
        else {
            eprintln!("skipped: no GPU device");
            return;
        };
        let mut sink = GpuErrorSink::new();
        sink.install(&device);
        // MAP_READ together with MAP_WRITE is invalid without a feature
        // nobody enables: the textbook validation error.
        let _bad = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("deliberately invalid"),
            size: 16,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::MAP_WRITE,
            mapped_at_creation: false,
        });
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let report = sink.take_report().expect("the error reached the sink");
        assert!(report.message.starts_with("validation"), "{report:?}");
    }
}
