//! The input-intent vocabulary: the one thing the shell and the UI both
//! speak.
//!
//! `xarast-shell` turns platform events into [`Intent`]s; `xarast-ui`
//! raises the same [`Intent`]s from menus, panels and buttons. Neither
//! depends on the other, and the application core never learns whether a
//! zoom came from a scroll wheel, a pinch gesture or a menu item.
//!
//! An intent is *what the user asked for*, never *what should happen to
//! the document*. The translation from intent to a `xarast_doc::Command`
//! is [`crate::commands`]'s job, and it is the only place a mutation is
//! created.

use xarast_doc::NodeId;

use crate::edit::{Modifiers, SelectMode, ToolId};
use crate::geometry::{DevicePoint, DeviceSize};
use crate::viewport::ZoomTarget;

/// Which pointer button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerButton {
    /// The primary button: draw, select, drag.
    Primary,
    /// The secondary button: context menu, and the original's
    /// "adjust" click.
    Secondary,
    /// The middle button, which pans.
    Middle,
}

/// One pointer sample, already in the canvas's device space.
///
/// The shell subtracts the canvas origin before it gets here, so an
/// intent is never relative to a window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerSample {
    /// Where, in device pixels.
    pub at: DevicePoint,
    /// Stylus pressure, `0..=1`, when the device reports it.
    pub pressure: Option<f32>,
    /// Milliseconds since the shell started, for velocity and for
    /// coalescing checks.
    pub time_ms: u64,
}

impl PointerSample {
    /// A sample with no stylus data.
    #[must_use]
    pub const fn at(at: DevicePoint) -> PointerSample {
        PointerSample {
            at,
            pressure: None,
            time_ms: 0,
        }
    }
}

