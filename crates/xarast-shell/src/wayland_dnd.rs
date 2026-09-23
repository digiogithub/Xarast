//! Drag and drop on Wayland, which `winit` 0.30 does not implement.
//!
//! `winit` 0.30 emits `HoveredFile`/`DroppedFile` on X11 only; on Wayland a
//! file dropped on the window simply vanishes (measured on GNOME 46 with a
//! GTK drag source, `docs/memory/ui.md`). This module binds
//! `wl_data_device` itself, on a second event queue over `winit`'s own
//! `wl_display`, and turns the protocol into the shell's [`DragEvent`]s —
//! with the drop position, which `winit` never reports on any platform, and
//! with every file of a multi-file drop in one event.
//!
//! Two layers:
//!
//! * [`DropTracker`] is the protocol-independent state machine — logical
//!   surface coordinates in, [`DragEvent`]s in device pixels out — and is
//!   what the tests exercise.
//! * [`WaylandDrops`] is the glue: `smithay-client-toolkit`'s data device,
//!   dispatched from the event loop's `about_to_wait`. The payload is read
//!   on a short-lived thread, because the drag source writes it whenever it
//!   likes and the frame loop must never wait for another process.
//!
//! The one `unsafe` call in the crate lives here: wrapping `winit`'s
//! `wl_display` pointer. See [`WaylandDrops::new`].

use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

use smithay_client_toolkit::data_device_manager::data_device::{DataDevice, DataDeviceHandler};
use smithay_client_toolkit::data_device_manager::data_offer::{DataOfferHandler, DragOffer};
use smithay_client_toolkit::data_device_manager::data_source::DataSourceHandler;
use smithay_client_toolkit::data_device_manager::{DataDeviceManagerState, WritePipe};
use smithay_client_toolkit::delegate_data_device;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_callback::WlCallback;
use wayland_client::protocol::wl_data_device::WlDataDevice;
use wayland_client::protocol::wl_data_device_manager::DndAction;
use wayland_client::protocol::wl_data_source::WlDataSource;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};

use crate::input::event::{DragEvent, PhysicalPos2, ShellEvent};
use crate::input::translate::parse_uri_list;
use crate::scale::ScaleFactor;
use crate::window::ShellWaker;

/// The only payload asked for: a list of `file:` URIs.
pub(crate) const URI_LIST: &str = "text/uri-list";

/// The drag state of one window, independent of the protocol.
#[derive(Debug, Default)]
pub(crate) struct DropTracker {
    /// A drag of something we can take is over the window.
    active: bool,
    /// Where it was last seen, in device pixels.
    at: Option<PhysicalPos2>,
}

impl DropTracker {
    fn device(x: f64, y: f64, scale: ScaleFactor) -> PhysicalPos2 {
        let s = scale.get();
        // Window-sized coordinates: well inside `i32`.
        #[allow(clippy::cast_possible_truncation)]
        PhysicalPos2::new((x * s).round() as i32, (y * s).round() as i32)
    }

    /// A drag entered at surface-local logical `(x, y)`. `acceptable` says
    /// whether it offers a file list; anything else is ignored entirely.
    pub(crate) fn enter(
        &mut self,
        x: f64,
        y: f64,
        acceptable: bool,
        scale: ScaleFactor,
    ) -> Option<DragEvent> {
        self.active = acceptable;
        if !acceptable {
            self.at = None;
            return None;
        }
        let at = Self::device(x, y, scale);
        self.at = Some(at);
        // The paths are only known once the payload is read, at the drop.
        Some(DragEvent::Entered {
            paths: Vec::new(),
            at: Some(at),
        })
    }

    /// The drag moved.
    pub(crate) fn motion(&mut self, x: f64, y: f64, scale: ScaleFactor) -> Option<DragEvent> {
        if !self.active {
            return None;
        }
        let at = Self::device(x, y, scale);
        if self.at == Some(at) {
            return None;
        }
        self.at = Some(at);
        Some(DragEvent::Moved { at })
    }

    /// The drag left without dropping, or was cancelled.
    pub(crate) fn leave(&mut self) -> Option<DragEvent> {
        let was = std::mem::take(&mut self.active);
        self.at = None;
        was.then_some(DragEvent::Left)
    }

    /// The user released over the window. Returns where, when the drag is
    /// one we accepted; the event itself waits for the payload.
    pub(crate) fn dropped(&mut self) -> Option<Option<PhysicalPos2>> {
        let was = std::mem::take(&mut self.active);
        let at = self.at.take();
        was.then_some(at)
    }

    /// The payload of a drop arrived.
    pub(crate) fn delivered(payload: &str, at: Option<PhysicalPos2>) -> DragEvent {
        let paths = parse_uri_list(payload);
        if paths.is_empty() {
            // Remote URIs only, or an empty list: nothing to open, and the
            // application must still hear that the drag is over.
            DragEvent::Left
        } else {
            DragEvent::Dropped { paths, at }
        }
    }
}

