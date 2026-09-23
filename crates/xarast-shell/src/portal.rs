//! XDG desktop portals, on a services thread.
//!
//! Two rules shape this module.
//!
//! *Portals are asynchronous and the event loop is not.* Every D-Bus call
//! happens on a thread of its own; the main thread posts a request, keeps
//! drawing, and later receives a [`PortalEvent`]. No `async` reaches the
//! core (`research/05 §10.1`), and no dialog can wedge the frame loop.
//!
//! *A missing portal is not a failure to start.* Inside a container, over
//! SSH, or on a desktop with no portal implementation, every request answers
//! [`PortalEvent::Failed`] with a reason a user can act on. The application
//! keeps running with the command line as its way in.
//!
//! File dialogs go straight to `org.freedesktop.portal.FileChooser` through
//! `ashpd`, never through a toolkit: the portal path is the one that works
//! inside an AppImage and a Flatpak sandbox, and a cancelled dialog must stay
//! distinguishable from a portal that is not there (`docs/memory/ui.md`).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError, channel};
use std::time::Duration;

use crate::input::event::ColorScheme;

/// Identifies one portal request, so that its answer can be matched to the
/// thing that asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PortalRequestId(pub u64);

/// A file type offered in a dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFilter {
    /// What the user sees, for example `Xara drawings`.
    pub name: String,
    /// Extensions without the dot, for example `xar`.
    pub extensions: Vec<String>,
}

impl FileFilter {
    /// A filter.
    #[must_use]
    pub fn new(name: impl Into<String>, extensions: &[&str]) -> Self {
        Self {
            name: name.into(),
            extensions: extensions.iter().map(|e| (*e).to_owned()).collect(),
        }
    }
}

/// A request to open one or more files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenFileRequest {
    /// Dialog title.
    pub title: String,
    /// Offered filters, in order.
    pub filters: Vec<FileFilter>,
    /// Allow more than one selection.
    pub multiple: bool,
    /// Where to start.
    pub directory: Option<PathBuf>,
}

/// A request to choose a save location.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveFileRequest {
    /// Dialog title.
    pub title: String,
    /// Offered filters, in order.
    pub filters: Vec<FileFilter>,
    /// Pre-filled file name.
    pub file_name: Option<String>,
    /// Where to start.
    pub directory: Option<PathBuf>,
}

/// The answer to a portal request.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PortalEvent {
    /// Files were chosen.
    FilesChosen {
        /// Which request this answers.
        request: PortalRequestId,
        /// The chosen files.
        paths: Vec<PathBuf>,
    },
    /// A save location was chosen.
    SaveChosen {
        /// Which request this answers.
        request: PortalRequestId,
        /// The chosen path.
        path: PathBuf,
    },
    /// The user dismissed the dialog.
    Cancelled {
        /// Which request this answers.
        request: PortalRequestId,
    },
    /// The request could not be made at all.
    Failed {
        /// Which request this answers.
        request: PortalRequestId,
        /// Why, in terms a user can act on.
        reason: String,
    },
    /// The desktop colour scheme changed, or was read for the first time.
    ColorSchemeChanged(ColorScheme),
}

/// A request posted to the services thread.
#[derive(Debug)]
enum PortalCommand {
    Open(PortalRequestId, Box<OpenFileRequest>),
    Save(PortalRequestId, Box<SaveFileRequest>),
    QueryColorScheme,
    Shutdown,
}

/// Whether portals can be reached at all.
///
/// Checked before the first request so that the failure message names the
/// real cause — "no session bus" — instead of a D-Bus timeout forty seconds
/// later.
pub fn portal_availability() -> Result<(), String> {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some() {
        return Ok(());
    }
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        let bus = PathBuf::from(dir).join("bus");
        if bus.exists() {
            return Ok(());
        }
    }
    Err(
        "no D-Bus session bus: DBUS_SESSION_BUS_ADDRESS is unset and \
         $XDG_RUNTIME_DIR/bus does not exist, so no XDG portal can be reached"
            .to_owned(),
    )
}

/// The handle the main thread keeps.
///
/// Cloneable and cheap: a panel can hold one without the shell handing out a
/// borrow of itself.
#[derive(Debug, Clone)]
pub struct PortalHandle {
    tx: Sender<PortalCommand>,
    next_id: Arc<AtomicU64>,
}

