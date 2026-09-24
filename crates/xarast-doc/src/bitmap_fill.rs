//! Bitmap fill editing (phase 10, T10.3.1): moving the origin and the
//! axes, the tiling, and the resolution — natural size included.
//!
//! # Stored points and on-canvas points
//!
//! A bitmap fill stores three corners of its placement parallelogram:
//! `origin` is the image's **bottom-left** corner, `axis_x` its
//! bottom-right and `axis_y` its top-left (`docs/memory/render.md`,
//! "Bitmap fill orientation"; only `paint::bitmap_frame` translates that
//! into sampling order). The fill tool does not show those corners. It
//! shows three *virtual* points (`research/04 §1.6`, facts of
//! `Kernel/opgrad.cpp:3701-3747`):
//!
//! | Handle | Virtual point |
//! |---|---|
//! | [`FillHandle::Centre`] | the parallelogram's centre; dragging it moves the whole fill |
//! | [`FillHandle::End`] | the middle of the edge the x axis ends on |
//! | [`FillHandle::End2`] | the middle of the edge the y axis ends on |
//!
//! A drag moves one virtual point and the stored corners are recomputed
//! from the three ([`bitmap_real_points`]), so the centre stays put while
//! an edge handle turns and stretches its own axis. With the aspect locked
//! (Adjust held, as in the original's `OnControlDrag`) the other axis
//! turns with it, stays perpendicular, and scales by the same ratio.
//!
//! A fill with **perspective** has four free corners instead: `Start`
//! (the origin), `End` (`axis_x`), `End2` (the perspective's top-left,
//! which `axis_y` follows) and `End3` (its top-right), plus `Centre`.
//!
//! # Tiling and resolution
//!
//! The tiling is both the fill's own `tiling` field and the channel's
//! fill-mapping attribute; the renderer lets the own field win when it is
//! set (`image.md`). [`SetBitmapTiling`] writes both, so neither can
//! contradict the other afterwards. [`SetBitmapDpi`] keeps the centre and
//! the axis directions and gives the axes the length `pixels × 72 000 /
//! dpi` millipoints; the natural size is that at the image's own
//! resolution.

use xarast_color::Stop;
use xarast_geom::{Mp, Point, Vector};

use crate::attr::AttrValue;
use crate::fill::{FillGeometry, Tiling};
use crate::fill_edit::{
    FillChannel, FillHandle, FillValue, PaintSlot, edit_fill, on_both, set_own_attr,
};
use crate::history::{Command, EditError, Tx};
use crate::tree::NodeId;

/// The resolution assumed for an image that declares none: the same
/// default `xarast-image` and every browser use.
pub const DEFAULT_BITMAP_DPI: u32 = 96;

/// The length, in millipoints, of `pixels` at `dpi` (`width × 72 000 /
/// dpi`), saturating. A zero resolution means [`DEFAULT_BITMAP_DPI`].
#[must_use]
pub fn natural_length(pixels: u32, dpi: u32) -> Mp {
    let dpi = if dpi == 0 { DEFAULT_BITMAP_DPI } else { dpi };
    let mp = u64::from(pixels) * u64::from(Mp::PER_INCH.unsigned_abs()) / u64::from(dpi);
    Mp::new(i32::try_from(mp).unwrap_or(i32::MAX))
}

fn f(p: Point) -> (f64, f64) {
    p.to_f64()
}

/// The three on-canvas points of a bitmap fill without perspective:
/// `[centre, middle of the x-axis edge, middle of the y-axis edge]`.
#[must_use]
pub fn bitmap_virtual_points(origin: Point, axis_x: Point, axis_y: Point) -> [Point; 3] {
    let (ox, oy) = f(origin);
    let (ux, uy) = (f(axis_x).0 - ox, f(axis_x).1 - oy);
    let (vx, vy) = (f(axis_y).0 - ox, f(axis_y).1 - oy);
    [
        Point::from_f64_round(ox + (ux + vx) / 2.0, oy + (uy + vy) / 2.0),
        Point::from_f64_round(ox + ux + vx / 2.0, oy + uy + vy / 2.0),
        Point::from_f64_round(ox + vx + ux / 2.0, oy + vy + uy / 2.0),
    ]
}

