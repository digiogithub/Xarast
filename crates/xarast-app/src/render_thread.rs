//! The render-thread protocol (`phase-05 §U5.5`, thread topology `§U2.7`).
//!
//! The document lives on the main thread and never leaves it
//! (architecture §5). What crosses to the render thread is a
//! [`FrameJob`]: an immutable [`DisplayList`] behind an `Arc`, the
//! [`Resolver`] snapshot its paints refer to, and the [`ViewParams`] it was
//! built for. Never a node, never the arena.
//!
//! # One channel, latest wins
//!
//! There is exactly one channel type between the two threads, and it is
//! not a queue. A queue is the wrong shape for frames: a fast pan produces
//! requests faster than a large document rasterises, a queue turns that
//! into a backlog, and the window goes soft — every frame on screen is a
//! frame the user has already scrolled past. The phase document asks for
//! the opposite, and so this is a **mailbox with one slot in each
//! direction**:
//!
//! * a submitted frame *replaces* any frame still waiting, which is
//!   counted as [`RenderStats::superseded`];
//! * a finished frame *replaces* any finished frame the main thread has not
//!   collected yet, counted as [`RenderStats::dropped`].
//!
//! That is the backpressure: at most one frame waits and one is in flight,
//! whatever the input rate, and [`RenderThread::submit`] never blocks.
//!
//! # Generations
//!
//! Every frame carries a generation, allocated by [`RenderThread::submit`]
//! from a counter the main thread owns, strictly increasing. They make the
//! two cancellation rules expressible:
//!
//! * [`RenderRequest::Cancel`] `{ up_to_generation }` drops the waiting
//!   frame if it is that old, and makes the worker throw away an in-flight
//!   result that old instead of publishing it — the document it showed was
//!   closed, or replaced;
//! * a result is never published over a newer one, so the main thread
//!   cannot present frames out of order.
//!
//! A rasterisation already under way is not interrupted: the CPU backend
//! renders a frame as one call. Superseding it costs at most that one
//! frame, and its result is still better than nothing until the next one
//! lands.
//!
//! # Waking the main thread
//!
//! The worker calls a waker after publishing. The shell passes one that
//! posts to its event loop; the application core never learns what an
//! event loop is.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

use xarast_render::{
    BackendError, CpuBackend, CpuConfig, DisplayList, FrameTimings, Resolver, Surface, ViewParams,
};

use crate::session::DocumentId;

/// One frame for the render thread to rasterise.
///
/// Everything in it is immutable or shared by `Arc`: the main thread may
/// keep editing the document the moment it has been sent.
#[derive(Debug, Clone)]
pub struct FrameJob {
    /// Which document it shows.
    pub doc: DocumentId,
    /// The commands, already in device space.
    pub list: Arc<DisplayList>,
    /// What the list's ramp and image ids refer to. A display list is never
    /// sent without it (`app-core.md` invariant 6).
    pub resolver: Arc<Resolver>,
    /// The view the list was built for; its `viewport` is the target size.
    pub view: ViewParams,
    /// Premultiplied colour the target is cleared to before drawing: the
    /// pasteboard.
    pub background: [u8; 4],
    /// The page, in device pixels, and the premultiplied colour it is
    /// filled with before the document is drawn over it. The scene has no
    /// page of its own: the page is the viewer's backdrop, not ink.
    pub page: Option<(xarast_render::DeviceRect, [u8; 4])>,
    /// Assigned by [`RenderThread::submit`]; whatever is here on the way in
    /// is overwritten.
    pub generation: u64,
}

/// A request to the render thread.
#[derive(Debug, Clone)]
pub enum RenderRequest {
    /// Rasterise this frame, replacing any frame still waiting.
    Frame(FrameJob),
    /// Forget every frame up to and including this generation, waiting or
    /// in flight.
    Cancel {
        /// The newest generation to forget.
        up_to_generation: u64,
    },
    /// Finish the frame in flight, if any, and stop.
    Shutdown,
}

/// A finished frame.
#[derive(Debug, Clone)]
pub struct RenderedFrame {
    /// Which document it shows.
    pub doc: DocumentId,
    /// Which request it answers.
    pub generation: u64,
    /// The view it was drawn for.
    pub view: ViewParams,
    /// The pixels: premultiplied RGBA8 in non-linear sRGB, the size of
    /// `view.viewport`.
    pub surface: Surface,
    /// Where the time went.
    pub timings: FrameTimings,
    /// Set when the backend refused the frame; the surface then holds only
    /// the background.
    pub error: Option<BackendError>,
}

