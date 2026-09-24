//! Pixel reuse between consecutive frames, on the render thread.
//!
//! A full 100 000-object frame costs 35–160 ms on the CPU
//! (`docs/memory/perf.md`), so a pan or zoom that re-rasterises the
//! whole viewport cannot meet the 16 ms budget. What can is not
//! rasterising what is already on screen:
//!
//! * **Pan** — the kept frame is moved by a whole number of pixels
//!   ([`scroll_surface`]) and only the exposed strips are rasterised.
//!   A `Draft` pan snaps a fractional offset to whole pixels and reports
//!   the snapped view; the `Final` that follows is drawn at the exact one.
//! * **Zoom** — a `Draft` frame resamples the kept frame to the new scale
//!   and paints only the backdrop into the border a zoom-out uncovers. It
//!   is blurry and incomplete by design; the `Final` that follows
//!   re-rasterises everything.
//! * **Edit** — a new scene at the same view is compared with the kept
//!   frame's scene ([`xarast_render::scene_damage`]) and only the
//!   rectangles whose pixels may differ are rasterised over the kept
//!   pixels (XARA-T-0221). A `Final` also redraws whatever the kept frame
//!   still holds at `Draft` quality.
//! * **Anything else** — a resize, a colour change, a rotation, an edit
//!   during a pan or a zoom, an edit that damages most of the view — is a
//!   full frame.
//!
//! Everything here is a pure function of a kept frame and a job, which is
//! what makes the policy testable without a thread.

use std::sync::Arc;

use xarast_render::{
    DeviceRect, RenderQuality, Resolver, Scene, Surface, Transform2D, ViewParams, scene_damage,
    scroll_surface,
};

use crate::render_thread::FrameJob;
use crate::session::DocumentId;

/// A translation within this distance of a whole pixel counts as whole.
/// A pan moves the centre by `dx / scale` and the transform recomputes
/// `scale * centre`, so a whole-pixel pan comes back a few ulps off.
const WHOLE_PIXEL_EPS: f64 = 1e-3;

/// An edit's damage is repainted in at most this many rectangles. Each one
/// builds a culled display list, which scans the scene's culling entries
/// (1–2 ms at 100 000 objects, `docs/memory/perf.md`).
pub(crate) const MAX_REPAINT_RECTS: usize = 4;

/// Above this fraction of the viewport, an edit's damage is drawn as a
/// full frame instead: the full frame is interruptible column by column,
/// and a repaint of most of the view saves little.
const MAX_REPAINT_FRACTION: f64 = 0.5;

/// The frame the worker keeps after publishing a copy of it.
#[derive(Debug)]
pub(crate) struct Kept {
    pub doc: DocumentId,
    pub scene_epoch: u64,
    /// The scene and resolver the pixels were drawn from, so that the
    /// next scene can be compared with it.
    pub scene: Arc<Scene>,
    pub resolver: Arc<Resolver>,
    pub background: [u8; 4],
    pub page: Option<(DeviceRect, [u8; 4])>,
    /// The view the pixels really show, which for a snapped `Draft` pan is
    /// not quite the view that was asked for.
    pub view: ViewParams,
    /// A rectangle holding every pixel that is not what a full `Final`
    /// rasterisation of `view` would produce: empty for an exact frame.
    /// A `Final` job may reuse pixels outside it only.
    pub inexact: DeviceRect,
    pub surface: Surface,
    /// The generation the frame was published as.
    pub generation: u64,
    /// The pixels of the picture; the rest is placeholder backdrop.
    pub covered: DeviceRect,
}

impl Kept {
    /// Every pixel is what a full `Final` rasterisation of `view` would
    /// produce.
    pub fn final_exact(&self) -> bool {
        self.inexact.is_empty()
    }
}

/// How to produce a job's frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Plan {
    /// Rasterise everything.
    Full,
    /// Move the kept pixels by `(dx, dy)` and rasterise the exposed strips
    /// at `view`. `(0, 0)` means the kept frame already shows it.
    Scroll { dx: i32, dy: i32, view: ViewParams },
    /// Resample the kept pixels to the job's view (`Draft` only).
    Rescale,
    /// Keep the kept pixels and rasterise these rectangles over them: a
    /// new scene at the same view, or a `Final` over a partly `Draft`
    /// frame. Empty when the new scene draws the same picture.
    Repaint { rects: Vec<DeviceRect> },
}

