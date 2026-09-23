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
//!   and rasterises only the border a zoom-out uncovers. It is blurry by
//!   design; the `Final` that follows re-rasterises everything.
//! * **Anything else** — a new scene, a resize, a colour change, a
//!   rotation — is a full frame.
//!
//! Everything here is a pure function of a kept frame and a job, which is
//! what makes the policy testable without a thread.

use xarast_render::{DeviceRect, RenderQuality, Surface, Transform2D, ViewParams, scroll_surface};

use crate::render_thread::FrameJob;
use crate::session::DocumentId;

/// A translation within this distance of a whole pixel counts as whole.
/// A pan moves the centre by `dx / scale` and the transform recomputes
/// `scale * centre`, so a whole-pixel pan comes back a few ulps off.
const WHOLE_PIXEL_EPS: f64 = 1e-3;

/// The frame the worker keeps after publishing a copy of it.
#[derive(Debug)]
pub(crate) struct Kept {
    pub doc: DocumentId,
    pub scene_epoch: u64,
    pub background: [u8; 4],
    pub page_colour: Option<[u8; 4]>,
    /// The view the pixels really show, which for a snapped `Draft` pan is
    /// not quite the view that was asked for.
    pub view: ViewParams,
    /// Every pixel is what a full `Final` rasterisation of `view` would
    /// produce. Only such a frame may be reused by a `Final` job.
    pub final_exact: bool,
    pub surface: Surface,
}

/// How to produce a job's frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Plan {
    /// Rasterise everything.
    Full,
    /// Move the kept pixels by `(dx, dy)` and rasterise the exposed strips
    /// at `view`. `(0, 0)` means the kept frame already shows it.
    Scroll { dx: i32, dy: i32, view: ViewParams },
    /// Resample the kept pixels to the job's view (`Draft` only).
    Rescale,
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
        || k.scene_epoch != job.scene_epoch
        || k.background != job.background
        || k.page_colour != job.page.map(|(_, c)| c)
        || k.view.viewport != job.view.viewport
        || !same(k.view.dpi, job.view.dpi)
    {
        return Plan::Full;
    }
    let draft = job.view.quality == RenderQuality::Draft;
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
        if draft || k.final_exact {
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

/// Moves the kept pixels, returning the strips that are now exposed.
pub(crate) fn scroll(surface: &mut Surface, dx: i32, dy: i32) -> Vec<DeviceRect> {
    scroll_surface(surface, dx, dy)
        .iter()
        .filter(|d| !d.is_empty())
        .map(xarast_render::DirtyRect::rect)
        .collect()
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
            background: [1, 2, 3, 255],
            page_colour: Some([255; 4]),
            view: v,
            final_exact,
            surface: Surface::new(100, 80),
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
        let mut j = base.clone();
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
