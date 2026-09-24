//! Bitmap edits repaint only their object (XARA-T-0271, XARA-T-0272), in
//! the manner of `edit_damage.rs`: a bitmap fill handle drag — its preview
//! frames and its commit — and a placed bitmap each repaint the edited
//! object's device bounds, never the viewport, and every repainted frame
//! equals a full render of the same job byte for byte.
//!
//! CPU tier only, on a document built here; no corpus needed.

use std::sync::Mutex;
use std::sync::mpsc;
use std::time::Duration;

use xarast_app::render_thread::CpuFrameRenderer;
use xarast_app::{
    DeviceSize, DocumentId, EditCommand, FrameReuse, Intent, PointerButton, PointerSample,
    RenderThread, RenderedFrame, SelectMode, Session, ToolId,
};
use xarast_doc::fill::{FillGeometry, Tiling};
use xarast_doc::{AttrValue, NodeId};
use xarast_geom::{BiasGain, Point};
use xarast_render::{CpuConfig, DeviceRect, RenderQuality, Surface};

const BG: [u8; 4] = [128, 128, 132, 255];
const PAGE: [u8; 4] = [255, 255, 255, 255];
const W: u32 = 320;
const H: u32 = 240;
const T: Duration = Duration::from_secs(60);

fn thread() -> (RenderThread, mpsc::Receiver<()>) {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let rt = RenderThread::spawn_with(
        CpuFrameRenderer::new(CpuConfig::deterministic()),
        Box::new(move || {
            let _ = tx.lock().map(|t| t.send(()));
        }),
    )
    .expect("render thread");
    (rt, rx)
}

fn next_frame(rt: &RenderThread, woken: &mpsc::Receiver<()>) -> RenderedFrame {
    woken.recv_timeout(T).expect("a frame");
    rt.take_latest().expect("published before the wake")
}

/// A full render of `job` at `q` on a thread that keeps nothing.
fn reference(job: &xarast_app::FrameJob, q: RenderQuality) -> Surface {
    let mut job = job.clone();
    job.view.quality = q;
    let (mut rt, woken) = thread();
    rt.submit(job);
    next_frame(&rt, &woken).surface
}

/// A 4 × 2 picture of four colours.
fn picture() -> xarast_app::place::ImageToPlace {
    let row: Vec<u8> = [
        [255u8, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 0, 255],
    ]
    .iter()
    .flatten()
    .copied()
    .collect();
    let rgba = [row.clone(), row].concat();
    xarast_app::place::image_from_rgba(4, 2, &rgba).unwrap()
}

/// Two rectangles; the first carries a bitmap fill. The page fits the
/// canvas; the first frame is rendered and kept.
struct Bench {
    s: Session,
    rt: RenderThread,
    woken: mpsc::Receiver<()>,
    filled: NodeId,
    other: NodeId,
}

impl Bench {
    fn new() -> Bench {
        let mut s = Session::new_empty(DocumentId(11));
        let spread = s.doc.active_spread();
        let layer = s.doc.active_layer(spread).unwrap();
        let image = s.doc.resources.insert_bitmap(picture().resource);
        let fill = FillGeometry::Bitmap {
            image,
            origin: Point::raw(130_000, 280_000),
            axis_x: Point::raw(170_000, 280_000),
            axis_y: Point::raw(130_000, 320_000),
            persp: None,
            tiling: Tiling::None,
            dpi: 96,
            contone: None,
            profile: BiasGain::IDENTITY,
        };
        for (x, attrs) in [
            (150_000, vec![AttrValue::Fill(fill)]),
            (400_000, Vec::new()),
        ] {
            s.apply_edit(EditCommand::CreateShape {
                layer,
                shape: Box::new(xarast_app::shapes::rectangle(
                    Point::raw(x, 300_000),
                    40_000.0,
                    30_000.0,
                )),
                attrs,
            })
            .unwrap();
        }
        let nodes: Vec<NodeId> = xarast_app::edit::selectable_objects(&s.doc).collect();
        s.apply(Intent::Resize(DeviceSize::new(W, H))).unwrap();
        s.apply(Intent::ZoomTo(xarast_app::ZoomTarget::Page))
            .unwrap();
        let (rt, woken) = thread();
        let mut b = Bench {
            s,
            rt,
            woken,
            filled: nodes[0],
            other: nodes[1],
        };
        b.frame(RenderQuality::Final);
        b
    }

