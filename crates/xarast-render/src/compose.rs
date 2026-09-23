//! Retained-tile composition: showing pixels that were already rasterised.
//!
//! A pan or a `Draft` zoom does not need new coverage for most of the
//! screen. It needs the pixels it already has, moved or resampled. This
//! module is that operation, stated once so that the GPU and the CPU do
//! exactly the same thing:
//!
//! * A **tile** is a `tile_size`² block of CPU-rasterised pixels in a
//!   *level space*: the device space of one zoom level with no pan applied.
//!   [`TileGrid`] names tiles ([`TileKey`]), gives the view that rasterises
//!   one ([`TileGrid::tile_view`]) and maps a tile into the current view
//!   ([`TileGrid::placement`]).
//! * A [`TilePlacement`] says where a tile's texels land in a target. The
//!   rule that picks a texel for a target pixel is [`source_texel`], and
//!   both [`compose_cpu`] and the GPU tile cache
//!   (`backend::gpu_tiles`, `gpu` feature) evaluate it with the same `f32`
//!   operations in the same order, so the two agree byte for byte on every
//!   IEEE-conformant device (measured, see `docs/memory/render.md`).
//!
//! Composition **replaces**: a tile is an opaque piece of the canvas,
//! background included, and where no tile lands the target shows the
//! backdrop colour. Nothing here blends, so nothing here depends on the
//! blend tables or on linear light.
//!
//! This is the decision of XARA-US-0011: rasterise on the CPU, keep the
//! pixels on the GPU, and let the GPU move them. See `render.md`.

use crate::display_list::ViewParams;
use crate::precision::Transform2D;
use crate::surface::{DeviceRect, Surface};

/// A tile of one zoom level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileKey {
    /// The zoom level the tile was rasterised at. The caller defines it
    /// (for instance `cache::scale_step`); tiles of different levels never
    /// share pixels.
    pub level: i32,
    /// Column, in tiles, in level space.
    pub tx: i32,
    /// Row, in tiles, in level space.
    pub ty: i32,
}

/// A rectangle of texels inside a tile, `[x0, y0, x1, y1]`, half open.
pub type TexelRect = [u32; 4];

/// The texel rectangle of a whole tile.
#[must_use]
pub const fn whole_tile(tile_size: u32) -> TexelRect {
    [0, 0, tile_size, tile_size]
}

/// The intersection of two texel rectangles, empty (`x1 <= x0` or
/// `y1 <= y0`) when they do not meet.
#[must_use]
pub fn texel_intersection(a: TexelRect, b: TexelRect) -> TexelRect {
    [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ]
}

/// Whether a texel rectangle holds no texel.
#[must_use]
pub const fn texel_rect_is_empty(r: TexelRect) -> bool {
    r[2] <= r[0] || r[3] <= r[1]
}

/// Where one tile's texels land in a target.
///
/// Texel `(u, v)` of the tile covers the target region starting at
/// `origin + (u, v) / inv_scale`. Coordinates are target-local device
/// pixels, which bounds them by the target size: `f32` is exact enough
/// here, and this is the one place where the GPU needs it to be.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TilePlacement {
    /// The tile.
    pub key: TileKey,
    /// Target position of the tile's top-left texel corner.
    pub origin: [f32; 2],
    /// Texels per target pixel, per axis: 1 at the level the tile was
    /// rasterised at, below 1 when zoomed in, above 1 when zoomed out.
    pub inv_scale: [f32; 2],
    /// The texels of the tile that hold pixels. A tile at the edge of a
    /// rasterised area is partial, on any side; the rest of it is never
    /// read, and the target shows whatever is under it.
    pub valid: TexelRect,
}

