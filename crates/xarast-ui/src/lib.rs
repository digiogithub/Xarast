//! The Xarast user interface: canvas, panels, rulers, theming.
//!
//! This crate is a **projection**. It owns no document state: every frame it
//! reads a [`UiModel`] that the application fills in, and it writes its
//! intentions back as [`UiCommand`]s into a [`CommandSink`]. Nothing here
//! mutates a document, a viewport or a selection directly, which is what
//! makes every panel testable with no window, no GPU and no compositor —
//! see the tests next to this crate, all of which run headless.
//!
//! # Boundary with `xarast-app` and `xarast-shell`
//!
//! Phase 5 is written by three hands at once, so the seam is deliberately
//! narrow and lives entirely on this side of it:
//!
//! * [`UiModel`] is a per-frame snapshot the application builds. When
//!   `xarast_app::AppState`, `Viewport` and `DocumentSession` land, the
//!   adapter that fills a `UiModel` from them is a single function; no panel
//!   changes.
//! * [`ViewTransform`] mirrors the application's viewport (document
//!   millipoints → device pixels) because rulers, grid and hit-testing need
//!   the mapping to draw. It is read-only here: pan and zoom are requests
//!   ([`UiCommand::Pan`], [`UiCommand::ZoomAbout`]), never local mutations,
//!   so there is exactly one owner of the view transform and it is not this
//!   crate.
//! * The shell owns the one [`Scale`] of the frame and hands it down. The
//!   canvas never reads a window.
//!
//! # Pass order
//!
//! The canvas region is left transparent by design: the shell paints the
//! document into the same surface *underneath* egui, in the fixed order
//! canvas → overlay → egui, within one command encoder. [`CanvasWidget`]
//! therefore reports the device rectangle it occupies rather than painting
//! pixels of its own.
//!
//! See `docs/phases/phase-05-shell-and-ui.md` and `docs/memory/ui.md`.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![doc(html_no_source)]

pub mod a11y;
pub mod canvas;
pub mod colour_bar;
pub mod colour_field;
pub mod density;
pub mod dialogs;
pub mod grid;
pub mod guides;
pub mod menus;
pub mod model;
pub mod overlay;
pub mod panel;
pub mod panels;
pub mod rulers;
pub mod scale;
pub mod theme;
pub mod toolbar;
pub mod units;
pub mod workspace;

mod serde_mp;

pub use canvas::{CanvasInput, CanvasNavigation, CanvasResponse, CanvasWidget};
pub use grid::GridSettings;
pub use guides::{Axis, Guide};
pub use menus::AppMenu;
pub use model::{
    CommandSink, DocumentView, EditingView, LayerInfo, LayerKey, PaletteEntry, RenderQuality,
    StatusInfo, UiCommand, UiModel, ViewTransform, ZoomTarget,
};
pub use overlay::{HandleKind, OverlayItem, OverlayPainter};
pub use panel::{LayoutState, Panel, PanelCtx, PanelId, UiHost, UiOutput};
pub use rulers::{Ruler, Tick};
pub use scale::{DeviceRect, Scale};
pub use theme::{ColorScheme, Theme, ThemeTokens};
pub use toolbar::{InfobarRow, ToolPalette};
pub use units::{Unit, format_measure, parse_measure};
pub use workspace::{Workspace, WorkspaceOutput};
