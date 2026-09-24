//! The font database: system enumeration, matching, substitution, fallback,
//! embedded faces and shared face data.
//!
//! [`FontDb`] wraps `fontique`; no `fontique` type crosses this crate's public
//! API. Faces are named by [`FaceId`], a dense index that is stable for the
//! life of the database, and their bytes are handed out as [`FaceData`], a
//! cheap reference-counted handle (a memory map for system faces).

mod lru;
mod script;
pub(crate) mod substitute;

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use fontique::{
    Blob, Collection, CollectionOptions, FallbackKey, FontInfoOverride, GenericFamily, QueryFamily,
    QueryStatus, SourceCache, SourceKind,
};
use skrifa::MetadataProvider;
use skrifa::string::StringId;

use crate::style::{FontQuery, FontStyle};
use lru::FaceLru;
pub use script::ScriptTag;
use substitute::{Generic, StyleHint};

/// A face in a [`FontDb`]. Dense and stable for the database's lifetime; it is
/// never reused, and it means nothing in another database.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct FaceId(u32);

impl FaceId {
    /// The raw index, for use as a cache key.
    #[must_use]
    pub fn index(self) -> u32 {
        self.0
    }
}

/// The bytes of one face: a shared handle to the font file plus the face's
/// index inside it (non-zero only for collections, `.ttc`/`.otc`).
///
/// Cloning is a reference-count increment. System faces are memory-mapped,
/// so holding one keeps the mapping alive, not a copy of the file.
#[derive(Clone)]
pub struct FaceData {
    blob: Blob<u8>,
    index: u32,
}

impl FaceData {
    /// The whole font file.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        self.blob.as_ref()
    }

    /// The face's index inside the file.
    #[must_use]
    pub fn index(&self) -> u32 {
        self.index
    }

    /// A `skrifa` view of the face, or `None` if the bytes do not parse.
    pub(crate) fn font_ref(&self) -> Option<skrifa::FontRef<'_>> {
        skrifa::FontRef::from_index(self.blob.as_ref(), self.index).ok()
    }

    pub(crate) fn blob_key(&self) -> (u64, u32) {
        (self.blob.id(), self.index)
    }
}

impl fmt::Debug for FaceData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FaceData")
            .field("len", &self.blob.len())
            .field("index", &self.index)
            .finish()
    }
}

/// What the database knows about a face without loading glyphs.
#[derive(Clone, PartialEq, Debug)]
pub struct FaceInfo {
    /// The family this face is registered under.
    pub family: Arc<str>,
    /// CSS weight.
    pub weight: u16,
    /// Upright, italic or oblique.
    pub style: FontStyle,
    /// Width in percent.
    pub stretch: u16,
    /// Registered from bytes (a document's embedded face) rather than found
    /// on the system.
    pub embedded: bool,
}

/// A family substituted for one that is not available.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct FontSubstitution {
    /// The family the document asked for. Never written back into the
    /// document: installing the font later restores it.
    pub requested: Arc<str>,
    /// The family used instead.
    pub used: Arc<str>,
    /// Which rung of the ladder found it.
    pub reason: SubstitutionReason,
}

/// The rung of the substitution ladder (phase 9, W9.1) that produced a match.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum SubstitutionReason {
    /// The name matched after stripping a style suffix ("Arial Bold").
    StyleSuffix,
    /// A metric-compatible alias ("Arial" → "Liberation Sans").
    MetricAlias,
    /// The generic family the missing face belongs to.
    Generic,
    /// Nothing better: the default sans-serif, or the first family known.
    LastResort,
}

/// Synthesis the renderer must apply because the matched face lacks the
/// requested weight or slant.
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct Synthesis {
    /// Embolden the outlines.
    pub embolden: bool,
    /// Skew by this angle in degrees.
    pub skew: Option<f32>,
}

/// The result of [`FontDb::query`].
#[derive(Clone, PartialEq, Debug)]
pub struct FontMatch {
    /// The face.
    pub face: FaceId,
    /// The family actually used.
    pub family: Arc<str>,
    /// `Some` when the requested family was not found and another was used.
    pub substitution: Option<FontSubstitution>,
    /// Whether the face was registered from a document.
    pub embedded: bool,
    /// Faux bold / oblique still to apply.
    pub synthesis: Synthesis,
}