/// What the user asked for.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Intent {
    // ── the view ──────────────────────────────────────────────────────
    /// The canvas changed size, in device pixels.
    Resize(DeviceSize),
    /// The device resolution changed, in dots per inch. The shell is the
    /// single owner of this number (`phase-05 §W3`).
    SetDpi(f64),
    /// Scroll or drag the view by a device-space displacement.
    Pan {
        /// Pixels right.
        dx: f64,
        /// Pixels down.
        dy: f64,
    },
    /// Multiply the zoom, keeping the document point under `anchor` fixed.
    Zoom {
        /// The multiplier; above 1 zooms in.
        factor: f64,
        /// The point to keep fixed.
        anchor: DevicePoint,
    },
    /// Set the zoom outright, keeping `anchor` fixed. `1.0` is 100 %.
    SetZoom {
        /// The new zoom.
        zoom: f64,
        /// The point to keep fixed; the viewport centre when `None`.
        anchor: Option<DevicePoint>,
    },
    /// Frame one of the standard targets.
    ZoomTo(ZoomTarget),

    // ── the pointer ───────────────────────────────────────────────────
    /// A button went down.
    PointerDown {
        /// Which button.
        button: PointerButton,
        /// Where.
        sample: PointerSample,
    },
    /// The pointer moved. Every sample of the frame is delivered, not
    /// just the last: coalescing loses the shape of a fast stroke.
    PointerMove(PointerSample),
    /// A button came up.
    PointerUp {
        /// Which button.
        button: PointerButton,
        /// Where.
        sample: PointerSample,
    },
    /// The pointer left the canvas.
    PointerLeft,
    /// The modifier keys changed, possibly with no pointer event at all.
    ModifiersChanged(Modifiers),

    // ── the selection ─────────────────────────────────────────────────
    /// Change the selection.
    Select {
        /// The nodes involved.
        nodes: Vec<NodeId>,
        /// How they combine with what is already selected.
        mode: SelectMode,
    },
    /// Select every object on every visible, unlocked layer.
    SelectAll,
    /// Select nothing.
    SelectNone,

    // ── the document ──────────────────────────────────────────────────
    /// Show or hide a layer.
    SetLayerVisible {
        /// The layer.
        layer: NodeId,
        /// Whether it is drawn.
        visible: bool,
    },
    /// Lock or unlock a layer.
    SetLayerLocked {
        /// The layer.
        layer: NodeId,
        /// Whether it is locked against editing.
        locked: bool,
    },
    /// Rename a layer.
    RenameLayer {
        /// The layer.
        layer: NodeId,
        /// Its new name.
        name: String,
    },
    /// Make a layer the one new objects go onto.
    SetActiveLayer(NodeId),
    /// Undo the last transaction.
    Undo,
    /// Redo the last undone transaction.
    Redo,
    /// Delete the selected objects.
    DeleteSelection,
    /// Group the selected objects (`Ctrl+G`).
    Group,
    /// Ungroup the selected groups (`Ctrl+U`).
    Ungroup,
    /// Change the selection's z-order.
    Arrange(crate::structure::ZOrder),
    /// Align or distribute the selected objects.
    Align(crate::structure::AlignSpec),
    /// Duplicate the selection, offset (`Ctrl+D`).
    Duplicate,
    /// Copy the selection to the clipboard (`Ctrl+C`).
    Copy,
    /// Copy the selection to the clipboard and delete it (`Ctrl+X`).
    Cut,
    /// Paste from the clipboard: into the middle of the view, or, in
    /// place, at the coordinates it was copied from. The core asks the
    /// shell for the clipboard's text ([`PlatformRequest::ReadClipboard`]);
    /// the answer comes back as [`Intent::PasteText`].
    Paste {
        /// At the original coordinates (`Ctrl+Shift+V`).
        in_place: bool,
    },
    /// The shell's answer to [`PlatformRequest::ReadClipboard`]: the
    /// clipboard's text, or `None` when there is no clipboard to read (the
    /// application's own last copy is pasted then).
    PasteText {
        /// The text.
        text: Option<String>,
        /// As in [`Intent::Paste`].
        in_place: bool,
    },
    /// `Esc`: cancel the gesture in flight, or, when there is none,
    /// select nothing (`research/04 §4.1`).
    Cancel,
    /// Sets one of the current attributes, which every object created
    /// from now on is given (`phase-07` T2.7).
    SetCurrentAttribute(xarast_doc::AttrValue),
    /// A value typed into a field of the active tool's infobar.
    InfobarEdit {
        /// Which field.
        field: crate::tool::InfobarField,
        /// The value, already parsed from its units.
        value: crate::tool::InfobarValue,
    },
    /// A frame tick while a drag holds the pointer at the canvas edge:
    /// scroll the view one step and carry the gesture along. The shell
    /// sends it while [`crate::Session::wants_autoscroll`] says so.
    AutoScroll,

    // ── snapping, grid and guides ─────────────────────────────────────
    /// Switch one kind of snapping on or off. Works in the middle of a
    /// drag, which re-evaluates at once (`research/04 §4.4`).
    ToggleSnap(crate::snap::SnapKind),
    /// Show or hide the grid.
    ToggleGrid,
    /// Show or hide the guides.
    ToggleGuides,
    /// Add, move or delete a guide, or change the grid: an undoable
    /// document edit (`phase-07` T8.2, T8.6).
    Guides(crate::snap::GuideOp),

    /// Open a dialog or panel of the interface (the core only relays it,
    /// as [`PlatformRequest::ShowDialog`]).
    ShowDialog(Dialog),

    // ── the tools ─────────────────────────────────────────────────────
    /// Choose a tool.
    ChooseTool(ToolId),
    /// Hold a tool for as long as a key is down, then fall back
    /// (`research/04 §4.9`).
    MomentaryTool(Option<ToolId>),
    /// A command for the tool in force: the shape editor's path
    /// operations, the pen's Enter.
    ToolAction(crate::tool::ToolAction),
    /// Turn the selected rectangles, ellipses and quick shapes into
    /// editable paths.
    ConvertToShapes,

    // ── rendering ─────────────────────────────────────────────────────
    /// Set the render quality. The shell drops to
    /// [`RenderQuality::Draft`](xarast_render::RenderQuality::Draft)
    /// while interacting and raises it again after the idle timer.
    SetQuality(xarast_render::RenderQuality),
    /// Throw away every cached pixel and rebuild the scene.
    InvalidateAll,

    // ── the application ───────────────────────────────────────────────
    //
    // These act on the set of open documents or on the platform rather
    // than on the active document, so `AppState::apply` handles them and a
    // `Session` ignores them. The platform ones (`ShowOpenDialog`, `Quit`)
    // are queued as `PlatformRequest`s for the shell to carry out: the core
    // never shows a dialog or ends the process itself.
    /// Ask the platform for a file to open (File › Open…). The answer, if
    /// the user picks something, comes back as [`Intent::OpenFile`].
    ShowOpenDialog,
    /// Open a file, replacing the current document. A file that fails to
    /// open leaves the current document where it was.
    OpenFile(std::path::PathBuf),
    /// Close the active document.
    CloseDocument,
    /// Forget the recently opened files.
    ClearRecent,
    /// End the application. Asks first about unsaved changes.
    Quit,
    /// File › Save: write the active document to its own `.xarast`, or ask
    /// for a name when it has none (an import, a new or read-only one).
    Save,
    /// File › Save As…: ask for a name, then save there.
    SaveAs,
    /// The name the save dialog came back with. A missing or foreign
    /// extension becomes `.xarast`.
    SaveTo(std::path::PathBuf),
    /// The save dialog was dismissed or failed: whatever was waiting on the
    /// save (a close, a quit) is dropped.
    SaveDialogClosed,
    /// The answer to the question [`crate::AppState::prompt`] is asking.
    AnswerPrompt(crate::prompt::PromptAnswer),
}

