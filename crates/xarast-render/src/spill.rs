//! The spill directory: where evicted full-resolution bitmap levels go
//! when re-producing them from their source would be expensive (phase 10,
//! T10.5.4).
//!
//! # Layout
//!
//! ```text
//! <root>/                       $XDG_CACHE_HOME/xarast/spill, or ~/.cache/…
//!   session-<pid>-<nanos>/      one per process that spilled anything
//!     lock                      held with an exclusive file lock for life
//!     <n>.rgba                  one evicted base level, raw bytes
//! ```
//!
//! The root is on disk, not in `/tmp`: on most distributions `/tmp` is a
//! RAM-backed `tmpfs`, and spilling pixels into RAM to save RAM is no
//! saving.
//!
//! # Lifetime and crash recovery
//!
//! A [`SpillFile`] deletes itself when dropped, and a [`SpillDir`] removes
//! its whole session directory when dropped. A process that dies without
//! running destructors (a crash, `kill -9`, or simply exiting with the
//! process-wide budget still alive in a static) leaves its session
//! directory behind; the kernel drops its file lock with it. The next
//! [`SpillDir::create`] under the same root sweeps every sibling session
//! whose lock it can take — those owners are gone — so leftovers live at
//! most until the next session that spills.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The environment variable that overrides the spill root.
pub const ENV_SPILL_DIR: &str = "XARAST_SPILL_DIR";

const SESSION_PREFIX: &str = "session-";
const LOCK_NAME: &str = "lock";

/// The default spill root: `$XARAST_SPILL_DIR`, else
/// `$XDG_CACHE_HOME/xarast/spill`, else `$HOME/.cache/xarast/spill`, else
/// `xarast-spill` in the temp directory.
#[must_use]
pub fn default_root() -> PathBuf {
    if let Some(dir) = std::env::var_os(ENV_SPILL_DIR).filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|d| !d.is_empty())
                .map(|h| PathBuf::from(h).join(".cache"))
        });
    match cache {
        Some(c) => c.join("xarast").join("spill"),
        None => std::env::temp_dir().join("xarast-spill"),
    }
}

/// One process's spill directory, removed with everything in it on drop.
#[derive(Debug)]
pub struct SpillDir {
    path: PathBuf,
    next: AtomicU64,
    // Held for the life of the directory: its lock is what tells a later
    // session that this one is still alive.
    _lock: File,
}

impl SpillDir {
    /// Creates a fresh session directory under `root` (made if missing),
    /// after sweeping the sessions under it that no live process holds.
    ///
    /// # Errors
    ///
    /// Any I/O error creating the directory or its lock file.
    pub fn create(root: &Path) -> io::Result<SpillDir> {
        std::fs::create_dir_all(root)?;
        sweep_stale(root);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let path = root.join(format!("{SESSION_PREFIX}{}-{nanos}", std::process::id()));
        std::fs::create_dir(&path)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path.join(LOCK_NAME))?;
        lock.lock()?;
        Ok(SpillDir {
            path,
            next: AtomicU64::new(0),
            _lock: lock,
        })
    }

    /// The session directory.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Writes `data` to a new file in the session directory.
    ///
    /// # Errors
    ///
    /// Any I/O error; a partly written file is removed.
    pub fn write(&self, data: &[u8]) -> io::Result<SpillFile> {
        let n = self.next.fetch_add(1, Ordering::Relaxed);
        let path = self.path.join(format!("{n}.rgba"));
        let result = File::create(&path).and_then(|mut f| {
            f.write_all(data)?;
            f.flush()
        });
        match result {
            Ok(()) => Ok(SpillFile {
                path,
                len: data.len() as u64,
            }),
            Err(e) => {
                let _ = std::fs::remove_file(&path);
                Err(e)
            }
        }
    }
}

