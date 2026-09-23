//! Rendering a session to pixels with no window, no GPU and no
//! compositor.
//!
//! This is not a convenience for the command line: it is how rendering is
//! tested. The CPU backend is the deterministic path (`render.md`), so a
//! headless render of a corpus file is reproducible, diffable and cheap
//! enough to run in CI on a machine with no `/dev/dri` at all.

use std::path::Path;

use xarast_color::Rgba8;
use xarast_render::{
    CpuBackend, CpuConfig, DirtyRect, DisplayList, FrameTimings, RenderQuality, SceneStats,
    Surface, ViewParams,
};

use crate::geometry::{DeviceSize, DocRect};
use crate::session::Session;
use crate::walker::WalkStats;

/// What a headless render frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HeadlessFrame {
    /// The session's own viewport, resized to the output: what the window
    /// would show.
    Session,
    /// The drawing, fitted with the viewport's margin; the page when
    /// nothing is drawn ([`crate::viewport::drawing_or_page_rect`]).
    FitDrawing,
    /// This document rectangle, fitted with the viewport's margin.
    Fit(DocRect),
    /// A fixed zoom (1.0 is 100 %) centred on the middle of a document
    /// rectangle. An empty rectangle keeps the session's centre.
    Fixed {
        /// The zoom factor.
        zoom: f64,
        /// What to centre on.
        centre_on: DocRect,
    },
}

/// How a headless render should be set up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeadlessOptions {
    /// The output size in pixels.
    pub size: DeviceSize,
    /// How hard to work.
    pub quality: RenderQuality,
    /// What to clear to before drawing. Opaque white by default:
    /// a page is white, and a PNG of a drawing on transparency is hard
    /// to look at.
    pub background: Rgba8,
    /// What to frame.
    pub frame: HeadlessFrame,
    /// Device pixels per inch at 100 %; `None` keeps the session's.
    pub dpi: Option<f64>,
    /// Use the bit-reproducible backend configuration. On by default,
    /// because a headless render that is not reproducible is not a test.
    pub deterministic: bool,
}

impl Default for HeadlessOptions {
    fn default() -> HeadlessOptions {
        HeadlessOptions {
            size: DeviceSize::new(1024, 768),
            quality: RenderQuality::Final,
            background: Rgba8::WHITE,
            frame: HeadlessFrame::FitDrawing,
            dpi: None,
            deterministic: true,
        }
    }
}

/// What one headless render did.
#[derive(Debug, Clone)]
pub struct HeadlessResult {
    /// The pixels.
    pub surface: Surface,
    /// Where the time went.
    pub timings: FrameTimings,
    /// How many commands the display list held.
    pub commands: usize,
    /// The view the frame was drawn at: transform, size, quality, dpi.
    pub view: ViewParams,
    /// The zoom the frame was drawn at (1.0 is 100 %).
    pub zoom: f64,
    /// What the walk drew.
    pub scene: SceneStats,
    /// What the walk could not draw: text, quick shapes, images, live
    /// effects, unsupported clips.
    pub walk: WalkStats,
    /// Font families the text asked for that were replaced, each once.
    pub font_substitutions: Vec<xarast_text::FontSubstitution>,
}

/// Why a headless render failed.
#[derive(Debug, thiserror::Error)]
pub enum HeadlessError {
    /// The scene could not be built.
    #[error(transparent)]
    Session(#[from] crate::session::SessionError),
    /// The walker produced an unbalanced scene.
    #[error(transparent)]
    Scene(#[from] xarast_render::SceneError),
    /// The backend refused the surface.
    #[error(transparent)]
    Backend(#[from] xarast_render::BackendError),
    /// The PNG could not be written.
    #[error(transparent)]
    Png(#[from] xarast_render::golden::GoldenError),
}

/// Renders a session into a fresh surface.
///
/// The session's own viewport is left untouched: a headless render must
/// not move the user's view, and a thumbnailer running on a background
/// thread must not race one that does.
///
/// # Errors
///
/// [`HeadlessError`] when the walk, the backend or the surface refuses.
pub fn render(session: &Session, opts: &HeadlessOptions) -> Result<HeadlessResult, HeadlessError> {
    render_with_fonts(session, opts, None)
}

/// [`render`] laying text out with `fonts` instead of the process's shared
/// service: golden renders and tests pass pinned fonts, so that the
/// output never depends on the machine's (`docs/memory/text.md`).
///
/// # Errors
///
/// As [`render`].
pub fn render_with_fonts(
    session: &Session,
    opts: &HeadlessOptions,
    fonts: Option<std::sync::Arc<crate::fonts::FontService>>,
) -> Result<HeadlessResult, HeadlessError> {
    let mut view = session.viewport.clone();
    if let Some(dpi) = opts.dpi {
        view.set_dpi(dpi);
    }
    view.resize(opts.size);
    match opts.frame {
        HeadlessFrame::Session => {}
        HeadlessFrame::FitDrawing => {
            view.fit_rect(crate::viewport::drawing_or_page_rect_with(
                &session.doc,
                fonts.as_deref(),
            ));
        }
        HeadlessFrame::Fit(r) => view.fit_rect(r),
        HeadlessFrame::Fixed { zoom, centre_on } => {
            view.set_zoom(zoom);
            if !centre_on.is_empty() {
                view.set_centre(centre_on.to_kurbo().center());
            }
        }
    }

    let mut walker = match fonts {
        Some(f) => crate::walker::SceneWalker::with_fonts(f),
        None => crate::walker::SceneWalker::new(),
    };
    let mut scene = xarast_render::Scene::new();
    let scene_stats = walker.rebuild(
        &session.doc,
        &session.edit,
        &view,
        opts.quality,
        None,
        &mut scene,
    )?;

    let params = xarast_render::ViewParams {
        transform: view.transform(),
        viewport: opts.size.to_rect(),
        quality: opts.quality,
        dpi: view.dpi(),
    };
    let dl = DisplayList::build(&scene, &params, &DirtyRect::of(opts.size.to_rect()));

    let mut surface = Surface::filled(
        opts.size.width.max(1),
        opts.size.height.max(1),
        [
            opts.background.r,
            opts.background.g,
            opts.background.b,
            opts.background.a,
        ],
    );
    let cfg = if opts.deterministic {
        CpuConfig::deterministic()
    } else {
        CpuConfig::interactive()
    };
    let mut backend = CpuBackend::new(cfg);
    let timings = backend.render(&dl, walker.resolver(), &mut surface)?;
    Ok(HeadlessResult {
        surface,
        timings,
        commands: dl.len(),
        view: params,
        zoom: view.zoom(),
        scene: scene_stats,
        walk: walker.stats(),
        font_substitutions: walker.font_substitutions().to_vec(),
    })
}

/// Renders a session and writes it to a PNG.
///
/// # Errors
///
/// As [`render`], plus [`HeadlessError::Png`] when the file cannot be
/// written.
pub fn render_to_png(
    session: &Session,
    opts: &HeadlessOptions,
    path: &Path,
) -> Result<HeadlessResult, HeadlessError> {
    let out = render(session, opts)?;
    xarast_render::golden::write_png(&out.surface, path)?;
    Ok(out)
}

/// Renders a file straight to a PNG: the whole headless path in one call.
///
/// # Errors
///
/// As [`render_to_png`], plus whatever opening the document returns.
pub fn convert_to_png(
    input: &Path,
    output: &Path,
    opts: &HeadlessOptions,
) -> Result<HeadlessResult, HeadlessError> {
    let session = Session::open(crate::session::DocumentId(0), input)?;
    render_to_png(&session, opts, output)
}