    /// Renders the session's current state at `q` over the kept frame and
    /// checks it against a full render.
    fn frame(&mut self, q: RenderQuality) -> RenderedFrame {
        if self.s.needs_scene() {
            self.s.rebuild_scene(None).expect("scene");
        }
        let mut job = self.s.frame_job(BG, PAGE);
        job.view.quality = q;
        self.rt.submit(job.clone());
        let f = next_frame(&self.rt, &self.woken);
        if q == RenderQuality::Final {
            assert!(
                f.surface == reference(&job, q),
                "a {:?} frame (fresh {:?}) differs from a full render",
                f.reuse,
                f.fresh
            );
        }
        f
    }

    fn device(&self, n: NodeId) -> DeviceRect {
        xarast_app::viewport::device_rect_of(
            &self.s.viewport,
            xarast_app::viewport::nodes_rect(&self.s.doc, [n]),
        )
    }

    fn pointer(&mut self, p: Point, phase: u8) {
        let at = self.s.viewport.doc_to_device(p);
        let sample = PointerSample {
            at,
            pressure: None,
            time_ms: 0,
        };
        let intent = match phase {
            0 => Intent::PointerDown {
                button: PointerButton::Primary,
                sample,
            },
            1 => Intent::PointerMove(sample),
            _ => Intent::PointerUp {
                button: PointerButton::Primary,
                sample,
            },
        };
        self.s.apply(intent).unwrap();
    }
}

fn union(fresh: &[DeviceRect]) -> DeviceRect {
    fresh.iter().fold(DeviceRect::EMPTY, |a, r| a.union(*r))
}

/// The fresh region lies within the object's bounds (with antialiasing
/// slack) and is nothing like the viewport.
fn within(fresh: &[DeviceRect], object: DeviceRect, what: &str) {
    let u = union(fresh);
    assert!(
        u.intersection(object.inflated(4)) == u,
        "{what}: fresh {u:?} outside the object {object:?}"
    );
    assert!(u.area() * 8 < u64::from(W * H), "{what}: {u:?}");
}

#[test]
fn a_bitmap_fill_drag_repaints_only_its_object() {
    let mut b = Bench::new();
    b.s.apply(Intent::Select {
        nodes: vec![b.filled],
        mode: SelectMode::Replace,
    })
    .unwrap();
    b.s.apply(Intent::ChooseTool(ToolId::Fill)).unwrap();
    b.frame(RenderQuality::Final);
    let object = b.device(b.filled);
    let other = b.device(b.other);
    // The x-axis handle, the middle of the fill's right edge: shrink the
    // fill inside the object, so its whole effect stays in the bounds.
    let from = Point::raw(170_000, 300_000);
    b.pointer(from, 1);
    b.pointer(from, 0);
    // Past the drag threshold first (a few device pixels at this zoom),
    // then every frame of the drag in flight.
    b.pointer(Point::raw(150_000, 302_000), 1);
    b.frame(RenderQuality::Final);
    for i in 1..=3 {
        b.pointer(Point::raw(150_000 - i * 3_000, 302_000 + i * 1_000), 1);
        let f = b.frame(RenderQuality::Final);
        assert_eq!(f.reuse, FrameReuse::Repainted, "drag frame {i}");
        within(&f.fresh, object, "a preview frame");
        assert!(
            f.fresh.iter().all(|r| r.intersection(other).is_empty()),
            "the other object is never repainted"
        );
    }
    b.pointer(Point::raw(141_000, 305_000), 2);
    assert_eq!(b.s.undo_label(), Some("Move Fill Handle"));
    let f = b.frame(RenderQuality::Final);
    assert!(
        matches!(
            f.reuse,
            FrameReuse::Repainted | FrameReuse::Scrolled { dx: 0, dy: 0 }
        ),
        "{:?}",
        f.reuse
    );
    within(&f.fresh, object, "the commit");
    // Undo repaints the same object only.
    b.s.apply(Intent::Undo).unwrap();
    let f = b.frame(RenderQuality::Final);
    assert_eq!(f.reuse, FrameReuse::Repainted);
    within(&f.fresh, object, "the undo");
}

#[test]
fn a_placed_bitmap_repaints_only_where_it_lands() {
    let mut b = Bench::new();
    let at = b.s.viewport.doc_to_device(Point::raw(280_000, 420_000));
    b.s.place_image(picture(), Some(at), "Paste").unwrap();
    let placed = b.s.edit.selection().next().expect("selected");
    let f = b.frame(RenderQuality::Final);
    assert_eq!(f.reuse, FrameReuse::Repainted);
    let object = b.device(placed);
    within(&f.fresh, object, "the placement");
    assert!(
        f.fresh
            .iter()
            .all(|r| r.intersection(b.device(b.filled)).is_empty()
                && r.intersection(b.device(b.other)).is_empty()),
        "the other objects are not repainted: {:?}",
        f.fresh
    );
}
