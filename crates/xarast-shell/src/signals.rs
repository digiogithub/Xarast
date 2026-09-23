//! SIGINT, SIGTERM and SIGHUP (XARA-T-0086, `research/06 §10.4`).
//!
//! A signal must not lose work and must not leave document locks behind;
//! `atexit` does neither, because a signal does not run it. The handler is
//! a thread reading `signal_hook`'s safe iterator:
//!
//! 1. on the first signal it sets a flag and wakes the event loop; the
//!    viewer sees the flag, writes an autosave of every modified document
//!    (`AppState::emergency_shutdown`), drops every session — which
//!    releases their locks — and exits;
//! 2. if the process is still alive [`GRACE`] later, or a second signal
//!    arrives, the thread removes every lock file this process holds
//!    (`xarast_app::locks::release_all`) and exits with `128 + signal`.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::ShellWaker;

/// How long the orderly shutdown gets before the locks are released by
/// force.
pub const GRACE: Duration = Duration::from_secs(5);

/// What the handler thread and the event loop share.
#[derive(Debug, Clone, Default)]
pub struct SignalWatch {
    inner: Arc<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    requested: AtomicBool,
    signal: AtomicI32,
    waker: Mutex<Option<ShellWaker>>,
}

impl SignalWatch {
    /// A watch no signal reaches, for tests and for platforms without
    /// signals.
    #[must_use]
    pub fn inert() -> SignalWatch {
        SignalWatch::default()
    }

    /// Installs the handler thread for SIGINT, SIGTERM and SIGHUP. On
    /// failure (no thread, a registration refused) the watch is inert and
    /// the default dispositions stay.
    #[must_use]
    pub fn install() -> SignalWatch {
        let watch = SignalWatch::default();
        #[cfg(unix)]
        {
            use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
            match signal_hook::iterator::Signals::new([SIGINT, SIGTERM, SIGHUP]) {
                Ok(mut signals) => {
                    let w = watch.clone();
                    let spawned = std::thread::Builder::new()
                        .name("xarast-signals".into())
                        .spawn(move || {
                            for sig in signals.forever() {
                                w.raise(sig);
                            }
                        });
                    if let Err(e) = spawned {
                        tracing::warn!(error = %e, "no signal thread; locks are released on exit only");
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "signal handlers not installed");
                }
            }
        }
        watch
    }

    /// What the handler thread does with one signal. The first asks for an
    /// orderly shutdown and arms the fallback; a second is the fallback.
    fn raise(&self, sig: i32) {
        self.inner.signal.store(sig, Ordering::SeqCst);
        let first = !self.inner.requested.swap(true, Ordering::SeqCst);
        if !first {
            fallback(sig);
        }
        tracing::info!(signal = sig, "shutting down on a signal");
        if let Ok(w) = self.inner.waker.lock()
            && let Some(w) = w.as_ref()
        {
            w.wake();
        }
        let _ = std::thread::Builder::new()
            .name("xarast-signal-grace".into())
            .spawn(move || {
                std::thread::sleep(GRACE);
                fallback(sig);
            });
    }

    /// Lets the handler wake the event loop. Set once the loop exists.
    pub fn set_waker(&self, waker: ShellWaker) {
        if let Ok(mut w) = self.inner.waker.lock() {
            *w = Some(waker);
        }
    }

    /// Whether a signal asked for shutdown.
    #[must_use]
    pub fn requested(&self) -> bool {
        self.inner.requested.load(Ordering::SeqCst)
    }

    /// The signal that asked for shutdown, if one did (for the exit
    /// status, `128 + signal`).
    #[must_use]
    pub fn signal(&self) -> Option<i32> {
        let sig = self.inner.signal.load(Ordering::SeqCst);
        (self.requested() && sig > 0).then_some(sig)
    }

    /// Marks a shutdown as asked for, as a signal would, without the
    /// fallback timer: for tests.
    pub fn simulate(&self) {
        self.inner.requested.store(true, Ordering::SeqCst);
    }
}

/// The last resort: release every lock this process holds and exit.
fn fallback(sig: i32) -> ! {
    let n = xarast_app::locks::release_all();
    eprintln!("xarast: signal {sig}: released {n} document lock(s) and exiting");
    std::process::exit(128 + sig);
}
