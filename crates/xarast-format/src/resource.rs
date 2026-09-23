//! Content-addressed resources (`research/06 §3.2.7`, `§4.4`).
//!
//! Every binary resource is identified by the BLAKE3-256 of its uncompressed
//! bytes ([`ResourceId`]) and stored under a name derived from that hash:
//! `resources/<dir>/b3-<first 32 hex>.<ext>`. Deduplication is therefore
//! structural — two entries with the same content and different names cannot
//! exist.
//!
//! [`ResourceIndex`] is the per-document index `hash → (path, refcount)` of
//! §4.4 rule 4. It lives as long as the document is open, so saving never
//! rehashes a resource it already knows, and a resource that came from the
//! package on disk ([`ResourceData::Package`]) is raw-copied rather than
//! decompressed and recompressed (§13.4 item 3).
//!
//! # Reference counting and GC
//!
//! The document layer (the SVG profile, W3/W4) owns the counts: it calls
//! [`ResourceIndex::retain`]/[`ResourceIndex::release`] as references come and
//! go, or recounts from scratch with [`ResourceIndex::begin_recount`] +
//! [`ResourceIndex::count_path`] before a save. [`ResourceIndex::gc`] then
//! drops what nothing references, **except** resources marked as preserved
//! (referenced from data this version does not understand, §8.3 rule 3) or as
//! history (referenced by `history/`, §3.2.8). The preserved scan is
//! deliberately over-inclusive: any occurrence of a resource's path in the
//! foreign text marks it ([`ResourceIndex::mark_referenced_in`]).

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::io::{self, Read, Seek};
use std::sync::Arc;

use crate::digest::{Digest, HashingReader, hex};
use crate::error::WriteError;
use crate::manifest::Role;
use crate::reader::XarastReader;

/// A resource's identity: the BLAKE3-256 of its uncompressed bytes.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ResourceId(pub [u8; 32]);

impl ResourceId {
    /// The id of `bytes`.
    pub fn of(bytes: &[u8]) -> ResourceId {
        ResourceId(Digest::of(bytes).0)
    }

    /// As a [`Digest`].
    pub fn digest(self) -> Digest {
        Digest(self.0)
    }

    /// The full 64-character hexadecimal form, as the manifest records it.
    pub fn to_hex(self) -> String {
        hex(&self.0)
    }

    /// The first 32 hexadecimal characters (128 bits), as the entry name
    /// carries it.
    pub fn short_hex(self) -> String {
        hex(self.0.get(..16).unwrap_or_default())
    }
}

impl From<Digest> for ResourceId {
    fn from(d: Digest) -> Self {
        ResourceId(d.0)
    }
}

impl fmt::Debug for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ResourceId({})", self.to_hex())
    }
}

impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// The normative subdirectories of `resources/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ResourceKind {
    /// Master bitmaps: the pixels the user imported.
    Image,
    /// Derived renditions (crops, baked adjustments).
    Derived,
    /// Fallback rasterisations of what SVG cannot express.
    Baked,
    /// Embedded font subsets.
    Font,
    /// ICC colour profiles.
    Profile,
    /// Brush and stroke definitions.
    Brush,
    /// Any other opaque binary.
    Blob,
}

impl ResourceKind {
    /// Every kind.
    pub const ALL: [ResourceKind; 7] = [
        ResourceKind::Image,
        ResourceKind::Derived,
        ResourceKind::Baked,
        ResourceKind::Font,
        ResourceKind::Profile,
        ResourceKind::Brush,
        ResourceKind::Blob,
    ];

    /// The directory, with its trailing `/`.
    pub fn dir(self) -> &'static str {
        match self {
            ResourceKind::Image => "resources/images/",
            ResourceKind::Derived => "resources/derived/",
            ResourceKind::Baked => "resources/baked/",
            ResourceKind::Font => "resources/fonts/",
            ResourceKind::Profile => "resources/profiles/",
            ResourceKind::Brush => "resources/brushes/",
            ResourceKind::Blob => "resources/blobs/",
        }
    }
}

/// The extensions `research/06 §3.2.7` allows, with their media types.
const EXTENSIONS: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("webp", "image/webp"),
    ("avif", "image/avif"),
    ("jxl", "image/jxl"),
    ("tiff", "image/tiff"),
    ("gif", "image/gif"),
    ("svg", "image/svg+xml"),
    ("woff2", "font/woff2"),
    ("icc", "application/vnd.iccprofile"),
    ("xml", "application/xml"),
    ("bin", "application/octet-stream"),
];