impl PortalHandle {
    fn next(&self) -> PortalRequestId {
        PortalRequestId(self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    /// Asks for files to open. Returns immediately.
    pub fn open_files(&self, request: OpenFileRequest) -> PortalRequestId {
        let id = self.next();
        let _ = self.tx.send(PortalCommand::Open(id, Box::new(request)));
        id
    }

    /// Asks for a save location. Returns immediately.
    pub fn save_file(&self, request: SaveFileRequest) -> PortalRequestId {
        let id = self.next();
        let _ = self.tx.send(PortalCommand::Save(id, Box::new(request)));
        id
    }

    /// Asks the settings portal for the desktop colour scheme.
    ///
    /// The answer arrives as [`PortalEvent::ColorSchemeChanged`]. Called once
    /// at start-up; later changes arrive on their own, from the watcher
    /// [`PortalService::start_with_waker`] starts.
    pub fn query_color_scheme(&self) {
        let _ = self.tx.send(PortalCommand::QueryColorScheme);
    }
}

/// The services thread and its answer queue.
///
/// Dropping it shuts the thread down. Queued requests are discarded. The
/// thread is joined when it is idle; when it is still inside a dialog it is
/// left to end with the process, because a join would hold the application
/// open until someone answered a dialog whose window is already gone
/// (measured: quitting with a dialog open hung until it was dismissed, and
/// then opened the next queued one).
#[derive(Debug)]
pub struct PortalService {
    handle: PortalHandle,
    answers: Receiver<PortalEvent>,
    thread: Option<std::thread::JoinHandle<()>>,
    /// Set on drop: requests still queued behind an open dialog are
    /// discarded instead of opening one dialog after another.
    stopping: Arc<AtomicBool>,
    /// Disconnects when the services thread ends.
    finished: Receiver<()>,
}

impl PortalService {
    /// Starts the services thread, with nothing to wake.
    ///
    /// Never fails: a portal-less system still gets a service, whose every
    /// answer is [`PortalEvent::Failed`] with the reason.
    #[must_use]
    pub fn start() -> Self {
        Self::start_with_waker(|| {})
    }

    /// Starts the services thread and, where the settings portal can be
    /// reached, a watcher that reports every change of the desktop colour
    /// scheme as a [`PortalEvent::ColorSchemeChanged`].
    ///
    /// `wake` is called after each answer is queued, from the thread that
    /// queued it, so that an event loop parked with nothing to do still
    /// notices a dialog closing or the desktop turning dark.
    #[must_use]
    pub fn start_with_waker(wake: impl Fn() + Send + Sync + 'static) -> Self {
        let availability = portal_availability();
        if let Err(reason) = &availability {
            tracing::info!(reason, "XDG portals unavailable; file dialogs are disabled");
        }
        Self::spawn(availability, Arc::new(wake))
    }

    /// A service that never touches D-Bus: every request answers
    /// [`PortalEvent::Failed`] with `reason`, and the colour scheme is
    /// [`ColorScheme::NoPreference`].
    ///
    /// For tests and headless tools. A test that posted a request to a real
    /// service on a desktop machine would open a real dialog on the
    /// developer's screen and then wait for a human to close it.
    #[must_use]
    pub fn offline(reason: impl Into<String>) -> Self {
        Self::spawn(Err(reason.into()), Arc::new(|| {}))
    }

    fn spawn(availability: Result<(), String>, wake: Wake) -> Self {
        let (tx, rx) = channel::<PortalCommand>();
        let (atx, answers) = channel::<PortalEvent>();
        if availability.is_ok() {
            watch_color_scheme(atx.clone(), wake.clone());
        }

        let stopping = Arc::new(AtomicBool::new(false));
        let (done, finished) = channel::<()>();
        let stop = stopping.clone();
        let thread = std::thread::Builder::new()
            .name("xarast-services".to_owned())
            .spawn(move || {
                let _done = done;
                service_loop(
                    &rx,
                    &atx,
                    availability.as_ref().err().cloned(),
                    &*wake,
                    &stop,
                );
            })
            .ok();

        Self {
            handle: PortalHandle {
                tx,
                next_id: Arc::new(AtomicU64::new(1)),
            },
            answers,
            thread,
            stopping,
            finished,
        }
    }

    /// The handle to post requests with.
    #[must_use]
    pub const fn handle(&self) -> &PortalHandle {
        &self.handle
    }

    /// Takes every answer that has arrived. Never blocks.
    pub fn poll(&self, out: &mut Vec<PortalEvent>) {
        loop {
            match self.answers.try_recv() {
                Ok(ev) => out.push(ev),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return,
            }
        }
    }
}

impl Drop for PortalService {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        let _ = self.handle.tx.send(PortalCommand::Shutdown);
        let Some(t) = self.thread.take() else { return };
        match self.finished.recv_timeout(SHUTDOWN_GRACE) {
            Err(RecvTimeoutError::Timeout) => {
                tracing::info!("a portal dialog is still open; not waiting for it");
            }
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                let _ = t.join();
            }
        }
    }
}

