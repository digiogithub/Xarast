//! The document lock (`research/06 §10.4`).
//!
//! `.<name>.lock` next to the document (the LibreOffice pattern, which works
//! on network filesystems), created with `O_CREAT|O_EXCL`, holding the
//! key/value text below, and additionally held with an advisory exclusive
//! `flock`, which gives reliable local detection even when the holder died
//! without cleaning up.
//!
//! ```text
//! xarast-lock/1
//! pid=48213
//! host=machine-name
//! user=jose
//! boot-id=8f1c3d2e-…
//! doc-id=01J9Q7ZB2K4M8N6P3R5T7V9W1X
//! since=2026-09-19T17:02:11Z
//! ```
//!
//! A lock is **stale** when its host and boot id match ours, its process is
//! not alive and nobody holds its `flock`. A stale lock is reclaimed by
//! [`DocumentLock::steal_if_stale`]; any other held lock is reported as
//! [`LockError::Held`] and the application offers "open read-only", "open a
//! copy" or "force" ([`DocumentLock::force`]).
//!
//! If the lock cannot be created at all — a read-only directory, a
//! filesystem without `O_EXCL` — the result is [`LockError::Unavailable`] and
//! the document **must still open** (§10.4, last rule).
//!
//! The lock is released on drop. Removing it on `SIGINT`/`SIGTERM` needs a
//! signal handler, which is the application's job (F6.4): this crate has no
//! `unsafe` and no signal dependency.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use thiserror::Error;

const MAGIC: &str = "xarast-lock/1";
/// A lock file larger than this is not ours; do not read more of it.
const MAX_LOCK_FILE: u64 = 4096;

/// Who holds a lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockHolder {
    /// Process id.
    pub pid: u32,
    /// Host name.
    pub host: String,
    /// User name.
    pub user: String,
    /// The kernel boot id, where the platform has one.
    pub boot_id: Option<String>,
    /// The document's persistent id, when known.
    pub doc_id: Option<String>,
    /// When the lock was taken, RFC 3339 UTC.
    pub since: String,
}

fn clean(v: &str) -> String {
    v.chars().filter(|c| !c.is_control()).collect()
}

impl LockHolder {
    /// This process, now.
    pub fn current(doc_id: Option<&str>) -> LockHolder {
        LockHolder {
            pid: std::process::id(),
            host: host_name(),
            user: user_name(),
            boot_id: boot_id(),
            doc_id: doc_id.map(clean),
            since: crate::time::rfc3339_utc(SystemTime::now()),
        }
    }

    /// The lock file's text.
    pub fn to_text(&self) -> String {
        let mut s = format!(
            "{MAGIC}\npid={}\nhost={}\nuser={}\n",
            self.pid,
            clean(&self.host),
            clean(&self.user)
        );
        if let Some(b) = &self.boot_id {
            s.push_str(&format!("boot-id={}\n", clean(b)));
        }
        if let Some(d) = &self.doc_id {
            s.push_str(&format!("doc-id={}\n", clean(d)));
        }
        s.push_str(&format!("since={}\n", clean(&self.since)));
        s
    }

    /// Parses a lock file. Unknown keys are ignored; `pid` and `host` are
    /// required.
    pub fn parse(text: &str) -> Option<LockHolder> {
        let mut lines = text.lines();
        if lines.next()?.trim() != MAGIC {
            return None;
        }
        let (mut pid, mut host, mut user, mut boot_id, mut doc_id, mut since) =
            (None, None, None, None, None, None);
        for line in lines {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let v = v.trim().to_owned();
            match k.trim() {
                "pid" => pid = v.parse().ok(),
                "host" => host = Some(v),
                "user" => user = Some(v),
                "boot-id" => boot_id = Some(v),
                "doc-id" => doc_id = Some(v),
                "since" => since = Some(v),
                _ => {}
            }
        }
        Some(LockHolder {
            pid: pid?,
            host: host?,
            user: user.unwrap_or_default(),
            boot_id,
            doc_id,
            since: since.unwrap_or_default(),
        })
    }

    /// Same host and same boot: the pid means something here.
    pub fn is_local(&self) -> bool {
        self.host == host_name() && self.boot_id.is_some() && self.boot_id == boot_id()
    }

    /// Whether the holding process is known to be gone.
    pub fn is_dead(&self) -> bool {
        self.is_local() && self.pid != std::process::id() && !pid_alive(self.pid)
    }
}

/// Why a lock was not taken.
#[derive(Debug, Error)]
pub enum LockError {
    /// Someone holds it. `holder` is `None` if the lock file is unreadable
    /// or not ours; `stale` says whether it may be reclaimed automatically.
    #[error("the document is locked{}", holder.as_ref().map(|h| format!(" by {}@{} (pid {})", h.user, h.host, h.pid)).unwrap_or_default())]
    Held {
        /// Who holds it.
        holder: Option<Box<LockHolder>>,
        /// Reclaimable without asking.
        stale: bool,
    },
    /// No lock can be created here (read-only directory, …). The document
    /// opens anyway, unlocked.
    #[error("cannot create a lock file: {0}")]
    Unavailable(io::Error),
}

