//! Document locks held by this process (XARA-T-0086, `research/06 §10.4`).
//!
//! A [`HeldLock`] is an `xarast_format::DocumentLock` that is also listed
//! in a process-wide registry, so that a signal handler can release every
//! lock the process holds without reaching into the documents
//! ([`release_all`]). A lock is released when its session closes (drop),
//! and the registry entry goes with it.
//!
//! The registry is the fallback path. The normal path on SIGINT/SIGTERM is
//! an orderly shutdown in the shell: autosave, then drop every session.
//! `release_all` is what runs if that does not finish in time — `atexit`
//! alone would not, because a signal does not run it.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use xarast_format::{DocumentLock, LockError, LockHolder};

static HELD: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// A document lock this process holds. Released on drop.
#[derive(Debug)]
pub struct HeldLock {
    lock: Option<DocumentLock>,
    file: PathBuf,
}

/// How to take a lock the user has been asked about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockMode {
    /// Take it if it is free, or if its holder is provably gone (a dead
    /// process on this machine).
    Normal,
    /// Take it whoever holds it: "Force (risky)".
    Force,
}

impl HeldLock {
    /// Takes the lock of `doc_path`.
    ///
    /// # Errors
    ///
    /// [`LockError::Held`] when someone else has it; [`LockError::Unavailable`]
    /// when no lock file can be made there (the document is then used
    /// unlocked).
    pub fn acquire(doc_path: &Path, mode: LockMode) -> Result<HeldLock, LockError> {
        let lock = match mode {
            LockMode::Normal => DocumentLock::steal_if_stale(doc_path)?,
            LockMode::Force => DocumentLock::force(doc_path)?,
        };
        let file = lock.path().to_path_buf();
        if let Ok(mut held) = HELD.lock() {
            held.push(file.clone());
        }
        Ok(HeldLock {
            lock: Some(lock),
            file,
        })
    }

    /// The lock file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.file
    }

    /// Who holds it (this process).
    #[must_use]
    pub fn holder(&self) -> Option<&LockHolder> {
        self.lock.as_ref().map(DocumentLock::holder)
    }
}

impl Drop for HeldLock {
    fn drop(&mut self) {
        if let Some(lock) = self.lock.take() {
            lock.release();
        }
        if let Ok(mut held) = HELD.lock()
            && let Some(i) = held.iter().position(|p| *p == self.file)
        {
            held.swap_remove(i);
        }
    }
}

/// The lock files this process holds now.
#[must_use]
pub fn held() -> Vec<PathBuf> {
    HELD.lock().map(|h| h.clone()).unwrap_or_default()
}

/// Removes every lock file this process holds, for a process that is about
/// to end without dropping its sessions (a signal the orderly shutdown did
/// not finish in time). A file is removed only if it still names this
/// process, so a lock another session forced in the meantime survives.
/// Returns how many were removed.
pub fn release_all() -> usize {
    release_where(|_| true)
}

fn release_where(pick: impl Fn(&Path) -> bool) -> usize {
    let files: Vec<PathBuf> = {
        let mut h = match HELD.lock() {
            Ok(h) => h,
            // A poisoned registry still holds the paths.
            Err(p) => p.into_inner(),
        };
        let (take, keep) = std::mem::take(&mut *h).into_iter().partition(|f| pick(f));
        *h = keep;
        take
    };
    let me = std::process::id();
    let mut removed = 0;
    for f in files {
        let ours = std::fs::read_to_string(&f)
            .ok()
            .and_then(|t| LockHolder::parse(&t))
            .is_some_and(|h| h.pid == me && h.is_local());
        if ours && std::fs::remove_file(&f).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("xarast-locks-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("drawing.xarast")
    }

    #[test]
    fn a_held_lock_is_registered_and_released_on_drop() {
        let doc = scratch("drop");
        let lock = HeldLock::acquire(&doc, LockMode::Normal).unwrap();
        let file = lock.path().to_path_buf();
        assert!(file.exists());
        assert!(held().contains(&file));
        assert!(matches!(
            HeldLock::acquire(&doc, LockMode::Normal),
            Err(LockError::Held { stale: false, .. })
        ));
        drop(lock);
        assert!(!file.exists());
        assert!(!held().contains(&file));
    }

    #[test]
    fn release_all_removes_what_this_process_holds() {
        let doc = scratch("all");
        let lock = HeldLock::acquire(&doc, LockMode::Normal).unwrap();
        let file = lock.path().to_path_buf();
        assert_eq!(release_where(|p| p == file), 1);
        assert!(!file.exists());
        // The session still drops its lock later; that must be harmless.
        drop(lock);
        assert!(!file.exists());
    }

    #[test]
    fn force_takes_a_live_lock() {
        let doc = scratch("force");
        let first = HeldLock::acquire(&doc, LockMode::Normal).unwrap();
        let second = HeldLock::acquire(&doc, LockMode::Force).unwrap();
        // The first holder no longer owns the file and must not remove it.
        drop(first);
        assert!(second.path().exists());
    }
}