/// Why a font could not be registered.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum FontError {
    /// The bytes are not a font file `fontique` and `skrifa` can read.
    Unreadable,
    /// The file parsed but contained no usable face.
    NoFaces,
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FontError::Unreadable => f.write_str("the data is not a readable font file"),
            FontError::NoFaces => f.write_str("the font file contains no usable face"),
        }
    }
}

impl std::error::Error for FontError {}

/// How a [`FontDb`] is built.
#[derive(Copy, Clone, Debug)]
pub struct FontDbOptions {
    /// Enumerate the system's fonts. Off for deterministic tests and for
    /// headless rendering with pinned fonts.
    pub system_fonts: bool,
    /// How many system faces keep their data mapped at once.
    pub face_cache_capacity: usize,
}

impl Default for FontDbOptions {
    fn default() -> FontDbOptions {
        FontDbOptions {
            system_fonts: true,
            face_cache_capacity: 64,
        }
    }
}

/// Where a face's bytes come from.
#[derive(Clone)]
enum Origin {
    /// Always resident: an embedded face, or one `parley` handed us directly.
    Resident(FaceData),
    /// A system face, loaded on demand and cached in the LRU.
    System(fontique::FontInfo),
}

struct FaceEntry {
    info: FaceInfo,
    origin: Origin,
}

/// Everything behind the database's lock. The `parley` font context lives
/// here too, so shaping sees exactly the faces the database knows.
pub(crate) struct DbInner {
    pub(crate) fcx: parley::FontContext,
    faces: Vec<FaceEntry>,
    /// `(fontique source id, face index)` → face, for system faces.
    by_source: HashMap<(u64, u32), FaceId>,
    /// `(blob id, face index)` → face, for anything whose bytes we have seen.
    by_blob: HashMap<(u64, u32), FaceId>,
    lru: FaceLru,
    substitutions: Vec<FontSubstitution>,
    families_cache: Option<Arc<[Arc<str>]>>,
    pub(crate) outlines: crate::outline::OutlineCache,
}

/// System plus embedded font database.
///
/// All methods take `&self`: the state sits behind one mutex, so a database
/// can be shared across threads in an `Arc`. Enumeration
/// ([`FontDb::load_system_fonts`]) holds that lock for its duration, so call
/// it off the UI thread (phase 9, T9.1.6).
pub struct FontDb {
    inner: Mutex<DbInner>,
}

impl fmt::Debug for FontDb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let n = self.lock().faces.len();
        f.debug_struct("FontDb").field("faces_seen", &n).finish()
    }
}

impl FontDb {
    /// A database meant for the system's fonts, with enumeration deferred:
    /// it holds nothing until [`FontDb::load_system_fonts`] runs, which the
    /// application does on its I/O thread so start-up is never blocked
    /// (phase 9, T9.1.6). Faces may be registered before that.
    #[must_use]
    pub fn new_system() -> FontDb {
        FontDb::with_options(FontDbOptions {
            system_fonts: false,
            ..FontDbOptions::default()
        })
    }

    /// A database with no system fonts at all: only what is registered.
    /// This is what deterministic tests and golden renders use.
    #[must_use]
    pub fn new_isolated() -> FontDb {
        FontDb::with_options(FontDbOptions {
            system_fonts: false,
            ..FontDbOptions::default()
        })
    }