impl Drop for SpillDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Removes every session directory under `root` whose lock nobody holds.
/// Returns how many it removed. Best effort: anything unreadable is left.
pub fn sweep_stale(root: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let is_session = name.to_str().is_some_and(|n| n.starts_with(SESSION_PREFIX));
        if !is_session || !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let dir = entry.path();
        let stale = match File::open(dir.join(LOCK_NAME)) {
            // Taking the lock proves the owner is gone; it is released when
            // `f` drops, before the directory goes.
            Ok(f) => match f.try_lock() {
                Ok(()) => true,
                Err(TryLockError::WouldBlock) => false,
                Err(TryLockError::Error(_)) => false,
            },
            // A session dies between creating its directory and its lock
            // file only by crashing; nothing can be in it.
            Err(e) => e.kind() == io::ErrorKind::NotFound,
        };
        if stale && std::fs::remove_dir_all(&dir).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// One spilled level. The file is deleted when this drops.
#[derive(Debug)]
pub struct SpillFile {
    path: PathBuf,
    len: u64,
}

impl SpillFile {
    /// The file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Its length in bytes.
    #[must_use]
    pub const fn len(&self) -> u64 {
        self.len
    }

    /// Whether it is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Reads the bytes back. A file whose length is not the length written
    /// is an error, never a short image.
    ///
    /// # Errors
    ///
    /// Any I/O error, or `InvalidData` for a length mismatch.
    pub fn read(&self) -> io::Result<Vec<u8>> {
        let mut f = File::open(&self.path)?;
        let len = usize::try_from(self.len).map_err(|_| io::ErrorKind::InvalidData)?;
        if f.metadata()?.len() != self.len {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let mut out = Vec::new();
        out.try_reserve_exact(len)
            .map_err(|_| io::Error::from(io::ErrorKind::OutOfMemory))?;
        f.read_to_end(&mut out)?;
        if out.len() != len {
            return Err(io::ErrorKind::InvalidData.into());
        }
        Ok(out)
    }
}

impl Drop for SpillFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "xarast-spill-test-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn a_spilled_level_reads_back_and_is_deleted_on_drop() {
        let root = scratch("roundtrip");
        let dir = SpillDir::create(&root).expect("create");
        let data: Vec<u8> = (0..10_000u32).map(|i| (i * 7) as u8).collect();
        let f = dir.write(&data).expect("write");
        assert_eq!(f.len(), 10_000);
        assert_eq!(f.read().expect("read"), data);
        let path = f.path().to_path_buf();
        drop(f);
        assert!(!path.exists());
        let session = dir.path().to_path_buf();
        drop(dir);
        assert!(!session.exists(), "the session directory goes on drop");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_truncated_spill_file_is_an_error_not_a_short_image() {
        let root = scratch("truncated");
        let dir = SpillDir::create(&root).expect("create");
        let f = dir.write(&[1, 2, 3, 4, 5, 6, 7, 8]).expect("write");
        std::fs::write(f.path(), [1, 2, 3]).expect("truncate");
        assert!(f.read().is_err());
        drop(f);
        drop(dir);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_dead_sessions_directory_is_swept_and_a_live_one_is_kept() {
        let root = scratch("sweep");
        std::fs::create_dir_all(&root).expect("root");
        // A crashed session: a directory, its lock file, a spilled level,
        // and no process holding the lock.
        let dead = root.join("session-999999999-1");
        std::fs::create_dir(&dead).expect("dead");
        std::fs::write(dead.join(LOCK_NAME), b"").expect("lock");
        std::fs::write(dead.join("0.rgba"), [0u8; 64]).expect("level");
        // A session that crashed before its lock file existed.
        let bare = root.join("session-999999999-2");
        std::fs::create_dir(&bare).expect("bare");
        // Something that is not ours.
        let other = root.join("keep-me");
        std::fs::create_dir(&other).expect("other");

        let live = SpillDir::create(&root).expect("create");
        assert!(!dead.exists(), "the dead session is swept");
        assert!(!bare.exists(), "the lockless session is swept");
        assert!(other.exists(), "foreign entries are left alone");
        assert!(live.path().exists());

        // A second session must not sweep the first, which is alive.
        let second = SpillDir::create(&root).expect("second");
        assert!(live.path().exists(), "a held lock keeps a session");
        drop(second);
        drop(live);
        let _ = std::fs::remove_dir_all(&root);
    }
}
