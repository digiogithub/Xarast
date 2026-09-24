//! Importing an image file in the background, with progress and cancel
//! (phase 10, T10.7.5).
//!
//! A small file is decoded where it is asked for (`place.rs`): it takes
//! less than a frame. A large one — a 24 Mpx photograph is 7 MB and some
//! 400 ms of decoding — goes to [`ImportWorker`], which reads it in chunks
//! on its own thread, reporting how much it has read, then decodes it under
//! the same default limits. The application polls the worker, as it polls
//! the save worker, and places what has arrived.
//!
//! Cancelling is immediate for the application: the job leaves the list at
//! once and whatever it later produces is thrown away. The thread itself
//! stops at the next chunk while reading; a decode already under way runs
//! to its end, bounded by `DecodeLimits::max_duration`, and is discarded.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::place::{ImageToPlace, PlaceError};
use crate::save::Waker;

/// Files up to this size are decoded inline; larger ones in the
/// background, with progress. A megabyte of PNG or JPEG decodes in well
/// under a frame's budget on the development machine.
pub const INLINE_IMPORT_BYTES: u64 = 1 << 20;

/// How much is read between two progress reports and cancel checks.
const CHUNK: usize = 256 << 10;

/// One import in flight, as the status bar shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportProgress {
    /// Which job.
    pub id: u64,
    /// The file's name.
    pub name: String,
    /// Bytes read so far.
    pub read: u64,
    /// The file's size.
    pub total: u64,
    /// Everything is read and the decoder is running.
    pub decoding: bool,
}

impl ImportProgress {
    /// The fraction done, `0..=1`: reading counts for half, decoding for
    /// the rest (a decode cannot report its own progress).
    #[must_use]
    pub fn fraction(&self) -> f32 {
        if self.decoding {
            return 0.5;
        }
        if self.total == 0 {
            return 0.0;
        }
        #[allow(clippy::cast_precision_loss)]
        let f = self.read as f32 / self.total as f32;
        (f * 0.5).clamp(0.0, 0.5)
    }

    /// What the status bar says.
    #[must_use]
    pub fn label(&self) -> String {
        if self.decoding {
            format!("Importing {}: decoding\u{2026}", self.name)
        } else {
            format!(
                "Importing {}: read {} of {} KB\u{2026}",
                self.name,
                self.read / 1024,
                self.total / 1024
            )
        }
    }
}

/// What a finished job hands back.
#[derive(Debug)]
pub struct Finished {
    /// Which job.
    pub id: u64,
    /// The image, or why not.
    pub result: Result<ImageToPlace, PlaceError>,
}

#[derive(Debug, Default)]
struct Shared {
    read: AtomicU64,
    decoding: AtomicBool,
    cancelled: AtomicBool,
}

#[derive(Debug)]
struct Job {
    id: u64,
    name: String,
    total: u64,
    shared: Arc<Shared>,
}

/// Runs imports on their own threads.
pub struct ImportWorker {
    tx: Sender<Finished>,
    rx: Receiver<Finished>,
    jobs: Vec<Job>,
    running: Vec<JoinHandle<()>>,
    next: u64,
    waker: Arc<Mutex<Option<Waker>>>,
}

impl std::fmt::Debug for ImportWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImportWorker")
            .field("jobs", &self.jobs)
            .finish_non_exhaustive()
    }
}

impl Default for ImportWorker {
    fn default() -> ImportWorker {
        ImportWorker::new()
    }
}

/// Reads a file in chunks, reporting progress and giving up when
/// cancelled, then decodes it.
fn run(path: &Path, shared: &Shared) -> Result<ImageToPlace, PlaceError> {
    let read_err = |e: std::io::Error| PlaceError::Read(format!("{}: {e}", path.display()));
    let mut file = std::fs::File::open(path).map_err(read_err)?;
    let mut bytes = Vec::new();
    let mut chunk = vec![0u8; CHUNK];
    loop {
        if shared.cancelled.load(Ordering::Relaxed) {
            return Err(PlaceError::Cancelled);
        }
        let n = file.read(&mut chunk).map_err(read_err)?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..n]);
        shared.read.store(bytes.len() as u64, Ordering::Relaxed);
    }
    shared.decoding.store(true, Ordering::Relaxed);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    crate::place::image_from_bytes(Arc::from(bytes), &name)
}

impl ImportWorker {
    /// A worker with nothing running.
    #[must_use]
    pub fn new() -> ImportWorker {
        let (tx, rx) = channel();
        ImportWorker {
            tx,
            rx,
            jobs: Vec::new(),
            running: Vec::new(),
            next: 1,
            waker: Arc::new(Mutex::new(None)),
        }
    }

    /// Calls `wake` whenever a job finishes.
    pub fn set_waker(&mut self, wake: Waker) {
        if let Ok(mut w) = self.waker.lock() {
            *w = Some(wake);
        }
    }

