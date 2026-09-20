//! Tiles for the GPU and bands for the CPU.
//!
//! The GPU bins commands into 256 × 256 tiles by bounding box, which keeps
//! the destination reads local enough to stay cache resident. The CPU
//! splits into horizontal bands using the same memory heuristic the
//! original's `GRenderDIB::SetFirstBand` uses, with sixteen scanlines as
//! the useful minimum.
//!
//! A command that reads the destination is a **barrier** inside its tile:
//! everything before it must be resolved before it runs. [`TilePlan`]
//! records the barrier positions so that a backend never has to rediscover
//! them.

use crate::display_list::DisplayList;
use crate::surface::DeviceRect;

/// The GPU tile edge, in pixels.
pub const GPU_TILE_SIZE: u32 = 256;

/// The fewest scanlines a CPU band may have; below this the per-band
/// overhead dominates.
pub const MIN_BAND_SCANLINES: u32 = 16;

/// One tile and the commands that touch it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileBin {
    /// The tile's device rectangle.
    pub rect: DeviceRect,
    /// Indices into [`DisplayList::commands`], in order.
    pub commands: Vec<usize>,
    /// Positions within `commands` at which a destination read forces a
    /// ping-pong.
    pub barriers: Vec<usize>,
}

impl TileBin {
    /// Whether the tile has any work.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

/// The result of binning a display list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TilePlan {
    /// Tiles or bands, in raster order.
    pub tiles: Vec<TileBin>,
    /// The area the plan covers.
    pub area: DeviceRect,
}

impl TilePlan {
    /// Tiles that have work.
    pub fn non_empty(&self) -> impl Iterator<Item = &TileBin> {
        self.tiles.iter().filter(|t| !t.is_empty())
    }

    /// Total command references across all tiles, which is how much work
    /// the binning actually scheduled.
    #[must_use]
    pub fn scheduled(&self) -> usize {
        self.tiles.iter().map(|t| t.commands.len()).sum()
    }
}

/// Bins a display list into square tiles.
#[must_use]
pub fn plan_tiles(dl: &DisplayList, area: DeviceRect, tile_size: u32) -> TilePlan {
    let tile_size = tile_size.max(1);
    let mut tiles = Vec::new();
    if area.is_empty() {
        return TilePlan { tiles, area };
    }
    let mut y = area.y0;
    while y < area.y1 {
        let y1 = (y + tile_size as i32).min(area.y1);
        let mut x = area.x0;
        while x < area.x1 {
            let x1 = (x + tile_size as i32).min(area.x1);
            tiles.push(TileBin {
                rect: DeviceRect::new(x, y, x1, y1),
                commands: Vec::new(),
                barriers: Vec::new(),
            });
            x = x1;
        }
        y = y1;
    }
    fill_bins(dl, &mut tiles);
    TilePlan { tiles, area }
}

/// Splits a display list into horizontal bands.
///
/// `budget_bytes` is the working memory a band may use; the band height
/// follows from it exactly as the original's heuristic does, and is never
/// below [`MIN_BAND_SCANLINES`].
#[must_use]
pub fn plan_bands(dl: &DisplayList, area: DeviceRect, budget_bytes: usize) -> TilePlan {
    let mut tiles = Vec::new();
    if area.is_empty() {
        return TilePlan { tiles, area };
    }
    let scanline_bytes = (area.width() as usize).max(1) * 4;
    let lines = (budget_bytes / scanline_bytes.max(1)).max(MIN_BAND_SCANLINES as usize);
    let lines = u32::try_from(lines.min(u32::MAX as usize)).unwrap_or(MIN_BAND_SCANLINES);
    let mut y = area.y0;
    while y < area.y1 {
        let y1 = (y + lines as i32).min(area.y1);
        tiles.push(TileBin {
            rect: DeviceRect::new(area.x0, y, area.x1, y1),
            commands: Vec::new(),
            barriers: Vec::new(),
        });
        y = y1;
    }
    fill_bins(dl, &mut tiles);
    TilePlan { tiles, area }
}

