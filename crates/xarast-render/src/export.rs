//! The export rasteriser: a document rectangle to an exact pixel grid, on
//! the deterministic CPU backend, in strips.
//!
//! This is the only entry point raster export uses
//! (`docs/phases/phase-11-export-filters.md` W11.2). It takes a [`CpuBackend`]
//! configuration and nothing else — never a trait object — so the GPU path
//! cannot leak into an export by construction.
//!
//! # Geometry that depends on the output size only
//!
//! Two numbers decide how the work is cut, and both are pure functions of
//! the output's pixel size, never of the thread count or the machine:
//!
//! * [`export_band_lines`], the height of the bands the backend rasterises
//!   in parallel. Bands are **shorter** than the interactive ones for small
//!   and medium outputs, so a 766 px export is ~48 bands rather than three
//!   and uses the whole machine (gintrack XARA-T-0038).
//! * [`export_strip_lines`], the height of the strips handed to the caller.
//!   A strip is a whole number of bands and starts on the band grid, so
//!   strips change memory, never pixels: an export assembled from strips is
//!   byte-identical to one rendered whole with the same band height.
//!
//! Peak memory is one strip plus the display list, whatever the output size.

use std::sync::Arc;

use xarast_geom::Rect;

use crate::backend::cpu::{CpuBackend, CpuConfig, Resolver, RowsOutcome};
use crate::backend::{BackendError, FrameTimings};
use crate::display_list::{DisplayList, ViewParams};
use crate::precision::Transform2D;
use crate::scene::{RenderQuality, Scene};
use crate::surface::{DeviceRect, DirtyRect, Surface};
use crate::tiling::MIN_BAND_SCANLINES;

/// The largest side an export may have: the rasteriser's pixmap limit.
pub const MAX_EXPORT_SIDE: u32 = u16::MAX as u32;

/// The default working memory for one strip: 64 MiB of RGBA.
pub const DEFAULT_STRIP_BUDGET: usize = 64 << 20;

/// The band count [`export_band_lines`] aims for.
const TARGET_BANDS: u32 = 64;

/// The largest band [`export_band_lines`] produces, in bytes: the same
/// 1 MiB the deterministic on-screen configuration uses.
const MAX_BAND_BYTES: u64 = 1 << 20;

/// The height of the bands an export of `width × height` pixels is
/// rasterised in.
///
/// Aims for [`TARGET_BANDS`] bands, never shorter than
/// [`MIN_BAND_SCANLINES`] and never over 1 MiB. A function of the size
/// alone: that is what makes an export byte-reproducible across thread
/// counts and machines.
#[must_use]
pub fn export_band_lines(width: u32, height: u32) -> u32 {
    let by_count = height.div_ceil(TARGET_BANDS);
    let by_bytes = (MAX_BAND_BYTES / (u64::from(width.max(1)) * 4)).max(1);
    let by_bytes = u32::try_from(by_bytes).unwrap_or(u32::MAX);
    by_count.min(by_bytes).max(MIN_BAND_SCANLINES)
}

/// The height of the strips an export is delivered in: a whole number of
/// bands, as many as fit in `budget_bytes`, at least one.
#[must_use]
pub fn export_strip_lines(width: u32, height: u32, budget_bytes: usize) -> u32 {
    let band = export_band_lines(width, height);
    let row = u64::from(width.max(1)) * 4;
    let bands = (budget_bytes as u64 / (row * u64::from(band))).max(1);
    let lines = u64::from(band) * bands;
    u32::try_from(lines.min(u64::from(height.max(1))))
        .unwrap_or(u32::MAX)
        .max(1)
}

/// The document→device transform that maps `area` exactly onto a
/// `width × height` grid: the area's top-left corner is pixel (0, 0) and
/// its bottom-right is (`width`, `height`). Document `y` points up and
/// device `y` down; the flip happens here. The two scales differ when the
/// aspect ratio is not locked.
#[must_use]
pub fn export_transform(area: Rect, width: u32, height: u32) -> Transform2D {
    let (x0, y1) = (area.lo.x.to_f64(), area.hi.y.to_f64());
    let aw = area.width().to_f64().max(1.0);
    let ah = area.height().to_f64().max(1.0);
    let sx = f64::from(width) / aw;
    let sy = f64::from(height) / ah;
    Transform2D::new([sx, 0.0, 0.0, -sy, -sx * x0, sy * y1])
}