/// What the Wayland thread reports, in the protocol's terms: logical
/// surface coordinates. The main thread turns them into [`DragEvent`]s with
/// the scale in force at that moment.
#[derive(Debug)]
enum Msg {
    Enter {
        x: f64,
        y: f64,
        acceptable: bool,
    },
    Motion {
        x: f64,
        y: f64,
    },
    Leave,
    Dropped,
    /// The payload of the last drop; empty when it could not be read.
    Payload(String),
}

/// The dispatch state of the second queue, owned by its thread.
struct State {
    /// Keeps the bound `wl_data_device_manager` alive for the devices.
    _data_devices: DataDeviceManagerState,
    /// One data device per seat. Kept alive: dropping one releases it.
    _devices: Vec<DataDevice>,
    /// The current drag offers a file list.
    acceptable: bool,
    tx: Sender<Msg>,
    waker: ShellWaker,
    conn: Connection,
}

impl State {
    fn send(&self, msg: Msg) {
        if self.tx.send(msg).is_ok() {
            self.waker.wake();
        }
    }

    fn offer(device: &WlDataDevice) -> Option<DragOffer> {
        device
            .data::<smithay_client_toolkit::data_device_manager::data_device::DataDeviceData>()
            .and_then(|d| d.drag_offer())
    }
}

/// `wl_data_device` for one window, on `winit`'s connection.
///
/// The queue is dispatched by a thread of its own, blocked in libwayland's
/// read. It cannot be dispatched from the event loop: during a drag the
/// compositor holds the pointer, `winit` receives nothing, its loop stays
/// parked, and the offer is never accepted in time — measured on GNOME 46:
/// every drop came back as a cancelled drag until the queue had its own
/// thread.
pub(crate) struct WaylandDrops {
    rx: Receiver<Msg>,
    tracker: DropTracker,
    /// Where the last accepted drop happened, until its payload arrives.
    pending: Option<Option<PhysicalPos2>>,
    conn: Connection,
    qh: QueueHandle<State>,
    stop: Arc<AtomicBool>,
    done: Receiver<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl std::fmt::Debug for WaylandDrops {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WaylandDrops")
            .field("tracker", &self.tracker)
            .finish_non_exhaustive()
    }
}

impl WaylandDrops {
    /// Binds the data device on the display `winit` is connected to and
    /// starts the thread that dispatches it.
    ///
    /// Returns `None` — with the reason logged — when the window is not a
    /// Wayland window or the compositor has no data device manager. Neither
    /// is an error: X11 drag and drop arrives through `winit`.
    ///
    /// The returned value must be dropped before `winit`'s event loop is:
    /// it borrows the loop's `wl_display` (see the safety note).
    pub(crate) fn new(
        display: raw_window_handle::RawDisplayHandle,
        waker: ShellWaker,
    ) -> Option<WaylandDrops> {
        let raw_window_handle::RawDisplayHandle::Wayland(handle) = display else {
            return None;
        };
        // SAFETY: FFI boundary. `handle.display` is the live `wl_display*`
        // of `winit`'s connection, obtained from the window this module
        // serves. `from_foreign_display` does not take ownership (it never
        // disconnects the display), and the shell drops this value — which
        // stops and joins the thread using it — in
        // `ApplicationHandler::exiting`, before the event loop and with it
        // the display are torn down. Both sides use the system libwayland
        // (`client_system`), whose `prepare_read`/`read_events` protocol is
        // what makes two queues on two threads over one display sound.
        #[allow(unsafe_code)]
        let backend = unsafe {
            wayland_backend::client::Backend::from_foreign_display(handle.display.as_ptr().cast())
        };
        let conn = Connection::from_backend(backend);
        let (globals, mut queue) = match registry_queue_init::<State>(&conn) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "no Wayland registry; drag and drop is off");
                return None;
            }
        };
        let qh = queue.handle();
        let data_devices = match DataDeviceManagerState::bind(&globals, &qh) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!(error = %e, "no wl_data_device_manager; drag and drop is off");
                return None;
            }
        };
        let seats: Vec<WlSeat> = globals.contents().with_list(|list| {
            list.iter()
                .filter(|g| g.interface == "wl_seat")
                .map(|g| {
                    globals
                        .registry()
                        .bind::<WlSeat, _, _>(g.name, g.version.min(5), &qh, ())
                })
                .collect()
        });
        let devices = seats
            .iter()
            .map(|seat| data_devices.get_data_device(&qh, seat))
            .collect::<Vec<_>>();
        let seat_count = devices.len();
        let (tx, rx) = channel();
        let mut state = State {
            _data_devices: data_devices,
            _devices: devices,
            acceptable: false,
            tx,
            waker,
            conn: conn.clone(),
        };
        if let Err(e) = queue.roundtrip(&mut state) {
            tracing::warn!(error = %e, "Wayland roundtrip failed; drag and drop is off");
            return None;
        }

        let stop = Arc::new(AtomicBool::new(false));
        let (done_tx, done) = channel::<()>();
        let stopping = stop.clone();
        let thread = std::thread::Builder::new()
            .name("xarast-dnd".to_owned())
            .spawn(move || {
                let _done = done_tx;
                while !stopping.load(Ordering::SeqCst) {
                    if let Err(e) = queue.blocking_dispatch(&mut state) {
                        tracing::warn!(error = %e, "Wayland drag-and-drop queue failed");
                        break;
                    }
                }
                drop(state);
                let _ = queue.flush();
            })
            .ok()?;
        tracing::info!(seats = seat_count, "Wayland drag and drop ready");
        Some(WaylandDrops {
            rx,
            tracker: DropTracker::default(),
            pending: None,
            conn,
            qh,
            stop,
            done,
            thread: Some(thread),
        })
    }

    /// Turns whatever the Wayland thread reported into
    /// [`ShellEvent::Drag`]s in `out`. Never blocks.
    pub(crate) fn dispatch(&mut self, scale: ScaleFactor, out: &mut Vec<ShellEvent>) {
        while let Ok(msg) = self.rx.try_recv() {
            let ev = match msg {
                Msg::Enter { x, y, acceptable } => self.tracker.enter(x, y, acceptable, scale),
                Msg::Motion { x, y } => self.tracker.motion(x, y, scale),
                Msg::Leave => self.tracker.leave(),
                Msg::Dropped => {
                    self.pending = self.tracker.dropped();
                    None
                }
                Msg::Payload(payload) => self
                    .pending
                    .take()
                    .map(|at| DropTracker::delivered(&payload, at)),
            };
            out.extend(ev.map(ShellEvent::Drag));
        }
    }
}

