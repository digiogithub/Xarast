//! The recently opened files, and where they are kept between runs.
//!
//! The list lives in `$XDG_STATE_HOME/xarast/recent` (falling back to
//! `~/.local/state/xarast/recent`): it is state the application
//! accumulates, not a setting anyone edits, which is exactly what the XDG
//! base directory specification puts under the state directory.
//!
//! The file is plain text, one absolute path per line, under a one-line
//! header. It is read tolerantly — a line that is not a usable path is
//! skipped, a header this build does not know yields an empty list, and a
//! file that cannot be read at all is treated as absent — because a
//! half-written or hand-edited list must never stop the application from
//! starting. It is written to a temporary file that is then renamed over the
//! old one, so a crash mid-write leaves the previous list intact.

use std::io::Write as _;
use std::path::{Path, PathBuf};

/// The most files the list keeps.
pub const MAX_RECENT: usize = 10;

/// The first line of the file, naming the format and its version.
const HEADER: &str = "xarast-recent 1";

/// The recently opened files, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecentFiles {
    paths: Vec<PathBuf>,
}

impl RecentFiles {
    /// An empty list.
    #[must_use]
    pub fn new() -> RecentFiles {
        RecentFiles::default()
    }

    /// The files, newest first.
    #[must_use]
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// Whether the list is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// Puts `path` at the front, removing an earlier mention of it and
    /// dropping the oldest entry past [`MAX_RECENT`]. A relative path is
    /// made absolute against the current directory first, so that the
    /// list still means something to the next run; a path that cannot be
    /// stored (see [`RecentFiles::storable`]) is ignored.
    pub fn remember(&mut self, path: &Path) {
        let path = absolute(path);
        if !Self::storable(&path) {
            return;
        }
        self.paths.retain(|p| *p != path);
        self.paths.insert(0, path);
        self.paths.truncate(MAX_RECENT);
    }

    /// Forgets one path. Returns whether it was listed.
    pub fn forget(&mut self, path: &Path) -> bool {
        let before = self.paths.len();
        self.paths.retain(|p| p != path);
        self.paths.len() != before
    }

    /// Forgets everything.
    pub fn clear(&mut self) {
        self.paths.clear();
    }

    /// Drops every entry that is no longer a file. Returns how many went.
    pub fn prune_missing(&mut self) -> usize {
        let before = self.paths.len();
        self.paths.retain(|p| p.is_file());
        before - self.paths.len()
    }

    /// Whether a path can round-trip through the file: absolute, and with
    /// no line break in it (the format is one path per line).
    #[must_use]
    pub fn storable(path: &Path) -> bool {
        path.is_absolute() && !path_bytes(path).iter().any(|b| *b == b'\n' || *b == b'\r')
    }

    /// Parses the file's contents. Never fails: see the module comment.
    #[must_use]
    pub fn parse(bytes: &[u8]) -> RecentFiles {
        let mut lines = bytes.split(|b| *b == b'\n');
        let header = lines.next().unwrap_or_default();
        if trim_cr(header) != HEADER.as_bytes() {
            return RecentFiles::new();
        }
        let mut list = RecentFiles::new();
        for line in lines {
            let line = trim_cr(line);
            if line.is_empty() || line.contains(&0) {
                continue;
            }
            let Some(path) = path_from_bytes(line) else {
                continue;
            };
            if Self::storable(&path) && !list.paths.contains(&path) {
                list.paths.push(path);
            }
            if list.paths.len() == MAX_RECENT {
                break;
            }
        }
        list
    }

    /// The file's contents.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(64 * (self.paths.len() + 1));
        out.extend_from_slice(HEADER.as_bytes());
        out.push(b'\n');
        for p in &self.paths {
            out.extend_from_slice(&path_bytes(p));
            out.push(b'\n');
        }
        out
    }

    /// Reads the list from `file`. A missing, unreadable or corrupt file
    /// is an empty list, never an error.
    #[must_use]
    pub fn load(file: &Path) -> RecentFiles {
        match std::fs::read(file) {
            Ok(bytes) => RecentFiles::parse(&bytes),
            Err(_) => RecentFiles::new(),
        }
    }

    /// Writes the list to `file`, creating its directory, through a
    /// temporary file renamed into place.
    ///
    /// # Errors
    ///
    /// Any I/O error; the caller reports it and carries on, since losing
    /// the recent list is not worth interrupting anyone for.
    pub fn save(&self, file: &Path) -> std::io::Result<()> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut tmp = file.as_os_str().to_owned();
        tmp.push(format!(".tmp{}", std::process::id()));
        let tmp = PathBuf::from(tmp);
        let written = std::fs::File::create(&tmp).and_then(|mut f| {
            f.write_all(&self.to_bytes())?;
            f.sync_all()
        });
        match written.and_then(|()| std::fs::rename(&tmp, file)) {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                Err(e)
            }
        }
    }
}

