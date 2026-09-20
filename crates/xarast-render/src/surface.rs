//! Pixel buffers, device rectangles and the incremental-redraw primitives.
//!
//! # The pixel format is not negotiable
//!
//! Surfaces are premultiplied RGBA8 in **non-linear sRGB**. Every one of
//! Xara's blend families is a 256-entry table defined on encoded sRGB
//! (`research/03 §2.10`); compositing in linear light gives visibly
//! different Stained Glass and Bleach. On the GPU the render target must
//! therefore be `Rgba8Unorm` and never `Rgba8UnormSrgb`, or the hardware
//! linearises behind our back.

use crate::precision::round_device;

/// A rectangle in whole device pixels, half-open: `x0..x1`, `y0..y1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DeviceRect {
    /// Left edge, inclusive.
    pub x0: i32,
    /// Top edge, inclusive.
    pub y0: i32,
    /// Right edge, exclusive.
    pub x1: i32,
    /// Bottom edge, exclusive.
    pub y1: i32,
}

impl DeviceRect {
    /// The empty rectangle.
    pub const EMPTY: DeviceRect = DeviceRect {
        x0: 0,
        y0: 0,
        x1: 0,
        y1: 0,
    };

    /// Builds a rectangle, normalising an inverted one to empty.
    #[must_use]
    pub fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> DeviceRect {
        if x1 <= x0 || y1 <= y0 {
            DeviceRect::EMPTY
        } else {
            DeviceRect { x0, y0, x1, y1 }
        }
    }

    /// The whole of a surface of this size.
    #[must_use]
    pub fn from_size(width: u32, height: u32) -> DeviceRect {
        DeviceRect::new(
            0,
            0,
            i32::try_from(width).unwrap_or(i32::MAX),
            i32::try_from(height).unwrap_or(i32::MAX),
        )
    }

    /// Rounds a continuous `kurbo` rectangle outwards to whole pixels, with
    /// the crate's rounding rule applied to the centre-consistent bounds.
    #[must_use]
    pub fn enclosing(r: kurbo::Rect) -> DeviceRect {
        if !(r.x0.is_finite() && r.y0.is_finite() && r.x1.is_finite() && r.y1.is_finite()) {
            return DeviceRect::EMPTY;
        }
        DeviceRect::new(
            round_device(r.x0.floor()),
            round_device(r.y0.floor()),
            round_device(r.x1.ceil()),
            round_device(r.y1.ceil()),
        )
    }

    /// Width in pixels.
    #[must_use]
    pub fn width(self) -> u32 {
        u32::try_from(self.x1 - self.x0).unwrap_or(0)
    }

    /// Height in pixels.
    #[must_use]
    pub fn height(self) -> u32 {
        u32::try_from(self.y1 - self.y0).unwrap_or(0)
    }

