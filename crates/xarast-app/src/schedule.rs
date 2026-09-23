//! Draft → Final scheduling (`phase-05 §U5.6`, render `R6.8`).
//!
//! While the user interacts, the canvas is drawn at
//! [`RenderQuality::Draft`], and the render thread is allowed to reuse
//! pixels aggressively (a pan scrolls the last frame, a zoom resamples
//! it). Once the input has been quiet for [`FINAL_AFTER`] (120 ms), one
//! [`RenderQuality::Final`] frame replaces the draft. New input before
//! that Final lands cancels it, waiting or in flight, and the cycle
//! starts again.
//!
//! Two layers:
//!
//! * [`QualityScheduler`] is the policy, a pure state machine. Time is
//!   passed in as an [`Instant`], never read, so the tests drive it with a
//!   synthetic clock and no sleeps.
//! * [`Canvas`] joins it to a [`RenderThread`] and a [`Session`]: the
//!   composition root calls [`Canvas::note`] with what each batch of
//!   intents changed, [`Canvas::pump`] once per frame, and
//!   [`Canvas::take_latest`] to collect pixels. `pump` returns the instant
//!   the owed Final is due, which the shell turns into a timed wake-up.
//!
//! # What "Draft" costs here
//!
//! The quality of a frame is a *view* parameter: the scene is walked at
//! the session's quality (normally `Final`) and a Draft frame only
//! overrides [`ViewParams::quality`](xarast_render::ViewParams), which
//! scales flatness. Re-walking the scene at Draft quality would also
//! shorten ramps and switch images to nearest sampling, but it costs a
//! walk of the whole document on the first frame of every gesture and
//! again when it ends, which is exactly the frame that must be cheap.
//! Moving the ramp length and the image filter into the view is a
//! render-crate change (see `docs/memory/app-core.md`).
//!
//! # The first frame of a document is Final
//!
//! A document just opened has nothing on screen to refine, and a Draft
//! followed 120 ms later by a Final is two full rasterisations where one
//! would do.

use std::time::{Duration, Instant};

use xarast_render::RenderQuality;

use crate::intent::Changed;
use crate::render_thread::{RenderRequest, RenderThread, RenderedFrame, Waker};
use crate::session::{DocumentId, Session, SessionError};

/// How long input must be quiet before the Final frame is drawn.
pub const FINAL_AFTER: Duration = Duration::from_millis(120);

/// What the scheduler wants done now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Submit a frame at this quality.
    Submit(RenderQuality),
    /// Nothing now; a Final is owed at [`QualityScheduler::deadline`].
    Wait,
    /// Nothing is owed.
    Idle,
}

/// The Draft → Final policy, with the clock injected.
#[derive(Debug, Clone)]
pub struct QualityScheduler {
    idle: Duration,
    last_input: Option<Instant>,
    /// The view changed since the last submission.
    stale: bool,
    /// A Draft is (or is about to be) on screen and a Final is owed.
    owed_final: bool,
    /// The generation of a Final submitted and not yet presented.
    final_in_flight: Option<u64>,
    /// The document the last frame showed.
    doc: Option<DocumentId>,
}

impl Default for QualityScheduler {
    fn default() -> Self {
        QualityScheduler::new(FINAL_AFTER)
    }
}

impl QualityScheduler {
    /// A scheduler that upgrades after `idle` of quiet.
    #[must_use]
    pub const fn new(idle: Duration) -> QualityScheduler {
        QualityScheduler {
            idle,
            last_input: None,
            stale: false,
            owed_final: false,
            final_in_flight: None,
            doc: None,
        }
    }

    /// Records what a batch of intents changed, at `now`. Anything that
    /// needs a redraw counts as interaction: the next frame is a Draft and
    /// the idle timer restarts.
    ///
    /// Returns the generation of a Final to cancel, when one was in flight
    /// — the caller sends [`RenderRequest::Cancel`] for it.
    pub fn note(&mut self, now: Instant, changed: Changed) -> Option<u64> {
        if !changed.needs_redraw() {
            return None;
        }
        self.stale = true;
        self.last_input = Some(now);
        self.final_in_flight.take()
    }

    /// Marks the view stale without counting it as interaction: the next
    /// frame is drawn at whatever quality is due.
    pub fn invalidate(&mut self) {
        self.stale = true;
    }

    fn interacting(&self, now: Instant) -> bool {
        self.last_input
            .is_some_and(|t| now.saturating_duration_since(t) < self.idle)
    }

    /// What to do at `now` for the document `doc`.
    pub fn next(&mut self, now: Instant, doc: DocumentId) -> Step {
        let first = self.doc != Some(doc);
        if first {
            self.doc = Some(doc);
            self.stale = true;
        }
        let interacting = !first && self.interacting(now);
        if self.stale {
            self.stale = false;
            self.owed_final = interacting;
            return Step::Submit(if interacting {
                RenderQuality::Draft
            } else {
                RenderQuality::Final
            });
        }
        if self.owed_final {
            if interacting {
                return Step::Wait;
            }
            self.owed_final = false;
            return Step::Submit(RenderQuality::Final);
        }
        Step::Idle
    }

