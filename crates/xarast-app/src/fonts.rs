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
//!
//! A document that embeds faces (a `.xarast` package's WOFF2 subsets) is
//! laid out with an **overlay** ([`for_document`]): a service over a
//! [`FontDb::overlay`] of the base service's database with the
//! document's faces added for display. Each document gets its own (two
//! documents with the same embedded files share one), so no document's
//! faces reach another (`docs/memory/text.md`, "Embedded fonts on read").

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use xarast_doc::{Document, EmbeddedFont};
use xarast_text::{EmbedError, FaceId, FontDb, Shaper, WebFont};

/// How many web fonts [`FontService::web_font`] remembers.
const WEB_FONT_CACHE: usize = 64;

/// The web fonts made so far, by face and character set.
type WebFontCache = HashMap<(FaceId, Vec<char>), Result<WebFont, EmbedError>>;

/// A font database, its shaper, and whether the system's fonts are in it.
pub struct FontService {
    db: Arc<FontDb>,
    shaper: Shaper,
    /// Set once the database is complete: the number of families known.
    loaded: OnceLock<usize>,
    system: bool,
    /// Web fonts already made: every save of a document embeds the same
    /// subsets, and autosave saves often.
    web_fonts: Mutex<WebFontCache>,
    /// For a document's overlay: what it was made from.
    overlay: Option<OverlayKey>,
}

/// What an overlay was made from: the base service and the document's
/// embedded files, compared by identity (a document and its clones share
/// the files' `Arc`s).
struct OverlayKey {
    base: Arc<FontService>,
    fonts: Vec<EmbeddedFont>,
}

impl OverlayKey {
    fn matches(&self, base: &Arc<FontService>, fonts: &[EmbeddedFont]) -> bool {
        Arc::ptr_eq(&self.base, base) && self.same_fonts(fonts)
    }

    fn same_fonts(&self, fonts: &[EmbeddedFont]) -> bool {
        self.fonts.len() == fonts.len()
            && self
                .fonts
                .iter()
                .zip(fonts)
                .all(|(a, b)| Arc::ptr_eq(&a.data, &b.data) && a.family == b.family)
    }
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
            web_fonts: Mutex::default(),
            overlay: None,
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
            web_fonts: Mutex::default(),
            overlay: None,
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

    /// [`FontDb::web_font`], remembered: the same face and characters give
    /// the same file without subsetting again. `chars` must be sorted.
    ///
    /// # Errors
    ///
    /// As [`FontDb::web_font`].
    pub fn web_font(&self, face: FaceId, chars: &[char]) -> Result<WebFont, EmbedError> {
        let key = (face, chars.to_vec());
        if let Some(hit) = self
            .web_fonts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
        {
            return hit.clone();
        }
        let made = self.db.web_font(face, chars);
        let mut cache = self
            .web_fonts
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if cache.len() >= WEB_FONT_CACHE {
            cache.clear();
        }
        cache.insert(key, made.clone());
        made
    }
}

/// How many document overlays [`for_document`] keeps.
const OVERLAY_CACHE: usize = 8;

/// The overlays made so far, most recently used last.
static OVERLAYS: Mutex<Vec<Arc<FontService>>> = Mutex::new(Vec::new());

/// The fonts `doc` is laid out with over `base`: `base` itself when the
/// document embeds no faces, else an overlay of `base` with the
/// document's faces registered for display
/// ([`FontDb::register_document_face`]). Overlays are remembered (a few,
/// by the identity of the document's files), so walks, saves and tools of
/// one document share one. Making one waits for `base` to be complete.
///
/// A file that is neither an OpenType font nor a WOFF2 file this build
/// reads is skipped: its text draws with the machine's fonts, as before.
#[must_use]
pub fn for_document(base: &Arc<FontService>, doc: &Document) -> Arc<FontService> {
    let fonts = doc.resources.fonts();
    // No faces, or `base` is this document's overlay already.
    if fonts.is_empty() || base.overlay.as_ref().is_some_and(|k| k.same_fonts(fonts)) {
        return Arc::clone(base);
    }
    let mut cache = OVERLAYS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(i) = cache
        .iter()
        .position(|o| o.overlay.as_ref().is_some_and(|k| k.matches(base, fonts)))
    {
        let hit = cache.remove(i);
        cache.push(Arc::clone(&hit));
        return hit;
    }
    base.ready();
    let db = base.db.overlay();
    for f in fonts {
        let bytes: Option<Vec<u8>> = if f.data.starts_with(b"wOF2") {
            xarast_text::embed::woff2::decode(&f.data)
        } else {
            Some(f.data.to_vec())
        };
        if let Some(bytes) = bytes {
            let _ = db.register_document_face(&f.family, bytes);
        }
    }
    let db = Arc::new(db);
    let loaded = OnceLock::new();
    let _ = loaded.set(db.families().len());
    let service = Arc::new(FontService {
        shaper: Shaper::new(Arc::clone(&db)),
        db,
        loaded,
        system: false,
        web_fonts: Mutex::default(),
        overlay: Some(OverlayKey {
            base: Arc::clone(base),
            fonts: fonts.to_vec(),
        }),
    });
    if cache.len() >= OVERLAY_CACHE {
        cache.remove(0);
    }
    cache.push(Arc::clone(&service));
    service
}

/// Whether `fonts` is what [`for_document`] gives for `doc` over `base`
/// (the process's service when `None`), without making anything.
#[must_use]
pub fn serves(fonts: &Arc<FontService>, base: Option<&Arc<FontService>>, doc: &Document) -> bool {
    let doc_fonts = doc.resources.fonts();
    let base = base.cloned().unwrap_or_else(shared);
    if doc_fonts.is_empty()
        || base
            .overlay
            .as_ref()
            .is_some_and(|k| k.same_fonts(doc_fonts))
    {
        return Arc::ptr_eq(fonts, &base);
    }
    fonts
        .overlay
        .as_ref()
        .is_some_and(|k| k.matches(&base, doc_fonts))
}

/// [`for_document`] over the process's service ([`shared`]).
#[must_use]
pub fn document(doc: &Document) -> Arc<FontService> {
    for_document(&shared(), doc)
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