/// The stored corners `(origin, axis_x, axis_y)` of the three on-canvas
/// points: the inverse of [`bitmap_virtual_points`], exact in integers.
#[must_use]
pub fn bitmap_real_points(centre: Point, mid_x: Point, mid_y: Point) -> (Point, Point, Point) {
    let a = mid_y - centre;
    let b = mid_x - centre;
    (centre - a - b, centre - a + b, centre + a - b)
}

/// `v` turned a quarter turn (anticlockwise when `left`) and scaled to
/// `len`.
fn quarter(v: (f64, f64), len: f64, left: bool) -> (f64, f64) {
    let n = v.0.hypot(v.1);
    if n <= 0.0 {
        return (0.0, 0.0);
    }
    let (px, py) = if left { (-v.1, v.0) } else { (v.1, -v.0) };
    (px / n * len, py / n * len)
}

/// Moves one handle of a bitmap fill. Pure: [`MoveBitmapControl`] is this
/// plus a write-back, and the fill tool's preview calls it every frame.
///
/// `lock_aspect` keeps the image's proportions and right angles while an
/// edge handle is dragged (no effect on the other handles).
///
/// # Errors
///
/// When the fill is not a bitmap or has no such handle.
pub fn move_bitmap_control<S: Stop>(
    g: &mut FillGeometry<S>,
    handle: FillHandle,
    to: Point,
    lock_aspect: bool,
) -> Result<(), &'static str> {
    const NO: &str = "the bitmap fill has no such handle";
    let FillGeometry::Bitmap {
        origin,
        axis_x,
        axis_y,
        persp,
        ..
    } = g
    else {
        return Err("not a bitmap fill");
    };
    if handle == FillHandle::Centre {
        let [c, _, _] = bitmap_virtual_points(*origin, *axis_x, *axis_y);
        let c = match persp {
            // The centre of a quadrilateral: the mean of its corners.
            Some(p) => {
                let pts = [*origin, *axis_x, p.p2, p.p3];
                let (sx, sy) = pts
                    .iter()
                    .fold((0.0, 0.0), |(x, y), q| (x + f(*q).0, y + f(*q).1));
                Point::from_f64_round(sx / 4.0, sy / 4.0)
            }
            None => c,
        };
        let d: Vector = to - c;
        *origin += d;
        *axis_x += d;
        *axis_y += d;
        if let Some(p) = persp {
            p.p2 += d;
            p.p3 += d;
        }
        return Ok(());
    }
    if let Some(p) = persp {
        match handle {
            FillHandle::Start => *origin = to,
            FillHandle::End => *axis_x = to,
            FillHandle::End2 => {
                p.p2 = to;
                *axis_y = to;
            }
            FillHandle::End3 => p.p3 = to,
            _ => return Err(NO),
        }
        return Ok(());
    }
    let [c, mx, my] = bitmap_virtual_points(*origin, *axis_x, *axis_y);
    let (cx, cy) = f(c);
    let rel = |p: Point| (f(p).0 - cx, f(p).1 - cy);
    // Which side of the x axis the y axis lies on: kept, so a mirrored
    // fill stays mirrored under the aspect lock.
    let (u, v) = (rel(mx), rel(my));
    let left = u.0 * v.1 - u.1 * v.0 >= 0.0;
    let (mx, my) = match handle {
        FillHandle::Start => {
            *origin = to;
            return Ok(());
        }
        FillHandle::End => {
            let my = if lock_aspect {
                let old = u.0.hypot(u.1);
                let ratio = if old > 0.0 {
                    rel(to).0.hypot(rel(to).1) / old
                } else {
                    0.0
                };
                let q = quarter(rel(to), v.0.hypot(v.1) * ratio, left);
                Point::from_f64_round(cx + q.0, cy + q.1)
            } else {
                my
            };
            (to, my)
        }
        FillHandle::End2 => {
            let mx = if lock_aspect {
                let old = v.0.hypot(v.1);
                let ratio = if old > 0.0 {
                    rel(to).0.hypot(rel(to).1) / old
                } else {
                    0.0
                };
                let q = quarter(rel(to), u.0.hypot(u.1) * ratio, !left);
                Point::from_f64_round(cx + q.0, cy + q.1)
            } else {
                mx
            };
            (mx, to)
        }
        _ => return Err(NO),
    };
    let (o, x, y) = bitmap_real_points(c, mx, my);
    *origin = o;
    *axis_x = x;
    *axis_y = y;
    Ok(())
}