    /// Records that a frame at `quality` went out as `generation`.
    pub fn submitted(&mut self, quality: RenderQuality, generation: u64) {
        if quality == RenderQuality::Final {
            self.final_in_flight = Some(generation);
        }
    }

    /// Records that the frame `generation` reached the screen.
    pub fn presented(&mut self, generation: u64) {
        if self.final_in_flight.is_some_and(|g| generation >= g) {
            self.final_in_flight = None;
        }
    }

    /// When the owed Final is due, if one is owed. The caller wakes up
    /// then and calls [`QualityScheduler::next`] again.
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        if !self.owed_final {
            return None;
        }
        self.last_input.map(|t| t + self.idle)
    }

    /// Nothing is owed and no Final is in flight: what is on screen, or
    /// about to be, is final.
    #[must_use]
    pub const fn is_settled(&self) -> bool {
        !self.stale && !self.owed_final && self.final_in_flight.is_none()
    }
}

/// The colours the render thread paints under the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backdrop {
    /// Premultiplied sRGB, everywhere off the page.
    pub pasteboard: [u8; 4],
    /// Premultiplied sRGB, under the drawing on the page.
    pub page: [u8; 4],
}

/// The canvas's render path: a [`RenderThread`] driven by a
/// [`QualityScheduler`].
///
/// ```text
///   intents ─► Session::apply ─► Changed ─► Canvas::note  (cancels an owed Final)
///   each frame:                              Canvas::pump  ─► RenderThread::submit
///                                              └─► Some(deadline): wake the loop then
///   waker ─► Canvas::take_latest ─► show the pixels
/// ```
#[derive(Debug)]
pub struct Canvas {
    rt: RenderThread,
    sched: QualityScheduler,
    backdrop: Backdrop,
    cpu_rescale: bool,
}

impl Canvas {
    /// A canvas over an existing render thread.
    #[must_use]
    pub fn new(rt: RenderThread, backdrop: Backdrop) -> Canvas {
        Canvas {
            rt,
            sched: QualityScheduler::default(),
            backdrop,
            cpu_rescale: true,
        }
    }

    /// A canvas over a new CPU render thread.
    ///
    /// # Errors
    ///
    /// When the operating system refuses a thread.
    pub fn spawn(waker: Waker, backdrop: Backdrop) -> std::io::Result<Canvas> {
        Ok(Canvas::new(RenderThread::spawn(waker)?, backdrop))
    }

    /// Replaces the scheduler, to change the idle delay.
    #[must_use]
    pub fn with_scheduler(mut self, sched: QualityScheduler) -> Canvas {
        self.sched = sched;
        self
    }

    /// Whether a `Draft` zoom is resampled on the render thread
    /// ([`crate::render_thread::FrameJob::cpu_rescale`]). A presenter that
    /// composites retained tiles at input time turns it off: the zoom is
    /// already on screen, and the render thread's resample would be
    /// thrown away.
    pub fn set_cpu_rescale(&mut self, on: bool) {
        self.cpu_rescale = on;
    }

    /// Records what a batch of intents changed, at `now`. Cancels a Final
    /// that is waiting or in flight when the change needs a redraw.
    pub fn note(&mut self, now: Instant, changed: Changed) {
        if let Some(g) = self.sched.note(now, changed) {
            self.rt.send(RenderRequest::Cancel {
                up_to_generation: g,
            });
        }
    }

    /// Forces a redraw at the quality that is due, without counting it as
    /// interaction.
    pub fn invalidate(&mut self) {
        self.sched.invalidate();
    }

    /// Does what is owed at `now`: rebuilds the scene if the session says
    /// it is stale, and submits a Draft or a Final. Returns the instant at
    /// which the caller should call again, when a Final is owed.
    ///
    /// # Errors
    ///
    /// [`SessionError::Scene`] from the rebuild; nothing is submitted then.
    pub fn pump(
        &mut self,
        now: Instant,
        session: &mut Session,
    ) -> Result<Option<Instant>, SessionError> {
        if session.needs_scene() {
            session.rebuild_scene(None)?;
        }
        if session.viewport.size().is_empty() {
            return Ok(self.sched.deadline());
        }
        if let Step::Submit(q) = self.sched.next(now, session.id) {
            let mut job = session.frame_job(self.backdrop.pasteboard, self.backdrop.page);
            // Never above what the session asks for: a user who chose
            // Draft gets Draft at rest too.
            job.view.quality = q.min(session.quality);
            job.cpu_rescale = self.cpu_rescale;
            let g = self.rt.submit(job);
            self.sched.submitted(q, g);
        }
        Ok(self.sched.deadline())
    }

    /// The newest finished frame, if one arrived since the last call.
    pub fn take_latest(&mut self) -> Option<RenderedFrame> {
        let f = self.rt.take_latest()?;
        self.sched.presented(f.generation);
        Some(f)
    }