impl Drop for WaylandDrops {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the thread out of its blocking read with a round trip on its
        // own queue; it sees the flag and ends.
        let _ = self.conn.display().sync(&self.qh, ());
        let _ = self.conn.flush();
        let Some(thread) = self.thread.take() else {
            return;
        };
        match self.done.recv_timeout(STOP_GRACE) {
            Err(RecvTimeoutError::Timeout) => {
                tracing::warn!("the Wayland drag-and-drop thread did not stop in time");
            }
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                let _ = thread.join();
            }
        }
    }
}

/// How long shutting down waits for the compositor to answer the wake-up.
const STOP_GRACE: Duration = Duration::from_secs(2);

impl DataDeviceHandler for State {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        device: &WlDataDevice,
        x: f64,
        y: f64,
        _surface: &WlSurface,
    ) {
        let Some(offer) = Self::offer(device) else {
            return;
        };
        let acceptable = offer.with_mime_types(|m| m.iter().any(|t| t == URI_LIST));
        if acceptable {
            offer.accept_mime_type(offer.serial, Some(URI_LIST.to_owned()));
            offer.set_actions(DndAction::Copy, DndAction::Copy);
        } else {
            offer.accept_mime_type(offer.serial, None);
        }
        self.acceptable = acceptable;
        self.send(Msg::Enter { x, y, acceptable });
    }

    fn leave(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _device: &WlDataDevice) {
        self.send(Msg::Leave);
    }

    fn motion(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _device: &WlDataDevice,
        x: f64,
        y: f64,
    ) {
        if self.acceptable {
            self.send(Msg::Motion { x, y });
        }
    }

    fn selection(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _device: &WlDataDevice) {
        // The clipboard is `arboard`'s; this device only takes drops.
    }

    fn drop_performed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        device: &WlDataDevice,
    ) {
        if !std::mem::take(&mut self.acceptable) {
            return;
        }
        self.send(Msg::Dropped);
        let Some(offer) = Self::offer(device) else {
            self.send(Msg::Payload(String::new()));
            return;
        };
        let pipe = match offer.receive(URI_LIST.to_owned()) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "could not receive a dropped file list");
                offer.destroy();
                self.send(Msg::Payload(String::new()));
                return;
            }
        };
        // The source only starts writing once the request reaches it, and
        // it may take its time: read on a thread of its own so that this
        // queue keeps being dispatched.
        let _ = self.conn.flush();
        let tx = self.tx.clone();
        let waker = self.waker.clone();
        let conn = self.conn.clone();
        let spawned = std::thread::Builder::new()
            .name("xarast-drop".to_owned())
            .spawn(move || {
                let mut payload = String::new();
                let mut pipe = pipe;
                if let Err(e) = pipe.read_to_string(&mut payload) {
                    tracing::warn!(error = %e, "reading a dropped file list failed");
                }
                offer.finish();
                offer.destroy();
                let _ = conn.flush();
                if tx.send(Msg::Payload(payload)).is_ok() {
                    waker.wake();
                }
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "could not start the drop reader");
        }
    }
}

