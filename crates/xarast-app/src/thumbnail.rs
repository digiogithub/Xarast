//! `thumbnail.png` and the per-spread previews, over the CPU renderer
//! (`research/06 §3.2.5`, XARA-T-0082).
//!
//! The format crate defines the contract ([`ThumbnailProvider`]) and never
//! renders; this is the implementation every save hands it. It frames the
//! active spread's first page exactly — no fit margin, the page edge is the
//! image edge — fills it with the page colour and draws the document over
//! it with the CPU backend (the parallel configuration by default; the
//! single-threaded reference one on request).

use xarast_doc::Document;
use xarast_format::ThumbnailProvider;
use xarast_format::thumbnail::{PREVIEW_PX, THUMBNAIL_PX, ThumbnailError};
use xarast_geom::Mp;
use xarast_render::{CpuBackend, CpuConfig, DirtyRect, DisplayList, RenderQuality, Scene, Surface};

use crate::decoded::DecodedImages;
use crate::edit::EditState;
use crate::geometry::{DeviceSize, DocPointF, DocRect};
use crate::viewport::Viewport;
use crate::walker::SceneWalker;

/// Renders thumbnails and previews with the CPU backend.
#[derive(Debug, Clone, Copy)]
pub struct CpuThumbnails {
    /// The colour under the page, straight RGBA. The page is opaque white
    /// unless a document says otherwise.
    pub page: [u8; 4],
    /// Render with the single-threaded reference configuration rather than
    /// the parallel one (2.7× slower on ProbeX16: 456 against 167 ms).
    pub deterministic: bool,
}

impl Default for CpuThumbnails {
    fn default() -> CpuThumbnails {
        CpuThumbnails {
            page: [0xff, 0xff, 0xff, 0xff],
            deterministic: false,
        }
    }
}

impl CpuThumbnails {
    /// Renders `area` of `doc` into a surface whose longer side is
    /// `max_px`, the area filling it edge to edge.
    ///
    /// # Errors
    ///
    /// When the area is empty or the renderer fails.
    pub fn render(
        &self,
        doc: &Document,
        area: DocRect,
        max_px: u32,
    ) -> Result<Surface, ThumbnailError> {
        self.render_with(doc, area, max_px, None)
    }

    /// [`CpuThumbnails::render`] with the document's decoded bitmaps
    /// ([`crate::decoded`]), so that none is decoded again.
    ///
    /// # Errors
    ///
    /// As [`CpuThumbnails::render`].
    pub fn render_with(
        &self,
        doc: &Document,
        area: DocRect,
        max_px: u32,
        images: Option<&DecodedImages>,
    ) -> Result<Surface, ThumbnailError> {
        if area.is_empty() || max_px == 0 {
            return Err(ThumbnailError("the document has no page to show".into()));
        }
        let (lo, hi) = (area.lo.to_f64(), area.hi.to_f64());
        let (dw, dh) = (hi.0 - lo.0, hi.1 - lo.1);
        let longer = dw.max(dh);
        let px = f64::from(max_px);
        // At least one pixel either way, however thin the page.
        let size = DeviceSize::new(
            ((dw / longer * px).round() as u32).clamp(1, max_px),
            ((dh / longer * px).round() as u32).clamp(1, max_px),
        );
        let mut view = Viewport::new(size);
        let per_mp = view.dpi() / f64::from(Mp::PER_INCH);
        view.set_zoom(f64::from(size.width) / (dw * per_mp));
        view.set_centre(DocPointF::new((lo.0 + hi.0) * 0.5, (lo.1 + hi.1) * 0.5));

        let edit = EditState::for_document(doc);
        let mut walker = match images {
            Some(i) => SceneWalker::new().with_decoded_images(i.clone()),
            None => SceneWalker::new(),
        };
        let mut scene = Scene::new();
        walker
            .rebuild(doc, &edit, &view, RenderQuality::Final, None, &mut scene)
            .map_err(|e| ThumbnailError(e.to_string()))?;
        let params = xarast_render::ViewParams {
            transform: view.transform(),
            viewport: size.to_rect(),
            quality: RenderQuality::Final,
            dpi: view.dpi(),
        };
        let dl = DisplayList::build(&scene, &params, &DirtyRect::of(size.to_rect()));
        let mut surface = Surface::filled(size.width, size.height, self.page);
        let config = if self.deterministic {
            CpuConfig::deterministic()
        } else {
            CpuConfig::interactive()
        };
        CpuBackend::new(config)
            .render(&dl, walker.resolver(), &mut surface)
            .map_err(|e| ThumbnailError(e.to_string()))?;
        Ok(surface)
    }