    /// A database built from explicit options. With `system_fonts` set the
    /// system is enumerated **now**, on the calling thread.
    #[must_use]
    pub fn with_options(options: FontDbOptions) -> FontDb {
        let collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: options.system_fonts,
        });
        let fcx = parley::FontContext {
            collection,
            source_cache: SourceCache::default(),
        };
        FontDb {
            inner: Mutex::new(DbInner {
                fcx,
                faces: Vec::new(),
                by_source: HashMap::new(),
                by_blob: HashMap::new(),
                lru: FaceLru::new(options.face_cache_capacity.max(1)),
                substitutions: Vec::new(),
                families_cache: None,
                outlines: crate::outline::OutlineCache::default(),
            }),
        }
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, DbInner> {
        // A panic while the lock was held cannot leave the tables
        // inconsistent in a way that matters more than losing every font, so
        // recover rather than propagate the poison.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Enumerates the system's fonts (fontconfig, DirectWrite or CoreText) on
    /// the calling thread and returns how many families are now known.
    /// Idempotent.
    pub fn load_system_fonts(&self) -> usize {
        let mut g = self.lock();
        g.fcx.collection.load_system_fonts();
        g.families_cache = None;
        g.families().len()
    }

    /// Registers a face from bytes, as a document's embedded font, under
    /// `family` (or the name the font gives itself when `None`).
    ///
    /// Registered families **shadow** system families of the same name: a
    /// query for that name finds only the registered faces from now on.
    pub fn register_embedded(
        &self,
        family: Option<&str>,
        data: impl Into<Arc<[u8]>>,
    ) -> Result<Vec<FaceId>, FontError> {
        let data: Arc<[u8]> = data.into();
        let blob = Blob::new(Arc::new(data));
        if skrifa::FontRef::from_index(blob.as_ref(), 0).is_err() {
            return Err(FontError::Unreadable);
        }
        let family = family.map(substitute::normalise);
        let mut g = self.lock();
        let registered = g.fcx.collection.register_fonts(
            blob.clone(),
            Some(FontInfoOverride {
                family_name: family.as_deref(),
                ..FontInfoOverride::default()
            }),
        );
        g.families_cache = None;
        let mut out = Vec::new();
        for (family_id, fonts) in registered {
            let name: Arc<str> = g
                .fcx
                .collection
                .family_name(family_id)
                .map(Arc::from)
                .unwrap_or_else(|| Arc::from(""));
            for font in fonts {
                out.push(g.face_for_font_info(&font, &name, true));
            }
        }
        if out.is_empty() {
            Err(FontError::NoFaces)
        } else {
            Ok(out)
        }
    }

    /// Every family name, sorted case-insensitively and without duplicates.
    #[must_use]
    pub fn families(&self) -> Arc<[Arc<str>]> {
        self.lock().families()
    }

    /// Matches a query, walking the substitution ladder when the family is
    /// missing. `None` only when the database holds no font at all.
    #[must_use]
    pub fn query(&self, q: &FontQuery) -> Option<FontMatch> {
        self.lock().query(q, None)
    }

    /// [`FontDb::query`] with the document's PANOSE bytes, which pick the
    /// generic family when the face is missing.
    #[must_use]
    pub fn query_with_panose(&self, q: &FontQuery, panose: Option<[u8; 10]>) -> Option<FontMatch> {
        self.lock().query(q, panose)
    }

    /// A face that covers `c`, for when the primary face does not: the
    /// preferred families for `c`'s script, then the platform's fallback for
    /// it, then the generic families. `None` if nothing known covers it.
    #[must_use]
    pub fn fallback_for(&self, c: char, base: &FontQuery) -> Option<FaceId> {
        self.lock().fallback_for(c, base)
    }

    /// Replaces the fallback chain for `script` with `families`, in order,
    /// ahead of the platform's own choice. Unknown families are skipped;
    /// returns how many were kept.
    pub fn set_fallback_preference(&self, script: ScriptTag, families: &[&str]) -> usize {
        let mut g = self.lock();
        let ids: Vec<_> = families
            .iter()
            .filter_map(|f| g.fcx.collection.family_id(&substitute::normalise(f)))
            .collect();
        let n = ids.len();
        g.fcx.collection.set_fallbacks(
            FallbackKey::new(script.to_fontique(), None),
            ids.into_iter(),
        );
        n
    }

    /// Sets the families a generic name resolves to, in order. Used by
    /// isolated databases, which have no platform defaults.
    pub fn set_generic_families(&self, generic: GenericName, families: &[&str]) -> usize {
        let mut g = self.lock();
        let ids: Vec<_> = families
            .iter()
            .filter_map(|f| g.fcx.collection.family_id(&substitute::normalise(f)))
            .collect();
        let n = ids.len();
        g.fcx
            .collection
            .set_generic_families(generic.to_fontique(), ids.into_iter());
        n
    }

    /// The face's bytes. System faces are (re)loaded through the LRU.
    #[must_use]
    pub fn face_data(&self, id: FaceId) -> Option<FaceData> {
        self.lock().face_data(id)
    }

    /// What the database knows about a face.
    #[must_use]
    pub fn face_info(&self, id: FaceId) -> Option<FaceInfo> {
        self.lock().faces.get(id.0 as usize).map(|e| e.info.clone())
    }

    /// True when the face's `OS/2.fsType` forbids embedding it in a document
    /// (restricted-licence embedding, or bitmap-only embedding). A face whose
    /// table cannot be read is treated as installable, the OpenType default.
    #[must_use]
    pub fn embedding_denied(&self, id: FaceId) -> bool {
        !self.embed_rights(id).level.allowed()
    }

    /// Every substitution made so far, in the order first made.
    #[must_use]
    pub fn substitutions(&self) -> Vec<FontSubstitution> {
        self.lock().substitutions.clone()
    }

    /// How many system faces currently have their data resident.
    #[must_use]
    pub fn resident_system_faces(&self) -> usize {
        self.lock().lru.len()
    }
}

