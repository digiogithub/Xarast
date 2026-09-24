//! The machine-wide lock every test and bench that touches a real GPU takes.
//!
//! Test support, not engine code: nothing in the library calls it and the
//! release binary never reaches it. It lives here, in the lowest crate that
//! talks to `wgpu`, so that the render tests, the render benches and the
//! shell's tests share one implementation and one lock file.
//!
//! # Why
//!
//! Several unattended `cargo test --workspace` runs once opened `wgpu`
//! devices on the maintainer's display GPU at the same time, and the
//! compositor hung (`docs/memory/render.md`, "GPU tests"). The fix is not to
//! avoid the GPU but to never touch it from two places at once, whichever
//! process or thread they are in.
//!
//! # How
//!
//! [`acquire`] returns a [`GpuTestLock`] guard, or `None` when
//! `XARAST_GPU_TESTS=0` asks to skip GPU tests. The guard holds two locks,
//! released when it drops:
//!
//! 1. a process-wide mutex, so that parallel test threads in one binary
//!    queue up without depending on how the platform scopes file locks;
//! 2. an exclusive [`File::lock`] on one well-known file,
//!    `$XDG_RUNTIME_DIR/xarast-gpu-tests.lock` (or the same name in
//!    [`std::env::temp_dir`]), so that separate test binaries, separate
//!    `cargo` invocations and separate checkouts queue up too.
//!
//! The kernel drops the file lock when the process exits, so a crashed or
//! killed test never leaves it stuck. A waiting test says so on stderr and
//! gives up with a panic after `XARAST_GPU_LOCK_TIMEOUT_SECS` (default 900).
//!
//! Callers take the guard **before** creating the `wgpu` instance and keep
//! it alive until every device, queue and resource is gone: bind it first
//! (`let Some(_gpu) = acquire("…") else { return };`), so that it drops
//! last.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, TryLockError as MutexTryLockError};
use std::time::{Duration, Instant};

/// The environment variable that turns GPU tests off when set to `0`.
pub const ENV_SWITCH: &str = "XARAST_GPU_TESTS";

/// The environment variable that bounds the wait for the lock, in seconds.
pub const ENV_TIMEOUT: &str = "XARAST_GPU_LOCK_TIMEOUT_SECS";

/// The lock file's name, in the runtime directory or the temp directory.
pub const LOCK_FILE_NAME: &str = "xarast-gpu-tests.lock";

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(900);
const POLL: Duration = Duration::from_millis(100);
const REMIND_EVERY: Duration = Duration::from_secs(30);

static IN_PROCESS: Mutex<()> = Mutex::new(());

/// Exclusive use of the machine's GPUs for tests, until dropped.
#[must_use = "the GPU is only reserved while the guard is alive"]
#[derive(Debug)]
pub struct GpuTestLock {
    // Field order is drop order: the file lock goes before the mutex, so a
    // thread of this process never sees the mutex free while the file is
    // still held by its sibling.
    _file: Option<File>,
    _in_process: MutexGuard<'static, ()>,
}

/// Whether `XARAST_GPU_TESTS` allows GPU tests. They run unless it is `0`.
#[must_use]
pub fn enabled() -> bool {
    std::env::var(ENV_SWITCH).as_deref() != Ok("0")
}

/// The lock file: `$XDG_RUNTIME_DIR/xarast-gpu-tests.lock`, or the same name
/// in the temp directory when there is no runtime directory.
#[must_use]
pub fn lock_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|d| d.is_dir())
        .unwrap_or_else(std::env::temp_dir);
    dir.join(LOCK_FILE_NAME)
}

/// Reserves the GPU for the test named `who`, waiting as long as needed, or
/// returns `None` (and says so) when `XARAST_GPU_TESTS=0`.
///
/// # Panics
///
/// When the lock is still held by someone else after the timeout, so that a
/// hung GPU test elsewhere fails this one instead of hanging it too.
pub fn acquire(who: &str) -> Option<GpuTestLock> {
    if !enabled() {
        eprintln!("{who}: skipping, {ENV_SWITCH}=0 turns GPU tests off");
        return None;
    }
    let timeout = std::env::var(ENV_TIMEOUT)
        .ok()
        .and_then(|s| s.parse().ok())
        .map_or(DEFAULT_TIMEOUT, Duration::from_secs);
    let start = Instant::now();

    let in_process = match IN_PROCESS.try_lock() {
        Ok(g) => g,
        Err(MutexTryLockError::Poisoned(p)) => p.into_inner(),
        Err(MutexTryLockError::WouldBlock) => {
            eprintln!("{who}: waiting for the GPU test lock (held by another test thread)");
            IN_PROCESS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }
    };

    let path = lock_path();
    let file = match OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
    {
        Ok(f) => f,
        Err(e) => {
            // No lock file means no cross-process serialisation; say so
            // loudly rather than silently skip the GPU tests.
            eprintln!(
                "{who}: cannot open the GPU test lock {}: {e}; serialising within this process only",
                path.display()
            );
            return Some(GpuTestLock {
                _file: None,
                _in_process: in_process,
            });
        }
    };

    let mut last_note: Option<Instant> = None;
    loop {
        match file.try_lock() {
            Ok(()) => break,
            Err(TryLockError::WouldBlock) => {}
            Err(TryLockError::Error(e)) if e.kind() == ErrorKind::Unsupported => {
                eprintln!(
                    "{who}: file locks are unsupported here; serialising within this process only"
                );
                return Some(GpuTestLock {
                    _file: None,
                    _in_process: in_process,
                });
            }
            Err(TryLockError::Error(e)) => {
                panic!("{who}: cannot lock {}: {e}", path.display())
            }
        }
        let waited = start.elapsed();
        if waited >= timeout {
            panic!(
                "{who}: gave up after {}s waiting for the GPU test lock {} \
                 (another process is using the GPU; raise {ENV_TIMEOUT} or \
                 check for a hung test)",
                waited.as_secs(),
                path.display()
            );
        }
        if last_note.is_none_or(|t| t.elapsed() >= REMIND_EVERY) {
            eprintln!(
                "{who}: waiting for the GPU test lock {} (held by another process, {}s so far)",
                path.display(),
                waited.as_secs()
            );
            last_note = Some(Instant::now());
        }
        std::thread::sleep(POLL);
    }

    Some(GpuTestLock {
        _file: Some(file),
        _in_process: in_process,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lock_file_is_a_well_known_name() {
        assert!(lock_path().ends_with(LOCK_FILE_NAME));
    }

    /// A second open of the same file cannot lock it while the first holds
    /// it: the property that serialises separate processes. No GPU here.
    #[test]
    fn a_held_lock_file_excludes_a_second_handle() {
        let path = std::env::temp_dir().join(format!(
            "xarast-gpu-lock-selftest-{}.lock",
            std::process::id()
        ));
        let open = || {
            OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&path)
                .unwrap()
        };
        let (a, b) = (open(), open());
        a.lock().unwrap();
        assert!(matches!(b.try_lock(), Err(TryLockError::WouldBlock)));
        drop(a);
        b.try_lock().unwrap();
        drop(b);
        let _ = std::fs::remove_file(&path);
    }
}