/// The resolution a bitmap fill of `pixels` shows its image at, per axis,
/// from the lengths of its axes; `None` for a degenerate or perspective
/// fill, or when `g` is not a bitmap fill.
#[must_use]
pub fn bitmap_fill_dpi<S: Stop>(g: &FillGeometry<S>, pixels: (u32, u32)) -> Option<(f64, f64)> {
    let FillGeometry::Bitmap {
        origin,
        axis_x,
        axis_y,
        persp: None,
        ..
    } = g
    else {
        return None;
    };
    let (w, h) = (origin.distance_to(*axis_x), origin.distance_to(*axis_y));
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let inch = f64::from(Mp::PER_INCH);
    Some((
        f64::from(pixels.0) * inch / w,
        f64::from(pixels.1) * inch / h,
    ))
}

/// Resizes a bitmap fill so that `pixels` show at `dpi`, keeping the
/// centre and the axis directions (an axis of zero length points along the
/// page's). Pure.
///
/// # Errors
///
/// When the fill is not a bitmap or has perspective.
pub fn set_bitmap_dpi<S: Stop>(
    g: &mut FillGeometry<S>,
    pixels: (u32, u32),
    dpi: (u32, u32),
) -> Result<(), &'static str> {
    let FillGeometry::Bitmap {
        origin,
        axis_x,
        axis_y,
        persp,
        dpi: own_dpi,
        ..
    } = g
    else {
        return Err("not a bitmap fill");
    };
    if persp.is_some() {
        return Err("a bitmap fill in perspective has no single resolution");
    }
    let [c, mx, my] = bitmap_virtual_points(*origin, *axis_x, *axis_y);
    let (cx, cy) = f(c);
    let dir = |p: Point, fallback: (f64, f64)| {
        let (dx, dy) = (f(p).0 - cx, f(p).1 - cy);
        let n = dx.hypot(dy);
        if n > 0.0 { (dx / n, dy / n) } else { fallback }
    };
    let (ux, uy) = dir(mx, (1.0, 0.0));
    let (vx, vy) = dir(my, (0.0, 1.0));
    let half_w = natural_length(pixels.0, dpi.0).to_f64() / 2.0;
    let half_h = natural_length(pixels.1, dpi.1).to_f64() / 2.0;
    let mx = Point::from_f64_round(cx + ux * half_w, cy + uy * half_w);
    let my = Point::from_f64_round(cx + vx * half_h, cy + vy * half_h);
    let (o, x, y) = bitmap_real_points(c, mx, my);
    *origin = o;
    *axis_x = x;
    *axis_y = y;
    *own_dpi = if dpi.0 == 0 {
        DEFAULT_BITMAP_DPI
    } else {
        dpi.0
    };
    Ok(())
}