    /// Nothing is owed, waiting or in flight: the next frame collected is
    /// the final one, or it is already on screen.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        self.sched.is_settled() && !self.rt.has_pending()
    }

    /// The render thread, for its counters.
    #[must_use]
    pub const fn render_thread(&self) -> &RenderThread {
        &self.rt
    }

    /// The scheduler, for its state.
    #[must_use]
    pub const fn scheduler(&self) -> &QualityScheduler {
        &self.sched
    }

    /// Stops the render thread and waits for it.
    pub fn shutdown(&mut self) {
        self.rt.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use RenderQuality::{Draft, Final};

    const DOC: DocumentId = DocumentId(1);

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn the_first_frame_of_a_document_is_final() {
        let t0 = Instant::now();
        let mut s = QualityScheduler::default();
        s.note(t0, Changed::VIEW);
        assert_eq!(s.next(t0, DOC), Step::Submit(Final));
        s.submitted(Final, 1);
        assert_eq!(s.next(t0, DOC), Step::Idle);
        assert_eq!(s.deadline(), None);
        assert!(!s.is_settled(), "the Final is still in flight");
        s.presented(1);
        assert!(s.is_settled());
    }

    #[test]
    fn interaction_draws_drafts_then_one_final_after_the_idle_delay() {
        let t0 = Instant::now();
        let mut s = QualityScheduler::default();
        assert_eq!(s.next(t0, DOC), Step::Submit(Final));
        s.submitted(Final, 1);
        s.presented(1);

        // A pan: three input batches 16 ms apart, each a Draft.
        let mut g = 1;
        for i in 0..3 {
            let now = t0 + ms(100 + 16 * i);
            assert_eq!(s.note(now, Changed::VIEW), None);
            assert_eq!(s.next(now, DOC), Step::Submit(Draft));
            g += 1;
            s.submitted(Draft, g);
            assert_eq!(s.deadline(), Some(now + FINAL_AFTER));
        }
        let last = t0 + ms(132);
        // Before the deadline: nothing, however often the loop wakes.
        assert_eq!(s.next(last + ms(60), DOC), Step::Wait);
        assert_eq!(s.next(last + ms(119), DOC), Step::Wait);
        // At it: exactly one Final.
        assert_eq!(s.next(last + ms(120), DOC), Step::Submit(Final));
        s.submitted(Final, g + 1);
        assert_eq!(s.next(last + ms(500), DOC), Step::Idle);
        assert_eq!(s.deadline(), None);
        s.presented(g + 1);
        assert!(s.is_settled());
    }

    #[test]
    fn new_input_cancels_the_final_in_flight_and_restarts_the_timer() {
        let t0 = Instant::now();
        let mut s = QualityScheduler::default();
        assert_eq!(s.next(t0, DOC), Step::Submit(Final));
        s.submitted(Final, 1);
        s.presented(1);
        s.note(t0 + ms(10), Changed::VIEW);
        assert_eq!(s.next(t0 + ms(10), DOC), Step::Submit(Draft));
        s.submitted(Draft, 2);
        assert_eq!(s.next(t0 + ms(130), DOC), Step::Submit(Final));
        s.submitted(Final, 3);

        // Input arrives while generation 3 renders.
        assert_eq!(s.note(t0 + ms(140), Changed::VIEW), Some(3));
        assert_eq!(s.next(t0 + ms(140), DOC), Step::Submit(Draft));
        s.submitted(Draft, 4);
        // The Final is owed again, from the new input.
        assert_eq!(s.deadline(), Some(t0 + ms(260)));
        assert_eq!(s.next(t0 + ms(259), DOC), Step::Wait);
        assert_eq!(s.next(t0 + ms(260), DOC), Step::Submit(Final));
    }

    #[test]
    fn a_change_that_needs_no_redraw_is_not_interaction() {
        let t0 = Instant::now();
        let mut s = QualityScheduler::default();
        s.next(t0, DOC);
        s.submitted(Final, 1);
        assert_eq!(s.note(t0 + ms(5), Changed::UI), None);
        assert_eq!(s.note(t0 + ms(5), Changed::empty()), None);
        assert_eq!(s.next(t0 + ms(5), DOC), Step::Idle);
    }

    #[test]
    fn a_new_document_starts_final_even_mid_gesture() {
        let t0 = Instant::now();
        let mut s = QualityScheduler::default();
        s.next(t0, DOC);
        s.note(t0 + ms(1), Changed::VIEW);
        assert_eq!(s.next(t0 + ms(1), DocumentId(2)), Step::Submit(Final));
        assert_eq!(s.deadline(), None);
    }

    #[test]
    fn invalidate_redraws_at_the_quality_that_is_due() {
        let t0 = Instant::now();
        let mut s = QualityScheduler::default();
        s.next(t0, DOC);
        s.invalidate();
        assert_eq!(s.next(t0 + ms(1000), DOC), Step::Submit(Final));
        s.note(t0 + ms(1001), Changed::VIEW);
        s.next(t0 + ms(1001), DOC);
        s.invalidate();
        assert_eq!(s.next(t0 + ms(1002), DOC), Step::Submit(Draft));
    }
}