/// How long dropping the service waits for an idle services thread.
const SHUTDOWN_GRACE: Duration = Duration::from_millis(250);

/// Called from a services thread after it queues an answer.
type Wake = Arc<dyn Fn() + Send + Sync>;

fn service_loop(
    rx: &Receiver<PortalCommand>,
    answers: &Sender<PortalEvent>,
    unavailable: Option<String>,
    wake: &(dyn Fn() + Send + Sync),
    stopping: &AtomicBool,
) {
    while let Ok(cmd) = rx.recv() {
        if stopping.load(Ordering::SeqCst) {
            return;
        }
        let answer = match cmd {
            PortalCommand::Shutdown => return,
            PortalCommand::Open(id, req) => match &unavailable {
                Some(reason) => Some(PortalEvent::Failed {
                    request: id,
                    reason: reason.clone(),
                }),
                None => Some(open_files(id, &req)),
            },
            PortalCommand::Save(id, req) => match &unavailable {
                Some(reason) => Some(PortalEvent::Failed {
                    request: id,
                    reason: reason.clone(),
                }),
                None => Some(save_file(id, &req)),
            },
            PortalCommand::QueryColorScheme => match &unavailable {
                Some(_) => Some(PortalEvent::ColorSchemeChanged(ColorScheme::NoPreference)),
                None => Some(PortalEvent::ColorSchemeChanged(color_scheme())),
            },
        };
        if let Some(answer) = answer {
            if answers.send(answer).is_err() {
                return;
            }
            wake();
        }
    }
}

/// Watches the settings portal's `SettingChanged` signal for the colour
/// scheme, on a thread of its own: the services thread blocks inside a
/// file dialog, and a theme change must not wait for one to close.
///
/// The thread is detached rather than joined. It spends its life parked on
/// the D-Bus signal, and it ends at the first change after the service is
/// dropped, when the answer channel is gone.
#[cfg(feature = "portals")]
fn watch_color_scheme(answers: Sender<PortalEvent>, wake: Wake) {
    let spawned = std::thread::Builder::new()
        .name("xarast-settings".to_owned())
        .spawn(move || {
            use futures_lite::StreamExt;
            pollster::block_on(async move {
                let settings = match ashpd::desktop::settings::Settings::new().await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::info!(error = %e, "no settings portal; the colour scheme will not follow the desktop");
                        return;
                    }
                };
                let mut changes = match settings.receive_color_scheme_changed().await {
                    Ok(stream) => stream,
                    Err(e) => {
                        tracing::info!(error = %e, "the settings portal does not report changes");
                        return;
                    }
                };
                while let Some(scheme) = changes.next().await {
                    let scheme = from_portal(scheme);
                    tracing::info!(?scheme, "desktop colour scheme changed");
                    if answers.send(PortalEvent::ColorSchemeChanged(scheme)).is_err() {
                        return;
                    }
                    wake();
                }
            });
        });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "could not start the settings watcher");
    }
}

#[cfg(not(feature = "portals"))]
fn watch_color_scheme(_answers: Sender<PortalEvent>, _wake: Wake) {}

/// The portal's colour scheme in the shell's vocabulary.
#[cfg(feature = "portals")]
const fn from_portal(scheme: ashpd::desktop::settings::ColorScheme) -> ColorScheme {
    use ashpd::desktop::settings::ColorScheme as P;
    match scheme {
        P::PreferDark => ColorScheme::Dark,
        P::PreferLight => ColorScheme::Light,
        P::NoPreference => ColorScheme::NoPreference,
    }
}