    /// Starts importing `path`, `total` bytes long. Returns the job's id.
    pub fn start(&mut self, path: PathBuf, total: u64) -> u64 {
        let id = self.next;
        self.next += 1;
        let shared = Arc::new(Shared::default());
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.jobs.push(Job {
            id,
            name,
            total,
            shared: Arc::clone(&shared),
        });
        let tx = self.tx.clone();
        let waker = Arc::clone(&self.waker);
        let theirs = Arc::clone(&shared);
        let spawned = std::thread::Builder::new()
            .name("xarast-import".into())
            .spawn(move || {
                let result = run(&path, &theirs);
                let _ = tx.send(Finished { id, result });
                if let Ok(w) = waker.lock()
                    && let Some(w) = w.as_ref()
                {
                    w();
                }
            });
        match spawned {
            Ok(h) => self.running.push(h),
            Err(e) => {
                let _ = self.tx.send(Finished {
                    id,
                    result: Err(PlaceError::Read(format!("no import thread: {e}"))),
                });
            }
        }
        id
    }

    /// Cancels one job: it leaves the list now, and its result, whenever
    /// it comes, is dropped. Returns whether it was running.
    pub fn cancel(&mut self, id: u64) -> bool {
        let Some(i) = self.jobs.iter().position(|j| j.id == id) else {
            return false;
        };
        let job = self.jobs.remove(i);
        job.shared.cancelled.store(true, Ordering::Relaxed);
        true
    }

    /// Cancels every job; returns how many there were.
    pub fn cancel_all(&mut self) -> usize {
        let n = self.jobs.len();
        for job in self.jobs.drain(..) {
            job.shared.cancelled.store(true, Ordering::Relaxed);
        }
        n
    }

    /// The jobs still running, oldest first.
    #[must_use]
    pub fn progress(&self) -> Vec<ImportProgress> {
        self.jobs
            .iter()
            .map(|j| ImportProgress {
                id: j.id,
                name: j.name.clone(),
                read: j.shared.read.load(Ordering::Relaxed).min(j.total),
                total: j.total,
                decoding: j.shared.decoding.load(Ordering::Relaxed),
            })
            .collect()
    }

    /// Whether any job is still wanted.
    #[must_use]
    pub fn busy(&self) -> bool {
        !self.jobs.is_empty()
    }

    /// The jobs that have finished and are still wanted, oldest first.
    /// Never blocks.
    pub fn poll(&mut self) -> Vec<Finished> {
        self.running.retain(|h| !h.is_finished());
        let mut out: Vec<Finished> = Vec::new();
        for f in self.rx.try_iter() {
            if let Some(i) = self.jobs.iter().position(|j| j.id == f.id) {
                self.jobs.remove(i);
                out.push(f);
            }
        }
        out.sort_by_key(|f| f.id);
        out
    }

    /// Blocks until every thread has ended. For tests and shutdown.
    pub fn wait(&mut self) {
        for h in self.running.drain(..) {
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let rgba = [10u8, 20, 30, 255].repeat((w * h) as usize);
        let header = xarast_io::png::PngHeader {
            width: w,
            height: h,
            colour: xarast_io::PngColour::Rgba,
            depth: xarast_io::PngDepth::Eight,
            interlace: false,
            ppm: None,
            level: 0,
        };
        let mut out = Vec::new();
        xarast_io::png::encode_png(&mut out, header, &rgba).unwrap();
        out
    }

    fn temp(name: &str, bytes: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("xarast-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn a_background_import_reports_and_finishes() {
        let bytes = png(700, 500);
        let path = temp("bg.png", &bytes);
        let mut w = ImportWorker::new();
        let id = w.start(path, bytes.len() as u64);
        let p = w.progress();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].name, "bg.png");
        assert!(p[0].fraction() <= 0.5);
        assert!(p[0].label().starts_with("Importing bg.png"));
        w.wait();
        let done = w.poll();
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].id, id);
        assert_eq!(done[0].result.as_ref().unwrap().pixels, (700, 500));
        assert!(!w.busy());
    }

    #[test]
    fn a_cancelled_import_leaves_nothing_behind() {
        let bytes = png(64, 64);
        let path = temp("c.png", &bytes);
        let mut w = ImportWorker::new();
        let id = w.start(path.clone(), bytes.len() as u64);
        assert!(w.cancel(id));
        assert!(!w.cancel(id));
        assert!(w.progress().is_empty());
        w.wait();
        assert!(w.poll().is_empty(), "a cancelled result is dropped");
        let a = w.start(path.clone(), 1);
        let b = w.start(path, 1);
        assert_ne!(a, b);
        assert_eq!(w.cancel_all(), 2);
        w.wait();
        assert!(w.poll().is_empty());
    }

    #[test]
    fn a_missing_file_is_a_read_error() {
        let mut w = ImportWorker::new();
        w.start(PathBuf::from("/nonexistent/x.png"), 10);
        w.wait();
        let done = w.poll();
        assert!(matches!(done[0].result, Err(PlaceError::Read(_))));
    }
}
