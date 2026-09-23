//! The window's handle for portal dialogs on Wayland (XARA-T-0048).
//!
//! A portal file chooser is drawn by another process. To stack it on the
//! Xarast window — modal to it, centred on it, following it between
//! workspaces — the portal needs a reference to our surface, and on
//! Wayland the only way to hand one to another client is
//! `xdg-foreign-unstable-v2`: export the toplevel, receive an opaque handle
//! string, pass `wayland:<handle>` as the dialog's parent window.
//!
//! The export is made once, when the window is created, on a connection of
//! its own over `winit`'s `wl_display` (as `wayland_dnd` does), and kept
//! alive with the window: the handle is valid for as long as the exported
//! object exists. A compositor without the protocol simply gets dialogs
//! without a parent, as before.

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols::xdg::foreign::zv2::client::zxdg_exported_v2::{self, ZxdgExportedV2};
use wayland_protocols::xdg::foreign::zv2::client::zxdg_exporter_v2::ZxdgExporterV2;

/// The exported toplevel. Must be dropped before `winit`'s event loop: it
/// borrows the loop's `wl_display`.
pub(crate) struct ExportedWindow {
    handle: String,
    exported: ZxdgExportedV2,
    exporter: ZxdgExporterV2,
    // Kept for the lifetime of the export; never dispatched again.
    _queue: EventQueue<State>,
    conn: Connection,
}

impl std::fmt::Debug for ExportedWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExportedWindow")
            .field("handle", &self.handle)
            .finish_non_exhaustive()
    }
}

#[derive(Default)]
struct State {
    handle: Option<String>,
}

impl ExportedWindow {
    /// Exports the window's surface. `None` — logged — when the window is
    /// not a Wayland window or the compositor does not offer
    /// `zxdg_exporter_v2`.
    pub(crate) fn new(
        display: raw_window_handle::RawDisplayHandle,
        window: raw_window_handle::RawWindowHandle,
    ) -> Option<ExportedWindow> {
        let (
            raw_window_handle::RawDisplayHandle::Wayland(display),
            raw_window_handle::RawWindowHandle::Wayland(window),
        ) = (display, window)
        else {
            return None;
        };
        // SAFETY: FFI boundary, the same as `WaylandDrops::new`: the live
        // `wl_display*` of `winit`'s connection, not owned (never
        // disconnected here), and this value is dropped in
        // `ApplicationHandler::exiting`, before the display.
        #[allow(unsafe_code)]
        let backend = unsafe {
            wayland_backend::client::Backend::from_foreign_display(display.display.as_ptr().cast())
        };
        let conn = Connection::from_backend(backend);
        let (globals, mut queue) = registry_queue_init::<State>(&conn).ok()?;
        let qh = queue.handle();
        let exporter: ZxdgExporterV2 = match globals.bind(&qh, 1..=1, ()) {
            Ok(e) => e,
            Err(e) => {
                tracing::info!(error = %e, "no xdg-foreign exporter; dialogs will not be parented");
                return None;
            }
        };
        // SAFETY: FFI boundary. `window.surface` is the `wl_surface*` of the
        // window `winit` created on this same display, which outlives this
        // value (the window is dropped after `exiting`). The id is only
        // passed as the argument of one request, and the interface is
        // checked by `from_ptr`.
        #[allow(unsafe_code)]
        let id = unsafe {
            wayland_backend::client::ObjectId::from_ptr(
                WlSurface::interface(),
                window.surface.as_ptr().cast(),
            )
        }
        .ok()?;
        let surface = WlSurface::from_id(&conn, id).ok()?;
        let exported = exporter.export_toplevel(&surface, &qh, ());
        let mut state = State::default();
        if let Err(e) = queue.roundtrip(&mut state) {
            tracing::warn!(error = %e, "exporting the window failed");
            return None;
        }
        let Some(handle) = state.handle else {
            tracing::info!("the compositor sent no handle for the exported window");
            return None;
        };
        tracing::info!(handle, "window exported for portal dialogs");
        Some(ExportedWindow {
            handle,
            exported,
            exporter,
            _queue: queue,
            conn,
        })
    }

    /// The portal's parent-window string: `wayland:<handle>`.
    pub(crate) fn parent_window(&self) -> String {
        format!("wayland:{}", self.handle)
    }
}

impl Drop for ExportedWindow {
    fn drop(&mut self) {
        self.exported.destroy();
        self.exporter.destroy();
        let _ = self.conn.flush();
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZxdgExporterV2, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZxdgExporterV2,
        _: <ZxdgExporterV2 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZxdgExportedV2, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZxdgExportedV2,
        event: zxdg_exported_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zxdg_exported_v2::Event::Handle { handle } = event {
            state.handle = Some(handle);
        }
    }
}
