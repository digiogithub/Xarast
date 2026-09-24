//! Import and export filters.
//!
//! Phase 11 (`docs/phases/phase-11-export-filters.md`) builds the export
//! side. What is here today:
//!
//! | Module | What it owns |
//! |---|---|
//! | [`model`] | [`ExportRequest`], [`ExportArea`], [`ExportSizing`] (pixels ↔ physical size ↔ dpi with one pinned axis), [`Background`] |
//! | [`options`] | Serialisable per-format options with defaults |
//! | [`registry`] | The [`Exporter`] trait, [`Capabilities`] and the [`Registry`] |
//! | [`source`] | [`ExportSource`], the seam the application implements, and [`Progress`] |
//! | [`raster`] | PNG, JPEG and WebP through the deterministic CPU rasteriser |
//! | [`pdf`] | PDF 1.7, vector, with the fidelity ladder (T11.4) |
//! | [`svg`] | SVG 1.1: the `.xarast` profile's mapper in its interchange dialect (W11.3) |
//! | [`fidelity`] | Colour fidelity across formats (W11.5): what the document holds that no output carries |
//! | [`report`] | [`ExportReport`], [`Compromise`], [`ExportError`] |
//!
//! # Three rules
//!
//! 1. **No `.xar` writer, ever** (architecture §3.5). [`Registry`] has no
//!    entry for it, `for_extension("xar")` is `None`, and the CLI prints
//!    [`XAR_EXPORT_REFUSAL`].
//! 2. **Raster export is deterministic.** It goes through
//!    `xarast_render::export` on the CPU backend only; the same request on
//!    the same document produces the same bytes on every run, every
//!    thread count and every machine of an architecture.
//! 3. **No silent loss.** Every lossy decision is a [`Compromise`] in the
//!    report.
//!
//! `xarast-io` sits below `xarast-app`, so it cannot walk a document
//! itself: the application implements [`ExportSource`] and hands it in.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod deflate;
pub mod fidelity;
pub mod jpeg;
pub mod model;
pub mod options;
pub mod pdf;
pub mod png;
pub mod raster;
pub mod registry;
pub mod report;
pub mod source;
pub mod svg;
pub mod webp;

pub use model::{
    Background, ExportArea, ExportRequest, ExportSizing, MAX_EXPORT_PIXELS, SizingAxis, SizingEdit,
    SizingError,
};
pub use options::{
    BlendFidelity, FormatId, FormatOptions, JpegOptions, PDF_RASTERISE_DPI, PdfOptions, PdfVersion,
    PngColour, PngCompression, PngDepth, PngOptions, Subsampling, SvgOptions, SvgResources,
    TextOutput, WebPMode, WebPOptions,
};
pub use pdf::PdfExporter;
pub use raster::{JpegExporter, PngExporter, WebPExporter};
pub use registry::{Capabilities, Exporter, Registry, XAR_EXPORT_REFUSAL};
pub use report::{Compromise, ExportError, ExportReport};
pub use source::{
    CancelFlag, ExportGlyph, ExportSource, NoProgress, Progress, SceneSource, SceneText,
    SourceScene, Stage, TextRun,
};
pub use svg::SvgExporter;
