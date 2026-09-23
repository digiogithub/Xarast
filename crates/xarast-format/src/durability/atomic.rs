//! Atomic save (`research/06 §10.1`).
//!
//! 1. Create `<name>.tmp-<pid>-<rand>` in the target's directory (same
//!    filesystem, so the rename is atomic), with `O_CREAT|O_EXCL`.
//! 2. Write the whole package into it.
//! 3. Flush and `fsync` it.
//! 4. Optionally keep the previous file as `<name>.bak` (one rotation).
//! 5. `rename` it over the target.
//! 6. `fsync` the directory, so the rename survives a power cut.
//!
//! The target is never opened for writing. If anything before step 5 fails,
//! the temporary file is removed and the target is exactly as it was. Step 6
//! runs after the target has been replaced; a directory that cannot be
//! synced (some filesystems refuse `fsync` on a directory) is tolerated.
//!
//! Removing the autosave and journal after a real save (step 6 of the spec)
//! belongs to the autosave session, which does not exist yet (F6.5).

use std::collections::hash_map::RandomState;
use std::fs::{self, File, OpenOptions};
use std::hash::{BuildHasher, Hasher};
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::error::WriteError;

/// Options for [`write_atomic_with`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AtomicOptions {
    /// Keep the previous file as `<name>.bak`, replacing an older backup.
    pub backup: bool,
}

/// Where a test makes the sequence fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fault {
    CreateTemp,
    Permissions,
    Flush,
    SyncFile,
    BackupLink,
    BackupRename,
    Rename,
}

fn inject(at: Option<Fault>, here: Fault) -> io::Result<()> {
    if at == Some(here) {
        Err(io::Error::other(format!("injected fault at {here:?}")))
    } else {
        Ok(())
    }
}

fn random_suffix() -> u64 {
    let mut h = RandomState::new().build_hasher();
    h.write_u128(
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    h.finish()
}

fn sibling(target: &Path, suffix: &str) -> io::Result<PathBuf> {
    let name = target
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target has no file name"))?;
    let mut s = name.to_os_string();
    s.push(suffix);
    Ok(target.with_file_name(s))
}

/// Writes `path` atomically with default options. `write` receives a
/// buffered handle on the temporary file; it may seek.
pub fn write_atomic<T>(
    path: &Path,
    write: impl FnOnce(&mut BufWriter<File>) -> Result<T, WriteError>,
) -> Result<T, WriteError> {
    write_atomic_with(path, AtomicOptions::default(), write)
}

/// Writes `path` atomically. See the module documentation.
pub fn write_atomic_with<T>(
    path: &Path,
    opts: AtomicOptions,
    write: impl FnOnce(&mut BufWriter<File>) -> Result<T, WriteError>,
) -> Result<T, WriteError> {
    write_atomic_inner(path, opts, write, None)
}

pub(crate) fn write_atomic_inner<T>(
    path: &Path,
    opts: AtomicOptions,
    write: impl FnOnce(&mut BufWriter<File>) -> Result<T, WriteError>,
    fault: Option<Fault>,
) -> Result<T, WriteError> {
    // Saving through a symlink replaces what it points at, not the link.
    let target = match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => fs::canonicalize(path)?,
        _ => path.to_path_buf(),
    };
    let dir = match target.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let tmp = sibling(
        &target,
        &format!(".tmp-{}-{:016x}", std::process::id(), random_suffix()),
    )?;

    inject(fault, Fault::CreateTemp)?;
    let file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
    let result = (|| {
        if let Ok(meta) = fs::metadata(&target) {
            inject(fault, Fault::Permissions)?;
            file.set_permissions(meta.permissions())?;
        }
        let mut w = BufWriter::new(file);
        let value = write(&mut w)?;
        inject(fault, Fault::Flush)?;
        let file = w.into_inner().map_err(|e| e.into_error())?;
        inject(fault, Fault::SyncFile)?;
        file.sync_all()?;
        drop(file);
        if opts.backup && target.exists() {
            let bak = sibling(&target, ".bak")?;
            let bak_tmp = sibling(
                &target,
                &format!(".bak.tmp-{}-{:016x}", std::process::id(), random_suffix()),
            )?;
            inject(fault, Fault::BackupLink)?;
            if fs::hard_link(&target, &bak_tmp).is_err()
                && let Err(e) = fs::copy(&target, &bak_tmp)
            {
                let _ = fs::remove_file(&bak_tmp);
                return Err(WriteError::Io(e));
            }
            let renamed =
                inject(fault, Fault::BackupRename).and_then(|()| fs::rename(&bak_tmp, &bak));
            if let Err(e) = renamed {
                let _ = fs::remove_file(&bak_tmp);
                return Err(WriteError::Io(e));
            }
        }
        inject(fault, Fault::Rename)?;
        fs::rename(&tmp, &target)?;
        Ok(value)
    })();
    let value = match result {
        Ok(v) => v,
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
    };
    sync_dir(&dir);
    Ok(value)
}

/// `fsync` of a directory. POSIX needs it for the rename to be durable;
/// elsewhere, or on a filesystem that refuses it, it is skipped.
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn leftovers(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".tmp-"))
            .collect()
    }

    #[test]
    fn writes_and_replaces() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("doc.xarast");
        write_atomic(&p, |w| Ok(w.write_all(b"one")?)).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"one");
        write_atomic_with(&p, AtomicOptions { backup: true }, |w| {
            Ok(w.write_all(b"two")?)
        })
        .unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        assert_eq!(fs::read(d.path().join("doc.xarast.bak")).unwrap(), b"one");
        write_atomic_with(&p, AtomicOptions { backup: true }, |w| {
            Ok(w.write_all(b"three")?)
        })
        .unwrap();
        assert_eq!(fs::read(d.path().join("doc.xarast.bak")).unwrap(), b"two");
        assert!(leftovers(d.path()).is_empty());
    }

    #[test]
    fn every_fault_leaves_the_original_intact() {
        let faults = [
            Some(Fault::CreateTemp),
            Some(Fault::Permissions),
            Some(Fault::Flush),
            Some(Fault::SyncFile),
            Some(Fault::BackupLink),
            Some(Fault::BackupRename),
            Some(Fault::Rename),
            None, // the writer itself failing, first byte
            None, // ... and half-way through
        ];
        for (i, fault) in faults.into_iter().enumerate() {
            let d = tempfile::tempdir().unwrap();
            let p = d.path().join("doc.xarast");
            fs::write(&p, b"original").unwrap();
            let r = write_atomic_inner(
                &p,
                AtomicOptions { backup: true },
                |w| {
                    if fault.is_none() {
                        if i == 8 {
                            w.write_all(&[0u8; 100_000])?;
                        }
                        return Err(WriteError::Zip("writer failed".into()));
                    }
                    Ok(w.write_all(b"replacement")?)
                },
                fault,
            );
            assert!(r.is_err(), "case {i}");
            assert_eq!(fs::read(&p).unwrap(), b"original", "case {i}");
            assert!(
                leftovers(d.path()).is_empty(),
                "case {i}: {:?}",
                leftovers(d.path())
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn keeps_permissions_and_follows_symlinks() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let real = d.path().join("real.xarast");
        fs::write(&real, b"x").unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o600)).unwrap();
        let link = d.path().join("link.xarast");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        write_atomic(&link, |w| Ok(w.write_all(b"y")?)).unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(&real).unwrap(), b"y");
        assert_eq!(
            fs::metadata(&real).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