/// Counters, for the status bar and for the tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderStats {
    /// Frames submitted.
    pub submitted: u64,
    /// Frames replaced while waiting, never rasterised.
    pub superseded: u64,
    /// Frames dropped by a [`RenderRequest::Cancel`], waiting or in flight.
    pub cancelled: u64,
    /// Frames rasterised.
    pub rendered: u64,
    /// Finished frames replaced before the main thread collected them.
    pub dropped: u64,
}

/// What rasterises a [`FrameJob`]. The CPU backend in production; a
/// scripted one in the tests, which is how supersession is tested
/// deterministically.
pub trait FrameRenderer: Send + 'static {
    /// Draws `job` into `target`, which is already the right size and
    /// cleared to the background.
    ///
    /// # Errors
    ///
    /// Whatever the backend refuses.
    fn render(
        &mut self,
        job: &FrameJob,
        target: &mut Surface,
    ) -> Result<FrameTimings, BackendError>;
}

/// The production renderer: the CPU backend in its interactive
/// configuration.
#[derive(Debug)]
pub struct CpuFrameRenderer {
    backend: CpuBackend,
}

impl CpuFrameRenderer {
    /// A renderer over a backend built with `cfg`.
    #[must_use]
    pub fn new(cfg: CpuConfig) -> CpuFrameRenderer {
        CpuFrameRenderer {
            backend: CpuBackend::new(cfg),
        }
    }
}

impl Default for CpuFrameRenderer {
    fn default() -> Self {
        CpuFrameRenderer::new(CpuConfig::interactive())
    }
}

impl FrameRenderer for CpuFrameRenderer {
    fn render(
        &mut self,
        job: &FrameJob,
        target: &mut Surface,
    ) -> Result<FrameTimings, BackendError> {
        self.backend.render(&job.list, &job.resolver, target)
    }
}

/// Called by the worker after it publishes a frame.
pub type Waker = Box<dyn Fn() + Send + Sync + 'static>;

#[derive(Debug, Default)]
struct State {
    pending: Option<FrameJob>,
    result: Option<RenderedFrame>,
    cancel_up_to: Option<u64>,
    shutdown: bool,
    stats: RenderStats,
}

impl State {
    fn is_cancelled(&self, generation: u64) -> bool {
        self.cancel_up_to.is_some_and(|c| generation <= c)
    }
}

#[derive(Debug, Default)]
struct Shared {
    state: Mutex<State>,
    work: Condvar,
}

impl Shared {
    /// A poisoned lock means the worker panicked mid-update. The state is a
    /// handful of options and counters that are valid at every step, so
    /// carrying on is sound, and it keeps a render bug from taking the
    /// document down with it.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The main thread's handle on the render thread.
///
/// Dropping it shuts the thread down and joins it.
pub struct RenderThread {
    shared: Arc<Shared>,
    next_generation: u64,
    handle: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for RenderThread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderThread")
            .field("next_generation", &self.next_generation)
            .field("running", &self.handle.is_some())
            .finish_non_exhaustive()
    }
}

impl RenderThread {
    /// Starts a render thread over the CPU backend.
    ///
    /// # Errors
    ///
    /// When the operating system refuses a thread.
    pub fn spawn(waker: Waker) -> std::io::Result<RenderThread> {
        RenderThread::spawn_with(CpuFrameRenderer::default(), waker)
    }

    /// Starts a render thread over any renderer.
    ///
    /// # Errors
    ///
    /// When the operating system refuses a thread.
    pub fn spawn_with<R: FrameRenderer>(
        renderer: R,
        waker: Waker,
    ) -> std::io::Result<RenderThread> {
        let shared = Arc::new(Shared::default());
        let worker = Arc::clone(&shared);
        let handle = std::thread::Builder::new()
            .name("xarast-render".to_owned())
            .spawn(move || worker_loop(&worker, renderer, &waker))?;
        Ok(RenderThread {
            shared,
            next_generation: 1,
            handle: Some(handle),
        })
    }

    /// Submits a frame, returning the generation it was given. Never
    /// blocks; a frame still waiting is superseded.
    pub fn submit(&mut self, mut job: FrameJob) -> u64 {
        let generation = self.next_generation;
        self.next_generation += 1;
        job.generation = generation;
        self.send(RenderRequest::Frame(job));
        generation
    }

