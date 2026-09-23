//! The top of the application core: every open document, the
//! preferences, and the problem list.
//!
//! `AppState` is what the UI is a projection of. It is owned by the main
//! thread and is never shared: the render thread gets an immutable
//! display list, never this (architecture §5).

use std::path::{Path, PathBuf};

use crate::intent::{Changed, Intent, PlatformRequest};
use crate::prefs::Preferences;
use crate::recent::RecentFiles;
use crate::session::{DocumentId, Session, SessionError};

/// How serious a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Worth knowing.
    Info,
    /// Something was repaired or approximated.
    Warning,
    /// Something did not work.
    Error,
}

/// One line of the non-modal problem list.
///
/// A modal box per importer warning is unusable on a file that produces
/// three hundred of them, which is why this is a list and not a dialog
/// (`phase-05 §U6.4`).
#[derive(Debug, Clone)]
pub struct DiagnosticEntry {
    /// How serious it is.
    pub severity: Severity,
    /// What happened, in one line.
    pub message: String,
    /// Which document it belongs to, when it belongs to one.
    pub document: Option<DocumentId>,
}

/// The problem list.
#[derive(Debug, Clone, Default)]
pub struct DiagnosticLog {
    entries: Vec<DiagnosticEntry>,
    limit: usize,
}

impl DiagnosticLog {
    /// A log holding at most `limit` entries, oldest dropped first.
    #[must_use]
    pub fn with_limit(limit: usize) -> DiagnosticLog {
        DiagnosticLog {
            entries: Vec::new(),
            limit,
        }
    }

    /// Adds an entry.
    pub fn push(&mut self, entry: DiagnosticEntry) {
        if self.limit > 0 && self.entries.len() >= self.limit {
            self.entries.remove(0);
        }
        self.entries.push(entry);
    }

    /// Every entry, oldest first.
    #[must_use]
    pub fn entries(&self) -> &[DiagnosticEntry] {
        &self.entries
    }

    /// How many entries are at or above a severity.
    #[must_use]
    pub fn count_at_least(&self, s: Severity) -> usize {
        self.entries.iter().filter(|e| e.severity >= s).count()
    }

    /// Forgets everything.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Every open document.
#[derive(Debug, Default)]
pub struct DocumentSessions {
    sessions: Vec<Session>,
    next_id: u64,
}

impl DocumentSessions {
    /// No documents open.
    #[must_use]
    pub fn new() -> DocumentSessions {
        DocumentSessions::default()
    }

    /// Allocates the next never-reused document id.
    pub fn next_id(&mut self) -> DocumentId {
        self.next_id += 1;
        DocumentId(self.next_id)
    }

    /// Adopts a session, returning its id.
    pub fn insert(&mut self, session: Session) -> DocumentId {
        let id = session.id;
        self.sessions.push(session);
        id
    }

    /// Closes a document. Returns whether it was open.
    pub fn close(&mut self, id: DocumentId) -> bool {
        let before = self.sessions.len();
        self.sessions.retain(|s| s.id != id);
        self.sessions.len() != before
    }

    /// Looks a session up.
    #[must_use]
    pub fn get(&self, id: DocumentId) -> Option<&Session> {
        self.sessions.iter().find(|s| s.id == id)
    }

    /// Looks a session up for editing.
    pub fn get_mut(&mut self, id: DocumentId) -> Option<&mut Session> {
        self.sessions.iter_mut().find(|s| s.id == id)
    }

    /// Every session, in the order the tabs are shown.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &Session> {
        self.sessions.iter()
    }

    /// How many documents are open.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Whether nothing is open.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// Whether any open document has unsaved changes.
    #[must_use]
    pub fn any_modified(&self) -> bool {
        self.sessions.iter().any(Session::is_modified)
    }
}

/// Everything the UI is a projection of.
#[derive(Debug, Default)]
pub struct AppState {
    /// Every open document.
    pub docs: DocumentSessions,
    /// Which one has the canvas.
    pub active: Option<DocumentId>,
    /// The settings that outlive a session.
    pub prefs: Preferences,
    /// The non-modal problem list.
    pub diagnostics: DiagnosticLog,
    /// Recently opened files, newest first.
    pub recent: RecentFiles,
    /// Where [`AppState::recent`] is persisted; `None` keeps it in memory
    /// (tests, probes, a system with no home directory).
    recent_store: Option<PathBuf>,
    /// What the platform layer has been asked to do and has not done yet.
    requests: Vec<PlatformRequest>,
}

impl AppState {
    /// An application with nothing open.
    #[must_use]
    pub fn new() -> AppState {
        AppState {
            docs: DocumentSessions::new(),
            active: None,
            prefs: Preferences::default(),
            diagnostics: DiagnosticLog::with_limit(1000),
            recent: RecentFiles::new(),
            recent_store: None,
            requests: Vec::new(),
        }
    }

    /// Keeps the recent-files list in `file`, loading what is there now
    /// and dropping entries whose files have gone. A missing or corrupt
    /// file is an empty list.
    #[must_use]
    pub fn with_recent_store(mut self, file: PathBuf) -> AppState {
        self.recent = RecentFiles::load(&file);
        self.recent_store = Some(file);
        if self.recent.prune_missing() > 0 {
            self.save_recent();
        }
        self
    }

