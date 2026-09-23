//! Scripted pan, zoom and editing latency probes
//! (`xarast --probe pan|zoom|drag|scale|rotate|rect|ellipse`).
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

use xarast_app::{DevicePoint, Intent, PointerButton, PointerSample};

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
    /// A selector drag: press on an object, then move it 3 px right and
    /// 1.5 px down a frame, reversing every 60 frames, and release at the
    /// end. Every frame is a live preview (a scene rebuild), and the
    /// release commits one Move — the whole phase-7 editing path.
    Drag,
    /// A selector scale: click the object, then drag its top-right blob
    /// the same way. The release commits one Scale.
    Scale,
    /// A selector rotation: click the object twice (the rotate/skew
    /// handles), then drag its top-right blob. The release commits one
    /// Rotate.
    Rotate,
    /// The rectangle tool: drag out a rectangle from beside the canvas
    /// centre. The release commits one Create Rectangle.
    Rect,
    /// The ellipse tool, likewise.
    Ellipse,
}

impl ProbeKind {
    /// Parses `pan` or `zoom`.
    #[must_use]
    pub fn parse(s: &str) -> Option<ProbeKind> {
        match s {
            "pan" => Some(ProbeKind::Pan),
            "zoom" => Some(ProbeKind::Zoom),
            "drag" => Some(ProbeKind::Drag),
            "scale" => Some(ProbeKind::Scale),
            "rotate" => Some(ProbeKind::Rotate),
            "rect" => Some(ProbeKind::Rect),
            "ellipse" => Some(ProbeKind::Ellipse),
            _ => None,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            ProbeKind::Pan => "pan",
            ProbeKind::Zoom => "zoom",
            ProbeKind::Drag => "drag",
            ProbeKind::Scale => "scale",
            ProbeKind::Rotate => "rotate",
            ProbeKind::Rect => "rect",
            ProbeKind::Ellipse => "ellipse",
        }
    }

    /// Whether the probe is a pointer gesture: a press, a drag of one
    /// step a frame, and a release at the end.
    #[must_use]
    pub const fn is_gesture(self) -> bool {
        !matches!(self, ProbeKind::Pan | ProbeKind::Zoom)
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
    /// Where a drag probe's pointer is, in canvas pixels.
    drag_at: Option<(f64, f64)>,
    /// Intents a gesture probe applies first, one a step (choosing a tool,
    /// clicks), before it presses.
    prelude: std::collections::VecDeque<Intent>,
    /// Steps of the gesture itself, after the prelude.
    gesture_step: u32,
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
            drag_at: None,
            prelude: std::collections::VecDeque::new(),
            gesture_step: 0,
        }
    }

    /// Sets up a gesture probe: the intents to apply first, and where to
    /// press, in canvas pixels.
    pub fn set_gesture(&mut self, prelude: Vec<Intent>, press: (f64, f64)) {
        self.prelude = prelude.into();
        self.drag_at = Some(press);
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

    /// What the probe does.
    #[must_use]
    pub const fn kind(&self) -> ProbeKind {
        self.kind
    }

    /// Whether a drag probe has chosen where to press.
    #[must_use]
    pub const fn has_anchor(&self) -> bool {
        self.drag_at.is_some()
    }

    /// The intent that ends the probe cleanly: a drag's release.
    pub fn finish(&mut self) -> Option<Intent> {
        let (x, y) = self.drag_at.take()?;
        Some(Intent::PointerUp {
            button: PointerButton::Primary,
            sample: PointerSample::at(DevicePoint::new(x, y)),
        })
    }

    /// The next intent, stamped now. `centre` is the canvas centre — for a
    /// drag, the point to press on.
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
            ProbeKind::Drag
            | ProbeKind::Scale
            | ProbeKind::Rotate
            | ProbeKind::Rect
            | ProbeKind::Ellipse => {
                if let Some(intent) = self.prelude.pop_front() {
                    return intent;
                }
                self.gesture_step += 1;
                let (x, y) = *self.drag_at.get_or_insert(centre);
                let at = |x: f64, y: f64| PointerSample::at(DevicePoint::new(x, y));
                match self.gesture_step {
                    1 => Intent::PointerMove(at(x, y)),
                    2 => Intent::PointerDown {
                        button: PointerButton::Primary,
                        sample: at(x, y),
                    },
                    s => {
                        let dir = if ((s - 3) / 60).is_multiple_of(2) {
                            1.0
                        } else {
                            -1.0
                        };
                        let next = (x + 3.0 * dir, y + 1.5 * dir);
                        self.drag_at = Some(next);
                        Intent::PointerMove(at(next.0, next.1))
                    }
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
    fn a_gesture_probe_plays_its_prelude_then_presses_and_drags() {
        let mut p = Probe::new(ProbeKind::Rect, 5);
        p.set_gesture(
            vec![Intent::ChooseTool(xarast_app::ToolId::Rectangle)],
            (50.0, 60.0),
        );
        assert!(matches!(p.next((0.0, 0.0)), Intent::ChooseTool(_)));
        assert!(matches!(p.next((0.0, 0.0)), Intent::PointerMove(_)));
        assert!(matches!(p.next((0.0, 0.0)), Intent::PointerDown { .. }));
        match p.next((0.0, 0.0)) {
            Intent::PointerMove(s) => assert_eq!((s.at.x, s.at.y), (53.0, 61.5)),
            other => panic!("{other:?}"),
        }
        assert!(matches!(p.finish(), Some(Intent::PointerUp { .. })));
        assert!(ProbeKind::parse("ellipse").unwrap().is_gesture());
        assert!(!ProbeKind::Pan.is_gesture());
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