/// One export render.
#[derive(Debug, Clone, Copy)]
pub struct ExportJob<'a> {
    /// What to draw, in document space.
    pub scene: &'a Scene,
    /// The ramps and images the scene refers to.
    pub resolver: &'a Resolver,
    /// The document rectangle that becomes the image.
    pub area: Rect,
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
    /// How hard to work; `Final` for every export the UI makes.
    pub quality: RenderQuality,
    /// Output resolution, for hairlines and the view's bookkeeping.
    pub dpi: f64,
    /// Straight RGBA the surface is cleared to before drawing.
    pub background: [u8; 4],
    /// Working memory for one strip; see [`export_strip_lines`].
    pub strip_budget_bytes: usize,
}

/// Why an export render stopped.
#[derive(Debug, thiserror::Error)]
pub enum ExportRenderError<E> {
    /// The size is zero or larger than [`MAX_EXPORT_SIDE`].
    #[error("an export of {width}x{height} px is outside 1..={MAX_EXPORT_SIDE} px a side")]
    BadSize {
        /// Requested width.
        width: u32,
        /// Requested height.
        height: u32,
    },
    /// The area is empty.
    #[error("the export area is empty")]
    EmptyArea,
    /// The backend refused.
    #[error(transparent)]
    Backend(#[from] BackendError),
    /// The caller cancelled.
    #[error("cancelled")]
    Cancelled,
    /// The strip consumer failed.
    #[error("{0}")]
    Sink(E),
}

/// What an export render did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExportStats {
    /// Display-list commands.
    pub commands: usize,
    /// Strips delivered.
    pub strips: u32,
    /// The band height used.
    pub band_lines: u32,
    /// Summed backend timings.
    pub timings: FrameTimings,
}

impl ExportJob<'_> {
    /// The view the display list is built for.
    #[must_use]
    pub fn view(&self) -> ViewParams {
        ViewParams {
            transform: export_transform(self.area, self.width, self.height),
            viewport: DeviceRect::from_size(self.width, self.height),
            quality: self.quality,
            dpi: self.dpi,
        }
    }

    fn check<E>(&self) -> Result<(), ExportRenderError<E>> {
        if self.width == 0
            || self.height == 0
            || self.width > MAX_EXPORT_SIDE
            || self.height > MAX_EXPORT_SIDE
        {
            return Err(ExportRenderError::BadSize {
                width: self.width,
                height: self.height,
            });
        }
        if self.area.is_empty() || self.area.width().raw() <= 0 || self.area.height().raw() <= 0 {
            return Err(ExportRenderError::EmptyArea);
        }
        Ok(())
    }
}

/// Renders an export strip by strip, top to bottom, handing each strip to
/// `sink` with the device row it starts on.
///
/// `cancelled` is polled before every band; `progress` is called after
/// every strip with (rows done, rows total).
///
/// # Errors
///
/// [`ExportRenderError`]; a sink error is returned as `Sink`.
pub fn render_export_strips<E>(
    job: &ExportJob<'_>,
    cancelled: &(dyn Fn() -> bool + Sync),
    progress: &mut dyn FnMut(u32, u32),
    sink: &mut dyn FnMut(u32, &Surface) -> Result<(), E>,
) -> Result<ExportStats, ExportRenderError<E>> {
    job.check()?;
    let view = job.view();
    let dl: Arc<DisplayList> = DisplayList::build(job.scene, &view, &DirtyRect::of(view.viewport));
    let band = export_band_lines(job.width, job.height);
    let strip = export_strip_lines(job.width, job.height, job.strip_budget_bytes);
    let mut backend = CpuBackend::new(CpuConfig::deterministic());
    let mut stats = ExportStats {
        commands: dl.len(),
        band_lines: band,
        ..ExportStats::default()
    };
    let mut surface: Option<Surface> = None;
    let mut y0 = 0u32;
    while y0 < job.height {
        if cancelled() {
            return Err(ExportRenderError::Cancelled);
        }
        let h = strip.min(job.height - y0);
        let s = match surface.take() {
            Some(mut s) if s.height() == h => {
                s.fill(job.background);
                s
            }
            _ => Surface::filled(job.width, h, job.background),
        };
        let mut s = s;
        match backend.render_rows(&dl, job.resolver, y0, band, &mut s, cancelled)? {
            RowsOutcome::Cancelled => return Err(ExportRenderError::Cancelled),
            RowsOutcome::Done(t) => {
                stats.timings.raster_us = stats.timings.raster_us.saturating_add(t.raster_us);
                stats.timings.tiles = stats.timings.tiles.saturating_add(t.tiles);
                stats.timings.rasterised_pixels += t.rasterised_pixels;
            }
        }
        sink(y0, &s).map_err(ExportRenderError::Sink)?;
        stats.strips += 1;
        y0 += h;
        progress(y0, job.height);
        surface = Some(s);
    }
    Ok(stats)
}