    /// Whether the rectangle covers no pixels.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    /// The smallest rectangle containing both.
    #[must_use]
    pub fn union(self, other: DeviceRect) -> DeviceRect {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        DeviceRect {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    /// The overlap, or the empty rectangle.
    #[must_use]
    pub fn intersection(self, other: DeviceRect) -> DeviceRect {
        DeviceRect::new(
            self.x0.max(other.x0),
            self.y0.max(other.y0),
            self.x1.min(other.x1),
            self.y1.min(other.y1),
        )
    }

    /// Whether the two rectangles share at least one pixel.
    #[must_use]
    pub fn intersects(self, other: DeviceRect) -> bool {
        !self.intersection(other).is_empty()
    }

    /// Grows the rectangle by `by` pixels on every side.
    #[must_use]
    pub fn inflated(self, by: i32) -> DeviceRect {
        DeviceRect::new(
            self.x0.saturating_sub(by),
            self.y0.saturating_sub(by),
            self.x1.saturating_add(by),
            self.y1.saturating_add(by),
        )
    }

    /// Moves the rectangle.
    #[must_use]
    pub fn translated(self, dx: i32, dy: i32) -> DeviceRect {
        if self.is_empty() {
            return self;
        }
        DeviceRect {
            x0: self.x0.saturating_add(dx),
            y0: self.y0.saturating_add(dy),
            x1: self.x1.saturating_add(dx),
            y1: self.y1.saturating_add(dy),
        }
    }

    /// The area in pixels, saturating rather than overflowing.
    #[must_use]
    pub fn area(self) -> u64 {
        u64::from(self.width()) * u64::from(self.height())
    }
}

/// The union of everything a frame touched: the equivalent of CDraw's
/// `GetChangedBBox`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DirtyRect(pub Option<DeviceRect>);

impl DirtyRect {
    /// Nothing changed.
    pub const NONE: DirtyRect = DirtyRect(None);

    /// One rectangle changed.
    #[must_use]
    pub fn of(r: DeviceRect) -> DirtyRect {
        if r.is_empty() {
            DirtyRect::NONE
        } else {
            DirtyRect(Some(r))
        }
    }

    /// The union of two dirty regions.
    #[must_use]
    pub fn union(self, other: DirtyRect) -> DirtyRect {
        match (self.0, other.0) {
            (None, o) => DirtyRect(o),
            (s, None) => DirtyRect(s),
            (Some(a), Some(b)) => DirtyRect(Some(a.union(b))),
        }
    }

    /// Whether nothing changed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_none_or(DeviceRect::is_empty)
    }

    /// The rectangle, or the empty rectangle.
    #[must_use]
    pub fn rect(&self) -> DeviceRect {
        self.0.unwrap_or(DeviceRect::EMPTY)
    }
}

/// A premultiplied RGBA8 pixel buffer in non-linear sRGB.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Surface {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl Surface {
    /// Allocates a fully transparent surface.
    ///
    /// # Panics
    ///
    /// Panics if `width * height * 4` does not fit in a `usize`.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Surface {
        let len = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            .expect("surface dimensions overflow");
        Surface {
            width,
            height,
            data: vec![0; len],
        }
    }

    /// Allocates a surface filled with one premultiplied colour.
    #[must_use]
    pub fn filled(width: u32, height: u32, rgba: [u8; 4]) -> Surface {
        let mut s = Surface::new(width, height);
        s.fill(rgba);
        s
    }

    /// Width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// The whole buffer, four bytes per pixel, row-major, top row first.
    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// The whole buffer, mutably.
    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    /// The bounds of the surface.
    #[must_use]
    pub fn bounds(&self) -> DeviceRect {
        DeviceRect::from_size(self.width, self.height)
    }

    /// Fills every pixel with one premultiplied colour.
    pub fn fill(&mut self, rgba: [u8; 4]) {
        for px in self.data.chunks_exact_mut(4) {
            px.copy_from_slice(&rgba);
        }
    }

    /// Reads one pixel, or `None` outside the surface.
    #[must_use]
    pub fn pixel(&self, x: i32, y: i32) -> Option<[u8; 4]> {
        let o = self.offset(x, y)?;
        Some([
            self.data[o],
            self.data[o + 1],
            self.data[o + 2],
            self.data[o + 3],
        ])
    }

    /// Writes one pixel; out-of-bounds writes are dropped.
    pub fn set_pixel(&mut self, x: i32, y: i32, rgba: [u8; 4]) {
        if let Some(o) = self.offset(x, y) {
            self.data[o..o + 4].copy_from_slice(&rgba);
        }
    }

    fn offset(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 {
            return None;
        }
        let (x, y) = (u32::try_from(x).ok()?, u32::try_from(y).ok()?);
        if x >= self.width || y >= self.height {
            return None;
        }
        Some(((y as usize) * (self.width as usize) + x as usize) * 4)
    }

    /// Copies a rectangle out into a new surface, clipped to the bounds.
    #[must_use]
    pub fn sub_surface(&self, r: DeviceRect) -> Surface {
        let r = r.intersection(self.bounds());
        let mut out = Surface::new(r.width(), r.height());
        for y in 0..r.height() {
            for x in 0..r.width() {
                let src = self
                    .pixel(r.x0 + x as i32, r.y0 + y as i32)
                    .unwrap_or([0; 4]);
                out.set_pixel(x as i32, y as i32, src);
            }
        }
        out
    }
}