/// The generic family names an isolated database can be given.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum GenericName {
    /// `sans-serif`.
    SansSerif,
    /// `serif`.
    Serif,
    /// `monospace`.
    Monospace,
    /// `system-ui`, the platform's UI face; the last rung of the ladder.
    SystemUi,
}

impl GenericName {
    fn to_fontique(self) -> GenericFamily {
        match self {
            GenericName::SansSerif => GenericFamily::SansSerif,
            GenericName::Serif => GenericFamily::Serif,
            GenericName::Monospace => GenericFamily::Monospace,
            GenericName::SystemUi => GenericFamily::SystemUi,
        }
    }
}

fn to_fontique_attrs(q: &FontQuery) -> fontique::Attributes {
    fontique::Attributes::new(
        fontique::FontWidth::from_percentage(f32::from(q.stretch.clamp(50, 200))),
        match q.style {
            FontStyle::Normal => fontique::FontStyle::Normal,
            FontStyle::Italic => fontique::FontStyle::Italic,
            FontStyle::Oblique(a) => fontique::FontStyle::Oblique(a),
        },
        fontique::FontWeight::new(f32::from(q.weight.clamp(1, 1000))),
    )
}

fn from_fontique_style(s: fontique::FontStyle) -> FontStyle {
    match s {
        fontique::FontStyle::Normal => FontStyle::Normal,
        fontique::FontStyle::Italic => FontStyle::Italic,
        fontique::FontStyle::Oblique(a) => FontStyle::Oblique(a),
    }
}

fn round_u16(v: f32, lo: u16, hi: u16) -> u16 {
    if v.is_finite() {
        // Clamped into u16 range first, so the cast cannot truncate.
        v.round().clamp(f32::from(lo), f32::from(hi)) as u16
    } else {
        lo
    }
}

impl DbInner {
    fn families(&mut self) -> Arc<[Arc<str>]> {
        if let Some(c) = &self.families_cache {
            return c.clone();
        }
        let mut v: Vec<Arc<str>> = self.fcx.collection.family_names().map(Arc::from).collect();
        v.sort_by(|a, b| {
            a.to_lowercase()
                .cmp(&b.to_lowercase())
                .then_with(|| a.cmp(b))
        });
        v.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        let arc: Arc<[Arc<str>]> = Arc::from(v);
        self.families_cache = Some(arc.clone());
        arc
    }

    fn face_for_font_info(
        &mut self,
        font: &fontique::FontInfo,
        family: &Arc<str>,
        embedded: bool,
    ) -> FaceId {
        let key = (font.source().id().to_u64(), font.index());
        if let Some(&id) = self.by_source.get(&key) {
            return id;
        }
        let origin = match font.source().kind() {
            SourceKind::Memory(blob) => Origin::Resident(FaceData {
                blob: blob.clone(),
                index: font.index(),
            }),
            #[allow(unreachable_patterns)]
            _ => Origin::System(font.clone()),
        };
        let info = FaceInfo {
            family: family.clone(),
            weight: round_u16(font.weight().value(), 1, 1000),
            style: from_fontique_style(font.style()),
            stretch: round_u16(font.width().ratio() * 100.0, 50, 200),
            embedded,
        };
        let id = self.push_face(info, origin);
        if let Some(Origin::Resident(d)) = self.faces.get(id.0 as usize).map(|e| &e.origin) {
            let bk = d.blob_key();
            self.by_blob.insert(bk, id);
        }
        self.by_source.insert(key, id);
        id
    }