/// Moves one handle of a bitmap fill: the fill tool's drag on a bitmap.
/// Labelled like every other handle move.
#[derive(Clone, Debug, PartialEq)]
pub struct MoveBitmapControl {
    /// The object.
    pub node: NodeId,
    /// Interior or outline.
    pub slot: PaintSlot,
    /// Colour or transparency.
    pub channel: FillChannel,
    /// Which handle ([`FillHandle::Centre`], `End`, `End2`; the corners of a
    /// fill in perspective).
    pub handle: FillHandle,
    /// Where it goes.
    pub to: Point,
    /// Whether the other axis keeps the image's proportions.
    pub lock_aspect: bool,
}

impl Command for MoveBitmapControl {
    fn label(&self) -> &'static str {
        "Move Fill Handle"
    }

    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        edit_fill(tx, self.node, self.slot, self.channel, |v| {
            on_both!(v, |g| move_bitmap_control(
                g,
                self.handle,
                self.to,
                self.lock_aspect
            ))
        })
    }
}

/// Sets how a bitmap fill repeats: its own tiling and the channel's
/// fill-mapping attribute together, so the two cannot disagree.
#[derive(Clone, Debug, PartialEq)]
pub struct SetBitmapTiling {
    /// The object.
    pub node: NodeId,
    /// Interior or outline.
    pub slot: PaintSlot,
    /// Colour or transparency.
    pub channel: FillChannel,
    /// `Simple`, `Repeat` or `RepeatInverted`.
    pub tiling: Tiling,
}

impl Command for SetBitmapTiling {
    fn label(&self) -> &'static str {
        "Fill Tiling"
    }

    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        edit_fill(tx, self.node, self.slot, self.channel, |v| {
            on_both!(v, |g| match g {
                FillGeometry::Bitmap { tiling, .. } => {
                    *tiling = self.tiling;
                    Ok(())
                }
                _ => Err("not a bitmap fill"),
            })
        })?;
        let v = match self.channel {
            FillChannel::Colour => AttrValue::FillMapping(self.tiling),
            FillChannel::Transparency => AttrValue::TranspFillMapping(self.tiling),
        };
        set_own_attr(tx, self.node, v)
    }
}

/// Resizes a bitmap fill so its image shows at a resolution, keeping the
/// centre and the axis directions ([`set_bitmap_dpi`]). With the image's
/// own resolution this is "natural size".
#[derive(Clone, Debug, PartialEq)]
pub struct SetBitmapDpi {
    /// The object.
    pub node: NodeId,
    /// Interior or outline.
    pub slot: PaintSlot,
    /// Colour or transparency.
    pub channel: FillChannel,
    /// The image's size in pixels.
    pub pixels: (u32, u32),
    /// The resolution, horizontal and vertical, in dots per inch.
    pub dpi: (u32, u32),
    /// Whether this is the image's own resolution, which names the step.
    pub natural: bool,
}

