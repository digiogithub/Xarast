//! The [`Exporter`] trait and the format registry (T11.1.3).
//!
//! The export dialog is generated from [`Capabilities`], never switched on
//! the format, which is what keeps adding a format a day's work.

use crate::model::ExportRequest;
use crate::options::{FormatId, FormatOptions};
use crate::pdf::PdfExporter;
use crate::raster::{JpegExporter, PngExporter, WebPExporter};
use crate::report::{ExportError, ExportReport};
use crate::source::{ExportSource, Progress};
use crate::svg::SvgExporter;

/// Why `.xar` cannot be exported, word for word what the CLI prints.
pub const XAR_EXPORT_REFUSAL: &str = "writing .xar is permanently out of scope: \
     docs/10-architecture.md §3.5 settles it (phase 11 banner). Export to SVG or PDF \
     for interchange, or save as .xarast";

/// What a format can do: the dialog shows a field only when the flag says
/// the format has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Keeps vector content.
    pub vector: bool,
    /// Can carry transparency.
    pub alpha: bool,
    /// Can hold several pages.
    pub multipage: bool,
    /// Can embed fonts.
    pub embeds_fonts: bool,
    /// Has a resolution to set.
    pub has_dpi: bool,
    /// Loses information by design (a quality slider).
    pub lossy: bool,
    /// Same input, same bytes. True for every format in this phase.
    pub deterministic: bool,
    /// The largest side it can store, in pixels.
    pub max_side: u32,
    /// Streams strip by strip, so memory does not grow with the image.
    pub streams: bool,
}

/// An export filter. There is no `.xar` implementation, and there must
/// never be one: architecture §3.5.
pub trait Exporter: std::fmt::Debug + Send + Sync {
    /// Which format.
    fn id(&self) -> FormatId;
    /// File extensions it answers to, lower case, without the dot.
    fn extensions(&self) -> &'static [&'static str];
    /// What the format can do.
    fn capabilities(&self) -> Capabilities;
    /// The options a fresh export starts from.
    fn default_options(&self) -> FormatOptions {
        FormatOptions::default_for(self.id())
    }
    /// Exports `src` as `req` describes, writing `req.destination`
    /// atomically.
    ///
    /// # Errors
    ///
    /// [`ExportError`]; nothing is left at the destination on failure or
    /// cancellation.
    fn export(
        &self,
        src: &dyn ExportSource,
        req: &ExportRequest,
        progress: &dyn Progress,
    ) -> Result<ExportReport, ExportError>;
}

/// Every export filter this build has.
#[derive(Debug)]
pub struct Registry {
    exporters: Vec<Box<dyn Exporter>>,
}

impl Registry {
    /// PNG, JPEG, WebP, PDF and SVG.
    #[must_use]
    pub fn with_builtin() -> Registry {
        Registry {
            exporters: vec![
                Box::new(PngExporter),
                Box::new(JpegExporter),
                Box::new(WebPExporter),
                Box::new(PdfExporter),
                Box::new(SvgExporter),
            ],
        }
    }

    /// The exporter for a format.
    #[must_use]
    pub fn get(&self, id: FormatId) -> Option<&dyn Exporter> {
        self.exporters
            .iter()
            .find(|e| e.id() == id)
            .map(AsRef::as_ref)
    }

    /// The exporter for a file extension, with or without the dot, any
    /// case. `xar` is always `None`.
    #[must_use]
    pub fn for_extension(&self, ext: &str) -> Option<&dyn Exporter> {
        let ext = ext.trim_start_matches('.').to_ascii_lowercase();
        self.exporters
            .iter()
            .find(|e| e.extensions().contains(&ext.as_str()))
            .map(AsRef::as_ref)
    }

    /// Every exporter, in registration order.
    pub fn all(&self) -> impl Iterator<Item = &dyn Exporter> {
        self.exporters.iter().map(AsRef::as_ref)
    }

    /// Runs `req` through the exporter its options name.
    ///
    /// # Errors
    ///
    /// As [`Exporter::export`], or `UnsupportedFormat` when no exporter
    /// is registered for it.
    pub fn export(
        &self,
        src: &dyn ExportSource,
        req: &ExportRequest,
        progress: &dyn Progress,
    ) -> Result<ExportReport, ExportError> {
        let id = req.options.format();
        self.get(id)
            .ok_or(ExportError::UnsupportedFormat {
                id: id.name().to_owned(),
                reason: "not built into this registry",
            })?
            .export(src, req, progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_by_id_and_extension() {
        let r = Registry::with_builtin();
        assert_eq!(r.all().count(), 5);
        for id in [
            FormatId::Png,
            FormatId::Jpeg,
            FormatId::WebP,
            FormatId::Pdf,
            FormatId::Svg,
        ] {
            assert_eq!(r.get(id).map(Exporter::id), Some(id));
            assert_eq!(r.for_extension(id.extension()).map(Exporter::id), Some(id));
        }
        assert_eq!(
            r.for_extension(".JPEG").map(Exporter::id),
            Some(FormatId::Jpeg)
        );
        assert_eq!(
            r.for_extension("Jpg").map(Exporter::id),
            Some(FormatId::Jpeg)
        );
    }

    #[test]
    fn xar_is_never_an_export_format() {
        // Architecture §3.5; phase 11 acceptance criterion 3.
        let r = Registry::with_builtin();
        for ext in ["xar", ".xar", "XAR", "web", "xarast"] {
            assert!(r.for_extension(ext).is_none(), "{ext}");
        }
        for e in r.all() {
            assert!(!e.extensions().iter().any(|x| x.contains("xar")));
        }
        assert!(XAR_EXPORT_REFUSAL.contains("§3.5"));
    }

    #[test]
    fn capabilities_drive_the_dialog() {
        let r = Registry::with_builtin();
        let png = r.get(FormatId::Png).unwrap().capabilities();
        let jpg = r.get(FormatId::Jpeg).unwrap().capabilities();
        let webp = r.get(FormatId::WebP).unwrap().capabilities();
        assert!(png.alpha && !png.lossy && png.has_dpi && png.streams);
        assert!(!jpg.alpha && jpg.lossy && jpg.has_dpi);
        assert!(webp.alpha && !webp.lossy && !webp.has_dpi);
        for c in [png, jpg, webp] {
            assert!(c.deterministic && !c.vector && !c.multipage && !c.embeds_fonts);
        }
        assert_eq!(webp.max_side, crate::webp::MAX_WEBP_SIDE);
        let pdf = r.get(FormatId::Pdf).unwrap().capabilities();
        assert!(pdf.vector && pdf.alpha && pdf.deterministic && !pdf.lossy && !pdf.has_dpi);
        let svg = r.get(FormatId::Svg).unwrap().capabilities();
        assert!(svg.vector && svg.alpha && svg.deterministic && !svg.lossy && !svg.has_dpi);
    }
}