/// The media type of an allowed extension.
pub fn media_type_for(ext: &str) -> Option<&'static str> {
    EXTENSIONS.iter().find(|(e, _)| *e == ext).map(|(_, m)| *m)
}

/// The hash-derived entry name.
pub fn resource_path(kind: ResourceKind, id: ResourceId, ext: &str) -> String {
    format!("{}b3-{}.{ext}", kind.dir(), id.short_hex())
}

/// A resource entry name, taken apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourcePath<'a> {
    /// The directory.
    pub kind: ResourceKind,
    /// The 32 hexadecimal characters of the name.
    pub short_hex: &'a str,
    /// The extension.
    pub ext: &'a str,
}

/// Parses `resources/<dir>/b3-<32 hex>.<ext>`. `None` if the name does not
/// follow the scheme exactly (such an entry is preserved as unknown data).
pub fn parse_resource_path(path: &str) -> Option<ResourcePath<'_>> {
    let kind = ResourceKind::ALL
        .into_iter()
        .find(|k| path.starts_with(k.dir()))?;
    let file = path.get(kind.dir().len()..)?;
    let rest = file.strip_prefix("b3-")?;
    let (short_hex, ext) = rest.split_once('.')?;
    let hex_ok = short_hex.len() == 32
        && short_hex
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    (hex_ok && media_type_for(ext).is_some()).then_some(ResourcePath {
        kind,
        short_hex,
        ext,
    })
}

/// Where a resource's bytes are.
#[derive(Debug, Clone)]
pub enum ResourceData {
    /// In memory: imported or modified in this session.
    Memory(Arc<[u8]>),
    /// Still in the package the document was opened from, at the record's
    /// path. Saving raw-copies it from there.
    Package,
}

/// One resource in the index.
#[derive(Debug, Clone)]
pub struct ResourceRecord {
    /// The identity.
    pub id: ResourceId,
    /// The directory it lives in.
    pub kind: ResourceKind,
    /// The extension of its name.
    pub ext: String,
    /// Uncompressed size in bytes.
    pub size: u64,
    /// Where the bytes are.
    pub data: ResourceData,
    /// References from the document.
    pub refcount: u64,
    /// Referenced from data this version does not understand (§8.3 rule 3).
    pub preserved: bool,
    /// Referenced from `history/` (§3.2.8).
    pub history: bool,
    /// The master a derived rendition was made from (§4.4).
    pub derived_from: Option<ResourceId>,
    /// The operation chain that produced a derived rendition.
    pub derivation: Option<String>,
}

impl ResourceRecord {
    /// The entry name.
    pub fn path(&self) -> String {
        resource_path(self.kind, self.id, &self.ext)
    }

    /// The media type.
    pub fn media_type(&self) -> &'static str {
        media_type_for(&self.ext).unwrap_or("application/octet-stream")
    }

    /// Whether [`ResourceIndex::gc`] keeps it.
    pub fn is_live(&self) -> bool {
        self.refcount > 0 || self.preserved || self.history
    }
}

/// Deduplication statistics since the index was created.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DedupStats {
    /// Inserts that found the resource already present.
    pub hits: u64,
    /// Bytes those inserts did not have to store again.
    pub bytes_saved: u64,
}

/// The per-document resource index.
#[derive(Debug, Clone, Default)]
pub struct ResourceIndex {
    records: HashMap<ResourceId, ResourceRecord>,
    /// Path → id, to detect a (2⁻¹²⁸) prefix collision and for path lookups.
    by_path: BTreeMap<String, ResourceId>,
    stats: DedupStats,
}

impl ResourceIndex {
    /// An empty index.
    pub fn new() -> ResourceIndex {
        ResourceIndex::default()
    }

    /// Adds a resource, or finds it already present, and takes one reference
    /// to it. `ext` must be one `research/06 §3.2.7` allows.
    ///
    /// When the content is already present its existing kind and extension
    /// win: the name is derived from the hash, so there is one entry per
    /// content.
    pub fn insert(
        &mut self,
        kind: ResourceKind,
        ext: &str,
        bytes: impl Into<Arc<[u8]>>,
    ) -> Result<ResourceId, WriteError> {
        let bytes = bytes.into();
        let id = ResourceId::of(&bytes);
        self.insert_hashed(kind, ext, id, ResourceData::Memory(bytes))
    }