/// Where the list is kept: `$XDG_STATE_HOME/xarast/recent`, or
/// `$HOME/.local/state/xarast/recent` when `XDG_STATE_HOME` is unset, empty
/// or relative (the specification says a relative value is to be ignored).
/// `None` when neither variable gives an absolute directory.
#[must_use]
pub fn default_store_path() -> Option<PathBuf> {
    store_path_from(
        std::env::var_os("XDG_STATE_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

/// [`default_store_path`] with the environment passed in, for the tests.
#[must_use]
pub fn store_path_from(state_home: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    let base = match state_home.filter(|p| p.is_absolute()) {
        Some(dir) => dir,
        None => home.filter(|p| p.is_absolute())?.join(".local/state"),
    };
    Some(base.join("xarast").join("recent"))
}

fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

fn trim_cr(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt as _;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(unix)]
fn path_from_bytes(bytes: &[u8]) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt as _;
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
}

#[cfg(not(unix))]
fn path_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().into_owned().into_bytes()
}

#[cfg(not(unix))]
fn path_from_bytes(bytes: &[u8]) -> Option<PathBuf> {
    std::str::from_utf8(bytes).ok().map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("xarast-recent-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn remember_puts_newest_first_dedupes_and_caps() {
        let mut r = RecentFiles::new();
        for i in 0..(MAX_RECENT + 3) {
            r.remember(Path::new(&format!("/d/{i}.xar")));
        }
        assert_eq!(r.paths().len(), MAX_RECENT);
        assert_eq!(
            r.paths()[0],
            Path::new(&format!("/d/{}.xar", MAX_RECENT + 2))
        );
        r.remember(Path::new("/d/5.xar"));
        assert_eq!(r.paths()[0], Path::new("/d/5.xar"));
        assert_eq!(r.paths().len(), MAX_RECENT);
        assert_eq!(
            r.paths()
                .iter()
                .filter(|p| *p == Path::new("/d/5.xar"))
                .count(),
            1
        );
    }

    #[test]
    fn a_relative_path_is_stored_absolute_and_a_line_break_is_refused() {
        let mut r = RecentFiles::new();
        r.remember(Path::new("relative.xar"));
        assert!(r.paths()[0].is_absolute());
        r.remember(Path::new("/a\nb.xar"));
        assert_eq!(r.paths().len(), 1);
    }

    #[test]
    fn the_file_round_trips() {
        let dir = scratch("round-trip");
        let file = dir.join("state/xarast/recent");
        let mut r = RecentFiles::new();
        r.remember(Path::new("/one.xar"));
        r.remember(Path::new("/with space/two.xar"));
        r.remember(Path::new("/ünïcode/three.xar"));
        r.save(&file).unwrap();
        assert_eq!(RecentFiles::load(&file), r);
        // No temporary file is left behind.
        let left: Vec<_> = std::fs::read_dir(file.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left.len(), 1, "{left:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_input_never_panics_and_keeps_what_is_readable() {
        assert!(RecentFiles::parse(b"").is_empty());
        assert!(RecentFiles::parse(b"\xff\xfe\x00garbage").is_empty());
        assert!(RecentFiles::parse(b"xarast-recent 99\n/a.xar\n").is_empty());
        let r = RecentFiles::parse(
            b"xarast-recent 1\r\n/a.xar\r\n\nrelative\n/b\0.xar\n/a.xar\n/c.xar",
        );
        assert_eq!(
            r.paths(),
            [PathBuf::from("/a.xar"), PathBuf::from("/c.xar")]
        );
        let mut many = b"xarast-recent 1\n".to_vec();
        for i in 0..100 {
            many.extend_from_slice(format!("/f{i}.xar\n").as_bytes());
        }
        assert_eq!(RecentFiles::parse(&many).paths().len(), MAX_RECENT);
        // Every prefix of a valid file parses.
        let bytes = r.to_bytes();
        for n in 0..bytes.len() {
            let _ = RecentFiles::parse(&bytes[..n]);
        }
    }

    #[test]
    fn a_missing_or_unreadable_store_is_empty_and_an_unwritable_one_is_an_error() {
        let dir = scratch("unreadable");
        assert!(RecentFiles::load(&dir.join("absent")).is_empty());
        // A directory where the file should be.
        assert!(RecentFiles::load(&dir).is_empty());
        let blocker = dir.join("file");
        std::fs::write(&blocker, b"x").unwrap();
        let mut r = RecentFiles::new();
        r.remember(Path::new("/a.xar"));
        assert!(r.save(&blocker.join("recent")).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_files_are_pruned() {
        let dir = scratch("prune");
        let here = dir.join("here.xar");
        std::fs::write(&here, b"x").unwrap();
        let mut r = RecentFiles::new();
        r.remember(&dir.join("gone.xar"));
        r.remember(&here);
        r.remember(&dir);
        assert_eq!(r.prune_missing(), 2);
        assert_eq!(r.paths(), [here]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_store_lives_under_the_state_directory() {
        assert_eq!(
            store_path_from(Some("/s".into()), Some("/h".into())),
            Some(PathBuf::from("/s/xarast/recent"))
        );
        assert_eq!(
            store_path_from(Some("rel".into()), Some("/h".into())),
            Some(PathBuf::from("/h/.local/state/xarast/recent"))
        );
        assert_eq!(
            store_path_from(None, Some("/h".into())),
            Some(PathBuf::from("/h/.local/state/xarast/recent"))
        );
        assert_eq!(store_path_from(None, None), None);
    }
}