/// Something only the platform layer can do, queued by
/// [`crate::AppState::apply`] and carried out by the shell.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlatformRequest {
    /// Show a file chooser for opening a document.
    ShowOpenDialog,
    /// End the application.
    Quit,
    /// Show a file chooser for saving a document. The answer comes back as
    /// [`Intent::SaveTo`], or [`Intent::SaveDialogClosed`].
    ShowSaveDialog {
        /// The dialog title ("Save As").
        title: String,
        /// The file name to offer, with its `.xarast` extension.
        file_name: String,
        /// Where to start, when the document has a directory.
        directory: Option<std::path::PathBuf>,
    },
    /// Put this text on the system clipboard: a copy's SVG flavour.
    SetClipboardText(String),
    /// Open a dialog or panel of the interface.
    ShowDialog(Dialog),
    /// Read the system clipboard's text and answer with
    /// [`Intent::PasteText`].
    ReadClipboard {
        /// Passed back in the answer.
        in_place: bool,
    },
}

/// A dialog or panel of the interface the core can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Dialog {
    /// Arrange › Alignment…
    Align,
}

bitflags::bitflags! {
    /// What an [`Intent`] changed, and therefore what the caller has to
    /// redo.
    ///
    /// Returned by [`crate::Session::apply`]. The shell uses it to decide
    /// between "present the same pixels again", "re-run the walker" and
    /// "rebuild every panel", which is the difference between an idle
    /// frame and a stutter.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct Changed: u8 {
        /// The view transform moved: the scene is still valid, the
        /// display list is not.
        const VIEW = 1 << 0;
        /// The selection or a control-point overlay changed: only the
        /// overlay needs redrawing.
        const SELECTION = 1 << 1;
        /// The document changed: the scene must be rebuilt.
        const DOCUMENT = 1 << 2;
        /// Panels must be rebuilt (layer list, undo labels, tool state).
        const UI = 1 << 3;
        /// Every cached pixel is stale.
        const CACHE = 1 << 4;
        /// A different document (or none) has the canvas: the caller
        /// re-titles the window, re-sizes the new view and frames it.
        const ACTIVE = 1 << 5;
    }
}

impl Changed {
    /// Whether anything at all happened.
    #[must_use]
    pub fn is_none(self) -> bool {
        self.is_empty()
    }

    /// Whether the canvas has to be redrawn.
    #[must_use]
    pub fn needs_redraw(self) -> bool {
        self.intersects(Changed::VIEW | Changed::SELECTION | Changed::DOCUMENT | Changed::CACHE)
    }

    /// Whether the scene must be walked again.
    #[must_use]
    pub fn needs_scene(self) -> bool {
        self.intersects(Changed::DOCUMENT | Changed::CACHE)
    }
}