/// Assigns commands to bins by bounding box, keeping structural commands in
/// every bin so that the stream stays balanced.
fn fill_bins(dl: &DisplayList, tiles: &mut [TileBin]) {
    for (i, cmd) in dl.commands().iter().enumerate() {
        let bounds = cmd.bounds();
        for tile in tiles.iter_mut() {
            let touches = match bounds {
                Some(b) => b.intersects(tile.rect),
                // Clip and layer commands are structural: every bin needs
                // them or its stack unbalances.
                None => true,
            };
            if !touches {
                continue;
            }
            if cmd.needs_dst_read() {
                tile.barriers.push(tile.commands.len());
            }
            tile.commands.push(i);
        }
    }
    // A bin that ended up with only structural commands has no work.
    for tile in tiles.iter_mut() {
        let has_draw = tile.commands.iter().any(|i| {
            matches!(
                dl.commands()[*i],
                crate::display_list::DrawCmd::Fill { .. }
                    | crate::display_list::DrawCmd::Stroke { .. }
                    | crate::display_list::DrawCmd::Image { .. }
                    | crate::display_list::DrawCmd::CachedSurface { .. }
            )
        });
        if !has_draw {
            tile.commands.clear();
            tile.barriers.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blend::{BlendFamily, Transparency};
    use crate::display_list::{DisplayList, ViewParams};
    use crate::paint::Paint;
    use crate::path::PathRef;
    use crate::scene::{RenderQuality, Scene, SceneBuilder, SceneNodeId};
    use crate::surface::DirtyRect;
    use xarast_color::Rgba8;
    use xarast_geom::{FillRule, Mp, Path, Point, Rect};

    fn rect_path(x0: f64, y0: f64, x1: f64, y1: f64) -> PathRef {
        let mut b = Path::builder();
        b.rect(Rect::new(
            Point::new(Mp::from_pt(x0), Mp::from_pt(y0)),
            Point::new(Mp::from_pt(x1), Mp::from_pt(y1)),
        ));
        PathRef::new(b.build())
    }

    fn list(transparency: Transparency) -> std::sync::Arc<DisplayList> {
        let mut scene = Scene::new();
        let mut b = SceneBuilder::begin(&mut scene, RenderQuality::Final);
        b.push_transparency(transparency);
        b.fill(
            SceneNodeId(1),
            &rect_path(0.0, 0.0, 20.0, 20.0),
            FillRule::NonZero,
            Paint::Solid(Rgba8::BLACK),
        );
        b.fill(
            SceneNodeId(2),
            &rect_path(300.0, 300.0, 320.0, 320.0),
            FillRule::NonZero,
            Paint::Solid(Rgba8::WHITE),
        );
        b.pop_transparency();
        b.finish().unwrap();
        let view = ViewParams::new(
            1024,
            1024,
            crate::precision::Transform2D::scale(1.0 / 1000.0),
            RenderQuality::Final,
        );
        DisplayList::build(&scene, &view, &DirtyRect::NONE)
    }

    #[test]
    fn tiles_cover_the_area_exactly_once() {
        let dl = list(Transparency::OPAQUE);
        let area = DeviceRect::new(0, 0, 600, 600);
        let plan = plan_tiles(&dl, area, GPU_TILE_SIZE);
        let total: u64 = plan.tiles.iter().map(|t| t.rect.area()).sum();
        assert_eq!(total, area.area());
        assert_eq!(plan.tiles.len(), 9);
    }

    #[test]
    fn binning_keeps_a_command_out_of_tiles_it_does_not_touch() {
        let dl = list(Transparency::OPAQUE);
        let plan = plan_tiles(&dl, DeviceRect::new(0, 0, 600, 600), GPU_TILE_SIZE);
        let with_work = plan.non_empty().count();
        // Each shape sits well inside one of the nine tiles, so exactly
        // two tiles have work and exactly two command references are
        // scheduled -- against the eighteen a no-op planner would produce.
        assert_eq!(with_work, 2, "{with_work} of 9 tiles have work");
        assert_eq!(plan.scheduled(), 2);
    }

    #[test]
    fn bands_are_never_thinner_than_the_minimum() {
        let dl = list(Transparency::OPAQUE);
        let area = DeviceRect::new(0, 0, 1024, 1024);
        let plan = plan_bands(&dl, area, 1024);
        assert!(
            plan.tiles
                .iter()
                .all(|t| t.rect.height() >= MIN_BAND_SCANLINES || t.rect.y1 == area.y1),
            "every band but possibly the last has at least 16 lines"
        );
        let total: u64 = plan.tiles.iter().map(|t| t.rect.area()).sum();
        assert_eq!(total, area.area());
    }

    #[test]
    fn a_destination_reading_command_is_recorded_as_a_barrier() {
        let dl = list(Transparency::flat(BlendFamily::StainedGlass, 100));
        let plan = plan_tiles(&dl, DeviceRect::new(0, 0, 600, 600), GPU_TILE_SIZE);
        assert!(
            plan.non_empty().all(|t| !t.barriers.is_empty()),
            "every tile with work has a barrier"
        );
    }

    #[test]
    fn an_empty_area_plans_nothing() {
        let dl = list(Transparency::OPAQUE);
        assert!(plan_tiles(&dl, DeviceRect::EMPTY, 256).tiles.is_empty());
        assert!(plan_bands(&dl, DeviceRect::EMPTY, 4096).tiles.is_empty());
    }
}