impl Command for SetBitmapDpi {
    fn label(&self) -> &'static str {
        if self.natural {
            "Natural Size"
        } else {
            "Bitmap Resolution"
        }
    }

    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        edit_fill(tx, self.node, self.slot, self.channel, |v| {
            on_both!(v, |g| set_bitmap_dpi(g, self.pixels, self.dpi))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xarast_color::{Colour, ColourValue};

    fn bitmap(origin: Point, axis_x: Point, axis_y: Point) -> FillGeometry<Colour> {
        FillGeometry::Bitmap {
            image: crate::resources::BitmapId::default(),
            origin,
            axis_x,
            axis_y,
            persp: None,
            tiling: Tiling::None,
            dpi: 96,
            contone: None::<(Colour, Colour)>,
            profile: xarast_geom::BiasGain::IDENTITY,
        }
    }

    fn corners(g: &FillGeometry<Colour>) -> (Point, Point, Point) {
        match g {
            FillGeometry::Bitmap {
                origin,
                axis_x,
                axis_y,
                ..
            } => (*origin, *axis_x, *axis_y),
            _ => unreachable!(),
        }
    }

    #[test]
    fn virtual_points_round_trip_exactly() {
        let (o, x, y) = (
            Point::raw(10_000, 20_000),
            Point::raw(110_000, 30_000),
            Point::raw(0, 80_000),
        );
        let [c, mx, my] = bitmap_virtual_points(o, x, y);
        assert_eq!(c, Point::raw(55_000, 55_000));
        assert_eq!(mx, Point::raw(105_000, 60_000));
        assert_eq!(my, Point::raw(50_000, 85_000));
        assert_eq!(bitmap_real_points(c, mx, my), (o, x, y));
    }

    #[test]
    fn the_centre_moves_the_whole_fill() {
        let mut g = bitmap(Point::raw(0, 0), Point::raw(100, 0), Point::raw(0, 50));
        move_bitmap_control(&mut g, FillHandle::Centre, Point::raw(1_050, 25), false).unwrap();
        assert_eq!(
            corners(&g),
            (
                Point::raw(1_000, 0),
                Point::raw(1_100, 0),
                Point::raw(1_000, 50)
            )
        );
    }

    #[test]
    fn an_edge_handle_keeps_the_centre_and_the_other_axis() {
        let mut g = bitmap(Point::raw(0, 0), Point::raw(100, 0), Point::raw(0, 50));
        // Middle right (100, 25) dragged to (150, 25): wider, same centre.
        move_bitmap_control(&mut g, FillHandle::End, Point::raw(150, 25), false).unwrap();
        assert_eq!(
            corners(&g),
            (Point::raw(-50, 0), Point::raw(150, 0), Point::raw(-50, 50))
        );
    }

    #[test]
    fn the_aspect_lock_turns_and_scales_the_other_axis() {
        let mut g = bitmap(Point::raw(0, 0), Point::raw(200, 0), Point::raw(0, 100));
        // Middle right (200, 50) about centre (100, 50) turned a quarter
        // turn and doubled: the middle top follows, also doubled.
        move_bitmap_control(&mut g, FillHandle::End, Point::raw(100, 250), true).unwrap();
        let (o, x, y) = corners(&g);
        let [c, mx, my] = bitmap_virtual_points(o, x, y);
        assert_eq!(c, Point::raw(100, 50));
        assert_eq!(mx, Point::raw(100, 250));
        assert_eq!(my, Point::raw(0, 50));
    }

    #[test]
    fn natural_size_follows_the_resolution() {
        assert_eq!(natural_length(96, 96), Mp::new(72_000));
        assert_eq!(natural_length(300, 300), Mp::new(72_000));
        assert_eq!(natural_length(96, 0), Mp::new(72_000));
        assert_eq!(natural_length(u32::MAX, 1), Mp::new(i32::MAX));
        let mut g = bitmap(
            Point::raw(0, 0),
            Point::raw(10_000, 0),
            Point::raw(0, 10_000),
        );
        set_bitmap_dpi(&mut g, (96, 48), (96, 96)).unwrap();
        let (o, x, y) = corners(&g);
        assert_eq!(o.distance_to(x), 72_000.0);
        assert_eq!(o.distance_to(y), 36_000.0);
        let dpi = bitmap_fill_dpi(&g, (96, 48)).unwrap();
        assert!((dpi.0 - 96.0).abs() < 1e-9 && (dpi.1 - 96.0).abs() < 1e-9);
        // The centre stays.
        assert_eq!(bitmap_virtual_points(o, x, y)[0], Point::raw(5_000, 5_000));
    }

    #[test]
    fn edits_refuse_what_is_not_a_bitmap() {
        let mut flat = FillGeometry::Flat {
            value: Colour::Direct(ColourValue::BLACK),
        };
        assert!(move_bitmap_control(&mut flat, FillHandle::Centre, Point::ORIGIN, false).is_err());
        assert!(set_bitmap_dpi(&mut flat, (1, 1), (96, 96)).is_err());
        let mut g = bitmap(Point::raw(0, 0), Point::raw(100, 0), Point::raw(0, 50));
        assert!(move_bitmap_control(&mut g, FillHandle::Major, Point::ORIGIN, false).is_err());
    }
}
