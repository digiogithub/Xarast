//! One open document, and everything the application knows about it.
//!
//! A [`Session`] owns the document, its session state, its view, its undo
//! history, its scene and its dirty tracking. It is the unit the UI shows
//! in a tab and the unit the headless renderer takes. It is `Send`, so it
//! can be moved to a worker thread — but it is never *shared*: the
//! document lives on one thread and the render thread gets an immutable
//! display list, never the arena (architecture §5).
//!
//! # The Phase 6 seam
//!
//! Opening and saving go through [`FileKind`]. `.xar` import is wired;
//! `.xarast` is not, and [`Session::save`] returns
//! [`SessionError::Unsupported`] rather than pretending. When
//! `xarast-format` lands, two match arms change and nothing else does.
//! Writing `.xar` is a permanent non-goal (architecture §3.5).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use xarast_doc::{Command, CommandBus, Document, EditError};
use xarast_render::{
    DeviceRect, DirtyRect, RenderQuality, Scene, SceneError, SceneStats, ViewParams,
};

use crate::edit::{EditState, SelectMode};
use crate::geometry::DeviceSize;
use crate::intent::{Changed, Intent};
use crate::viewport::Viewport;
use crate::walker::{SceneWalker, WalkStats};

/// Identifies one open document within an [`crate::AppState`].
///
/// Monotonic and never reused, so a stale id from a closed tab resolves
/// to `None` rather than to somebody else's document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocumentId(pub u64);

/// Which container a path names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// The legacy `.xar` format. Read-only, for ever.
    Xar,
    /// The native `.xarast` container. Phase 6.
    Xarast,
    /// Something `xarast-io`'s filters may know about.
    Other,
}

impl FileKind {
    /// Classifies a path by extension, case-insensitively.
    #[must_use]
    pub fn of(path: &Path) -> FileKind {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("xar") => FileKind::Xar,
            Some("xarast") => FileKind::Xarast,
            _ => FileKind::Other,
        }
    }

    /// Whether Xarast can write this kind today.
    #[must_use]
    pub const fn is_writable(self) -> bool {
        // `.xarast` becomes writable in Phase 6; `.xar` never does.
        false
    }
}

/// Why a document could not be opened or saved.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// The file could not be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The path involved.
        path: PathBuf,
        /// What the operating system said.
        #[source]
        source: std::io::Error,
    },
    /// The `.xar` importer refused the file.
    #[error("{path}: {source}")]
    Xar {
        /// The path involved.
        path: PathBuf,
        /// What the importer said.
        #[source]
        source: xarast_xar::XarError,
    },
    /// The format is understood but not implemented yet, or never will
    /// be.
    #[error("{what} is not supported yet ({path})")]
    Unsupported {
        /// What was asked for.
        what: &'static str,
        /// The path involved.
        path: PathBuf,
    },
    /// The walker produced an unbalanced scene, which is a bug here.
    #[error("the scene walker produced an unbalanced scene: {0}")]
    Scene(#[from] SceneError),
    /// A command was refused.
    #[error(transparent)]
    Edit(#[from] EditError),
}

/// Which parts of the frame are stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Dirty {
    /// The device region whose pixels are stale, if it is narrower than
    /// the whole viewport.
    pub region: DirtyRect,
    /// Whether the scene itself has to be rebuilt.
    pub scene: bool,
}

impl Dirty {
    /// Nothing is stale.
    #[must_use]
    pub const fn clean() -> Dirty {
        Dirty {
            region: DirtyRect(None),
            scene: false,
        }
    }

    /// Everything is stale.
    #[must_use]
    pub fn everything(size: DeviceSize) -> Dirty {
        Dirty {
            region: DirtyRect::of(size.to_rect()),
            scene: true,
        }
    }

    /// Whether anything needs doing.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.region.is_empty() && !self.scene
    }

    /// Adds a stale region.
    pub fn add(&mut self, r: DeviceRect) {
        self.region = self.region.union(DirtyRect::of(r));
    }

    /// Marks the scene as needing a rebuild, over the whole viewport.
    pub fn invalidate(&mut self, size: DeviceSize) {
        self.scene = true;
        self.add(size.to_rect());
    }
}