fn same(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

fn coeffs(t: Transform2D) -> [f64; 6] {
    t.to_affine().as_coeffs()
}

/// Decides how `job` can be drawn from `kept`.
pub(crate) fn plan(kept: Option<&Kept>, job: &FrameJob) -> Plan {
    let Some(k) = kept else {
        return Plan::Full;
    };
    if k.doc != job.doc
        || k.background != job.background
        || k.page.map(|(_, c)| c) != job.page.map(|(_, c)| c)
        || k.view.viewport != job.view.viewport
        || !same(k.view.dpi, job.view.dpi)
    {
        return Plan::Full;
    }
    let draft = job.view.quality == RenderQuality::Draft;
    if let Some(p) = repaint(k, job, draft) {
        return p;
    }
    if k.scene_epoch != job.scene_epoch {
        return Plan::Full;
    }
    let (a, b) = (coeffs(k.view.transform), coeffs(job.view.transform));
    let same_linear = (0..4).all(|i| same(a[i], b[i]));
    if !same_linear {
        let axis_aligned = [a[1], a[2], b[1], b[2]].iter().all(|v| *v == 0.0);
        return if draft && axis_aligned {
            Plan::Rescale
        } else {
            Plan::Full
        };
    }
    let (dx, dy) = (b[4] - a[4], b[5] - a[5]);
    let (rx, ry) = (dx.round(), dy.round());
    let whole = (dx - rx).abs() < WHOLE_PIXEL_EPS && (dy - ry).abs() < WHOLE_PIXEL_EPS;
    let (w, h) = (
        f64::from(job.view.viewport.width()),
        f64::from(job.view.viewport.height()),
    );
    if rx.abs() >= w || ry.abs() >= h {
        return Plan::Full;
    }
    // In range by the check above, so the casts are exact.
    #[allow(clippy::cast_possible_truncation, reason = "|rx| < width")]
    let (ix, iy) = (rx as i32, ry as i32);
    if whole {
        // The exact view. A `Final` job needs the kept pixels to be exact
        // `Final` pixels too; a `Draft` takes whatever is there.
        if draft || k.final_exact() {
            return Plan::Scroll {
                dx: ix,
                dy: iy,
                view: job.view,
            };
        }
        return Plan::Full;
    }
    if !draft {
        return Plan::Full;
    }
    // A `Draft` pan by a fractional offset: snap it, and draw the strips
    // at the snapped view so that they meet the moved pixels seamlessly.
    let mut c = a;
    c[4] += rx;
    c[5] += ry;
    Plan::Scroll {
        dx: ix,
        dy: iy,
        view: ViewParams {
            transform: Transform2D::new(c),
            ..job.view
        },
    }
}

/// The plan for a job at exactly the kept frame's view: repaint what the
/// new scene changed (and, for a `Final`, what is still `Draft`), or a
/// full frame when that is most of the view. `None` when the view moved,
/// or when nothing is owed at all, for [`plan`] to decide as before.
fn repaint(k: &Kept, job: &FrameJob, draft: bool) -> Option<Plan> {
    // Bit-identical: the kept pixels were drawn at this very transform, and
    // an edit does not move the view.
    if coeffs(k.view.transform) != coeffs(job.view.transform)
        || k.page != job.page
        || k.covered != job.view.viewport
    {
        return None;
    }
    let mut rects = if k.scene_epoch == job.scene_epoch {
        Vec::new()
    } else {
        match scene_damage(
            (&k.scene, &k.resolver),
            (&job.scene, &job.resolver),
            &job.view,
            MAX_REPAINT_RECTS,
        ) {
            Some(d) => d.rects,
            None => return Some(Plan::Full),
        }
    };
    if !draft && !k.inexact.is_empty() {
        rects.push(k.inexact);
    }
    if rects.is_empty() {
        // Same picture: a new epoch reuses every pixel; the same epoch is
        // the scroll by nothing it always was.
        return (k.scene_epoch != job.scene_epoch).then_some(Plan::Repaint { rects });
    }
    let rects = xarast_render::damage::coalesce(rects, MAX_REPAINT_RECTS);
    let area: u64 = rects.iter().map(|r| r.area()).sum();
    // Areas of a viewport are far below 2^52, so the conversion is exact.
    #[allow(clippy::cast_precision_loss, reason = "pixel counts < 2^52")]
    let too_big = area as f64 > MAX_REPAINT_FRACTION * job.view.viewport.area() as f64;
    Some(if too_big {
        Plan::Full
    } else {
        Plan::Repaint { rects }
    })
}

/// Moves the kept pixels, returning the strips that are now exposed.
pub(crate) fn scroll(surface: &mut Surface, dx: i32, dy: i32) -> Vec<DeviceRect> {
    scroll_surface(surface, dx, dy)
        .iter()
        .filter(|d| !d.is_empty())
        .map(xarast_render::DirtyRect::rect)
        .collect()
}

/// What a frame scrolled by `(dx, dy)` covers, given what the kept frame
/// covered in a viewport `all`. A fully covered frame stays fully covered,
/// because the exposed strips are rasterised; a partly covered one (a
/// `Draft` zoom-out) keeps only its moved rectangle, conservatively.
pub(crate) fn scrolled_cover(covered: DeviceRect, all: DeviceRect, dx: i32, dy: i32) -> DeviceRect {
    if covered == all {
        return all;
    }
    covered.translated(dx, dy).intersection(all)
}

/// Resamples `old`, drawn at `from`, to the view `to`, nearest-neighbour.
///
/// Returns the new surface and the device rectangle the old pixels
/// covered; the rest of the surface is left transparent for the caller to
/// fill. `None` when the old frame covers nothing of the new one, or the
/// mapping is degenerate.
pub(crate) fn rescale(
    old: &Surface,
    from: &ViewParams,
    to: &ViewParams,
) -> Option<(Surface, DeviceRect)> {
    // New device pixel → document → old device pixel.
    let m = from.transform.to_affine() * to.transform.to_affine().inverse();
    let [sx, b, c, sy, tx, ty] = m.as_coeffs();
    if b != 0.0 || c != 0.0 || !m.is_finite() || sx == 0.0 || sy == 0.0 {
        return None;
    }
    let (w, h) = (to.viewport.width(), to.viewport.height());
    let (ow, oh) = (old.width(), old.height());
    let map = |n: u32, s: f64, t: f64, limit: u32| -> Vec<Option<u32>> {
        (0..n)
            .map(|i| {
                let v = (s * (f64::from(i) + 0.5) + t).floor();
                // `v` is finite and compared against `limit` first.
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "0 <= v < limit"
                )]
                (v >= 0.0 && v < f64::from(limit)).then_some(v as u32)
            })
            .collect()
    };
    let xs = map(w, sx, tx, ow);
    let ys = map(h, sy, ty, oh);
    let span = |v: &[Option<u32>]| -> Option<(u32, u32)> {
        let first = v.iter().position(Option::is_some)?;
        let last = v.iter().rposition(Option::is_some)?;
        Some((u32::try_from(first).ok()?, u32::try_from(last + 1).ok()?))
    };
    let ((x0, x1), (y0, y1)) = (span(&xs)?, span(&ys)?);

    let mut out = Surface::new(w, h);
    let stride = w as usize * 4;
    let ostride = ow as usize * 4;
    let src = old.data();
    let dst = out.data_mut();
    for y in y0..y1 {
        let Some(oy) = ys[y as usize] else { continue };
        let srow = &src[oy as usize * ostride..(oy as usize + 1) * ostride];
        let drow = &mut dst[y as usize * stride..(y as usize + 1) * stride];
        for x in x0..x1 {
            if let Some(ox) = xs[x as usize] {
                let (d, s) = (x as usize * 4, ox as usize * 4);
                drow[d..d + 4].copy_from_slice(&srow[s..s + 4]);
            }
        }
    }
    let covered = DeviceRect::new(
        i32::try_from(x0).ok()?,
        i32::try_from(y0).ok()?,
        i32::try_from(x1).ok()?,
        i32::try_from(y1).ok()?,
    );
    Some((out, covered))
}