/// How a file-chooser request ended, before it becomes a [`PortalEvent`].
///
/// Kept apart from the D-Bus call so that the classification — the part
/// that decides what the user is told — is testable without a portal.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(feature = "portals"), allow(dead_code))]
enum Chosen {
    Paths(Vec<PathBuf>),
    Cancelled,
    Failed(String),
}

#[cfg_attr(not(feature = "portals"), allow(dead_code))]
impl Chosen {
    /// Turns the portal's list of URIs into local paths. The file chooser
    /// hands back `file:` URIs (document-portal paths inside a sandbox); a
    /// choice that holds none is reported rather than treated as a cancel.
    fn from_uris<'a>(uris: impl IntoIterator<Item = &'a str>) -> Chosen {
        let mut any = false;
        let mut paths = Vec::new();
        for uri in uris {
            any = true;
            paths.extend(crate::input::translate::parse_uri_list(uri));
        }
        match (any, paths.is_empty()) {
            (false, _) => Chosen::Cancelled,
            (true, true) => Chosen::Failed(
                "the portal returned no local file (only remote locations)".to_owned(),
            ),
            (true, false) => Chosen::Paths(paths),
        }
    }
}

/// The classification of an `ashpd` result. A dismissal is the user's
/// choice; everything else is a failure with a reason a user can act on.
#[cfg(feature = "portals")]
fn classify(result: Result<Chosen, ashpd::Error>) -> Chosen {
    use ashpd::desktop::ResponseError;
    match result {
        Ok(chosen) => chosen,
        // Response code 2, "ended in some other way", is how
        // xdg-desktop-portal-gtk reports a dialog dismissed with Escape or
        // its close button (measured on GNOME 46): a dismissal, not a fault.
        Err(ashpd::Error::Response(ResponseError::Cancelled | ResponseError::Other)) => {
            Chosen::Cancelled
        }
        Err(ashpd::Error::PortalNotFound(_)) => Chosen::Failed(
            "no XDG desktop portal provides a file chooser; install xdg-desktop-portal and a \
             backend for this desktop (-gnome, -gtk, -kde, -wlr, -cosmic)"
                .to_owned(),
        ),
        Err(e) => Chosen::Failed(format!("the file chooser portal could not be used: {e}")),
    }
}

#[cfg(feature = "portals")]
fn portal_filters(filters: &[FileFilter]) -> Vec<ashpd::desktop::file_chooser::FileFilter> {
    filters
        .iter()
        .map(|f| {
            f.extensions.iter().fold(
                ashpd::desktop::file_chooser::FileFilter::new(&f.name),
                |acc, ext| acc.glob(&format!("*.{ext}")),
            )
        })
        .collect()
}

#[cfg(feature = "portals")]
fn open_files(id: PortalRequestId, req: &OpenFileRequest) -> PortalEvent {
    use ashpd::desktop::file_chooser::SelectedFiles;
    let result = pollster::block_on(async {
        let mut builder = SelectedFiles::open_file()
            .title(req.title.as_str())
            .modal(true)
            .multiple(req.multiple)
            .filters(portal_filters(&req.filters));
        if let Some(dir) = &req.directory {
            builder = builder.current_folder(dir)?;
        }
        let files = builder.send().await?.response()?;
        Ok(Chosen::from_uris(files.uris().iter().map(|u| u.as_str())))
    });
    match classify(result) {
        Chosen::Paths(paths) => PortalEvent::FilesChosen { request: id, paths },
        Chosen::Cancelled => PortalEvent::Cancelled { request: id },
        Chosen::Failed(reason) => PortalEvent::Failed {
            request: id,
            reason,
        },
    }
}

#[cfg(feature = "portals")]
fn save_file(id: PortalRequestId, req: &SaveFileRequest) -> PortalEvent {
    use ashpd::desktop::file_chooser::SelectedFiles;
    let result = pollster::block_on(async {
        let mut builder = SelectedFiles::save_file()
            .title(req.title.as_str())
            .modal(true)
            .current_name(req.file_name.as_deref())
            .filters(portal_filters(&req.filters));
        if let Some(dir) = &req.directory {
            builder = builder.current_folder(dir)?;
        }
        let files = builder.send().await?.response()?;
        Ok(Chosen::from_uris(files.uris().iter().map(|u| u.as_str())))
    });
    match classify(result) {
        Chosen::Paths(mut paths) => PortalEvent::SaveChosen {
            request: id,
            path: paths.swap_remove(0),
        },
        Chosen::Cancelled => PortalEvent::Cancelled { request: id },
        Chosen::Failed(reason) => PortalEvent::Failed {
            request: id,
            reason,
        },
    }
}

