//! On-canvas fill handles (phase 8, W8.3): what the fill and transparency
//! tools draw over an object's fill, and what the pointer is over.
//!
//! **Handles live in device space for hit-testing and in document space for
//! semantics.** [`fill_handles`] derives a handle set in document
//! coordinates; [`hit_handle`] maps each handle to device pixels and compares
//! there, with a pick radius that is a constant in pixels
//! ([`FILL_PICK_RADIUS_PX`]) so handles stay grabbable at any zoom.
//!
//! The handle geometry per shape is the contract of `phase-08 §W8.3`:
//!
//! | Shape | Blobs | Arms (arrows) | Dashed guide |
//! |---|---|---|---|
//! | linear | `start` | `start → end` | — |
//! | radial | `centre` | to `major`, and to `minor` unless circular | the ellipse |
//! | conical | `centre` | to the zero direction | the sweep's circle |
//! | diamond | `centre` | to `corner1` and `corner2` | the rhombus |
//! | three / four colour | one per colour | — | the edges between them |
//!
//! Ramp stops are diamonds *on* the arm at their parametric position.

use xarast_color::Stop;
use xarast_doc::fill::FillGeometry;
use xarast_doc::fill_edit::{FillHandle, StopTarget, fill_arm};
use xarast_geom::{Matrix, Point, Vector};

use crate::geometry::{DevicePoint, DocPoint};
use crate::tool::{HandleShape, OverlayShape};
use crate::viewport::Viewport;

/// The pick radius, in device pixels, for a mouse. Constant on screen
/// (`phase-08 §W8.3`, proposal kept: 5 px).
pub const FILL_PICK_RADIUS_PX: f64 = 5.0;

/// How a handle is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandleKind {
    /// The tail of an arm: a linear fill's start.
    ArrowTail,
    /// The head of an arm: an end, a major/minor axis end, a corner.
    ArrowHead,
    /// A centre blob.
    Blob,
    /// An intermediate ramp stop.
    StopDiamond,
    /// A three/four-colour fill's colour point.
    CornerBlob,
}

/// A single on-canvas control, in document coordinates.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Handle {
    /// Which control it is.
    pub id: FillHandle,
    /// Where it is.
    pub pos: Point,
    /// How it is drawn.
    pub kind: HandleKind,
}

/// A construction line drawn with a handle set.
#[derive(Clone, PartialEq, Debug)]
pub enum Guide {
    /// A solid arm with an arrowhead at `to`.
    Arrow {
        /// The tail.
        from: Point,
        /// The head.
        to: Point,
    },
    /// A dashed outline.
    Dashed {
        /// The points, in order.
        points: Vec<Point>,
        /// Whether the last joins the first.
        closed: bool,
    },
}

/// Everything the overlay draws for one fill.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct FillHandles {
    /// The handles, in drawing order.
    pub handles: Vec<Handle>,
    /// Arms and dashed guides.
    pub guides: Vec<Guide>,
    /// The parametric line the stops sit on, `(t = 0, t = 1)`.
    pub arm: Option<(Point, Point)>,
}

/// What the pointer is over.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum FillHit {
    /// A handle.
    Handle(FillHandle),
    /// A point on the arm away from any handle, as a ramp position.
    Arm(f32),
}

fn perp(v: Vector) -> Vector {
    Vector::new(v.dy.saturating_neg(), v.dx)
}

/// `c + cos θ · u + sin θ · v`, sampled round a full turn.
fn ellipse(c: Point, u: Point, v: Point, n: usize) -> Vec<Point> {
    let (cx, cy) = c.to_f64();
    let (ux, uy) = (u.to_f64().0 - cx, u.to_f64().1 - cy);
    let (vx, vy) = (v.to_f64().0 - cx, v.to_f64().1 - cy);
    (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            let (s, k) = a.sin_cos();
            Point::from_f64_round(cx + k * ux + s * vx, cy + k * uy + s * vy)
        })
        .collect()
}