    /// As [`ResourceIndex::insert`], hashing while reading: the bytes are
    /// digested as they stream in rather than in a second pass.
    pub fn insert_reader(
        &mut self,
        kind: ResourceKind,
        ext: &str,
        r: impl Read,
    ) -> Result<ResourceId, WriteError> {
        let mut h = HashingReader::new(r);
        let mut buf = Vec::new();
        h.read_to_end(&mut buf)?;
        let (d, _) = h.finish();
        self.insert_hashed(kind, ext, d.into(), ResourceData::Memory(buf.into()))
    }

    fn insert_hashed(
        &mut self,
        kind: ResourceKind,
        ext: &str,
        id: ResourceId,
        data: ResourceData,
    ) -> Result<ResourceId, WriteError> {
        if media_type_for(ext).is_none() {
            return Err(WriteError::BadExtension(ext.to_owned()));
        }
        let size = match &data {
            ResourceData::Memory(b) => b.len() as u64,
            ResourceData::Package => 0,
        };
        if let Some(rec) = self.records.get_mut(&id) {
            rec.refcount = rec.refcount.saturating_add(1);
            self.stats.hits = self.stats.hits.saturating_add(1);
            self.stats.bytes_saved = self.stats.bytes_saved.saturating_add(size);
            return Ok(id);
        }
        let rec = ResourceRecord {
            id,
            kind,
            ext: ext.to_owned(),
            size,
            data,
            refcount: 1,
            preserved: false,
            history: false,
            derived_from: None,
            derivation: None,
        };
        let path = rec.path();
        if self.by_path.get(&path).is_some_and(|other| *other != id) {
            return Err(WriteError::Duplicate(path));
        }
        self.by_path.insert(path, id);
        self.records.insert(id, rec);
        Ok(id)
    }

    /// Builds the index of a package that was just opened: one record per
    /// manifest row with role `resource`, a hash-derived name and a digest
    /// that agrees with it. The bytes stay in the package.
    ///
    /// The initial refcount is the manifest's informative `mf:refcount`, or 1
    /// when absent, so that a save that happens before the document layer
    /// recounts cannot collect anything.
    pub fn from_package<R: Read + Seek>(reader: &XarastReader<R>) -> ResourceIndex {
        let mut index = ResourceIndex::new();
        for row in &reader.manifest().entries {
            if row.effective_role() != Role::Resource {
                continue;
            }
            let (Some(p), Some(d), Some(info)) = (
                parse_resource_path(&row.full_path),
                row.blake3(),
                reader.entry_info(&row.full_path),
            ) else {
                continue;
            };
            let id = ResourceId::from(d);
            if id.short_hex() != p.short_hex || index.records.contains_key(&id) {
                continue;
            }
            let rec = ResourceRecord {
                id,
                kind: p.kind,
                ext: p.ext.to_owned(),
                size: info.size,
                data: ResourceData::Package,
                refcount: row.refcount.unwrap_or(1),
                preserved: false,
                history: false,
                derived_from: row.derived_from.map(ResourceId::from),
                derivation: row.derivation.clone(),
            };
            index.by_path.insert(row.full_path.clone(), id);
            index.records.insert(id, rec);
        }
        index
    }

    /// The record for `id`.
    pub fn get(&self, id: ResourceId) -> Option<&ResourceRecord> {
        self.records.get(&id)
    }

    /// The record stored at `path`.
    pub fn by_path(&self, path: &str) -> Option<&ResourceRecord> {
        self.by_path.get(path).and_then(|id| self.records.get(id))
    }

    /// Number of records, live or not.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// `true` if there are no records.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Deduplication statistics.
    pub fn stats(&self) -> DedupStats {
        self.stats
    }