/// The lock file of a document: `.<file name>.lock` in the same directory.
pub fn lock_path(doc_path: &Path) -> io::Result<PathBuf> {
    let name = doc_path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "document path has no file name",
        )
    })?;
    let mut s = std::ffi::OsString::from(".");
    s.push(name);
    s.push(".lock");
    Ok(doc_path.with_file_name(s))
}

/// A held document lock. Released on drop.
#[derive(Debug)]
pub struct DocumentLock {
    path: PathBuf,
    file: Option<File>,
    holder: LockHolder,
    text: String,
}

impl DocumentLock {
    /// Takes the lock of `doc_path`.
    pub fn acquire(doc_path: &Path) -> Result<DocumentLock, LockError> {
        DocumentLock::acquire_with(doc_path, None)
    }

    /// Takes the lock, recording the document id in it.
    pub fn acquire_with(doc_path: &Path, doc_id: Option<&str>) -> Result<DocumentLock, LockError> {
        let path = lock_path(doc_path).map_err(LockError::Unavailable)?;
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                let holder = LockHolder::current(doc_id);
                let text = holder.to_text();
                let written = file
                    .write_all(text.as_bytes())
                    .and_then(|()| file.sync_all())
                    .map_err(LockError::Unavailable);
                if let Err(e) = written {
                    let _ = fs::remove_file(&path);
                    return Err(e);
                }
                // Best effort: a filesystem without `flock` still has the
                // O_EXCL file, which is the part that works over NFS/SMB.
                let _ = file.try_lock();
                Ok(DocumentLock {
                    path,
                    file: Some(file),
                    holder,
                    text,
                })
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(inspect(&path)),
            Err(e) => Err(LockError::Unavailable(e)),
        }
    }

    /// Takes the lock, reclaiming it first if it is stale.
    pub fn steal_if_stale(doc_path: &Path) -> Result<DocumentLock, LockError> {
        match DocumentLock::acquire(doc_path) {
            Err(LockError::Held { stale: true, .. }) => {
                let path = lock_path(doc_path).map_err(LockError::Unavailable)?;
                fs::remove_file(&path).map_err(LockError::Unavailable)?;
                DocumentLock::acquire(doc_path)
            }
            other => other,
        }
    }

    /// Takes the lock whoever holds it: the "Force (risky)" choice.
    pub fn force(doc_path: &Path) -> Result<DocumentLock, LockError> {
        let path = lock_path(doc_path).map_err(LockError::Unavailable)?;
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(LockError::Unavailable(e)),
        }
        DocumentLock::acquire(doc_path)
    }

    /// This lock's holder (this process).
    pub fn holder(&self) -> &LockHolder {
        &self.holder
    }

    /// The lock file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Releases the lock now (also done on drop).
    pub fn release(mut self) {
        self.release_inner();
    }

    fn release_inner(&mut self) {
        let Some(file) = self.file.take() else { return };
        // Only remove the file if it is still ours: after a `force` by
        // someone else it is theirs, even if its text happens to be
        // identical (same process, same second).
        if still_ours(&file, &self.path, &self.text) {
            let _ = fs::remove_file(&self.path);
        }
        let _ = file.unlock();
    }
}

impl Drop for DocumentLock {
    fn drop(&mut self) {
        self.release_inner();
    }
}

#[cfg(unix)]
fn still_ours(file: &File, path: &Path, _text: &str) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (file.metadata(), fs::metadata(path)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn still_ours(_file: &File, path: &Path, text: &str) -> bool {
    read_capped(path).is_ok_and(|t| t == text)
}

fn read_capped(path: &Path) -> io::Result<String> {
    let mut s = String::new();
    File::open(path)?
        .take(MAX_LOCK_FILE)
        .read_to_string(&mut s)?;
    Ok(s)
}

fn inspect(path: &Path) -> LockError {
    let holder = read_capped(path)
        .ok()
        .and_then(|t| LockHolder::parse(&t))
        .map(Box::new);
    // A live holder on this machine keeps its `flock`: if we can take it,
    // nobody local holds the file. It proves nothing across hosts.
    let flock_free = File::open(path).is_ok_and(|f| match f.try_lock() {
        Ok(()) => {
            let _ = f.unlock();
            true
        }
        Err(TryLockError::WouldBlock) => false,
        Err(TryLockError::Error(_)) => false,
    });
    let stale = holder.as_ref().is_some_and(|h| h.is_dead() && flock_free);
    LockError::Held { holder, stale }
}

fn first_line(path: &str) -> Option<String> {
    let s = fs::read_to_string(path).ok()?;
    let l = s.lines().next()?.trim().to_owned();
    (!l.is_empty()).then_some(l)
}

fn host_name() -> String {
    first_line("/proc/sys/kernel/hostname")
        .or_else(|| std::env::var("HOSTNAME").ok())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn user_name() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_owned())
}