/// Derives the handle set of a fill. Pure: no document, no UI.
///
/// `object_to_doc` maps the fill's control points into document space.
/// Fill points are stored in document space and follow their object's
/// transforms, so today this is the identity; it is kept so a fill under a
/// live transform (a mould, phase 13) can be shown where it renders.
#[must_use]
pub fn fill_handles<S: Stop>(g: &FillGeometry<S>, object_to_doc: &Matrix) -> FillHandles {
    let m = |p: &Point| object_to_doc.transform_point(*p);
    let mut out = FillHandles::default();
    let h = |id, pos, kind| Handle { id, pos, kind };
    match g {
        FillGeometry::Linear { start, end, .. } => {
            let (a, b) = (m(start), m(end));
            out.guides.push(Guide::Arrow { from: a, to: b });
            out.handles
                .push(h(FillHandle::Start, a, HandleKind::ArrowTail));
            out.handles
                .push(h(FillHandle::End, b, HandleKind::ArrowHead));
        }
        FillGeometry::Radial {
            centre,
            major,
            minor,
            aspect_locked,
            ..
        } => {
            let (c, a, b) = (m(centre), m(major), m(minor));
            out.guides.push(Guide::Dashed {
                points: ellipse(c, a, b, 48),
                closed: true,
            });
            out.guides.push(Guide::Arrow { from: c, to: a });
            if !aspect_locked {
                out.guides.push(Guide::Arrow { from: c, to: b });
            }
            out.handles.push(h(FillHandle::Centre, c, HandleKind::Blob));
            out.handles
                .push(h(FillHandle::Major, a, HandleKind::ArrowHead));
            if !aspect_locked {
                out.handles
                    .push(h(FillHandle::Minor, b, HandleKind::ArrowHead));
            }
        }
        FillGeometry::Conical {
            centre, zero_dir, ..
        } => {
            let (c, z) = (m(centre), m(zero_dir));
            out.guides.push(Guide::Dashed {
                points: ellipse(c, z, c + perp(z - c), 48),
                closed: true,
            });
            out.guides.push(Guide::Arrow { from: c, to: z });
            out.handles.push(h(FillHandle::Centre, c, HandleKind::Blob));
            out.handles
                .push(h(FillHandle::End, z, HandleKind::ArrowHead));
        }
        FillGeometry::Diamond {
            centre,
            corner1,
            corner2,
            ..
        } => {
            let (c, a, b) = (m(centre), m(corner1), m(corner2));
            let (u, v) = (a - c, b - c);
            out.guides.push(Guide::Dashed {
                points: vec![c + u, c + v, c - u, c - v],
                closed: true,
            });
            out.guides.push(Guide::Arrow { from: c, to: a });
            out.guides.push(Guide::Arrow { from: c, to: b });
            out.handles.push(h(FillHandle::Centre, c, HandleKind::Blob));
            out.handles
                .push(h(FillHandle::Corner1, a, HandleKind::ArrowHead));
            out.handles
                .push(h(FillHandle::Corner2, b, HandleKind::ArrowHead));
        }
        FillGeometry::ThreeColour {
            origin,
            axis1,
            axis2,
            ..
        } => {
            let (o, a, b) = (m(origin), m(axis1), m(axis2));
            out.guides.push(Guide::Dashed {
                points: vec![o, a, b],
                closed: true,
            });
            out.handles
                .push(h(FillHandle::Start, o, HandleKind::CornerBlob));
            out.handles
                .push(h(FillHandle::End, a, HandleKind::CornerBlob));
            out.handles
                .push(h(FillHandle::End2, b, HandleKind::CornerBlob));
        }
        FillGeometry::FourColour {
            origin,
            axis1,
            axis2,
            axis3,
            ..
        } => {
            let (o, a, b, d) = (m(origin), m(axis1), m(axis2), m(axis3));
            out.guides.push(Guide::Dashed {
                points: vec![o, a, d, b],
                closed: true,
            });
            out.handles
                .push(h(FillHandle::Start, o, HandleKind::CornerBlob));
            out.handles
                .push(h(FillHandle::End, a, HandleKind::CornerBlob));
            out.handles
                .push(h(FillHandle::End2, b, HandleKind::CornerBlob));
            out.handles
                .push(h(FillHandle::End3, d, HandleKind::CornerBlob));
        }
        // Flat fills have no handles; bitmap handles are phase 10's,
        // fractal and noise fills phase 13's.
        FillGeometry::Flat { .. }
        | FillGeometry::Bitmap { .. }
        | FillGeometry::Fractal { .. }
        | FillGeometry::Noise { .. } => {}
    }
    if let Some((a, b)) = fill_arm(g) {
        let (a, b) = (m(&a), m(&b));
        out.arm = Some((a, b));
        if let Some(ramp) = ramp_of(g) {
            for (i, s) in ramp.iter().enumerate() {
                let Ok(i) = u16::try_from(i) else { break };
                out.handles.push(h(
                    FillHandle::Stop(i),
                    point_on(a, b, f64::from(*s)),
                    HandleKind::StopDiamond,
                ));
            }
        }
    }
    out
}