impl TilePlacement {
    /// The target pixels this placement may write, before clipping to the
    /// target: every pixel whose centre can map into the valid texels, with
    /// a pixel of margin for rounding. The per-pixel rule in
    /// [`source_texel`] decides the rest, so the rectangle only has to be
    /// large enough, never exact.
    #[must_use]
    pub fn target_rect(&self) -> DeviceRect {
        let axis = |o: f32, inv: f32, t0: u32, t1: u32| -> (i32, i32) {
            if !(o.is_finite() && inv.is_finite() && inv > 0.0) || t1 <= t0 {
                return (0, 0);
            }
            let o = f64::from(o);
            let start = o + f64::from(t0) / f64::from(inv);
            let end = o + f64::from(t1) / f64::from(inv);
            let lo = (start - 1.0)
                .floor()
                .clamp(f64::from(i32::MIN), f64::from(i32::MAX));
            let hi = (end + 1.0)
                .ceil()
                .clamp(f64::from(i32::MIN), f64::from(i32::MAX));
            // Clamped to the i32 range above, and integral.
            #[allow(clippy::cast_possible_truncation, reason = "clamped and integral")]
            (lo as i32, hi as i32)
        };
        let v = self.valid;
        let (x0, x1) = axis(self.origin[0], self.inv_scale[0], v[0], v[2]);
        let (y0, y1) = axis(self.origin[1], self.inv_scale[1], v[1], v[3]);
        DeviceRect::new(x0, y0, x1, y1)
    }
}

/// The texel coordinate that target pixel `p` (an integer) samples, along
/// one axis: `floor((p + 0.5 - origin) · inv_scale)`, in `f32`.
///
/// The GPU shader computes the same expression from `@builtin(position)`,
/// which is exactly `p + 0.5` for a single-sampled target: one subtraction
/// and one multiplication, both correctly rounded in WGSL, with nothing a
/// compiler may fuse into an FMA. Changing the order here without changing
/// the shader breaks parity.
#[must_use]
pub fn source_texel(p: i32, origin: f32, inv_scale: f32) -> f32 {
    // f32-ok: `p` is a target-local pixel index, bounded by the target size.
    let centre = p as f32 + 0.5;
    ((centre - origin) * inv_scale).floor()
}

/// Composites tiles into a CPU surface: the reference for the GPU tile
/// cache, and the software tier's implementation of the same operation.
///
/// The target is first filled with `backdrop`, then every placement whose
/// tile `fetch` returns is drawn in order, later placements over earlier
/// ones. A tile surface is indexed by texel, so it is normally
/// `tile_size`² ; texels outside it are treated as not valid. Returns how
/// many placements were drawn.
pub fn compose_cpu<'a>(
    target: &mut Surface,
    backdrop: [u8; 4],
    placements: &[TilePlacement],
    mut fetch: impl FnMut(&TileKey) -> Option<&'a Surface>,
) -> u32 {
    target.fill(backdrop);
    let bounds = target.bounds();
    let stride = target.width() as usize * 4;
    let mut drawn = 0;
    for p in placements {
        let Some(tile) = fetch(&p.key) else { continue };
        let v = texel_intersection(p.valid, [0, 0, tile.width(), tile.height()]);
        let r = p.target_rect().intersection(bounds);
        if r.is_empty() || texel_rect_is_empty(v) {
            continue;
        }
        drawn += 1;
        let pick = |t: f32, lo: u32, hi: u32| -> Option<usize> {
            // f32-ok: texel bounds, at most a tile edge.
            let (lo, hi) = (lo as f32, hi as f32);
            // `t` is integral; the comparison keeps the cast in range.
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "lo <= t < hi"
            )]
            (t >= lo && t < hi).then_some(t as usize)
        };
        let cols: Vec<Option<usize>> = (r.x0..r.x1)
            .map(|x| pick(source_texel(x, p.origin[0], p.inv_scale[0]), v[0], v[2]))
            .collect();
        let tstride = tile.width() as usize * 4;
        let src = tile.data();
        let dst = target.data_mut();
        for y in r.y0..r.y1 {
            let Some(sy) = pick(source_texel(y, p.origin[1], p.inv_scale[1]), v[1], v[3]) else {
                continue;
            };
            let srow = &src[sy * tstride..(sy + 1) * tstride];
            // `r` is inside the target, so `y` and `x` are non-negative.
            let row = y as usize * stride;
            for (x, sx) in (r.x0..r.x1).zip(&cols) {
                if let Some(sx) = sx {
                    let d = row + x as usize * 4;
                    dst[d..d + 4].copy_from_slice(&srow[sx * 4..sx * 4 + 4]);
                }
            }
        }
    }
    drawn
}

/// The tile grid of a level space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileGrid {
    /// Tile edge, in pixels.
    pub tile_size: u32,
}