    fn push_face(&mut self, info: FaceInfo, origin: Origin) -> FaceId {
        // More than four billion distinct faces in one process is not a
        // real situation; saturate rather than wrap if it ever happened.
        let id = FaceId(u32::try_from(self.faces.len()).unwrap_or(u32::MAX));
        self.faces.push(FaceEntry { info, origin });
        id
    }

    /// The face for bytes `parley` shaped with. Normally one we already
    /// loaded (the source cache hands out the same blob); otherwise the face
    /// is recorded from its own name table and kept resident.
    pub(crate) fn face_for_parley(&mut self, font: &parley::FontData) -> FaceId {
        let key = (font.data.id(), font.index);
        if let Some(&id) = self.by_blob.get(&key) {
            return id;
        }
        let data = FaceData {
            blob: font.data.clone(),
            index: font.index,
        };
        let info = info_from_bytes(&data);
        let id = self.push_face(info, Origin::Resident(data));
        self.by_blob.insert(key, id);
        id
    }

    pub(crate) fn face_data(&mut self, id: FaceId) -> Option<FaceData> {
        let entry = self.faces.get(id.0 as usize)?;
        match &entry.origin {
            Origin::Resident(d) => Some(d.clone()),
            Origin::System(info) => {
                if let Some(d) = self.lru.get(id) {
                    return Some(d);
                }
                let info = info.clone();
                let blob = info.load(Some(&mut self.fcx.source_cache))?;
                let data = FaceData {
                    blob,
                    index: info.index(),
                };
                self.by_blob.insert(data.blob_key(), id);
                self.lru.insert(id, data.clone());
                Some(data)
            }
        }
    }

    fn match_in_family(
        &mut self,
        family: &str,
        q: &FontQuery,
    ) -> Option<(FaceId, Arc<str>, Synthesis, bool)> {
        let fam = self.fcx.collection.family_by_name(family)?;
        let attrs = to_fontique_attrs(q);
        let idx = fam.match_index(attrs.width, attrs.style, attrs.weight, true)?;
        let font = fam.fonts().get(idx)?.clone();
        let synth = font.synthesis(attrs.width, attrs.style, attrs.weight);
        let name: Arc<str> = Arc::from(fam.name());
        let embedded = matches!(font.source().kind(), SourceKind::Memory(_));
        let id = self.face_for_font_info(&font, &name, embedded);
        let synthesis = Synthesis {
            embolden: synth.embolden(),
            skew: synth.skew(),
        };
        Some((id, name, synthesis, embedded))
    }

    fn first_generic(&mut self, g: GenericFamily) -> Option<Arc<str>> {
        let id = self.fcx.collection.generic_families(g).next()?;
        self.fcx.collection.family_name(id).map(Arc::from)
    }

    pub(crate) fn query(&mut self, q: &FontQuery, panose: Option<[u8; 10]>) -> Option<FontMatch> {
        let requested = substitute::normalise(&q.family);
        let make = |(face, family, synthesis, embedded): (FaceId, Arc<str>, Synthesis, bool),
                    sub: Option<SubstitutionReason>,
                    requested: &str| FontMatch {
            face,
            substitution: sub.map(|reason| FontSubstitution {
                requested: Arc::from(requested),
                used: family.clone(),
                reason,
            }),
            family,
            embedded,
            synthesis,
        };

        // 1. Exact family name (case-insensitive, whitespace-normalised).
        if let Some(m) = self.match_in_family(&requested, q) {
            return Some(make(m, None, &requested));
        }
        // 2. Style suffix stripped: "Arial Bold" → "Arial" at weight 700.
        if let Some((base, StyleHint { weight, italic })) =
            substitute::strip_style_suffix(&requested)
        {
            let mut q2 = q.clone();
            if let Some(w) = weight {
                q2.weight = w;
            }
            if italic && q2.style == FontStyle::Normal {
                q2.style = FontStyle::Italic;
            }
            if let Some(m) = self.match_in_family(&base, &q2) {
                return Some(self.record(make(
                    m,
                    Some(SubstitutionReason::StyleSuffix),
                    &requested,
                )));
            }
        }
        // 3. Metric-compatible aliases.
        let aliases: Vec<&'static str> = substitute::metric_aliases(&requested).collect();
        for alias in aliases {
            if let Some(m) = self.match_in_family(alias, q) {
                return Some(self.record(make(
                    m,
                    Some(SubstitutionReason::MetricAlias),
                    &requested,
                )));
            }
        }
        // 4. The generic family the missing face belongs to.
        let generic = match substitute::classify(&requested, panose) {
            Generic::SansSerif => GenericFamily::SansSerif,
            Generic::Serif => GenericFamily::Serif,
            Generic::Monospace => GenericFamily::Monospace,
        };
        if let Some(name) = self.first_generic(generic)
            && let Some(m) = self.match_in_family(&name, q)
        {
            return Some(self.record(make(m, Some(SubstitutionReason::Generic), &requested)));
        }
        // 5. Last resort: the UI face, the sans-serif default, then the first
        //    family in sorted order so the choice is deterministic.
        let mut candidates = Vec::new();
        candidates.extend(self.first_generic(GenericFamily::SystemUi));
        candidates.extend(self.first_generic(GenericFamily::SansSerif));
        candidates.extend(self.families().iter().next().cloned());
        for name in candidates {
            if let Some(m) = self.match_in_family(&name, q) {
                return Some(self.record(make(
                    m,
                    Some(SubstitutionReason::LastResort),
                    &requested,
                )));
            }
        }
        None
    }