    /// The generation the next [`RenderThread::submit`] will allocate.
    #[must_use]
    pub const fn next_generation(&self) -> u64 {
        self.next_generation
    }

    /// Sends a raw request. [`RenderThread::submit`] is the usual way to
    /// send a frame, because it allocates the generation.
    pub fn send(&self, request: RenderRequest) {
        let mut st = self.shared.lock();
        match request {
            RenderRequest::Frame(job) => {
                st.stats.submitted += 1;
                if st.is_cancelled(job.generation) {
                    st.stats.cancelled += 1;
                    return;
                }
                if let Some(old) = st.pending.take() {
                    if old.generation > job.generation {
                        // An out-of-order raw send: keep the newer frame.
                        st.pending = Some(old);
                        st.stats.superseded += 1;
                        return;
                    }
                    st.stats.superseded += 1;
                }
                st.pending = Some(job);
            }
            RenderRequest::Cancel { up_to_generation } => {
                st.cancel_up_to = Some(
                    st.cancel_up_to
                        .map_or(up_to_generation, |c| c.max(up_to_generation)),
                );
                if st
                    .pending
                    .as_ref()
                    .is_some_and(|p| p.generation <= up_to_generation)
                {
                    st.pending = None;
                    st.stats.cancelled += 1;
                }
                if st
                    .result
                    .as_ref()
                    .is_some_and(|r| r.generation <= up_to_generation)
                {
                    st.result = None;
                    st.stats.cancelled += 1;
                }
            }
            RenderRequest::Shutdown => st.shutdown = true,
        }
        drop(st);
        self.shared.work.notify_one();
    }

    /// Takes the newest finished frame, if one arrived since the last call.
    pub fn take_latest(&self) -> Option<RenderedFrame> {
        self.shared.lock().result.take()
    }

    /// Whether a frame is waiting to be rasterised.
    #[must_use]
    pub fn has_pending(&self) -> bool {
        self.shared.lock().pending.is_some()
    }

    /// The counters so far.
    #[must_use]
    pub fn stats(&self) -> RenderStats {
        self.shared.lock().stats
    }

    /// Stops the thread and waits for it: the frame in flight finishes,
    /// the waiting one is dropped.
    pub fn shutdown(&mut self) {
        self.send(RenderRequest::Shutdown);
        if let Some(h) = self.handle.take()
            && h.join().is_err()
        {
            // The panic was already reported by the hook; the thread is
            // gone either way and there is nothing to hand back.
        }
    }
}

