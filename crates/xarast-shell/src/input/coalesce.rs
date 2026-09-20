//! Per-frame sample accumulation.
//!
//! The compositor delivers pointer motion faster than we draw. The rule is
//! that **every** sample of a frame reaches the application: the freehand
//! tool reconstructs the stroke from all of them, and keeping only the last
//! one of each frame visibly changes the curve at speed.
//!
//! That is why this is not a fixed-capacity ring buffer. A ring that
//! overwrites would make sample loss a silent, load-dependent behaviour —
//! the worst possible failure mode for a drawing tool. The queue grows
//! instead, and records its high-water mark so an unreasonable burst shows up
//! in the diagnostics rather than in the geometry.

use super::tablet::StrokeSample;

/// Samples accumulated since the application last drained the queue.
#[derive(Debug, Default)]
pub struct SampleQueue {
    samples: Vec<StrokeSample>,
    received: u64,
    delivered: u64,
    high_water: usize,
}

impl SampleQueue {
    /// An empty queue.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one sample.
    pub fn push(&mut self, sample: StrokeSample) {
        self.samples.push(sample);
        self.received += 1;
        self.high_water = self.high_water.max(self.samples.len());
    }

    /// Records a batch, as polled from a [`super::tablet::TabletSource`].
    pub fn extend(&mut self, samples: impl IntoIterator<Item = StrokeSample>) {
        for s in samples {
            self.push(s);
        }
    }

    /// Hands every accumulated sample to `out`, in arrival order, and empties
    /// the queue.
    ///
    /// Appends rather than replaces, so a caller can drain several sources
    /// into one buffer.
    pub fn drain_into(&mut self, out: &mut Vec<StrokeSample>) {
        #[allow(clippy::cast_possible_truncation)]
        let n = self.samples.len() as u64;
        self.delivered += n;
        out.append(&mut self.samples);
    }

    /// Number of samples waiting.
    #[must_use]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// True when nothing is waiting.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Total samples ever accepted.
    #[must_use]
    pub const fn received(&self) -> u64 {
        self.received
    }

    /// Total samples ever handed to the application.
    #[must_use]
    pub const fn delivered(&self) -> u64 {
        self.delivered
    }

    /// Samples accepted but not yet delivered.
    ///
    /// Once the application has drained a frame this is zero. It is never
    /// negative and never lossy: if it ever were, the invariant this module
    /// exists to hold would be broken.
    #[must_use]
    pub const fn outstanding(&self) -> u64 {
        self.received - self.delivered
    }

    /// The largest the queue ever grew within one frame. Purely diagnostic.
    #[must_use]
    pub const fn high_water(&self) -> usize {
        self.high_water
    }
}

#[cfg(test)]
mod tests {
    use super::super::tablet::{InputSource, ToolAxes, normalise};
    use super::*;
    use std::time::Instant;

    fn sample(i: u32) -> StrokeSample {
        normalise(
            f64::from(i),
            f64::from(i),
            ToolAxes {
                force: Some(f64::from(i % 100) / 100.0),
                ..ToolAxes::default()
            },
            InputSource::Pen,
            Instant::now(),
        )
    }

    #[test]
    fn every_sample_of_a_frame_reaches_the_application() {
        // 200 Hz for a 3-second stroke at 60 fps is about ten samples a
        // frame; the burst here is far beyond anything real, and still
        // nothing may be dropped.
        let mut q = SampleQueue::new();
        for i in 0..5_000 {
            q.push(sample(i));
        }
        let mut out = Vec::new();
        q.drain_into(&mut out);
        assert_eq!(out.len(), 5_000);
        assert_eq!(q.outstanding(), 0);
        assert_eq!(q.received(), q.delivered());
    }

    #[test]
    fn order_is_arrival_order() {
        let mut q = SampleQueue::new();
        for i in 0..32 {
            q.push(sample(i));
        }
        let mut out = Vec::new();
        q.drain_into(&mut out);
        for (i, s) in out.iter().enumerate() {
            assert!((s.x - i as f64).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn draining_appends_so_several_sources_share_one_buffer() {
        let mut a = SampleQueue::new();
        let mut b = SampleQueue::new();
        a.push(sample(1));
        b.push(sample(2));
        let mut out = Vec::new();
        a.drain_into(&mut out);
        b.drain_into(&mut out);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn draining_twice_does_not_duplicate() {
        let mut q = SampleQueue::new();
        q.push(sample(1));
        let mut out = Vec::new();
        q.drain_into(&mut out);
        q.drain_into(&mut out);
        assert_eq!(out.len(), 1);
        assert!(q.is_empty());
    }

    #[test]
    fn the_high_water_mark_survives_a_drain() {
        let mut q = SampleQueue::new();
        for i in 0..10 {
            q.push(sample(i));
        }
        let mut out = Vec::new();
        q.drain_into(&mut out);
        q.push(sample(0));
        assert_eq!(q.high_water(), 10);
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn extend_counts_the_same_as_push() {
        let mut q = SampleQueue::new();
        q.extend((0..4).map(sample));
        assert_eq!(q.received(), 4);
        assert_eq!(q.outstanding(), 4);
    }
}