/// The up to four rectangles of `all` outside `inner`: full-width bands
/// above and below, then the sides between them.
pub(crate) fn ring(all: DeviceRect, inner: DeviceRect) -> Vec<DeviceRect> {
    let inner = inner.intersection(all);
    if inner.is_empty() {
        return vec![all];
    }
    [
        DeviceRect::new(all.x0, all.y0, all.x1, inner.y0),
        DeviceRect::new(all.x0, inner.y1, all.x1, all.y1),
        DeviceRect::new(all.x0, inner.y0, inner.x0, inner.y1),
        DeviceRect::new(inner.x1, inner.y0, all.x1, inner.y1),
    ]
    .into_iter()
    .filter(|r| !r.is_empty())
    .collect()
}

/// `all` cut into at most `n` full-height columns, left first.
///
/// Columns, not rows: the CPU backend parallelises over horizontal bands,
/// so a full-height column keeps every core busy, where a short row slab
/// would leave most of them idle (measured: 192-row slabs doubled a
/// 1080p `Final`).
pub(crate) fn columns(all: DeviceRect, n: u32) -> Vec<DeviceRect> {
    let w = all.width();
    if w == 0 || all.height() == 0 {
        return Vec::new();
    }
    let step = i32::try_from(w.div_ceil(n.clamp(1, w))).unwrap_or(i32::MAX);
    let mut out = Vec::new();
    let mut x = all.x0;
    while x < all.x1 {
        let x1 = x.saturating_add(step).min(all.x1);
        out.push(DeviceRect::new(x, all.y0, x1, all.y1));
        x = x1;
    }
    out
}

