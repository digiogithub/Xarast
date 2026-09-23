//! The freehand tool (XARA-US-0034, T6.9–T6.10): acceptance criterion 16
//! of phase 7 and the rub-out.

use std::time::Instant;

use xarast_app::freehand::{FreehandTool, tolerance};
use xarast_app::tool::{GestureEvent, Tool, ToolCtx, ToolRequests};
use xarast_app::{
    Changed, DevicePoint, DocumentId, EditCommand, Intent, Modifiers, PointerButton, PointerSample,
    Session, ToolId,
};
use xarast_doc::NodeKind;
use xarast_geom::{EditPath, max_sample_deviation};

fn sample(at: DevicePoint, t: u64) -> PointerSample {
    PointerSample {
        at,
        pressure: None,
        time_ms: t,
    }
}

fn freehand() -> Session {
    let mut s = Session::new_empty(DocumentId(1));
    s.apply(Intent::ChooseTool(ToolId::Freehand)).unwrap();
    s
}

/// A wavy 200 Hz stroke across the canvas, in device pixels.
fn stroke(n: usize) -> Vec<(DevicePoint, u64)> {
    (0..n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let x = 60.0 + 680.0 * t;
            let y = 300.0 + 120.0 * (t * 18.0).sin() + 40.0 * (t * 51.0).cos();
            (DevicePoint::new(x, y), (i as u64) * 5)
        })
        .collect()
}

fn only_path(s: &Session) -> (xarast_doc::NodeId, EditPath, bool) {
    let n = xarast_app::edit::selectable_objects(&s.doc)
        .next()
        .expect("a path");
    match s.doc.tree.kind(n) {
        Some(NodeKind::Path(p)) => (n, EditPath::from_path(&p.data), p.filled),
        other => panic!("{other:?}"),
    }
}

fn doc_samples(s: &Session, pts: &[(DevicePoint, u64)]) -> Vec<kurbo::Point> {
    pts.iter()
        .map(|(d, _)| {
            let p = s.viewport.device_to_doc_f64(*d);
            let q = xarast_app::geometry::DocPointF64Ext::to_doc_point(p);
            let (x, y) = q.to_f64();
            kurbo::Point::new(x, y)
        })
        .collect()
}

#[test]
fn a_4000_sample_stroke_keeps_up_and_fits_within_the_tolerance() {
    let mut s = freehand();
    let pts = stroke(4000);
    let (first, _) = pts[0];
    s.apply(Intent::PointerMove(sample(first, 0))).unwrap();
    s.apply(Intent::PointerDown {
        button: PointerButton::Primary,
        sample: sample(first, 0),
    })
    .unwrap();
    let mut worst = 0.0f64;
    let mut drawing = false;
    for (at, t) in &pts[1..] {
        let started = Instant::now();
        let changed = s.apply(Intent::PointerMove(sample(*at, *t))).unwrap();
        worst = worst.max(started.elapsed().as_secs_f64() * 1e3);
        // Once past the drag threshold, the preview updates on every
        // sample (the ones below it are replayed when the drag starts).
        drawing |= changed.contains(Changed::SELECTION);
        assert!(
            !drawing || changed.contains(Changed::SELECTION),
            "{changed:?}"
        );
    }
    assert!(drawing);
    let (last, t) = pts[pts.len() - 1];
    s.apply(Intent::PointerUp {
        button: PointerButton::Primary,
        sample: sample(last, t),
    })
    .unwrap();
    assert!(worst < 16.0, "sample → preview {worst:.2} ms");
    assert_eq!(s.undo_label(), Some("Draw Freehand"));
    assert_eq!(s.bus.history().len(), 1);
    let (_, path, filled) = only_path(&s);
    assert!(!filled);
    // Every sample is within the tolerance of the committed curve.
    let samples = doc_samples(&s, &pts);
    let tol = tolerance(xarast_app::freehand::DEFAULT_SMOOTHING, &s.viewport);
    let dev = max_sample_deviation(&path, &samples);
    assert!(dev <= tol + 1.0, "deviation {dev} > tolerance {tol}");
    let nodes = path.subpaths[0].nodes.len();
    assert!(nodes > 4 && nodes < 400, "{nodes} nodes");
}

