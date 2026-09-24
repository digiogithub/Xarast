//! What an exporter needs from the application.
//!
//! `xarast-io` sits *below* `xarast-app` in the crate graph (the app
//! depends on it), so it cannot walk a document into a scene or know what
//! is selected. [`ExportSource`] is that seam: the application implements
//! it over its session — the scene walker, the selection, the page — and
//! every exporter consumes it. The export dialog and `xarast-cli export`
//! therefore drive the same code.

use std::sync::Arc;

use xarast_color::Rgba8;
use xarast_geom::Rect;
use xarast_render::{PathRef, RenderQuality, Resolver, Scene};
use xarast_text::{FaceId, FontDb};

use crate::model::ExportArea;
use crate::report::{Compromise, ExportError};

/// A scene built for export, with what it refers to.
#[derive(Debug)]
pub struct SourceScene {
    /// The whole document's display commands, in document space.
    pub scene: Scene,
    /// The ramps and images the scene refers to.
    pub resolver: Resolver,
    /// What the build could not draw, for the report.
    pub compromises: Vec<Compromise>,
    /// The glyphs behind the text the scene draws as outlines, for
    /// formats that embed fonts (PDF, T11.4.7). `None`: text stays
    /// outlines.
    pub text: Option<SceneText>,
}

/// The text of a scene: each story run the scene fills as one outline
/// path, with the glyphs that path is made of.
#[derive(Debug, Clone)]
pub struct SceneText {
    /// The database the glyphs' faces belong to.
    pub fonts: Arc<FontDb>,
    /// The runs, in the order the scene paints them.
    pub runs: Vec<TextRun>,
}

/// One run of glyphs the scene draws as a single outline path.
#[derive(Debug, Clone)]
pub struct TextRun {
    /// The path the scene fills (and strokes) for the run: the very
    /// allocation its `Fill` / `Stroke` ops hold, which is how an exporter
    /// finds the run behind an op.
    pub path: PathRef,
    /// The glyphs, in the order the layout placed them.
    pub glyphs: Arc<[ExportGlyph]>,
    /// What the path holds besides the glyphs (the underline), in the
    /// path's space.
    pub decoration: Option<Arc<kurbo::BezPath>>,
}

/// One placed glyph.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportGlyph {
    /// Its face.
    pub face: FaceId,
    /// Its glyph id in the face.
    pub id: u32,
    /// Font units (y up) to the path's space: what drew its outline.
    pub transform: kurbo::Affine,
    /// Drawn at a variation instance other than the face's default.
    pub variable: bool,
    /// The characters the glyph stands for: its cluster's text on the
    /// cluster's first glyph, empty on the others.
    pub text: Arc<str>,
}

/// A document as an exporter sees it.
pub trait ExportSource {
    /// The document rectangle (millipoints, y up) an area stands for.
    ///
    /// # Errors
    ///
    /// [`ExportError::Area`] when it does not exist: nothing selected, no
    /// such page, nothing drawn.
    fn resolve_area(&self, area: &ExportArea) -> Result<Rect, ExportError>;

    /// The paper colour, for [`Background::Paper`](crate::Background) and
    /// for flattening. White unless the document says otherwise.
    fn paper_colour(&self) -> Rgba8 {
        Rgba8::WHITE
    }

    /// Builds the scene to export: the whole document, no culling, no
    /// selection handles or other overlays.
    ///
    /// # Errors
    ///
    /// [`ExportError::Scene`].
    fn build_scene(&self, quality: RenderQuality) -> Result<SourceScene, ExportError>;

    /// The document itself, for exporters that map the model rather than
    /// draw a scene (SVG shares the `.xarast` profile's mapper, W11.3).
    /// `None` when the source only has a scene; such exporters then refuse.
    fn document(&self) -> Option<&xarast_doc::Document> {
        None
    }

    /// Where the application lays text out, so that SVG text sits where
    /// Xarast draws it (`xarast_format::svg::TextPlacer`). `None`: each
    /// text line starts at its story's origin.
    fn svg_text_placer(&self) -> Option<xarast_format::svg::Placer> {
        None
    }

    /// A copy of the document with text stories replaced by their glyph
    /// outlines (the application's "Convert to shapes", applied to the
    /// copy): every story when `all`, otherwise only the stories drawn
    /// with a face whose licence (`OS/2.fsType`) forbids embedding it
    /// (T9.6.5). Returns the copy and the families of those refusing
    /// faces. `None` when no story would change, or the source cannot lay
    /// text out: exporters then write the document as it is.
    fn text_as_outlines(&self, all: bool) -> Option<(xarast_doc::Document, Vec<Arc<str>>)> {
        let _ = all;
        None
    }
}

/// A prebuilt scene with a fixed area: tests, benchmarks, and callers that
/// already hold a scene.
#[derive(Debug)]
pub struct SceneSource<'a> {
    /// The scene.
    pub scene: &'a Scene,
    /// Its resolver.
    pub resolver: &'a Resolver,
    /// What every area except [`ExportArea::Rect`] resolves to.
    pub area: Rect,
    /// The paper colour.
    pub paper: Rgba8,
}

impl ExportSource for SceneSource<'_> {
    fn resolve_area(&self, area: &ExportArea) -> Result<Rect, ExportError> {
        match area {
            ExportArea::Rect(r) => Ok(*r),
            _ => Ok(self.area),
        }
    }

    fn paper_colour(&self) -> Rgba8 {
        self.paper
    }

    fn build_scene(&self, _quality: RenderQuality) -> Result<SourceScene, ExportError> {
        Ok(SourceScene {
            scene: self.scene.clone(),
            resolver: self.resolver.clone(),
            compromises: Vec::new(),
            text: None,
        })
    }
}

/// Progress reporting and cancellation. `Sync`: it is polled from the
/// render threads.
pub trait Progress: Sync {
    /// Called as work advances: `stage` names it, `fraction` is 0 to 1
    /// within it.
    fn report(&self, _stage: Stage, _fraction: f32) {}

    /// Whether the caller wants the export abandoned. Polled before every
    /// band, so the latency is one band's work.
    fn cancelled(&self) -> bool {
        false
    }
}

/// What an export is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Walking the document.
    Scene,
    /// Rasterising.
    Render,
    /// Encoding and writing.
    Encode,
    /// The optional optimisation pass.
    Optimise,
}

/// No reporting, never cancelled.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoProgress;

impl Progress for NoProgress {}

/// Cancellation from an atomic flag another thread sets.
#[derive(Debug, Default)]
pub struct CancelFlag(pub std::sync::atomic::AtomicBool);

impl CancelFlag {
    /// Asks the export to stop.
    pub fn cancel(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Progress for CancelFlag {
    fn cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}
