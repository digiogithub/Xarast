//! The backend facade: one trait, two implementations.
//!
//! [`Rasterizer`] is the 1:1 replacement for the original's
//! `GDrawContext`. Nothing above `xarast-render` names a backend type, so
//! swapping CPU for GPU is a constructor change and nothing else.

use crate::paint::{GradMapping, ImageId, Paint};
use crate::path::PathRef;
use crate::precision::Transform2D;
use crate::scene::LayerKind;
use crate::surface::{DeviceRect, DirtyRect, Surface};
use xarast_geom::{FillRule, StrokeStyle};

pub mod cpu;
#[cfg(feature = "gpu")]
pub mod gpu;
#[cfg(feature = "gpu")]
pub mod gpu_tiles;

/// An offscreen layer opened by [`Rasterizer::push_layer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayerId(pub u32);

/// What a backend can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RasterizerCaps {
    /// Whether the same input always produces the same bytes. Only the CPU
    /// backend promises this, and export depends on it.
    pub deterministic: bool,
    /// The largest surface or texture the backend will accept.
    pub max_texture_dim: u32,
    /// Whether destination-reading blends are supported natively.
    pub supports_dst_read: bool,
    /// The tile or band granularity the backend schedules at.
    pub tile_size: u32,
}

/// Per-frame timings, for the status bar and for the benchmarks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameTimings {
    /// Microseconds spent planning: binning and setup.
    pub build_us: u32,
    /// Microseconds spent producing coverage.
    pub raster_us: u32,
    /// Microseconds spent in the compositor.
    pub composite_us: u32,
    /// Tiles or bands that had work.
    pub tiles: u32,
    /// Cache lookups that hit.
    pub cache_hits: u32,
    /// Cache lookups that missed.
    pub cache_misses: u32,
    /// Pixels the rasteriser actually produced coverage for. Counting this
    /// rather than timing it is what makes the incremental-redraw test a
    /// fact rather than a measurement.
    pub rasterised_pixels: u64,
}

impl FrameTimings {
    /// Total microseconds.
    #[must_use]
    pub const fn total_us(&self) -> u32 {
        self.build_us
            .saturating_add(self.raster_us)
            .saturating_add(self.composite_us)
    }
}

/// Why a backend could not be created or could not draw.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BackendError {
    /// No GPU adapter, or one without the features the backend needs.
    #[error("no usable graphics adapter: {0}")]
    NoAdapter(String),
    /// The surface is larger than the backend accepts.
    #[error("surface {width}x{height} exceeds the backend's maximum of {max}")]
    SurfaceTooLarge {
        /// Requested width.
        width: u32,
        /// Requested height.
        height: u32,
        /// The backend's limit.
        max: u32,
    },
    /// The render target format is wrong. Compositing happens in non-linear
    /// sRGB, so an sRGB-converting target silently changes every blend.
    #[error("the render target must be Rgba8Unorm, never Rgba8UnormSrgb (research/03 §2.10)")]
    WrongTargetFormat,
    /// A paint could not be evaluated.
    #[error(transparent)]
    Paint(#[from] crate::paint::PaintError),
}

/// The immediate-mode drawing facade, the 1:1 replacement for
/// `GDrawContext`.
pub trait Rasterizer {
    /// Begins a frame against a target and a clip rectangle.
    fn begin_frame(&mut self, target: &mut Surface, clip: DeviceRect);

    /// Fills a path.
    fn fill_path(&mut self, path: &PathRef, rule: FillRule, paint: &Paint, xf: &Transform2D);

    /// Strokes a path. A zero width is a hairline: one device pixel at any
    /// zoom.
    fn stroke_path(&mut self, path: &PathRef, style: &StrokeStyle, paint: &Paint, xf: &Transform2D);

    /// Draws an image into a parallelogram or quadrilateral.
    fn draw_image(&mut self, image: ImageId, mapping: &GradMapping, paint: &Paint);

    /// Begins an offscreen layer: the equivalent of a Xara capture.
    fn push_layer(&mut self, kind: LayerKind, bounds: DeviceRect) -> LayerId;

    /// Composites the layer back.
    fn pop_layer(&mut self, id: LayerId, blend: crate::blend::BlendFamily, opacity: u8);

    /// Ends the frame and returns everything touched, the equivalent of
    /// `GetChangedBBox`.
    fn end_frame(&mut self) -> DirtyRect;

    /// What this backend can do.
    fn capabilities(&self) -> RasterizerCaps;
}