/// Reads `org.freedesktop.appearance color-scheme` through the settings
/// portal.
///
/// The encoding is the portal's: 0 no preference, 1 dark, 2 light. Anything
/// else is a portal we do not understand, which is not an error — it just
/// means we keep our own default.
#[cfg(feature = "portals")]
fn color_scheme() -> ColorScheme {
    use ashpd::desktop::settings::{ColorScheme as AshColorScheme, Settings};

    let read: Option<AshColorScheme> = pollster::block_on(async {
        let settings: Settings = Settings::new().await.ok()?;
        let scheme: AshColorScheme = settings.color_scheme().await.ok()?;
        Some(scheme)
    });
    read.map_or(ColorScheme::NoPreference, from_portal)
}

#[cfg(not(feature = "portals"))]
fn open_files(id: PortalRequestId, _req: &OpenFileRequest) -> PortalEvent {
    PortalEvent::Failed {
        request: id,
        reason: "this build was compiled without the `portals` feature".to_owned(),
    }
}

#[cfg(not(feature = "portals"))]
fn save_file(id: PortalRequestId, _req: &SaveFileRequest) -> PortalEvent {
    PortalEvent::Failed {
        request: id,
        reason: "this build was compiled without the `portals` feature".to_owned(),
    }
}

#[cfg(not(feature = "portals"))]
fn color_scheme() -> ColorScheme {
    ColorScheme::NoPreference
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn availability_is_decided_by_the_session_bus_not_by_hope() {
        // Whatever this machine has, the answer must be a decision with a
        // reason attached, never a panic and never a silent assumption.
        match portal_availability() {
            Ok(()) => assert!(
                std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some()
                    || std::env::var_os("XDG_RUNTIME_DIR").is_some()
            ),
            Err(reason) => assert!(reason.contains("D-Bus"), "{reason}"),
        }
    }

    #[test]
    fn request_ids_are_unique_and_increasing() {
        let service = PortalService::offline("test");
        let a = service.handle().open_files(OpenFileRequest::default());
        let b = service.handle().open_files(OpenFileRequest::default());
        let c = service.handle().save_file(SaveFileRequest::default());
        assert!(a < b && b < c);
    }

    #[test]
    fn a_machine_without_portals_answers_with_a_reason_rather_than_hanging() {
        // Offline by construction: on a desktop with a session bus a real
        // dialog would open on the developer's screen and wait for a human.
        let service =
            PortalService::offline("no D-Bus session bus: DBUS_SESSION_BUS_ADDRESS is unset");
        let id = service.handle().open_files(OpenFileRequest {
            title: "Open".to_owned(),
            filters: vec![FileFilter::new("Xara drawings", &["xar"])],
            ..OpenFileRequest::default()
        });

        let mut out = Vec::new();
        for _ in 0..200 {
            service.poll(&mut out);
            if !out.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        match out.first() {
            Some(PortalEvent::Failed { request, reason }) => {
                assert_eq!(*request, id);
                assert!(reason.contains("D-Bus"), "{reason}");
            }
            other => panic!("expected a Failed answer with a reason, got {other:?}"),
        }
    }

    #[test]
    fn the_colour_scheme_query_always_answers() {
        let service = PortalService::start();
        service.handle().query_color_scheme();
        let mut out = Vec::new();
        for _ in 0..200 {
            service.poll(&mut out);
            if !out.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            matches!(out.first(), Some(PortalEvent::ColorSchemeChanged(_))),
            "got {out:?}"
        );
    }

    #[test]
    fn an_answer_wakes_whoever_is_waiting() {
        let woken = Arc::new(AtomicU64::new(0));
        let counter = woken.clone();
        let service = PortalService::start_with_waker(move || {
            counter.fetch_add(1, Ordering::Relaxed);
        });
        service.handle().query_color_scheme();
        let mut out = Vec::new();
        for _ in 0..400 {
            service.poll(&mut out);
            if !out.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!out.is_empty());
        assert!(woken.load(Ordering::Relaxed) >= 1, "the loop was woken");
    }

    #[test]
    fn dropping_the_service_stops_its_thread() {
        let service = PortalService::offline("test");
        let handle = service.handle().clone();
        drop(service);
        // The handle outlives the service; posting to a shut-down service is
        // a no-op, not a panic.
        let _ = handle.open_files(OpenFileRequest::default());
    }

    #[test]
    fn polling_an_idle_service_yields_nothing_and_does_not_block() {
        let service = PortalService::start();
        let mut out = Vec::new();
        service.poll(&mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn filters_keep_their_order_and_shape() {
        let f = FileFilter::new("Xara drawings", &["xar", "web"]);
        assert_eq!(f.name, "Xara drawings");
        assert_eq!(f.extensions, vec!["xar".to_owned(), "web".to_owned()]);
    }

    #[test]
    fn chosen_uris_become_paths_with_escapes_decoded() {
        let c = Chosen::from_uris(["file:///home/a/My%20Drawing.xar", "file:///tmp/b.xar"]);
        assert_eq!(
            c,
            Chosen::Paths(vec![
                PathBuf::from("/home/a/My Drawing.xar"),
                PathBuf::from("/tmp/b.xar")
            ])
        );
    }

    #[test]
    fn an_empty_choice_is_a_cancel_and_a_remote_only_choice_is_a_failure() {
        assert_eq!(Chosen::from_uris([]), Chosen::Cancelled);
        assert!(matches!(
            Chosen::from_uris(["sftp://host/x.xar"]),
            Chosen::Failed(r) if r.contains("no local file")
        ));
    }

    #[cfg(feature = "portals")]
    #[test]
    fn a_cancel_is_the_users_and_everything_else_is_a_failure_with_a_reason() {
        use ashpd::desktop::ResponseError;
        assert_eq!(
            classify(Err(ashpd::Error::Response(ResponseError::Cancelled))),
            Chosen::Cancelled
        );
        assert_eq!(
            classify(Err(ashpd::Error::Response(ResponseError::Other))),
            Chosen::Cancelled,
            "Escape in the GTK portal dialog answers 2, not 1"
        );
        let missing =
            ashpd::Error::PortalNotFound(zbus_names_owned("org.freedesktop.portal.FileChooser"));
        assert!(
            matches!(classify(Err(missing)), Chosen::Failed(r) if r.contains("xdg-desktop-portal"))
        );
        assert!(matches!(
            classify(Err(ashpd::Error::NoResponse)),
            Chosen::Failed(_)
        ));
    }

    #[cfg(feature = "portals")]
    fn zbus_names_owned(name: &str) -> ashpd::zbus::names::OwnedInterfaceName {
        ashpd::zbus::names::InterfaceName::try_from(name)
            .expect("a valid interface name")
            .into()
    }

    #[cfg(feature = "portals")]
    #[test]
    fn filters_become_portal_globs() {
        let f = portal_filters(&[FileFilter::new("Xara drawings", &["xar", "web"])]);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].label(), "Xara drawings");
        assert_eq!(f[0].pattern_filters(), vec!["*.xar", "*.web"]);
    }

    #[test]
    fn dropping_a_busy_service_neither_hangs_nor_serves_the_queue() {
        // A request that blocks like an open dialog: the offline service
        // answers at once, so block the thread with a slow waker instead.
        let gate = Arc::new(std::sync::Barrier::new(2));
        let g = gate.clone();
        let served = Arc::new(AtomicU64::new(0));
        let count = served.clone();
        let service = PortalService::spawn(
            Err("offline".to_owned()),
            Arc::new(move || {
                if count.fetch_add(1, Ordering::SeqCst) == 0 {
                    g.wait(); // the first answer blocks like a dialog
                    std::thread::sleep(Duration::from_secs(2));
                }
            }),
        );
        for _ in 0..3 {
            let _ = service.handle().open_files(OpenFileRequest::default());
        }
        gate.wait(); // the thread is now "inside a dialog"
        let t0 = std::time::Instant::now();
        drop(service);
        assert!(
            t0.elapsed() < Duration::from_secs(1),
            "drop did not wait on the dialog"
        );
        std::thread::sleep(Duration::from_millis(2300));
        assert_eq!(
            served.load(Ordering::SeqCst),
            1,
            "the queued requests were discarded"
        );
    }
}