    fn record(&mut self, m: FontMatch) -> FontMatch {
        if let Some(s) = &m.substitution
            && !self
                .substitutions
                .iter()
                .any(|x| x.requested == s.requested && x.used == s.used)
        {
            self.substitutions.push(s.clone());
        }
        m
    }

    fn fallback_for(&mut self, c: char, base: &FontQuery) -> Option<FaceId> {
        let script = ScriptTag::of(c);
        let attrs = to_fontique_attrs(base);
        let mut found: Option<(fontique::FamilyId, usize)> = None;
        {
            let DbInner { fcx, .. } = self;
            let mut query = fcx.collection.query(&mut fcx.source_cache);
            query.set_families(std::iter::empty::<QueryFamily<'static>>());
            query.set_attributes(attrs);
            query.set_fallbacks(FallbackKey::new(script.to_fontique(), None));
            query.matches_with(|font| {
                if font.charmap().and_then(|m| m.map(c)).is_some() {
                    found = Some(font.family);
                    QueryStatus::Stop
                } else {
                    QueryStatus::Continue
                }
            });
            if found.is_none() {
                query.set_families([
                    QueryFamily::Generic(GenericFamily::SansSerif),
                    QueryFamily::Generic(GenericFamily::Serif),
                    QueryFamily::Generic(GenericFamily::Monospace),
                ]);
                query.matches_with(|font| {
                    if font.charmap().and_then(|m| m.map(c)).is_some() {
                        found = Some(font.family);
                        QueryStatus::Stop
                    } else {
                        QueryStatus::Continue
                    }
                });
            }
        }
        let (family_id, index) = found?;
        let fam = self.fcx.collection.family(family_id)?;
        let font = fam.fonts().get(index)?.clone();
        let name: Arc<str> = Arc::from(fam.name());
        let embedded = matches!(font.source().kind(), SourceKind::Memory(_));
        Some(self.face_for_font_info(&font, &name, embedded))
    }
}

/// Reads a face's identity from its own tables, for faces that reach us as
/// bytes only.
fn info_from_bytes(data: &FaceData) -> FaceInfo {
    let Some(font) = data.font_ref() else {
        return FaceInfo {
            family: Arc::from(""),
            weight: 400,
            style: FontStyle::Normal,
            stretch: 100,
            embedded: false,
        };
    };
    let strings = |id| {
        font.localized_strings(id)
            .english_or_first()
            .map(|s| s.chars().collect::<String>())
    };
    let family = strings(StringId::TYPOGRAPHIC_FAMILY_NAME)
        .or_else(|| strings(StringId::FAMILY_NAME))
        .unwrap_or_default();
    let a = font.attributes();
    FaceInfo {
        family: Arc::from(family),
        weight: round_u16(a.weight.value(), 1, 1000),
        style: match a.style {
            skrifa::attribute::Style::Normal => FontStyle::Normal,
            skrifa::attribute::Style::Italic => FontStyle::Italic,
            skrifa::attribute::Style::Oblique(x) => FontStyle::Oblique(x),
        },
        stretch: round_u16(a.stretch.ratio() * 100.0, 50, 200),
        embedded: false,
    }
}