/// An open document plus everything the application keeps about it.
#[derive(Debug)]
pub struct Session {
    /// Which tab this is.
    pub id: DocumentId,
    /// The document itself.
    pub doc: Document,
    /// Selection, active layer, tool state: never serialised, never
    /// undone (architecture §3.5b).
    pub edit: EditState,
    /// The view transform.
    pub viewport: Viewport,
    /// Where it came from, when it came from anywhere.
    pub path: Option<PathBuf>,
    /// The only public route to a mutation.
    pub bus: CommandBus,
    /// How hard the next render should work.
    pub quality: RenderQuality,
    /// Shared with the render thread, which builds its display lists from
    /// it. Rebuilt in place while nobody else holds it.
    scene: Arc<Scene>,
    /// Bumped by every rebuild, so the render thread can tell whether the
    /// pixels it kept were drawn from the scene it is now given.
    scene_epoch: u64,
    /// A document-space superset of what the scene can draw, taken at the
    /// last rebuild: the render thread skips strips outside it.
    scene_ink: crate::geometry::DocRect,
    walker: SceneWalker,
    /// The resolver as of the last walk, shared with the render thread.
    /// Taken lazily and dropped by every rebuild, so a pan (which does not
    /// rebuild) sends the same `Arc` frame after frame.
    resolver_snapshot: Option<Arc<xarast_render::Resolver>>,
    dirty: Dirty,
    modified: bool,
    diagnostics: Vec<String>,
}

impl Session {
    /// A session over the canonical empty document.
    #[must_use]
    pub fn new_empty(id: DocumentId) -> Session {
        Session::adopt(id, Document::new_empty(), None)
    }

    /// A session over a document that came from somewhere else.
    #[must_use]
    pub fn adopt(id: DocumentId, doc: Document, path: Option<PathBuf>) -> Session {
        let edit = EditState::for_document(&doc);
        let mut viewport = Viewport::new(DeviceSize::new(1024, 768));
        viewport.fit_bounds_to(&doc);
        viewport.zoom_to(
            crate::viewport::ZoomTarget::Page,
            &doc,
            xarast_geom::Rect::EMPTY,
        );
        let size = viewport.size();
        Session {
            id,
            doc,
            edit,
            viewport,
            path,
            bus: CommandBus::new(),
            quality: RenderQuality::Final,
            scene: Arc::new(Scene::new()),
            scene_epoch: 0,
            scene_ink: crate::geometry::DocRect::EMPTY,
            walker: SceneWalker::new(),
            resolver_snapshot: None,
            dirty: Dirty::everything(size),
            modified: false,
            diagnostics: Vec::new(),
        }
    }