fn boot_id() -> Option<String> {
    first_line("/proc/sys/kernel/random/boot_id")
}

/// Whether a local process exists. Only Linux can tell without `unsafe`;
/// elsewhere every process is assumed alive, which only ever makes a lock
/// look held rather than stale (the safe direction).
fn pid_alive(pid: u32) -> bool {
    if cfg!(target_os = "linux") {
        Path::new(&format!("/proc/{pid}")).exists()
    } else {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_round_trip() {
        let h = LockHolder {
            pid: 48213,
            host: "machine-name".into(),
            user: "jose".into(),
            boot_id: Some("8f1c3d2e".into()),
            doc_id: Some("01J9Q7ZB2K4M8N6P3R5T7V9W1X".into()),
            since: "2026-09-19T17:02:11Z".into(),
        };
        let t = h.to_text();
        assert!(t.starts_with("xarast-lock/1\npid=48213\n"));
        assert_eq!(LockHolder::parse(&t), Some(h));
        assert_eq!(LockHolder::parse("garbage"), None);
        assert_eq!(LockHolder::parse("xarast-lock/1\nhost=x\n"), None);
        // Control characters cannot forge extra keys.
        let evil = LockHolder {
            user: "a\npid=1".into(),
            ..LockHolder::current(None)
        };
        assert_eq!(
            LockHolder::parse(&evil.to_text()).unwrap().pid,
            std::process::id()
        );
    }

    #[test]
    fn exclusive_and_released_on_drop() {
        let d = tempfile::tempdir().unwrap();
        let doc = d.path().join("a.xarast");
        let lock = DocumentLock::acquire_with(&doc, Some("DOC1")).unwrap();
        assert_eq!(lock.path(), d.path().join(".a.xarast.lock"));
        assert_eq!(lock.holder().doc_id.as_deref(), Some("DOC1"));
        match DocumentLock::acquire(&doc) {
            Err(LockError::Held {
                holder: Some(h),
                stale,
            }) => {
                assert_eq!(h.pid, std::process::id());
                assert!(!stale, "our own live lock is never stale");
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            DocumentLock::steal_if_stale(&doc),
            Err(LockError::Held { .. })
        ));
        drop(lock);
        assert!(!d.path().join(".a.xarast.lock").exists());
        DocumentLock::acquire(&doc).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_dead_local_holder_is_stale_and_reclaimed() {
        let d = tempfile::tempdir().unwrap();
        let doc = d.path().join("b.xarast");
        // A pid that cannot exist: above the kernel's pid_max ceiling.
        let dead = LockHolder {
            pid: 4_194_305,
            ..LockHolder::current(None)
        };
        fs::write(lock_path(&doc).unwrap(), dead.to_text()).unwrap();
        match DocumentLock::acquire(&doc) {
            Err(LockError::Held { stale, .. }) => assert!(stale),
            other => panic!("{other:?}"),
        }
        let lock = DocumentLock::steal_if_stale(&doc).unwrap();
        assert_eq!(lock.holder().pid, std::process::id());
    }

    #[test]
    fn a_foreign_host_is_never_stale_but_can_be_forced() {
        let d = tempfile::tempdir().unwrap();
        let doc = d.path().join("c.xarast");
        let other = LockHolder {
            pid: 4_194_305,
            host: "another-machine".into(),
            ..LockHolder::current(None)
        };
        fs::write(lock_path(&doc).unwrap(), other.to_text()).unwrap();
        assert!(matches!(
            DocumentLock::steal_if_stale(&doc),
            Err(LockError::Held { stale: false, .. })
        ));
        let lock = DocumentLock::force(&doc).unwrap();
        assert_eq!(lock.holder().host, host_name());
    }

    #[test]
    fn a_forced_lock_is_not_removed_by_the_loser() {
        let d = tempfile::tempdir().unwrap();
        let doc = d.path().join("e.xarast");
        let first = DocumentLock::acquire(&doc).unwrap();
        let second = DocumentLock::force(&doc).unwrap();
        drop(first);
        assert!(
            second.path().exists(),
            "the loser must not delete the winner's lock"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_read_only_directory_reports_unavailable() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let ro = d.path().join("ro");
        fs::create_dir(&ro).unwrap();
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o555)).unwrap();
        let r = DocumentLock::acquire(&ro.join("d.xarast"));
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o755)).unwrap();
        // Root ignores directory permissions; only assert when it applied.
        if let Err(e) = r {
            assert!(matches!(e, LockError::Unavailable(_)), "{e:?}");
        }
    }
}
