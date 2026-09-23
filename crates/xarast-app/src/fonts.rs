//! The application's font database and shaper (phase 9, T9.1.6).
//!
//! One [`FontService`] per process holds one [`FontDb`] and one [`Shaper`]
//! over it. System font enumeration costs ~40 ms (fontconfig, 2 000 families
//! on the reference machine), so it never runs on the path to the first
//! frame of a document without text:
//!
//! * [`FontService::start_loading`] enumerates on a background thread; the
//!   shell calls it at start-up (see [`prewarm`]).
//! * [`FontService::ready`] is what a walk calls when it meets its first
//!   story. It returns at once once enumeration is done, joins it when it is
//!   under way, and runs it on the calling thread when nothing started it.
//!
//! Both paths go through one [`OnceLock`], so enumeration runs exactly once
//! and nobody lays out text against a half-loaded database.
//!
//! Tests and golden renders never see the host's fonts: they build an
//! [`FontService::isolated`] service from pinned fonts and hand it to the
//! walker (`docs/memory/text.md`, invariant 3).

use std::sync::{Arc, OnceLock};

use xarast_text::{FontDb, Shaper};

/// A font database, its shaper, and whether the system's fonts are in it.
pub struct FontService {
    db: Arc<FontDb>,
    shaper: Shaper,
    /// Set once the database is complete: the number of families known.
    loaded: OnceLock<usize>,
    system: bool,
}

impl std::fmt::Debug for FontService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontService")
            .field("system", &self.system)
            .field("families", &self.loaded.get())
            .finish_non_exhaustive()
    }
}

impl FontService {
    /// A service over the system's fonts, not enumerated yet.
    #[must_use]
    pub fn system() -> Arc<FontService> {
        let db = Arc::new(FontDb::new_system());
        Arc::new(FontService {
            shaper: Shaper::new(Arc::clone(&db)),
            db,
            loaded: OnceLock::new(),
            system: true,
        })
    }

    /// A service over exactly the faces already registered in `db`, never
    /// the system's: what tests and golden renders use.
    #[must_use]
    pub fn isolated(db: FontDb) -> Arc<FontService> {
        let db = Arc::new(db);
        let loaded = OnceLock::new();
        let _ = loaded.set(db.families().len());
        Arc::new(FontService {
            shaper: Shaper::new(Arc::clone(&db)),
            db,
            loaded,
            system: false,
        })
    }

    /// An isolated service over the font files (`.ttf`, `.otf`, `.ttc`,
    /// `.otc`) directly inside `dir`, never the system's: what tests,
    /// golden renders and `XARAST_FONT_DIR` use. Every generic family
    /// resolves to "Noto Sans" when the directory has it, else to the first
    /// family registered, so the substitution ladder always ends there.
    /// Unreadable files are skipped.
    #[must_use]
    pub fn from_dir(dir: &std::path::Path) -> Arc<FontService> {
        let db = FontDb::new_isolated();
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
            .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
            .unwrap_or_default();
        files.sort();
        let mut families: Vec<String> = Vec::new();
        for f in files {
            let is_font = f.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                matches!(
                    e.to_ascii_lowercase().as_str(),
                    "ttf" | "otf" | "ttc" | "otc"
                )
            });
            if !is_font {
                continue;
            }
            let Ok(bytes) = std::fs::read(&f) else {
                continue;
            };
            if let Ok(faces) = db.register_embedded(None, bytes) {
                for face in faces {
                    if let Some(info) = db.face_info(face)
                        && !families.iter().any(|x| **x == *info.family)
                    {
                        families.push(info.family.to_string());
                    }
                }
            }
        }
        if let Some(i) = families.iter().position(|f| f == "Noto Sans") {
            let f = families.remove(i);
            families.insert(0, f);
        }
        if let Some(first) = families.first() {
            for g in [
                xarast_text::GenericName::SansSerif,
                xarast_text::GenericName::Serif,
                xarast_text::GenericName::Monospace,
                xarast_text::GenericName::SystemUi,
            ] {
                db.set_generic_families(g, &[first.as_str()]);
            }
        }
        // With no system fallback, every family in the directory is a
        // fallback candidate for the scripts a Latin face lacks; one that
        // does not cover a character is skipped.
        let all: Vec<&str> = families.iter().map(String::as_str).collect();
        for script in [
            xarast_text::ScriptTag::HEBREW,
            xarast_text::ScriptTag::ARABIC,
            xarast_text::ScriptTag::HAN,
            xarast_text::ScriptTag::HIRAGANA,
            xarast_text::ScriptTag::KATAKANA,
        ] {
            db.set_fallback_preference(script, &all);
        }
        FontService::isolated(db)
    }

    /// Starts enumerating the system's fonts on a background thread. Does
    /// nothing when they are loaded, loading, or this service is isolated.
    pub fn start_loading(self: &Arc<Self>) {
        if !self.system || self.loaded.get().is_some() {
            return;
        }
        let me = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("xarast-fonts".into())
            .spawn(move || {
                me.ready();
            });
        // No thread: the first story enumerates on its own thread instead.
        drop(spawned);
    }

    /// Whether the database is complete, without waiting.
    #[must_use]
    pub fn is_loaded(&self) -> bool {
        self.loaded.get().is_some()
    }

    /// The shaper, once the database is complete: waits for (or performs)
    /// system enumeration the first time.
    pub fn ready(&self) -> &Shaper {
        self.loaded.get_or_init(|| {
            if self.system {
                self.db.load_system_fonts()
            } else {
                self.db.families().len()
            }
        });
        &self.shaper
    }

    /// The font database. May still be enumerating: call
    /// [`FontService::ready`] before querying it for layout.
    #[must_use]
    pub fn db(&self) -> &Arc<FontDb> {
        &self.db
    }
}

static SHARED: OnceLock<Arc<FontService>> = OnceLock::new();

/// The process's font service: the system's fonts, unless
/// [`set_shared`] installed another one first or `XARAST_FONT_DIR` names a
/// directory of font files to use instead (golden renders and tests, which
/// must never depend on the machine's fonts).
#[must_use]
pub fn shared() -> Arc<FontService> {
    Arc::clone(
        SHARED.get_or_init(|| match std::env::var_os("XARAST_FONT_DIR") {
            Some(dir) if !dir.is_empty() => FontService::from_dir(std::path::Path::new(&dir)),
            _ => FontService::system(),
        }),
    )
}

/// Installs the process's font service. Returns `false` (and changes
/// nothing) when one is already in use: walkers keep the one they have.
pub fn set_shared(service: Arc<FontService>) -> bool {
    SHARED.set(service).is_ok()
}

/// Starts enumerating the system's fonts in the background, so that the
/// first document with text does not wait for it. Cheap and idempotent;
/// the shell calls it at start-up.
pub fn prewarm() {
    shared().start_loading();
}