    /// Every record, in entry-name order (deterministic).
    pub fn records(&self) -> impl Iterator<Item = &ResourceRecord> + '_ {
        self.by_path.values().filter_map(|id| self.records.get(id))
    }

    /// Takes one more reference. `false` if the id is unknown.
    pub fn retain(&mut self, id: ResourceId) -> bool {
        match self.records.get_mut(&id) {
            Some(r) => {
                r.refcount = r.refcount.saturating_add(1);
                true
            }
            None => false,
        }
    }

    /// Drops one reference and returns the new count. The record stays until
    /// [`ResourceIndex::gc`], so an undo can take the reference back.
    pub fn release(&mut self, id: ResourceId) -> Option<u64> {
        let r = self.records.get_mut(&id)?;
        r.refcount = r.refcount.saturating_sub(1);
        Some(r.refcount)
    }

    /// Zeroes every count, before a full recount by the document layer.
    pub fn begin_recount(&mut self) {
        for r in self.records.values_mut() {
            r.refcount = 0;
        }
    }

    /// Counts one reference by entry name. `false` if no record has it.
    pub fn count_path(&mut self, path: &str) -> bool {
        match self.by_path.get(path).copied() {
            Some(id) => self.retain(id),
            None => false,
        }
    }

    /// Marks a resource as referenced from unknown data (§8.3 rule 3).
    pub fn mark_preserved(&mut self, id: ResourceId) -> bool {
        self.records
            .get_mut(&id)
            .map(|r| r.preserved = true)
            .is_some()
    }

    /// Marks a resource as referenced from `history/` (§3.2.8).
    pub fn mark_history(&mut self, id: ResourceId) -> bool {
        self.records
            .get_mut(&id)
            .map(|r| r.history = true)
            .is_some()
    }

    /// Records that `id` is a derived rendition of `master` (§4.4).
    pub fn set_derivation(
        &mut self,
        id: ResourceId,
        master: ResourceId,
        derivation: impl Into<String>,
    ) -> bool {
        match self.records.get_mut(&id) {
            Some(r) => {
                r.derived_from = Some(master);
                r.derivation = Some(derivation.into());
                true
            }
            None => false,
        }
    }

    /// The conservative scan of §8.3 rule 3: every resource whose entry name
    /// occurs anywhere in `foreign_text` is marked preserved. Returns how
    /// many were newly marked.
    pub fn mark_referenced_in(&mut self, foreign_text: &str) -> usize {
        let mut hits = Vec::new();
        let mut rest = foreign_text;
        while let Some(i) = rest.find("resources/") {
            let tail = rest.get(i..).unwrap_or_default();
            // The longest run of name characters: the path ends at a quote,
            // whitespace, `)`, `#` or anything else a name cannot contain.
            let end = tail
                .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '-' | '_')))
                .unwrap_or(tail.len());
            if let Some(id) = tail.get(..end).and_then(|p| self.by_path.get(p)) {
                hits.push(*id);
            }
            rest = tail.get("resources/".len()..).unwrap_or_default();
        }
        let mut n = 0;
        for id in hits {
            if let Some(r) = self.records.get_mut(&id)
                && !r.preserved
            {
                r.preserved = true;
                n += 1;
            }
        }
        n
    }

    /// Drops every record that is not live (refcount 0, not preserved, not
    /// history). Returns what was dropped, in name order.
    pub fn gc(&mut self) -> Vec<ResourceId> {
        let dead: Vec<(String, ResourceId)> = self
            .by_path
            .iter()
            .filter(|(_, id)| self.records.get(id).is_some_and(|r| !r.is_live()))
            .map(|(p, id)| (p.clone(), *id))
            .collect();
        for (p, id) in &dead {
            self.by_path.remove(p);
            self.records.remove(id);
        }
        dead.into_iter().map(|(_, id)| id).collect()
    }

    /// Bytes kept only because of the preserved or history exemptions: the
    /// figure `research/06` risk K9 asks to report.
    pub fn retained_unreferenced_bytes(&self) -> u64 {
        self.records
            .values()
            .filter(|r| r.refcount == 0 && r.is_live())
            .map(|r| r.size)
            .sum()
    }

    /// Reads a resource's bytes: from memory, or from the package it came
    /// from (verified against its digest).
    pub fn read<R: Read + Seek>(
        &self,
        id: ResourceId,
        package: Option<&mut XarastReader<R>>,
    ) -> io::Result<Arc<[u8]>> {
        let rec = self
            .records
            .get(&id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "unknown resource"))?;
        match (&rec.data, package) {
            (ResourceData::Memory(b), _) => Ok(b.clone()),
            (ResourceData::Package, Some(pkg)) => pkg
                .entry(&rec.path())
                .map(Arc::from)
                .map_err(|e| io::Error::other(e.to_string())),
            (ResourceData::Package, None) => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "resource is in the source package, which was not given",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_hash_derived() {
        let id = ResourceId::of(b"pixels");
        let p = resource_path(ResourceKind::Image, id, "png");
        assert_eq!(p, format!("resources/images/b3-{}.png", &id.to_hex()[..32]));
        let parsed = parse_resource_path(&p).unwrap();
        assert_eq!(parsed.kind, ResourceKind::Image);
        assert_eq!(parsed.short_hex, id.short_hex());
        assert_eq!(parsed.ext, "png");
        for bad in [
            "resources/images/b3-0123.png",
            "resources/images/x3-0123456789abcdef0123456789abcdef.png",
            "resources/images/b3-0123456789ABCDEF0123456789abcdef.png",
            "resources/images/b3-0123456789abcdef0123456789abcdef.exe",
            "resources/other/b3-0123456789abcdef0123456789abcdef.png",
            "resources/images/sub/b3-0123456789abcdef0123456789abcdef.png",
        ] {
            assert_eq!(parse_resource_path(bad), None, "{bad}");
        }
    }

    #[test]
    fn dedup_and_refcounts() {
        let mut ix = ResourceIndex::new();
        let img: Arc<[u8]> = vec![7u8; 1000].into();
        let a = ix.insert(ResourceKind::Image, "png", img.clone()).unwrap();
        for _ in 0..7 {
            assert_eq!(
                ix.insert(ResourceKind::Image, "png", img.clone()).unwrap(),
                a
            );
        }
        assert_eq!(ix.len(), 1);
        assert_eq!(ix.get(a).unwrap().refcount, 8);
        assert_eq!(
            ix.stats(),
            DedupStats {
                hits: 7,
                bytes_saved: 7000
            }
        );
        assert!(ix.insert(ResourceKind::Image, "exe", img).is_err());
    }

    #[test]
    fn streaming_insert_matches() {
        let mut ix = ResourceIndex::new();
        let data = vec![3u8; 100_000];
        let a = ix
            .insert_reader(ResourceKind::Blob, "bin", &data[..])
            .unwrap();
        assert_eq!(a, ResourceId::of(&data));
        assert_eq!(ix.get(a).unwrap().size, 100_000);
    }

    #[test]
    fn gc_honours_the_exemptions() {
        let mut ix = ResourceIndex::new();
        let live = ix.insert(ResourceKind::Image, "png", &b"live"[..]).unwrap();
        let dead = ix.insert(ResourceKind::Image, "png", &b"dead"[..]).unwrap();
        let foreign = ix
            .insert(ResourceKind::Blob, "bin", &b"foreign"[..])
            .unwrap();
        let hist = ix.insert(ResourceKind::Image, "jpg", &b"hist"[..]).unwrap();
        ix.begin_recount();
        assert!(ix.count_path(&ix.get(live).unwrap().path()));
        assert!(ix.mark_history(hist));
        let foreign_path = ix.get(foreign).unwrap().path();
        let baggage = format!(r#"<acme:thing href="{foreign_path}#frag"/>"#);
        assert_eq!(ix.mark_referenced_in(&baggage), 1);
        assert_eq!(ix.mark_referenced_in(&baggage), 0);
        assert_eq!(ix.retained_unreferenced_bytes(), 7 + 4);
        assert_eq!(ix.gc(), vec![dead]);
        assert!(ix.get(live).is_some() && ix.get(foreign).is_some() && ix.get(hist).is_some());
        assert!(
            ix.by_path(&resource_path(ResourceKind::Image, dead, "png"))
                .is_none()
        );
    }

    #[test]
    fn release_keeps_the_record_until_gc() {
        let mut ix = ResourceIndex::new();
        let a = ix.insert(ResourceKind::Font, "woff2", &b"f"[..]).unwrap();
        assert_eq!(ix.release(a), Some(0));
        assert!(ix.retain(a));
        assert!(ix.gc().is_empty());
        assert_eq!(ix.release(a), Some(0));
        assert_eq!(ix.gc(), vec![a]);
        assert_eq!(ix.release(a), None);
    }
}