#[test]
fn no_sample_is_dropped_between_the_machine_and_the_fitter() {
    let mut s = freehand();
    let pts = stroke(4000);
    let mut tool = FreehandTool::default();
    let mut cmds: Vec<EditCommand> = Vec::new();
    let mut req = ToolRequests::default();
    let mut preview = xarast_app::Preview::default();
    let picker = xarast_app::picking::Picker::new();
    let doc_pts = doc_samples(&s, &pts);
    {
        let mut cx = ToolCtx {
            doc: &s.doc,
            edit: &s.edit,
            viewport: &s.viewport,
            modifiers: Modifiers::default(),
            preview: &mut preview,
            commands: &mut cmds,
            requests: &mut req,
            picker: &picker,
        };
        let at = |p: &kurbo::Point| xarast_geom::Point::from_f64_round(p.x, p.y);
        tool.on_gesture(
            &GestureEvent::DragStart {
                from: at(&doc_pts[0]),
                hit: None,
            },
            &mut cx,
        );
        for (p, (d, _)) in doc_pts.iter().zip(&pts).skip(1) {
            tool.on_gesture(
                &GestureEvent::DragUpdate {
                    from: at(&doc_pts[0]),
                    to: at(p),
                    to_device: *d,
                },
                &mut cx,
            );
        }
        let mut distinct = doc_pts.clone();
        distinct.dedup();
        assert_eq!(tool.sample_count(), distinct.len());
    }
    let _ = &mut s;
}

#[test]
fn going_back_with_adjust_rubs_the_stroke_out() {
    let mut s = freehand();
    let at = |x: f64| DevicePoint::new(x, 300.0);
    s.apply(Intent::PointerMove(sample(at(100.0), 0))).unwrap();
    s.apply(Intent::PointerDown {
        button: PointerButton::Primary,
        sample: sample(at(100.0), 0),
    })
    .unwrap();
    for i in 1..=100 {
        s.apply(Intent::PointerMove(sample(
            at(100.0 + f64::from(i) * 4.0),
            0,
        )))
        .unwrap();
    }
    // Back to x = 300 with Shift: everything after it goes.
    s.apply(Intent::ModifiersChanged(Modifiers {
        adjust: true,
        ..Modifiers::default()
    }))
    .unwrap();
    for i in 1..=25 {
        s.apply(Intent::PointerMove(sample(
            at(500.0 - f64::from(i) * 8.0),
            0,
        )))
        .unwrap();
    }
    s.apply(Intent::PointerUp {
        button: PointerButton::Primary,
        sample: sample(at(300.0), 0),
    })
    .unwrap();
    let (_, path, _) = only_path(&s);
    let end = path.subpaths[0].nodes.last().unwrap().at;
    let right = s.viewport.doc_to_device(end).x;
    assert!((right - 300.0).abs() < 8.0, "ends at {right}");
}

#[test]
fn escape_mid_stroke_draws_nothing_and_a_closed_stroke_is_filled() {
    let mut s = freehand();
    let before = s.doc.canonical_digest();
    let pts = stroke(200);
    s.apply(Intent::PointerMove(sample(pts[0].0, 0))).unwrap();
    s.apply(Intent::PointerDown {
        button: PointerButton::Primary,
        sample: sample(pts[0].0, 0),
    })
    .unwrap();
    for (d, t) in &pts[1..100] {
        s.apply(Intent::PointerMove(sample(*d, *t))).unwrap();
    }
    s.apply(Intent::Cancel).unwrap();
    s.apply(Intent::PointerUp {
        button: PointerButton::Primary,
        sample: sample(pts[99].0, 500),
    })
    .unwrap();
    assert_eq!(s.doc.canonical_digest(), before);
    assert_eq!(s.bus.history().len(), 0);
    // A circle that ends where it began.
    let c = DevicePoint::new(400.0, 300.0);
    let on = |a: f64| DevicePoint::new(c.x + 100.0 * a.cos(), c.y + 100.0 * a.sin());
    s.apply(Intent::PointerMove(sample(on(0.0), 0))).unwrap();
    s.apply(Intent::PointerDown {
        button: PointerButton::Primary,
        sample: sample(on(0.0), 0),
    })
    .unwrap();
    for i in 1..=120 {
        let a = f64::from(i) / 120.0 * std::f64::consts::TAU;
        s.apply(Intent::PointerMove(sample(on(a), 0))).unwrap();
    }
    s.apply(Intent::PointerUp {
        button: PointerButton::Primary,
        sample: sample(on(0.0), 0),
    })
    .unwrap();
    let (_, path, filled) = only_path(&s);
    assert!(path.subpaths[0].closed);
    assert!(filled);
}
