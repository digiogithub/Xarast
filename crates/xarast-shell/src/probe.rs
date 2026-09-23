//! Scripted pan and zoom latency probes (`xarast --probe pan|zoom`).
//!
//! The probe drives the real viewer with intents it generates itself, one
//! per frame, never with input injected into the desktop: the maintainer's
//! session is not ours to type into. Each sample is the time from the
//! intent being applied to the frame that shows it being handed to the
//! compositor (`Queue::present` returned) and, with the shell's `probe`
//! setting, to the GPU having finished it. That is the whole interactive
//! path: the application core, the interface frame, the tile uploads of
//! whatever the render thread delivered meanwhile, the composite and the
//! present. Vsync is off while probing, so no sample waits for a vblank.

use std::time::Instant;

use xarast_app::{DevicePoint, Intent};

use crate::PresentTiming;

/// What the probe does to the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeKind {
    /// A drag: 23.4 px a frame, fractional on purpose, reversing every
    /// 60 frames.
    Pan,
    /// A wheel zoom: 1.05× a frame about the canvas centre, ten in, ten
    /// out.
    Zoom,
}

impl ProbeKind {
    /// Parses `pan` or `zoom`.
    #[must_use]
    pub fn parse(s: &str) -> Option<ProbeKind> {
        match s {
            "pan" => Some(ProbeKind::Pan),
            "zoom" => Some(ProbeKind::Zoom),
            _ => None,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            ProbeKind::Pan => "pan",
            ProbeKind::Zoom => "zoom",
        }
    }
}

/// Frames discarded before measuring: the first composites allocate.
const WARMUP: u32 = 10;

/// A running probe.
#[derive(Debug)]
pub struct Probe {
    kind: ProbeKind,
    wanted: usize,
    step: u32,
    input_at: Option<Instant>,
    present_ms: Vec<f64>,
    gpu_ms: Vec<f64>,
    /// Canvas frames the render thread delivered during the run.
    pub frames_delivered: u32,
}

impl Probe {
    /// A probe of `kind` that measures `samples` frames.
    #[must_use]
    pub fn new(kind: ProbeKind, samples: usize) -> Probe {
        Probe {
            kind,
            wanted: samples.max(1),
            step: 0,
            input_at: None,
            present_ms: Vec::new(),
            gpu_ms: Vec::new(),
            frames_delivered: 0,
        }
    }

    /// Records the present of the frame that showed the last intent.
    pub fn record(&mut self, t: Option<PresentTiming>) {
        let (Some(at), Some(t)) = (self.input_at, t) else {
            return;
        };
        if t.presented < at {
            return;
        }
        self.input_at = None;
        if self.step <= WARMUP {
            return;
        }
        self.present_ms
            .push(t.presented.duration_since(at).as_secs_f64() * 1e3);
        if let Some(g) = t.gpu_done {
            self.gpu_ms.push(g.duration_since(at).as_secs_f64() * 1e3);
        }
    }

    /// Whether enough samples are in.
    #[must_use]
    pub fn done(&self) -> bool {
        self.present_ms.len() >= self.wanted
    }

    /// The next intent, stamped now. `centre` is the canvas centre.
    pub fn next(&mut self, centre: (f64, f64)) -> Intent {
        self.step += 1;
        self.input_at = Some(Instant::now());
        match self.kind {
            ProbeKind::Pan => {
                let dir = if (self.step / 60).is_multiple_of(2) {
                    1.0
                } else {
                    -1.0
                };
                Intent::Pan {
                    dx: 23.4 * dir,
                    dy: 7.7 * dir,
                }
            }
            ProbeKind::Zoom => {
                let factor = if (self.step / 10).is_multiple_of(2) {
                    1.05
                } else {
                    1.0 / 1.05
                };
                Intent::Zoom {
                    factor,
                    anchor: DevicePoint::new(centre.0, centre.1),
                }
            }
        }
    }

    /// One line per measure, for the terminal and for `perf.md`.
    #[must_use]
    pub fn report(&self, renderer: &str, canvas: (u32, u32)) -> String {
        let mut out = format!(
            "probe {}: {} samples, canvas {}x{}, renderer {renderer}, {} render-thread frames\n",
            self.kind.name(),
            self.present_ms.len(),
            canvas.0,
            canvas.1,
            self.frames_delivered,
        );
        out.push_str(&line("input -> presented", &self.present_ms));
        if !self.gpu_ms.is_empty() {
            out.push_str(&line("input -> GPU idle ", &self.gpu_ms));
        }
        out
    }
}

fn line(what: &str, v: &[f64]) -> String {
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let q = |p: f64| -> f64 {
        if s.is_empty() {
            return f64::NAN;
        }
        // An index into a non-empty sorted vector.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let i = ((s.len() - 1) as f64 * p).round() as usize;
        s[i]
    };
    format!(
        "  {what}: p50 {:.2} ms, p95 {:.2} ms, p99 {:.2} ms, max {:.2} ms\n",
        q(0.5),
        q(0.95),
        q(0.99),
        q(1.0)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CanvasTier;
    use std::time::Duration;

    #[test]
    fn samples_are_input_to_present_after_the_warmup() {
        let mut p = Probe::new(ProbeKind::Pan, 3);
        for i in 0..(WARMUP + 3) {
            let intent = p.next((100.0, 100.0));
            assert!(matches!(intent, Intent::Pan { .. }));
            let at = p.input_at.unwrap();
            p.record(Some(PresentTiming {
                started: at,
                presented: at + Duration::from_millis(u64::from(i % 3) + 1),
                gpu_done: None,
                tier: CanvasTier::Cpu,
            }));
        }
        assert!(p.done());
        let r = p.report("CPU", (10, 10));
        assert!(r.contains("3 samples"), "{r}");
        assert!(r.contains("p50"), "{r}");
    }

    #[test]
    fn a_present_before_the_input_is_not_its_sample() {
        let mut p = Probe::new(ProbeKind::Zoom, 1);
        let old = Instant::now()
            .checked_sub(Duration::from_millis(5))
            .unwrap();
        p.next((0.0, 0.0));
        p.record(Some(PresentTiming {
            started: old,
            presented: old,
            gpu_done: None,
            tier: CanvasTier::GpuTiles,
        }));
        assert!(p.input_at.is_some(), "still waiting for its frame");
    }
}