impl Drop for RenderThread {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Fills a device rectangle, clipped to the surface.
fn fill_rect(s: &mut Surface, r: xarast_render::DeviceRect, colour: [u8; 4]) {
    let r = r.intersection(s.bounds());
    if r.is_empty() {
        return;
    }
    let stride = s.width() as usize * 4;
    let (x0, x1) = (r.x0 as usize * 4, r.x1 as usize * 4);
    for row in s
        .data_mut()
        .chunks_mut(stride)
        .skip(r.y0 as usize)
        .take(r.height() as usize)
    {
        for px in row[x0..x1].as_chunks_mut::<4>().0 {
            *px = colour;
        }
    }
}

fn worker_loop<R: FrameRenderer>(shared: &Shared, mut renderer: R, waker: &Waker) {
    loop {
        let job = {
            let mut st = shared.lock();
            loop {
                if st.shutdown {
                    return;
                }
                if let Some(job) = st.pending.take() {
                    break job;
                }
                st = shared.work.wait(st).unwrap_or_else(PoisonError::into_inner);
            }
        };

        let (w, h) = (job.view.viewport.width(), job.view.viewport.height());
        let mut surface = Surface::filled(w.max(1), h.max(1), job.background);
        if let Some((rect, colour)) = job.page {
            fill_rect(&mut surface, rect, colour);
        }
        let (timings, error) = match renderer.render(&job, &mut surface) {
            Ok(t) => (t, None),
            Err(e) => (FrameTimings::default(), Some(e)),
        };

        let mut st = shared.lock();
        st.stats.rendered += 1;
        if st.is_cancelled(job.generation) {
            st.stats.cancelled += 1;
            continue;
        }
        if let Some(prev) = &st.result {
            if prev.generation > job.generation {
                st.stats.dropped += 1;
                continue;
            }
            st.stats.dropped += 1;
        }
        st.result = Some(RenderedFrame {
            doc: job.doc,
            generation: job.generation,
            view: job.view,
            surface,
            timings,
            error,
        });
        drop(st);
        waker();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;
    use xarast_render::{DeviceRect, DirtyRect, Scene};

    fn job(w: u32, h: u32) -> FrameJob {
        let view = ViewParams {
            viewport: DeviceRect::from_size(w, h),
            ..ViewParams::default()
        };
        FrameJob {
            doc: DocumentId(7),
            list: DisplayList::build(&Scene::new(), &view, &DirtyRect::of(view.viewport)),
            resolver: Arc::new(Resolver::new()),
            view,
            background: [10, 20, 30, 255],
            page: Some((DeviceRect::new(-5, 20, 4, 40), [200, 200, 200, 255])),
            generation: 0,
        }
    }

    /// A renderer that blocks until the test lets it go, and reports which
    /// generation it started — the only way to hold a frame "in flight"
    /// deterministically.
    struct Gated {
        started: mpsc::Sender<u64>,
        release: mpsc::Receiver<()>,
    }

    impl FrameRenderer for Gated {
        fn render(
            &mut self,
            job: &FrameJob,
            _t: &mut Surface,
        ) -> Result<FrameTimings, BackendError> {
            self.started.send(job.generation).unwrap();
            self.release.recv().unwrap();
            Ok(FrameTimings::default())
        }
    }

    fn gated() -> (
        RenderThread,
        mpsc::Receiver<u64>,
        mpsc::Sender<()>,
        Arc<AtomicUsize>,
    ) {
        let (started_tx, started) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let wakes = Arc::new(AtomicUsize::new(0));
        let w = Arc::clone(&wakes);
        let rt = RenderThread::spawn_with(
            Gated {
                started: started_tx,
                release: release_rx,
            },
            Box::new(move || {
                w.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .unwrap();
        (rt, started, release, wakes)
    }

    fn wait_for_result(rt: &RenderThread) -> RenderedFrame {
        for _ in 0..2000 {
            if let Some(f) = rt.take_latest() {
                return f;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("no frame arrived");
    }

    const T: Duration = Duration::from_secs(5);

    #[test]
    fn a_frame_is_rendered_at_its_size_and_wakes_the_main_thread() {
        let wakes = Arc::new(AtomicUsize::new(0));
        let w = Arc::clone(&wakes);
        let mut rt = RenderThread::spawn(Box::new(move || {
            w.fetch_add(1, Ordering::SeqCst);
        }))
        .unwrap();
        let g = rt.submit(job(64, 32));
        let f = wait_for_result(&rt);
        assert_eq!(f.generation, g);
        assert_eq!(f.doc, DocumentId(7));
        assert_eq!((f.surface.width(), f.surface.height()), (64, 32));
        assert_eq!(f.surface.pixel(3, 3), Some([10, 20, 30, 255]));
        // The page is filled, clipped to the surface.
        assert_eq!(f.surface.pixel(0, 25), Some([200, 200, 200, 255]));
        assert_eq!(f.surface.pixel(3, 31), Some([200, 200, 200, 255]));
        assert_eq!(f.surface.pixel(4, 25), Some([10, 20, 30, 255]));
        assert!(f.error.is_none());
        assert_eq!(wakes.load(Ordering::SeqCst), 1);
        assert_eq!(rt.stats().rendered, 1);
    }

    #[test]
    fn generations_increase_strictly() {
        let (mut rt, started, release, _) = gated();
        let a = rt.submit(job(4, 4));
        assert_eq!(started.recv_timeout(T).unwrap(), a);
        let b = rt.submit(job(4, 4));
        assert!(b > a);
        assert_eq!(rt.next_generation(), b + 1);
        release.send(()).unwrap();
        assert_eq!(started.recv_timeout(T).unwrap(), b);
        release.send(()).unwrap();
        rt.shutdown();
    }

    #[test]
    fn frames_submitted_during_a_render_supersede_each_other() {
        let (mut rt, started, release, _) = gated();
        let first = rt.submit(job(8, 8));
        assert_eq!(started.recv_timeout(T).unwrap(), first);

        // Five frames arrive while the first is in flight: only the last
        // survives. This is the fast pan that must not build a backlog.
        let mut last = 0;
        for _ in 0..5 {
            last = rt.submit(job(8, 8));
        }
        assert!(rt.has_pending());
        release.send(()).unwrap();
        assert_eq!(started.recv_timeout(T).unwrap(), last);
        release.send(()).unwrap();

        // The first frame may or may not have been collected in between;
        // the last one must arrive, and nothing after it.
        let mut f = wait_for_result(&rt);
        if f.generation != last {
            assert_eq!(f.generation, first);
            f = wait_for_result(&rt);
        }
        assert_eq!(f.generation, last);
        let s = rt.stats();
        assert_eq!(s.submitted, 6);
        assert_eq!(s.superseded, 4);
        assert_eq!(s.rendered, 2);
        assert!(started.recv_timeout(Duration::from_millis(50)).is_err());
    }

    #[test]
    fn cancel_drops_the_waiting_frame_and_the_one_in_flight() {
        let (mut rt, started, release, wakes) = gated();
        let first = rt.submit(job(8, 8));
        assert_eq!(started.recv_timeout(T).unwrap(), first);
        let second = rt.submit(job(8, 8));
        rt.send(RenderRequest::Cancel {
            up_to_generation: second,
        });
        assert!(!rt.has_pending());
        release.send(()).unwrap();

        // The in-flight frame finishes but is thrown away, not published.
        std::thread::sleep(Duration::from_millis(50));
        assert!(rt.take_latest().is_none());
        assert_eq!(wakes.load(Ordering::SeqCst), 0);
        let s = rt.stats();
        assert_eq!(s.cancelled, 2);

        // A cancelled generation stays cancelled; a newer one renders.
        let third = rt.submit(job(8, 8));
        assert_eq!(started.recv_timeout(T).unwrap(), third);
        release.send(()).unwrap();
        assert_eq!(wait_for_result(&rt).generation, third);
    }

    #[test]
    fn an_uncollected_result_is_replaced_by_a_newer_one() {
        let (mut rt, started, release, _) = gated();
        let a = rt.submit(job(4, 4));
        assert_eq!(started.recv_timeout(T).unwrap(), a);
        release.send(()).unwrap();
        // Wait until `a` is published, then render `b` without collecting.
        for _ in 0..2000 {
            if rt.stats().rendered == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let b = rt.submit(job(4, 4));
        assert_eq!(started.recv_timeout(T).unwrap(), b);
        release.send(()).unwrap();
        for _ in 0..2000 {
            if rt.stats().rendered == 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(rt.take_latest().unwrap().generation, b);
        assert!(rt.take_latest().is_none());
        assert_eq!(rt.stats().dropped, 1);
    }

    #[test]
    fn shutdown_joins_even_with_a_frame_waiting() {
        let (mut rt, started, release, _) = gated();
        let a = rt.submit(job(4, 4));
        assert_eq!(started.recv_timeout(T).unwrap(), a);
        rt.submit(job(4, 4));
        rt.send(RenderRequest::Shutdown);
        release.send(()).unwrap();
        rt.shutdown();
        // The waiting frame was never started.
        assert!(started.recv_timeout(Duration::from_millis(50)).is_err());
    }

    #[test]
    fn a_session_frame_carries_its_resolver_and_reuses_it_across_a_pan() {
        let mut s = crate::Session::new_empty(DocumentId(3));
        s.apply(crate::Intent::Resize(crate::DeviceSize::new(320, 200)))
            .unwrap();
        s.rebuild_scene(None).unwrap();
        let a = s.frame_job([128, 128, 128, 255], [255, 255, 255, 255]);
        s.apply(crate::Intent::Pan { dx: 5.0, dy: 0.0 }).unwrap();
        let b = s.frame_job([128, 128, 128, 255], [255, 255, 255, 255]);
        assert!(Arc::ptr_eq(&a.resolver, &b.resolver), "a pan re-uses it");
        assert_ne!(a.view.transform, b.view.transform);
        s.rebuild_scene(None).unwrap();
        let c = s.frame_job([128, 128, 128, 255], [255, 255, 255, 255]);
        assert!(
            !Arc::ptr_eq(&b.resolver, &c.resolver),
            "a rebuild renews it"
        );
        assert_eq!(c.view.viewport, DeviceRect::from_size(320, 200));

        let mut rt = RenderThread::spawn(Box::new(|| {})).unwrap();
        let g = rt.submit(c);
        let f = wait_for_result(&rt);
        assert_eq!((f.generation, f.doc), (g, DocumentId(3)));
        assert_eq!((f.surface.width(), f.surface.height()), (320, 200));
    }

    #[test]
    fn the_handle_and_the_jobs_cross_threads() {
        const fn assert_send<T: Send>() {}
        assert_send::<RenderThread>();
        assert_send::<FrameJob>();
        assert_send::<RenderedFrame>();
    }
}