    /// Opens a file.
    ///
    /// # Errors
    ///
    /// [`SessionError::Io`] when the file cannot be read,
    /// [`SessionError::Xar`] when the importer refuses it, and
    /// [`SessionError::Unsupported`] for a container no phase has
    /// implemented yet.
    pub fn open(id: DocumentId, path: &Path) -> Result<Session, SessionError> {
        let bytes = std::fs::read(path).map_err(|source| SessionError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Session::open_bytes(id, path, &bytes)
    }

    /// Opens bytes that are already in memory, which is what a drop from
    /// the desktop and the test harness both have.
    ///
    /// # Errors
    ///
    /// As [`Session::open`].
    pub fn open_bytes(id: DocumentId, path: &Path, bytes: &[u8]) -> Result<Session, SessionError> {
        match FileKind::of(path) {
            FileKind::Xar => {
                let (doc, report) =
                    xarast_xar::import(bytes, &xarast_xar::ImportOptions::default()).map_err(
                        |source| SessionError::Xar {
                            path: path.to_path_buf(),
                            source,
                        },
                    )?;
                let mut s = Session::adopt(id, doc, Some(path.to_path_buf()));
                s.diagnostics = report
                    .diagnostics
                    .iter()
                    .map(|d| format!("{d:?}"))
                    .collect();
                Ok(s)
            }
            // Phase 6 replaces this arm with a call into `xarast-format`.
            FileKind::Xarast => Err(SessionError::Unsupported {
                what: "opening .xarast",
                path: path.to_path_buf(),
            }),
            // Phase 11 replaces this arm with the `xarast-io` filters.
            FileKind::Other => Err(SessionError::Unsupported {
                what: "this file type",
                path: path.to_path_buf(),
            }),
        }
    }

    /// Saves to the session's own path.
    ///
    /// # Errors
    ///
    /// [`SessionError::Unsupported`] until Phase 6 lands the `.xarast`
    /// writer. Writing `.xar` is a permanent non-goal.
    pub fn save(&mut self) -> Result<(), SessionError> {
        let path = self.path.clone().unwrap_or_default();
        self.save_as(&path)
    }

    /// Saves to a given path.
    ///
    /// # Errors
    ///
    /// As [`Session::save`].
    pub fn save_as(&mut self, path: &Path) -> Result<(), SessionError> {
        Err(SessionError::Unsupported {
            what: match FileKind::of(path) {
                FileKind::Xar => "writing .xar (a permanent non-goal)",
                _ => "saving",
            },
            path: path.to_path_buf(),
        })
    }

    /// Whether the document has changed since it was opened or saved.
    #[must_use]
    pub const fn is_modified(&self) -> bool {
        self.modified
    }

    /// What the importer said about the file, as lines for a non-modal
    /// problem list.
    #[must_use]
    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    /// What the frame still owes.
    #[must_use]
    pub const fn dirty(&self) -> Dirty {
        self.dirty
    }

    /// Declares the whole frame stale.
    pub fn invalidate(&mut self) {
        self.dirty.invalidate(self.viewport.size());
    }

    /// The scene as last built.
    #[must_use]
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// The ramps and images the scene's paints refer to.
    #[must_use]
    pub const fn resolver(&self) -> &xarast_render::Resolver {
        self.walker.resolver()
    }

    /// What the last walk found: pending images, skipped text, and so on.
    #[must_use]
    pub const fn walk_stats(&self) -> WalkStats {
        self.walker.stats()
    }

    /// The view parameters the renderer needs for the current viewport.
    #[must_use]
    pub fn view_params(&self) -> ViewParams {
        ViewParams {
            transform: self.viewport.transform(),
            viewport: self.viewport.device_rect(),
            quality: self.quality,
            dpi: self.viewport.dpi(),
        }
    }

    /// Rebuilds the scene from the document, in place.
    ///
    /// This is the hot path: it reuses the walker's interned ramps, its
    /// image registry and its scene allocation. `dirty` prunes the walk;
    /// `None` walks everything.
    ///
    /// # Errors
    ///
    /// [`SessionError::Scene`] if the recording came out unbalanced,
    /// which would be a bug in the walker.
    pub fn rebuild_scene(&mut self, dirty: Option<DeviceRect>) -> Result<SceneStats, SessionError> {
        // The render thread may still hold the previous scene. Cloning it
        // only to clear it would be waste, so start from an empty one.
        if Arc::get_mut(&mut self.scene).is_none() {
            self.scene = Arc::new(Scene::new());
        }
        // Unique now, so this never clones.
        let scene = Arc::make_mut(&mut self.scene);
        let stats = self.walker.rebuild(
            &self.doc,
            &self.edit,
            &self.viewport,
            self.quality,
            dirty,
            scene,
        )?;
        self.dirty.scene = false;
        self.scene_epoch += 1;
        self.scene_ink = crate::viewport::content_rect(&self.doc);
        self.resolver_snapshot = None;
        Ok(stats)
    }

    /// Whether the scene is stale: the document, the quality or the walker
    /// changed since the last [`Session::rebuild_scene`].
    #[must_use]
    pub const fn needs_scene(&self) -> bool {
        self.dirty.scene
    }

    /// The scene as last built, as a snapshot another thread can hold.
    #[must_use]
    pub fn scene_snapshot(&self) -> Arc<Scene> {
        Arc::clone(&self.scene)
    }

    /// Counts scene rebuilds. Two frames with the same epoch were drawn
    /// from the same scene.
    #[must_use]
    pub const fn scene_epoch(&self) -> u64 {
        self.scene_epoch
    }

    /// The resolver as of the last walk, as a snapshot another thread can
    /// hold. Cloned once per rebuild, not once per frame.
    pub fn resolver_snapshot(&mut self) -> Arc<xarast_render::Resolver> {
        Arc::clone(
            self.resolver_snapshot
                .get_or_insert_with(|| Arc::new(self.walker.resolver().clone())),
        )
    }

    /// Packages the current scene and view as a frame for the render
    /// thread: the scene, the resolver it needs and the view to draw it
    /// for. Never a node. The render thread builds the display list, which
    /// is what lets it draw only the strips a pan exposed.
    ///
    /// Rebuild the scene first when [`Session::dirty`] says it is stale;
    /// this only re-projects the scene that exists.
    ///
    /// `background` is the pasteboard; `page` is the colour the page is
    /// filled with underneath the drawing.
    pub fn frame_job(
        &mut self,
        background: [u8; 4],
        page: [u8; 4],
    ) -> crate::render_thread::FrameJob {
        let view = self.view_params();
        crate::render_thread::FrameJob {
            doc: self.id,
            scene: self.scene_snapshot(),
            scene_epoch: self.scene_epoch,
            ink: {
                let r = crate::viewport::device_rect_of(&self.viewport, self.scene_ink);
                // Two pixels of slack: antialiasing, and a Draft pan
                // snapped by up to half a pixel.
                if r.is_empty() { r } else { r.inflated(2) }
            },
            resolver: self.resolver_snapshot(),
            view,
            background,
            page: Some((
                crate::viewport::device_rect_of(
                    &self.viewport,
                    crate::viewport::page_rect(&self.doc),
                ),
                page,
            )),
            generation: 0,
            cpu_rescale: true,
        }
    }

    /// Runs a command and keeps the session consistent with the result.
    ///
    /// # Errors
    ///
    /// Whatever the command returns. The document is left exactly as it
    /// was.
    pub fn dispatch(&mut self, cmd: &dyn Command) -> Result<&'static str, SessionError> {
        let label = self.bus.dispatch(&mut self.doc, cmd)?;
        self.after_mutation();
        Ok(label)
    }