/// The positions of a fill's intermediate stops.
fn ramp_of<S: Stop>(g: &FillGeometry<S>) -> Option<Vec<f32>> {
    match g {
        FillGeometry::Linear { ramp, .. }
        | FillGeometry::Radial { ramp, .. }
        | FillGeometry::Conical { ramp, .. }
        | FillGeometry::Diamond { ramp, .. } => Some(ramp.stops().iter().map(|s| s.pos).collect()),
        _ => None,
    }
}

/// The point at parameter `t` along `a → b`.
#[must_use]
pub fn point_on(a: Point, b: Point, t: f64) -> Point {
    let (ax, ay) = a.to_f64();
    let (bx, by) = b.to_f64();
    Point::from_f64_round(ax + (bx - ax) * t, ay + (by - ay) * t)
}

fn dist(a: DevicePoint, b: DevicePoint) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// Device-space hit test. `radius_px` is in pixels, never millipoints.
///
/// Z-order: a stop beats an end handle, which beats the arm; among handles
/// of one kind, the nearest wins.
#[must_use]
pub fn hit_handle(
    h: &FillHandles,
    vp: &Viewport,
    p: DevicePoint,
    radius_px: f64,
) -> Option<FillHit> {
    let nearest = |stops: bool| {
        h.handles
            .iter()
            .filter(|x| (x.kind == HandleKind::StopDiamond) == stops)
            .map(|x| (x.id, dist(vp.doc_to_device(x.pos), p)))
            .filter(|(_, d)| *d <= radius_px)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| FillHit::Handle(id))
    };
    if let Some(hit) = nearest(true).or_else(|| nearest(false)) {
        return Some(hit);
    }
    let (a, b) = h.arm?;
    let (da, db) = (vp.doc_to_device(a), vp.doc_to_device(b));
    let (dx, dy) = (db.x - da.x, db.y - da.y);
    let len2 = dx * dx + dy * dy;
    if len2 <= f64::EPSILON {
        return None;
    }
    let t = (((p.x - da.x) * dx + (p.y - da.y) * dy) / len2).clamp(0.0, 1.0);
    let q = DevicePoint::new(da.x + dx * t, da.y + dy * t);
    (dist(q, p) <= radius_px).then_some(FillHit::Arm(t as f32))
}

/// The value a handle names: what a colour dropped on it, or the infobar's
/// stop fields, edit.
#[must_use]
pub fn stop_target<S: Stop>(g: &FillGeometry<S>, h: FillHandle) -> Option<StopTarget> {
    let corner = matches!(
        g,
        FillGeometry::ThreeColour { .. } | FillGeometry::FourColour { .. }
    );
    Some(match h {
        FillHandle::Stop(i) => StopTarget::Mid(i),
        FillHandle::Start if corner => StopTarget::Corner(0),
        FillHandle::End if corner => StopTarget::Corner(1),
        FillHandle::End2 if corner => StopTarget::Corner(2),
        FillHandle::End3 if corner => StopTarget::Corner(3),
        FillHandle::Start | FillHandle::Centre => StopTarget::From,
        FillHandle::End
        | FillHandle::Major
        | FillHandle::Minor
        | FillHandle::Corner1
        | FillHandle::Corner2 => StopTarget::To,
        FillHandle::End2 | FillHandle::End3 => return None,
    })
}