/// Fills a device rectangle, clipped to the surface.
pub(crate) fn fill_rect(s: &mut Surface, r: DeviceRect, colour: [u8; 4]) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn view(scale: f64, tx: f64, ty: f64, q: RenderQuality) -> ViewParams {
        ViewParams {
            transform: Transform2D::new([scale, 0.0, 0.0, -scale, tx, ty]),
            viewport: DeviceRect::from_size(100, 80),
            quality: q,
            dpi: 96.0,
        }
    }

    fn kept(v: ViewParams, final_exact: bool) -> Kept {
        Kept {
            doc: DocumentId(1),
            scene_epoch: 4,
            scene: Arc::default(),
            resolver: Arc::default(),
            background: [1, 2, 3, 255],
            page: Some((DeviceRect::EMPTY, [255; 4])),
            view: v,
            inexact: if final_exact {
                DeviceRect::EMPTY
            } else {
                v.viewport
            },
            surface: Surface::new(100, 80),
            generation: 1,
            covered: v.viewport,
        }
    }

    fn job(v: ViewParams) -> FrameJob {
        FrameJob {
            doc: DocumentId(1),
            scene: std::sync::Arc::default(),
            scene_epoch: 4,
            ink: DeviceRect::EMPTY,
            resolver: std::sync::Arc::default(),
            view: v,
            background: [1, 2, 3, 255],
            page: Some((DeviceRect::EMPTY, [255; 4])),
            generation: 0,
            cpu_rescale: true,
        }
    }

    use RenderQuality::{Draft, Final};

    #[test]
    fn a_whole_pixel_pan_scrolls_at_either_quality_from_an_exact_frame() {
        let k = kept(view(0.01, 10.0, 20.0, Final), true);
        for q in [Draft, Final] {
            let j = job(view(0.01, 13.0, 15.0, q));
            assert_eq!(
                plan(Some(&k), &j),
                Plan::Scroll {
                    dx: 3,
                    dy: -5,
                    view: j.view
                }
            );
        }
    }

    #[test]
    fn a_final_never_reuses_draft_pixels() {
        let k = kept(view(0.01, 10.0, 20.0, Draft), false);
        assert_eq!(
            plan(Some(&k), &job(view(0.01, 13.0, 20.0, Final))),
            Plan::Full
        );
        assert_eq!(
            plan(Some(&k), &job(view(0.01, 10.0, 20.0, Final))),
            Plan::Full
        );
    }

    #[test]
    fn a_fractional_draft_pan_is_snapped_and_reports_the_snapped_view() {
        let k = kept(view(0.01, 10.0, 20.0, Final), true);
        let j = job(view(0.01, 12.6, 19.8, Draft));
        let Plan::Scroll { dx, dy, view: v } = plan(Some(&k), &j) else {
            panic!("expected a scroll");
        };
        assert_eq!((dx, dy), (3, 0));
        let c = coeffs(v.transform);
        assert!((c[4] - 13.0).abs() < 1e-12 && (c[5] - 20.0).abs() < 1e-12);
        assert_eq!(v.quality, Draft);
        // A `Final` at the same fractional offset is drawn whole.
        assert_eq!(
            plan(Some(&k), &job(view(0.01, 12.6, 19.8, Final))),
            Plan::Full
        );
    }

    #[test]
    fn a_zoom_rescales_in_draft_only() {
        let k = kept(view(0.01, 10.0, 20.0, Final), true);
        assert_eq!(
            plan(Some(&k), &job(view(0.02, 0.0, 0.0, Draft))),
            Plan::Rescale
        );
        assert_eq!(
            plan(Some(&k), &job(view(0.02, 0.0, 0.0, Final))),
            Plan::Full
        );
    }

    #[test]
    fn anything_else_changing_is_a_full_frame() {
        let k = kept(view(0.01, 10.0, 20.0, Final), true);
        let base = job(view(0.01, 10.0, 20.0, Draft));
        assert_eq!(plan(None, &base), Plan::Full);
        // A new scene during a pan.
        let mut j = job(view(0.01, 13.0, 20.0, Draft));
        j.scene_epoch = 5;
        assert_eq!(plan(Some(&k), &j), Plan::Full);
        let mut j = base.clone();
        j.doc = DocumentId(2);
        assert_eq!(plan(Some(&k), &j), Plan::Full);
        let mut j = base.clone();
        j.background = [0; 4];
        assert_eq!(plan(Some(&k), &j), Plan::Full);
        let mut j = base.clone();
        j.view.viewport = DeviceRect::from_size(101, 80);
        assert_eq!(plan(Some(&k), &j), Plan::Full);
        // A pan larger than the viewport has nothing to reuse.
        assert_eq!(
            plan(Some(&k), &job(view(0.01, 110.0, 20.0, Draft))),
            Plan::Full
        );
    }

    #[test]
    fn a_new_scene_at_the_same_view_repaints_only_what_changed() {
        let k = kept(view(0.01, 10.0, 20.0, Final), true);
        // The same (empty) picture under a new epoch: nothing to draw.
        let mut j = job(view(0.01, 10.0, 20.0, Final));
        j.scene_epoch = 5;
        assert_eq!(plan(Some(&k), &j), Plan::Repaint { rects: Vec::new() });
        // A page that moved is not something the scenes know about.
        j.page = Some((DeviceRect::new(0, 0, 5, 5), [255; 4]));
        assert_eq!(plan(Some(&k), &j), Plan::Full);
    }

    #[test]
    fn a_final_over_a_partly_draft_frame_repaints_the_draft_part() {
        let mut k = kept(view(0.01, 10.0, 20.0, Draft), true);
        k.inexact = DeviceRect::new(10, 10, 30, 20);
        let j = job(view(0.01, 10.0, 20.0, Final));
        assert_eq!(
            plan(Some(&k), &j),
            Plan::Repaint {
                rects: vec![k.inexact]
            }
        );
        // A Draft at the same view owes nothing.
        let d = job(view(0.01, 10.0, 20.0, Draft));
        assert_eq!(
            plan(Some(&k), &d),
            Plan::Scroll {
                dx: 0,
                dy: 0,
                view: d.view
            }
        );
        // Mostly Draft: a full frame.
        k.inexact = DeviceRect::new(0, 0, 90, 80);
        assert_eq!(plan(Some(&k), &j), Plan::Full);
    }

    #[test]
    fn rescaling_maps_pixels_and_reports_what_it_covered() {
        let mut old = Surface::new(100, 80);
        for y in 0..80 {
            for x in 0..100 {
                old.set_pixel(x, y, [x as u8, y as u8, 0, 255]);
            }
        }
        let from = view(1.0, 0.0, 80.0, Draft);
        // Zoom out by 2 about the origin: the old frame lands in the top
        // left quarter and the rest is uncovered.
        let to = view(0.5, 0.0, 40.0, Draft);
        let (s, covered) = rescale(&old, &from, &to).unwrap();
        assert_eq!(covered, DeviceRect::new(0, 0, 50, 40));
        assert_eq!(s.pixel(10, 10), Some([21, 21, 0, 255]));
        assert_eq!(s.pixel(60, 10), Some([0; 4]));
        // Zoom in by 2: everything is covered.
        let to = view(2.0, 0.0, 160.0, Draft);
        let (s, covered) = rescale(&old, &from, &to).unwrap();
        assert_eq!(covered, DeviceRect::from_size(100, 80));
        assert_eq!(s.pixel(10, 10), Some([5, 5, 0, 255]));
    }

    #[test]
    fn the_ring_and_the_columns_tile_exactly() {
        let all = DeviceRect::from_size(100, 80);
        let inner = DeviceRect::new(10, 20, 60, 50);
        let r = ring(all, inner);
        let area: u64 = r.iter().map(|r| r.area()).sum();
        assert_eq!(area + inner.area(), all.area());
        for (i, a) in r.iter().enumerate() {
            assert!(!a.intersects(inner));
            for b in &r[i + 1..] {
                assert!(!a.intersects(*b));
            }
        }
        assert_eq!(ring(all, DeviceRect::EMPTY), vec![all]);
        assert!(ring(all, all).is_empty());

        let s = columns(all, 3);
        assert_eq!(s.len(), 3);
        assert_eq!(s[2], DeviceRect::new(68, 0, 100, 80));
        assert_eq!(columns(DeviceRect::from_size(2, 5), 4).len(), 2);
        assert!(columns(DeviceRect::EMPTY, 4).is_empty());
        assert_eq!(s.iter().map(|r| r.area()).sum::<u64>(), all.area());
    }
}