    /// Undoes the last transaction. Returns its label.
    pub fn undo(&mut self) -> Option<&'static str> {
        let label = self.bus.history_mut().undo(&mut self.doc);
        if label.is_some() {
            self.after_mutation();
        }
        label
    }

    /// Redoes the last undone transaction. Returns its label.
    pub fn redo(&mut self) -> Option<&'static str> {
        let label = self.bus.history_mut().redo(&mut self.doc);
        if label.is_some() {
            self.after_mutation();
        }
        label
    }

    fn after_mutation(&mut self) {
        self.modified = true;
        self.edit.prune(&self.doc);
        self.viewport.fit_bounds_to(&self.doc);
        self.invalidate();
    }

    /// Applies one input intent.
    ///
    /// This is the whole input surface of the application core: the shell
    /// and the UI both call it, and neither has to know what the other
    /// did. The returned [`Changed`] says what the caller now owes —
    /// a present, a scene rebuild, a panel rebuild — so that an idle
    /// frame really is idle.
    ///
    /// # Errors
    ///
    /// Whatever a dispatched command returns.
    pub fn apply(&mut self, intent: Intent) -> Result<Changed, SessionError> {
        let mut changed = Changed::empty();
        match intent {
            Intent::Resize(size) => {
                if size != self.viewport.size() {
                    self.viewport.resize(size);
                    changed |= Changed::VIEW | Changed::CACHE;
                }
            }
            Intent::SetDpi(dpi) => {
                if dpi != self.viewport.dpi() {
                    self.viewport.set_dpi(dpi);
                    changed |= Changed::VIEW | Changed::CACHE;
                }
            }
            Intent::Pan { dx, dy } => {
                self.viewport.pan_by(dx, dy);
                changed |= Changed::VIEW;
            }
            Intent::Zoom { factor, anchor } => {
                self.viewport.zoom_about(factor, anchor);
                changed |= Changed::VIEW | Changed::UI;
            }
            Intent::SetZoom { zoom, anchor } => {
                match anchor {
                    Some(a) => self.viewport.set_zoom_about(zoom, a),
                    None => self.viewport.set_zoom(zoom),
                }
                changed |= Changed::VIEW | Changed::UI;
            }
            Intent::ZoomTo(target) => {
                let sel = self.edit.selection_bounds(&self.doc);
                self.viewport.zoom_to(target, &self.doc, sel);
                changed |= Changed::VIEW | Changed::UI;
            }
            Intent::PointerDown { sample, .. } => {
                self.edit.tool.drag_from = Some(sample.at);
            }
            Intent::PointerMove(_) | Intent::PointerLeft => {}
            Intent::PointerUp { .. } => {
                self.edit.tool.drag_from = None;
            }
            Intent::ModifiersChanged(m) => {
                if m != self.edit.modifiers {
                    self.edit.modifiers = m;
                    changed |= Changed::UI;
                }
            }
            Intent::Select { nodes, mode } => {
                if self.edit.select(nodes, mode) {
                    changed |= Changed::SELECTION | Changed::UI;
                }
            }
            Intent::SelectAll => {
                if self.edit.select_all(&self.doc) {
                    changed |= Changed::SELECTION | Changed::UI;
                }
            }
            Intent::SelectNone => {
                if self.edit.clear_selection() {
                    changed |= Changed::SELECTION | Changed::UI;
                }
            }
            Intent::SetLayerVisible { layer, visible } => {
                self.dispatch(&crate::commands::SetLayerVisible { layer, visible })?;
                changed |= Changed::DOCUMENT | Changed::UI;
            }
            Intent::SetLayerLocked { layer, locked } => {
                self.dispatch(&crate::commands::SetLayerLocked { layer, locked })?;
                changed |= Changed::UI;
            }
            Intent::RenameLayer { layer, name } => {
                self.dispatch(&crate::commands::RenameLayer {
                    layer,
                    name: name.into(),
                })?;
                changed |= Changed::UI;
            }
            Intent::SetActiveLayer(layer) => {
                self.dispatch(&crate::commands::SetActiveLayer { layer })?;
                self.edit.set_active_layer(&self.doc, layer);
                changed |= Changed::UI;
            }
            Intent::Undo => {
                if self.undo().is_some() {
                    changed |= Changed::DOCUMENT | Changed::UI;
                }
            }
            Intent::Redo => {
                if self.redo().is_some() {
                    changed |= Changed::DOCUMENT | Changed::UI;
                }
            }
            Intent::ChooseTool(tool) => {
                if self.edit.tool.active != tool {
                    self.edit.tool.active = tool;
                    changed |= Changed::UI | Changed::SELECTION;
                }
            }
            Intent::MomentaryTool(tool) => {
                if self.edit.tool.momentary != tool {
                    self.edit.tool.momentary = tool;
                    changed |= Changed::UI;
                }
            }
            Intent::SetQuality(q) => {
                if self.quality != q {
                    self.quality = q;
                    changed |= Changed::DOCUMENT | Changed::CACHE;
                }
            }
            Intent::InvalidateAll => {
                self.walker.reset();
                changed |= Changed::CACHE | Changed::DOCUMENT;
            }
        }
        if changed.needs_scene() {
            self.dirty.invalidate(self.viewport.size());
        } else if changed.needs_redraw() {
            self.dirty.add(self.viewport.size().to_rect());
        }
        Ok(changed)
    }

    /// Selects everything inside a device-space rectangle — the marquee.
    ///
    /// Hit testing proper is Phase 7; this is the bounding-box test the
    /// viewer needs to make the selection plumbing real and testable.
    pub fn select_in_rect(&mut self, rect: DeviceRect, mode: SelectMode) -> bool {
        let doc_rect = {
            use crate::geometry::{DevicePoint, DocPointF64Ext};
            let a = self
                .viewport
                .device_to_doc_f64(DevicePoint::new(f64::from(rect.x0), f64::from(rect.y0)))
                .to_doc_point();
            let b = self
                .viewport
                .device_to_doc_f64(DevicePoint::new(f64::from(rect.x1), f64::from(rect.y1)))
                .to_doc_point();
            xarast_geom::Rect::new(a, b)
        };
        let hits: Vec<_> = crate::edit::selectable_objects(&self.doc)
            .filter(|id| {
                let b = crate::viewport::nodes_rect(&self.doc, [*id]);
                !b.is_empty() && doc_rect.contains_rect(b)
            })
            .collect();
        self.edit.select(hits, mode)
    }
}