/// Renders an export into one surface. Memory is the whole image; use
/// [`render_export_strips`] to stream instead.
///
/// # Errors
///
/// As [`render_export_strips`].
pub fn render_export(
    job: &ExportJob<'_>,
    cancelled: &(dyn Fn() -> bool + Sync),
    progress: &mut dyn FnMut(u32, u32),
) -> Result<(Surface, ExportStats), ExportRenderError<std::convert::Infallible>> {
    job.check()?;
    let mut out = Surface::new(job.width, job.height);
    let stride = job.width as usize * 4;
    let stats = render_export_strips(job, cancelled, progress, &mut |y0, strip| {
        let o = y0 as usize * stride;
        out.data_mut()[o..o + strip.data().len()].copy_from_slice(strip.data());
        Ok(())
    })?;
    Ok((out, stats))
}

/// Rasterises prebuilt display lists for export, on the same deterministic
/// CPU backend as [`render_export_strips`].
///
/// Vector export uses it for the objects it cannot express natively (the
/// PDF fidelity ladder's "rasterise" step). It builds the backend once, so
/// many small renders share the blend tables. Like the rest of this module
/// it names [`CpuBackend`] and never a trait object: the GPU cannot reach
/// an export through it.
#[derive(Debug)]
pub struct ListRasteriser {
    backend: CpuBackend,
}

impl Default for ListRasteriser {
    fn default() -> ListRasteriser {
        ListRasteriser::new()
    }
}

impl ListRasteriser {
    /// A rasteriser on the deterministic CPU configuration.
    #[must_use]
    pub fn new() -> ListRasteriser {
        ListRasteriser {
            backend: CpuBackend::new(CpuConfig::deterministic()),
        }
    }

    /// Renders `dl` onto a `width × height` surface cleared to `background`
    /// (premultiplied RGBA). The list's view must map onto that grid.
    /// Bands follow [`export_band_lines`], so the pixels depend on the list
    /// and the size only.
    ///
    /// # Errors
    ///
    /// `BadSize` for an empty or oversized surface, `Backend`, or
    /// `Cancelled`.
    pub fn render(
        &mut self,
        dl: &DisplayList,
        resolver: &Resolver,
        width: u32,
        height: u32,
        background: [u8; 4],
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Surface, ExportRenderError<std::convert::Infallible>> {
        if width == 0 || height == 0 || width > MAX_EXPORT_SIDE || height > MAX_EXPORT_SIDE {
            return Err(ExportRenderError::BadSize { width, height });
        }
        let mut s = Surface::filled(width, height, background);
        let band = export_band_lines(width, height);
        match self
            .backend
            .render_rows(dl, resolver, 0, band, &mut s, cancelled)?
        {
            RowsOutcome::Cancelled => Err(ExportRenderError::Cancelled),
            RowsOutcome::Done(_) => Ok(s),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_depend_on_the_size_only_and_stay_in_range() {
        assert_eq!(export_band_lines(766, 766), MIN_BAND_SCANLINES);
        assert_eq!(export_band_lines(2480, 3508), 55);
        // A 20 000 px wide row is 80 000 bytes: 13 rows fit in 1 MiB, and
        // the floor wins.
        assert_eq!(export_band_lines(20_000, 20_000), MIN_BAND_SCANLINES);
        assert_eq!(export_band_lines(1, 1), MIN_BAND_SCANLINES);
    }

    #[test]
    fn strips_are_whole_bands() {
        for (w, h) in [(766, 766), (2480, 3508), (20_000, 20_000), (10, 5)] {
            let band = export_band_lines(w, h);
            let strip = export_strip_lines(w, h, DEFAULT_STRIP_BUDGET);
            assert!(
                strip.is_multiple_of(band) || strip == h,
                "{w}x{h}: {strip} vs {band}"
            );
            assert!(strip >= 1);
        }
        // Tiny budgets still make progress.
        assert_eq!(export_strip_lines(1000, 1000, 1), 16);
    }

    #[test]
    fn the_transform_maps_the_area_onto_the_grid() {
        use crate::precision::Point64;
        let area = Rect::raw(1000, 2000, 73_000, 146_000);
        let xf = export_transform(area, 96, 192);
        let tl = xf.apply(Point64::new(1000.0, 146_000.0));
        let br = xf.apply(Point64::new(73_000.0, 2000.0));
        assert!((tl.x).abs() < 1e-9 && (tl.y).abs() < 1e-9);
        assert!((br.x - 96.0).abs() < 1e-9 && (br.y - 192.0).abs() < 1e-9);
    }
}