    /// Takes the requests the platform layer owes, oldest first.
    pub fn take_requests(&mut self) -> Vec<PlatformRequest> {
        std::mem::take(&mut self.requests)
    }

    /// The document with the canvas.
    #[must_use]
    pub fn active(&self) -> Option<&Session> {
        self.active.and_then(|id| self.docs.get(id))
    }

    /// The document with the canvas, for editing.
    pub fn active_mut(&mut self) -> Option<&mut Session> {
        let id = self.active?;
        self.docs.get_mut(id)
    }

    /// Opens an empty document and makes it active.
    pub fn new_document(&mut self) -> DocumentId {
        let id = self.docs.next_id();
        let id = self.docs.insert(Session::new_empty(id));
        self.active = Some(id);
        id
    }

    /// Takes a document built in memory (a synthetic one, for the
    /// latency probes) and makes it active.
    pub fn adopt(&mut self, doc: xarast_doc::Document) -> DocumentId {
        let id = self.docs.next_id();
        let id = self.docs.insert(Session::adopt(id, doc, None));
        self.active = Some(id);
        id
    }

    /// Opens a file and makes it active.
    ///
    /// Importer diagnostics land in the problem list; they never block
    /// the open.
    ///
    /// # Errors
    ///
    /// Whatever [`Session::open`] returns.
    pub fn open(&mut self, path: &Path) -> Result<DocumentId, SessionError> {
        let id = self.docs.next_id();
        let session = match Session::open(id, path) {
            Ok(s) => s,
            Err(e) => {
                self.diagnostics.push(DiagnosticEntry {
                    severity: Severity::Error,
                    message: e.to_string(),
                    document: None,
                });
                return Err(e);
            }
        };
        for line in session.diagnostics() {
            self.diagnostics.push(DiagnosticEntry {
                severity: Severity::Warning,
                message: line.clone(),
                document: Some(id),
            });
        }
        let id = self.docs.insert(session);
        self.active = Some(id);
        self.remember_recent(path);
        Ok(id)
    }

    /// Closes a document, activating the one before it.
    pub fn close(&mut self, id: DocumentId) -> bool {
        let closed = self.docs.close(id);
        if closed && self.active == Some(id) {
            self.active = self.docs.iter().last().map(|s| s.id);
        }
        closed
    }

    /// Feeds an intent to the active document.
    ///
    /// Returns [`Changed::empty()`] when nothing is open, so a shell does
    /// not have to guard every call.
    ///
    /// # Errors
    ///
    /// Whatever the session returns.
    pub fn apply(&mut self, intent: Intent) -> Result<Changed, SessionError> {
        match intent {
            Intent::ShowOpenDialog => {
                self.requests.push(PlatformRequest::ShowOpenDialog);
                Ok(Changed::empty())
            }
            Intent::Quit => {
                self.requests.push(PlatformRequest::Quit);
                Ok(Changed::empty())
            }
            Intent::OpenFile(path) => self.open_replacing(&path),
            Intent::CloseDocument => {
                let closed = self.active.is_some_and(|id| self.close(id));
                Ok(if closed {
                    Changed::ACTIVE | Changed::UI
                } else {
                    Changed::empty()
                })
            }
            Intent::ClearRecent => {
                self.recent.clear();
                self.save_recent();
                Ok(Changed::UI)
            }
            intent => match self.active_mut() {
                Some(s) => s.apply(intent),
                None => Ok(Changed::empty()),
            },
        }
    }

    /// Opens `path` as the only document (the single-document model of
    /// the first usable viewer). The documents open before are closed
    /// only once the new one has opened, so a file that fails to open
    /// leaves the current one on screen; the failure is in the problem
    /// list and in the returned error. A path that fails is also dropped
    /// from the recent files, since picking it again cannot work.
    ///
    /// # Errors
    ///
    /// Whatever [`Session::open`] returns.
    pub fn open_replacing(&mut self, path: &Path) -> Result<Changed, SessionError> {
        match self.open(path) {
            Ok(id) => {
                let others: Vec<DocumentId> = self
                    .docs
                    .iter()
                    .map(|s| s.id)
                    .filter(|d| *d != id)
                    .collect();
                for d in others {
                    self.docs.close(d);
                }
                self.active = Some(id);
                Ok(Changed::ACTIVE | Changed::DOCUMENT | Changed::UI | Changed::CACHE)
            }
            Err(e) => {
                if self.recent.forget(path) {
                    self.save_recent();
                }
                Err(e)
            }
        }
    }

    fn remember_recent(&mut self, path: &Path) {
        self.recent.remember(path);
        self.save_recent();
    }

    fn save_recent(&mut self) {
        let Some(file) = self.recent_store.as_deref() else {
            return;
        };
        if let Err(e) = self.recent.save(file) {
            self.diagnostics.push(DiagnosticEntry {
                severity: Severity::Info,
                message: format!(
                    "Could not save the recent files list to {}: {e}",
                    file.display()
                ),
                document: None,
            });
        }
    }
}