/// Builds a scene from a session, as the phase's public API names it.
///
/// The `&Session` form allocates a fresh scene and resolver every call;
/// [`Session::rebuild_scene`] is the one to use per frame, because it
/// reuses both. Use this one when a caller has only a shared reference —
/// a thumbnailer, a test, an export.
///
/// # Panics
///
/// Never: an unbalanced scene would be a bug in the walker, and this
/// entry point returns the empty scene rather than propagating it. Use
/// [`Session::rebuild_scene`] when the error matters.
#[must_use]
pub fn build_scene(session: &Session, dirty: Option<DeviceRect>) -> BuiltScene {
    let mut walker = SceneWalker::new();
    let mut scene = Scene::new();
    let stats = walker
        .rebuild(
            &session.doc,
            &session.edit,
            &session.viewport,
            session.quality,
            dirty,
            &mut scene,
        )
        .unwrap_or_default();
    BuiltScene {
        scene,
        resolver: walker.into_resolver(),
        view: session.view_params(),
        stats,
    }
}

/// A scene together with everything needed to render it.
///
/// A [`Scene`] is not self-contained: its paints carry ids into a
/// [`Resolver`](xarast_render::Resolver). Handing the two around
/// together is the difference between a gradient and a transparent hole.
#[derive(Debug)]
pub struct BuiltScene {
    /// The display commands.
    pub scene: Scene,
    /// The ramps and images they refer to.
    pub resolver: xarast_render::Resolver,
    /// The view they were built for.
    pub view: ViewParams,
    /// What the scene contains.
    pub stats: SceneStats,
}

impl std::ops::Deref for BuiltScene {
    type Target = Scene;

    fn deref(&self) -> &Scene {
        &self.scene
    }
}