impl Default for TileGrid {
    fn default() -> TileGrid {
        TileGrid {
            tile_size: crate::tiling::GPU_TILE_SIZE,
        }
    }
}

impl TileGrid {
    fn size_i32(self) -> i32 {
        i32::try_from(self.tile_size.max(1)).unwrap_or(i32::MAX)
    }

    /// The level-space rectangle of a tile.
    #[must_use]
    pub fn rect(self, key: TileKey) -> DeviceRect {
        let s = self.size_i32();
        let x0 = key.tx.saturating_mul(s);
        let y0 = key.ty.saturating_mul(s);
        DeviceRect::new(x0, y0, x0.saturating_add(s), y0.saturating_add(s))
    }

    /// The tiles of `level` that intersect a level-space rectangle, row by
    /// row.
    #[must_use]
    pub fn covering(self, level: i32, r: DeviceRect) -> Vec<TileKey> {
        if r.is_empty() {
            return Vec::new();
        }
        let s = self.size_i32();
        let (tx0, tx1) = (r.x0.div_euclid(s), (r.x1 - 1).div_euclid(s));
        let (ty0, ty1) = (r.y0.div_euclid(s), (r.y1 - 1).div_euclid(s));
        let mut out = Vec::new();
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                out.push(TileKey { level, tx, ty });
            }
        }
        out
    }

    /// The view that rasterises one tile: the level's transform shifted so
    /// that the tile's corner is the device origin, over a tile-sized
    /// viewport. Everything else is `level_view`'s.
    #[must_use]
    pub fn tile_view(self, level_view: &ViewParams, key: TileKey) -> ViewParams {
        let r = self.rect(key);
        ViewParams {
            transform: level_view
                .transform
                .then(Transform2D::translate(-f64::from(r.x0), -f64::from(r.y0))),
            viewport: DeviceRect::from_size(self.tile_size, self.tile_size),
            ..*level_view
        }
    }

    /// Where a tile rasterised under the level transform `level` lands in a
    /// view whose transform is `view`.
    ///
    /// `None` unless the mapping from level space to the view is an
    /// axis-aligned positive scale plus a translation, which is every pan
    /// and zoom and nothing else: a rotation needs new coverage.
    #[must_use]
    pub fn placement(
        self,
        key: TileKey,
        level: Transform2D,
        view: Transform2D,
        valid: TexelRect,
    ) -> Option<TilePlacement> {
        let m = level.invert()?.then(view).to_affine();
        let [sx, b, c, sy, tx, ty] = m.as_coeffs();
        let tiny = 1e-12 * sx.abs().max(sy.abs());
        if !m.is_finite() || b.abs() > tiny || c.abs() > tiny || sx <= 0.0 || sy <= 0.0 {
            return None;
        }
        let r = self.rect(key);
        let ox = sx * f64::from(r.x0) + tx;
        let oy = sy * f64::from(r.y0) + ty;
        // f32-ok: target-local position; the caller only places tiles that
        // land near the viewport, and `target_rect` clips the rest.
        let origin = [ox as f32, oy as f32];
        // f32-ok: a ratio of zoom levels, not a coordinate.
        let inv_scale = [(1.0 / sx) as f32, (1.0 / sy) as f32];
        Some(TilePlacement {
            key,
            origin,
            inv_scale,
            valid,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(w: u32, h: u32, seed: u8) -> Surface {
        let mut s = Surface::new(w, h);
        for (i, px) in s.data_mut().as_chunks_mut::<4>().0.iter_mut().enumerate() {
            // Truncation is the point: a cheap, varied pattern.
            #[allow(clippy::cast_possible_truncation)]
            let v = (i as u32).wrapping_mul(2_654_435_761) as u8;
            *px = [v, v.wrapping_add(seed), seed, 255];
        }
        s
    }

    #[test]
    fn an_identity_placement_is_a_copy() {
        let t = tile(16, 16, 7);
        let p = TilePlacement {
            key: TileKey::default_key(),
            origin: [0.0, 0.0],
            inv_scale: [1.0, 1.0],
            valid: [0, 0, 16, 16],
        };
        let mut out = Surface::new(16, 16);
        assert_eq!(compose_cpu(&mut out, [0; 4], &[p], |_| Some(&t)), 1);
        assert_eq!(out, t);
    }

    #[test]
    fn a_whole_pixel_offset_moves_pixels_and_leaves_the_backdrop() {
        let t = tile(8, 8, 3);
        let p = TilePlacement {
            key: TileKey::default_key(),
            origin: [3.0, -2.0],
            inv_scale: [1.0, 1.0],
            valid: [0, 0, 8, 8],
        };
        let mut out = Surface::new(12, 12);
        compose_cpu(&mut out, [9, 9, 9, 255], &[p], |_| Some(&t));
        assert_eq!(out.pixel(3, 0), t.pixel(0, 2));
        assert_eq!(out.pixel(10, 5), t.pixel(7, 7));
        assert_eq!(out.pixel(2, 0), Some([9, 9, 9, 255]));
        assert_eq!(out.pixel(3, 6), Some([9, 9, 9, 255]));
    }

    #[test]
    fn partial_tiles_are_read_only_where_valid() {
        let t = tile(8, 8, 1);
        let p = TilePlacement {
            key: TileKey::default_key(),
            origin: [0.0, 0.0],
            inv_scale: [1.0, 1.0],
            valid: [2, 1, 5, 3],
        };
        let mut out = Surface::new(8, 8);
        compose_cpu(&mut out, [0, 0, 0, 255], &[p], |_| Some(&t));
        assert_eq!(out.pixel(4, 2), t.pixel(4, 2));
        assert_eq!(out.pixel(2, 1), t.pixel(2, 1));
        assert_eq!(out.pixel(5, 2), Some([0, 0, 0, 255]));
        assert_eq!(out.pixel(4, 3), Some([0, 0, 0, 255]));
        assert_eq!(out.pixel(1, 2), Some([0, 0, 0, 255]));
        assert_eq!(out.pixel(3, 0), Some([0, 0, 0, 255]));
    }

    #[test]
    fn the_grid_covers_negative_space_by_floor_division() {
        let g = TileGrid { tile_size: 256 };
        let keys = g.covering(0, DeviceRect::new(-1, -257, 256, 0));
        let cells: Vec<(i32, i32)> = keys.iter().map(|k| (k.tx, k.ty)).collect();
        assert_eq!(cells, vec![(-1, -2), (0, -2), (-1, -1), (0, -1)]);
        assert_eq!(g.rect(keys[0]), DeviceRect::new(-256, -512, 0, -256));
    }

    #[test]
    fn a_pan_places_tiles_by_the_translation_and_a_zoom_scales_them() {
        let g = TileGrid { tile_size: 256 };
        let level = Transform2D::new([0.01, 0.0, 0.0, -0.01, 0.0, 0.0]);
        let key = TileKey {
            level: 0,
            tx: 1,
            ty: -1,
        };
        let pan = level.then(Transform2D::translate(-100.0, 40.0));
        let p = g
            .placement(key, level, pan, whole_tile(256))
            .expect("a pan");
        assert_eq!(p.origin, [156.0, -216.0]);
        assert_eq!(p.inv_scale, [1.0, 1.0]);

        let zoom = level.then(Transform2D::scale(2.0));
        let z = g
            .placement(key, level, zoom, whole_tile(256))
            .expect("a zoom");
        assert_eq!(z.origin, [512.0, -512.0]);
        assert_eq!(z.inv_scale, [0.5, 0.5]);

        let rot = level.then(Transform2D::rotate(0.1));
        assert!(g.placement(key, level, rot, whole_tile(256)).is_none());
    }

    #[test]
    fn a_tile_view_puts_the_tile_corner_at_the_device_origin() {
        let g = TileGrid { tile_size: 256 };
        let lv = ViewParams::new(
            1920,
            1080,
            Transform2D::scale(0.01),
            crate::RenderQuality::Final,
        );
        let key = TileKey {
            level: 0,
            tx: 2,
            ty: 1,
        };
        let v = g.tile_view(&lv, key);
        let p = v.transform.apply(crate::Point64::new(51_200.0, 25_600.0));
        assert!((p.x).abs() < 1e-9 && (p.y).abs() < 1e-9, "{p:?}");
        assert_eq!(v.viewport, DeviceRect::from_size(256, 256));
    }

    impl TileKey {
        fn default_key() -> TileKey {
            TileKey {
                level: 0,
                tx: 0,
                ty: 0,
            }
        }
    }
}