/// Moves the contents of a surface by `(dx, dy)`, the equivalent of
/// `GDraw_ScrollBitmap`, and returns the two strips that are now invalid.
///
/// Panning reuses the pixels that are still on screen and rasterises only
/// the newly exposed strips; the returned pair is `[horizontal, vertical]`,
/// either of which may be empty.
#[must_use]
pub fn scroll_surface(s: &mut Surface, dx: i32, dy: i32) -> [DirtyRect; 2] {
    let (w, h) = (s.width() as i32, s.height() as i32);
    if dx.abs() >= w || dy.abs() >= h {
        s.fill([0; 4]);
        return [DirtyRect::of(s.bounds()), DirtyRect::NONE];
    }
    if dx == 0 && dy == 0 {
        return [DirtyRect::NONE, DirtyRect::NONE];
    }

    let stride = (w as usize) * 4;
    let copy_w = (w - dx.abs()) as usize * 4;
    let rows: Vec<i32> = if dy > 0 {
        (0..h - dy).rev().collect()
    } else {
        (0..h + dy).collect()
    };
    for y in rows {
        let (src_y, dst_y) = if dy > 0 { (y, y + dy) } else { (y - dy, y) };
        let (src_x, dst_x) = if dx > 0 {
            (0usize, dx as usize)
        } else {
            ((-dx) as usize, 0usize)
        };
        let src = src_y as usize * stride + src_x * 4;
        let dst = dst_y as usize * stride + dst_x * 4;
        s.data_mut().copy_within(src..src + copy_w, dst);
    }

    // The exposed strips: a horizontal band and a vertical band.
    let horizontal = if dy > 0 {
        DeviceRect::new(0, 0, w, dy)
    } else if dy < 0 {
        DeviceRect::new(0, h + dy, w, h)
    } else {
        DeviceRect::EMPTY
    };
    let vertical = if dx > 0 {
        DeviceRect::new(0, horizontal.height() as i32 * i32::from(dy > 0), dx, h)
    } else if dx < 0 {
        DeviceRect::new(w + dx, 0, w, h)
    } else {
        DeviceRect::EMPTY
    };
    [DirtyRect::of(horizontal), DirtyRect::of(vertical)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_algebra() {
        let a = DeviceRect::new(0, 0, 10, 10);
        let b = DeviceRect::new(5, 5, 20, 20);
        assert_eq!(a.union(b), DeviceRect::new(0, 0, 20, 20));
        assert_eq!(a.intersection(b), DeviceRect::new(5, 5, 10, 10));
        assert!(DeviceRect::new(3, 3, 3, 9).is_empty());
        assert_eq!(a.area(), 100);
    }

    #[test]
    fn dirty_union_is_associative_and_has_an_identity() {
        let a = DirtyRect::of(DeviceRect::new(0, 0, 4, 4));
        let b = DirtyRect::of(DeviceRect::new(8, 8, 9, 9));
        assert_eq!(a.union(DirtyRect::NONE), a);
        assert_eq!(DirtyRect::NONE.union(a), a);
        assert_eq!(a.union(b), b.union(a));
        assert!(DirtyRect::NONE.is_empty());
    }

    #[test]
    fn scroll_moves_pixels_and_reports_the_exposed_strips() {
        let mut s = Surface::new(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                s.set_pixel(x, y, [x as u8, y as u8, 0, 255]);
            }
        }
        let [horiz, vert] = scroll_surface(&mut s, 0, 2);
        assert_eq!(horiz.rect(), DeviceRect::new(0, 0, 8, 2));
        assert!(vert.is_empty());
        // Row 5 of the original is now row 7.
        assert_eq!(s.pixel(3, 7), Some([3, 5, 0, 255]));
    }

    #[test]
    fn scrolling_further_than_the_surface_invalidates_everything() {
        let mut s = Surface::filled(4, 4, [1, 2, 3, 4]);
        let [a, b] = scroll_surface(&mut s, 99, 0);
        assert_eq!(a.rect(), DeviceRect::new(0, 0, 4, 4));
        assert!(b.is_empty());
        assert_eq!(s.pixel(0, 0), Some([0, 0, 0, 0]));
    }
}