/// The overlay shapes of a handle set, with `selected` highlighted.
pub fn overlay_of(h: &FillHandles, selected: Option<FillHandle>, out: &mut Vec<OverlayShape>) {
    for g in &h.guides {
        match g {
            Guide::Arrow { from, to } => out.push(OverlayShape::Arrow {
                from: *from,
                to: *to,
            }),
            Guide::Dashed { points, closed } => out.push(OverlayShape::Polyline {
                points: points.clone(),
                closed: *closed,
                dashed: true,
            }),
        }
    }
    for x in &h.handles {
        let sel = selected == Some(x.id);
        let shape = match (x.kind, sel) {
            (HandleKind::StopDiamond, false) => HandleShape::FillStop,
            (HandleKind::StopDiamond, true) => HandleShape::FillStopSelected,
            (HandleKind::Blob, false) => HandleShape::FillCentre,
            (HandleKind::Blob, true) => HandleShape::FillCentreSelected,
            (_, false) => HandleShape::FillBlob,
            (_, true) => HandleShape::FillBlobSelected,
        };
        out.push(OverlayShape::Handle {
            at: DocPoint::new(x.pos.x, x.pos.y),
            shape,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::DeviceSize;
    use xarast_color::Transparency;
    use xarast_doc::fill::{Ramp, RampStop};

    fn linear() -> FillGeometry<Transparency> {
        let mut ramp = Ramp::new();
        ramp.insert(RampStop {
            pos: 0.25,
            value: Transparency::mix(40),
        });
        FillGeometry::Linear {
            start: Point::raw(0, 0),
            end: Point::raw(100_000, 0),
            persp: None,
            from: Transparency::mix(0),
            to: Transparency::mix(255),
            ramp,
        }
    }

    #[test]
    fn handle_counts_follow_the_shape_table() {
        let t = Transparency::mix(0);
        let c = Point::raw(0, 0);
        let a = Point::raw(10_000, 0);
        let b = Point::raw(0, 5_000);
        let cases: Vec<(FillGeometry<Transparency>, usize)> = vec![
            (FillGeometry::Flat { value: t }, 0),
            (linear(), 3),
            (
                FillGeometry::Radial {
                    centre: c,
                    major: a,
                    minor: b,
                    aspect_locked: false,
                    persp: None,
                    from: t,
                    to: t,
                    ramp: Ramp::new(),
                },
                3,
            ),
            (
                FillGeometry::Radial {
                    centre: c,
                    major: a,
                    minor: b,
                    aspect_locked: true,
                    persp: None,
                    from: t,
                    to: t,
                    ramp: Ramp::new(),
                },
                2,
            ),
            (
                FillGeometry::Conical {
                    centre: c,
                    zero_dir: a,
                    from: t,
                    to: t,
                    ramp: Ramp::new(),
                },
                2,
            ),
            (
                FillGeometry::Diamond {
                    centre: c,
                    corner1: a,
                    corner2: b,
                    persp: None,
                    from: t,
                    to: t,
                    ramp: Ramp::new(),
                },
                3,
            ),
            (
                FillGeometry::ThreeColour {
                    origin: c,
                    axis1: a,
                    axis2: b,
                    c0: t,
                    c1: t,
                    c2: t,
                },
                3,
            ),
            (
                FillGeometry::FourColour {
                    origin: c,
                    axis1: a,
                    axis2: b,
                    axis3: a + (b - c),
                    c0: t,
                    c1: t,
                    c2: t,
                    c3: t,
                },
                4,
            ),
        ];
        for (g, n) in cases {
            let h = fill_handles(&g, &Matrix::IDENTITY);
            assert_eq!(h.handles.len(), n, "{g:?}");
        }
        let h = fill_handles(&linear(), &Matrix::IDENTITY);
        let stop = h
            .handles
            .iter()
            .find(|x| x.id == FillHandle::Stop(0))
            .unwrap();
        assert_eq!(stop.pos, Point::raw(25_000, 0), "a stop sits on the arm");
    }

    #[test]
    fn hits_are_measured_in_pixels_at_every_zoom() {
        let h = fill_handles(&linear(), &Matrix::IDENTITY);
        for zoom in [0.05, 1.0, 32.0] {
            let mut vp = Viewport::new(DeviceSize::new(800, 600));
            vp.zoom_about(zoom, DevicePoint::new(400.0, 300.0));
            let end = vp.doc_to_device(Point::raw(100_000, 0));
            let near = DevicePoint::new(end.x + 4.0, end.y);
            let far = DevicePoint::new(end.x + 7.0, end.y);
            assert_eq!(
                hit_handle(&h, &vp, near, FILL_PICK_RADIUS_PX),
                Some(FillHit::Handle(FillHandle::End)),
                "zoom {zoom}"
            );
            assert_ne!(
                hit_handle(&h, &vp, far, FILL_PICK_RADIUS_PX),
                Some(FillHit::Handle(FillHandle::End)),
                "zoom {zoom}"
            );
        }
    }

    #[test]
    fn a_stop_beats_an_end_and_an_end_beats_the_arm() {
        // A stop right next to the start: both within the radius.
        let mut g = linear();
        if let FillGeometry::Linear { ramp, .. } = &mut g {
            *ramp = Ramp::new();
            ramp.insert(RampStop {
                pos: 0.02,
                value: Transparency::mix(9),
            });
        }
        let h = fill_handles(&g, &Matrix::IDENTITY);
        let vp = Viewport::new(DeviceSize::new(800, 600));
        let start = vp.doc_to_device(Point::raw(0, 0));
        assert_eq!(
            hit_handle(&h, &vp, start, 10.0),
            Some(FillHit::Handle(FillHandle::Stop(0)))
        );
        let mid = vp.doc_to_device(Point::raw(60_000, 0));
        match hit_handle(&h, &vp, mid, FILL_PICK_RADIUS_PX) {
            Some(FillHit::Arm(t)) => assert!((t - 0.6).abs() < 0.01, "{t}"),
            other => panic!("{other:?}"),
        }
    }
}