impl DataOfferHandler for State {
    fn source_actions(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        offer: &mut DragOffer,
        _actions: DndAction,
    ) {
        offer.set_actions(DndAction::Copy, DndAction::Copy);
    }

    fn selected_action(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _offer: &mut DragOffer,
        _actions: DndAction,
    ) {
    }
}

// This queue never creates a data source; the trait is required by the
// data-device dispatch all the same.
impl DataSourceHandler for State {
    fn accept_mime(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &WlDataSource,
        _mime: Option<String>,
    ) {
    }
    fn send_request(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &WlDataSource,
        _mime: String,
        _fd: WritePipe,
    ) {
    }
    fn cancelled(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _source: &WlDataSource) {}
    fn dnd_dropped(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _source: &WlDataSource) {
    }
    fn dnd_finished(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &WlDataSource,
    ) {
    }
    fn action(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &WlDataSource,
        _action: DndAction,
    ) {
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _state: &mut Self,
        _registry: &WlRegistry,
        _event: wayland_client::protocol::wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // A seat added later is not picked up; one seat is the norm.
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(
        _state: &mut Self,
        _seat: &WlSeat,
        _event: wayland_client::protocol::wl_seat::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // Capabilities and names are `winit`'s business.
    }
}

impl Dispatch<WlCallback, ()> for State {
    fn event(
        _state: &mut Self,
        _callback: &WlCallback,
        _event: wayland_client::protocol::wl_callback::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // The shutdown wake-up: its only job was to end the blocking read.
    }
}

delegate_data_device!(State);

#[cfg(test)]
mod tests {
    use super::*;

    fn s(f: f64) -> ScaleFactor {
        ScaleFactor::new(f)
    }

    #[test]
    fn a_file_drag_reports_its_position_in_device_pixels() {
        let mut t = DropTracker::default();
        assert_eq!(
            t.enter(10.0, 20.4, true, s(1.25)),
            Some(DragEvent::Entered {
                paths: vec![],
                at: Some(PhysicalPos2::new(13, 26))
            })
        );
        assert_eq!(
            t.motion(100.0, 50.0, s(1.25)),
            Some(DragEvent::Moved {
                at: PhysicalPos2::new(125, 63)
            })
        );
        // Sub-pixel jitter that lands on the same pixel is not an event.
        assert_eq!(t.motion(100.1, 50.1, s(1.25)), None);
    }

    #[test]
    fn a_drag_that_offers_no_file_list_is_invisible() {
        let mut t = DropTracker::default();
        assert_eq!(t.enter(1.0, 1.0, false, s(1.0)), None);
        assert_eq!(t.motion(5.0, 5.0, s(1.0)), None);
        assert_eq!(t.dropped(), None);
        assert_eq!(t.leave(), None);
    }

    #[test]
    fn a_drop_keeps_the_last_position_and_ends_the_drag() {
        let mut t = DropTracker::default();
        t.enter(0.0, 0.0, true, s(2.0));
        t.motion(40.0, 30.0, s(2.0));
        assert_eq!(t.dropped(), Some(Some(PhysicalPos2::new(80, 60))));
        // The leave that follows a drop on Wayland is not a second "left".
        assert_eq!(t.leave(), None);
    }

    #[test]
    fn a_multi_file_payload_is_one_drop_with_every_path() {
        let ev = DropTracker::delivered(
            "file:///a/One%20Line.xar\r\nfile:///b/two.xar\r\n",
            Some(PhysicalPos2::new(3, 4)),
        );
        assert_eq!(
            ev,
            DragEvent::Dropped {
                paths: vec!["/a/One Line.xar".into(), "/b/two.xar".into()],
                at: Some(PhysicalPos2::new(3, 4))
            }
        );
    }

    #[test]
    fn a_payload_with_no_local_file_ends_the_drag_instead_of_dropping_nothing() {
        assert_eq!(
            DropTracker::delivered("https://example.org/x.xar\n", None),
            DragEvent::Left
        );
        assert_eq!(DropTracker::delivered("", None), DragEvent::Left);
    }

    #[test]
    fn a_non_wayland_display_is_not_an_error() {
        let xlib = raw_window_handle::RawDisplayHandle::Xlib(
            raw_window_handle::XlibDisplayHandle::new(None, 0),
        );
        assert!(WaylandDrops::new(xlib, ShellWaker::none()).is_none());
    }
}