    fn png(&self, doc: &Document, area: DocRect, max_px: u32) -> Result<Vec<u8>, ThumbnailError> {
        let surface = self.render(doc, area, max_px)?;
        xarast_render::golden::encode_png(&surface).map_err(|e| ThumbnailError(e.to_string()))
    }
}

impl ThumbnailProvider for CpuThumbnails {
    fn thumbnail(&self, doc: &Document, max_px: u32) -> Result<Vec<u8>, ThumbnailError> {
        self.png(doc, crate::viewport::page_rect(doc), max_px)
    }

    fn preview(&self, doc: &Document, spread: u32, max_px: u32) -> Result<Vec<u8>, ThumbnailError> {
        // Only the active spread can be framed today; the documents the
        // application opens have one.
        if spread != 1 {
            return Err(ThumbnailError(format!(
                "spread {spread} is not rendered yet"
            )));
        }
        self.png(doc, crate::viewport::spread_rect(doc), max_px)
    }
}

/// The recommended thumbnail of a document, or `None` when it cannot be
/// made (a document with no page). A save never fails for want of one.
#[must_use]
pub fn thumbnail_png(doc: &Document) -> Option<Vec<u8>> {
    thumbnail_png_with(doc, None)
}

/// [`thumbnail_png`] with the document's decoded bitmaps
/// ([`crate::Session::decoded_images`]): no bitmap is decoded again.
#[must_use]
pub fn thumbnail_png_with(doc: &Document, images: Option<&DecodedImages>) -> Option<Vec<u8>> {
    let t = CpuThumbnails::default();
    let surface = t
        .render_with(doc, crate::viewport::page_rect(doc), THUMBNAIL_PX, images)
        .ok()?;
    xarast_render::golden::encode_png(&surface).ok()
}

/// The recommended preview size, re-exported for callers that ask for one.
pub const PREVIEW_SIZE: u32 = PREVIEW_PX;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_thumbnail_is_a_valid_rgba_png_of_the_page_shape() {
        let doc = Document::new_empty();
        let png = CpuThumbnails::default()
            .thumbnail(&doc, THUMBNAIL_PX)
            .unwrap();
        let h =
            xarast_format::thumbnail::check_png(&png, xarast_format::thumbnail::THUMBNAIL_MAX_PX)
                .unwrap();
        assert_eq!(h.colour_type, 6);
        assert_eq!(h.width.max(h.height), THUMBNAIL_PX);
        let page = crate::viewport::page_rect(&doc);
        let (lo, hi) = (page.lo.to_f64(), page.hi.to_f64());
        let aspect = (hi.0 - lo.0) / (hi.1 - lo.1);
        let got = f64::from(h.width) / f64::from(h.height);
        assert!((aspect - got).abs() < 0.02, "{aspect} vs {got}");
        // Deterministic: the same document gives the same bytes.
        assert_eq!(
            png,
            CpuThumbnails::default()
                .thumbnail(&doc, THUMBNAIL_PX)
                .unwrap()
        );
    }

    #[test]
    fn an_empty_page_is_the_page_colour() {
        let doc = Document::new_empty();
        let s = CpuThumbnails::default()
            .render(&doc, crate::viewport::page_rect(&doc), 64)
            .unwrap();
        assert!(
            s.data()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [0xff, 0xff, 0xff, 0xff])
        );
    }
}
