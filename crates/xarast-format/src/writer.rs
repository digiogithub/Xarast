//! Writing a package (`research/06 §3.2`, `§4`, `§13.4`).
//!
//! [`PackageWriter`] collects the entries first and writes them in one pass
//! at [`PackageWriter::finish`], because the manifest is the *second* entry
//! and has to describe every other one — their sizes and digests must be
//! known before the first byte of the first of them is written.
//!
//! # The `mimetype` entry
//!
//! First, STORED, no extra field, so that its content sits at offset 38
//! (§3.2.1). `zip` adds no extra field unless one is asked for (no extended
//! timestamp, no alignment padding, no ZIP64 block for a 26-byte entry), and
//! `tests/container.rs` asserts the bytes.
//!
//! # Determinism (O8, §13.4 item 5)
//!
//! With [`WriteOptions::deterministic`], the same inputs produce the same
//! bytes: DOS timestamp 1980-01-01 00:00:00, permissions `0o644`, host system
//! `Unix` whatever the OS, canonical entry order, a canonical manifest, and
//! DEFLATE through the workspace's one backend (`miniz_oxide`, see the
//! workspace manifest; `tests/container.rs::deterministic_bytes_are_pinned`
//! fails if it changes).
//!
//! # Raw copies
//!
//! Entries that come unchanged from the package the document was opened
//! from — resources still on disk, preserved unknown entries — are copied
//! compressed, with `ZipWriter::raw_copy_file_touch` (§13.4 item 3, §8.3 rule
//! 2): no decompression, no recompression, same method.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Seek, Write};
use std::sync::Arc;
use std::time::SystemTime;

use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::digest::Digest;
use crate::error::WriteError;
use crate::manifest::{EntryDigest, FileEntry, Manifest, Role};
use crate::name::{Group, group_of, normative_cmp, validate_name};
use crate::policy;
use crate::reader::XarastReader;
use crate::resource::{ResourceData, ResourceIndex, parse_resource_path};
use crate::thumbnail::{PREVIEW_MAX_PX, THUMBNAIL_MAX_PX, check_png};
use crate::{
    DOCUMENT_ENTRY, MANIFEST_ENTRY, META_ENTRY, MIME_TYPE, MIMETYPE_ENTRY, Method, Profile,
    THUMBNAIL_ENTRY,
};

/// Options for [`PackageWriter`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteOptions {
    /// Only [`Profile::Portable`] is written in v0.1; `Compact` is v1.0.
    pub profile: Profile,
    /// DEFLATE level, 0–9: 6 for an interactive save, 9 for "optimised".
    pub deflate_level: u8,
    /// Fixed timestamps: the same inputs give the same bytes.
    pub deterministic: bool,
    /// The entry timestamp when not deterministic; `None` is "now".
    pub mtime: Option<SystemTime>,
    /// `mf:generator`.
    pub generator: String,
}

impl Default for WriteOptions {
    fn default() -> Self {
        WriteOptions {
            profile: Profile::Portable,
            deflate_level: 6,
            deterministic: false,
            mtime: None,
            generator: default_generator(),
        }
    }
}

impl WriteOptions {
    /// The options for a byte-reproducible save.
    pub fn deterministic() -> Self {
        WriteOptions {
            deterministic: true,
            ..WriteOptions::default()
        }
    }
}

/// `Xarast/<version> (<os>; <arch>)`.
pub fn default_generator() -> String {
    format!(
        "Xarast/{} ({}; {})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

#[derive(Debug, Clone)]
enum Payload {
    Bytes(Arc<[u8]>),
    /// Copied compressed from the source package, under the same name.
    Raw,
}

#[derive(Debug, Clone)]
struct Pending {
    payload: Payload,
    row: FileEntry,
}

/// One entry as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenEntry {
    /// The name.
    pub name: String,
    /// The method used.
    pub method: Method,
    /// Uncompressed size.
    pub size: u64,
    /// Copied compressed from the source package.
    pub raw_copy: bool,
}

/// What a save did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WriteReport {
    /// Size of the package.
    pub bytes_written: u64,
    /// Every entry, in the order written.
    pub entries: Vec<WrittenEntry>,
    /// Resource inserts that found the content already present.
    pub resources_deduplicated: u64,
    /// Bytes those inserts did not store again.
    pub bytes_saved_by_dedup: u64,
    /// Bytes kept only because of the preservation or history exemptions.
    pub retained_unreferenced_bytes: u64,
}

impl WriteReport {
    /// The method an entry was written with.
    pub fn method_of(&self, name: &str) -> Option<Method> {
        self.entries
            .iter()
            .find(|e| e.name == name)
            .map(|e| e.method)
    }

    /// How many entries were written under `prefix`.
    pub fn entries_under(&self, prefix: &str) -> usize {
        self.entries
            .iter()
            .filter(|e| e.name.starts_with(prefix))
            .count()
    }
}

/// Carried from a source manifest: the parts of it that are not about any
/// one entry.
#[derive(Debug, Clone, Default)]
struct Carry {
    foreign_attrs: Vec<crate::manifest::ForeignAttr>,
    foreign_children: Vec<crate::manifest::ForeignElement>,
}

/// Builds a package. See the module documentation.
#[derive(Debug, Clone)]
pub struct PackageWriter {
    opts: WriteOptions,
    entries: BTreeMap<String, Pending>,
    carry: Carry,
    dedup_hits: u64,
    dedup_bytes: u64,
    retained: u64,
}

fn check_name(name: &str) -> Result<(), WriteError> {
    validate_name(name).map_err(|reason| WriteError::BadName {
        name: name.to_owned(),
        reason,
    })
}

impl PackageWriter {
    /// An empty package.
    pub fn new(opts: WriteOptions) -> PackageWriter {
        PackageWriter {
            opts,
            entries: BTreeMap::new(),
            carry: Carry::default(),
            dedup_hits: 0,
            dedup_bytes: 0,
            retained: 0,
        }
    }

    fn put(&mut self, name: &str, media_type: &str, role: Role, bytes: Arc<[u8]>) {
        let mut row = FileEntry::new(name, media_type);
        row.role = Some(role);
        self.entries.insert(
            name.to_owned(),
            Pending {
                payload: Payload::Bytes(bytes),
                row,
            },
        );
    }

    /// Sets `meta.xml` (mandatory).
    pub fn set_meta(&mut self, bytes: impl Into<Arc<[u8]>>) {
        self.put(META_ENTRY, "application/xml", Role::Meta, bytes.into());
    }

    /// Sets `document.svg` (mandatory). The SVG profile layer produces it.
    pub fn set_document(&mut self, bytes: impl Into<Arc<[u8]>>) {
        self.put(
            DOCUMENT_ENTRY,
            "image/svg+xml",
            Role::Document,
            bytes.into(),
        );
    }

    /// Sets `thumbnail.png`: RGBA8, longer side ≤ 512 px.
    pub fn set_thumbnail(&mut self, png: impl Into<Arc<[u8]>>) -> Result<(), WriteError> {
        let png = png.into();
        check_png(&png, THUMBNAIL_MAX_PX).map_err(WriteError::BadThumbnail)?;
        self.put(THUMBNAIL_ENTRY, "image/png", Role::Thumbnail, png);
        Ok(())
    }

    /// Sets `previews/spread-<spread>.png` (1-based): RGBA8, ≤ 1024 px.
    pub fn set_preview(
        &mut self,
        spread: u32,
        png: impl Into<Arc<[u8]>>,
    ) -> Result<(), WriteError> {
        if spread == 0 {
            return Err(WriteError::BadThumbnail("spreads are numbered from 1"));
        }
        let png = png.into();
        check_png(&png, PREVIEW_MAX_PX).map_err(WriteError::BadThumbnail)?;
        self.put(
            &format!("previews/spread-{spread}.png"),
            "image/png",
            Role::Preview,
            png,
        );
        Ok(())
    }

    /// Adds any other entry: `history/…`, `extensions/…`, or a foreign one.
    /// The names the writer owns (`mimetype`, the manifest, `meta.xml`,
    /// `document.svg`, `thumbnail.png`, `previews/`, `resources/`) are
    /// refused: they have dedicated setters.
    pub fn add_entry(
        &mut self,
        name: &str,
        media_type: &str,
        role: Role,
        bytes: impl Into<Arc<[u8]>>,
    ) -> Result<(), WriteError> {
        check_name(name)?;
        if !matches!(
            group_of(name),
            Group::History | Group::Extensions | Group::Other
        ) {
            return Err(WriteError::ReservedName(name.to_owned()));
        }
        if name.starts_with("META-INF/manifest") {
            return Err(WriteError::ReservedName(name.to_owned()));
        }
        if self.entries.contains_key(name) {
            return Err(WriteError::Duplicate(name.to_owned()));
        }
        self.put(name, media_type, role, bytes.into());
        Ok(())
    }

    /// Adds every live resource of the index: in-memory ones as bytes, ones
    /// still in the source package as raw copies. Run [`ResourceIndex::gc`]
    /// first to drop what nothing references; records that are not live are
    /// skipped here anyway.
    pub fn add_resources(&mut self, index: &ResourceIndex) {
        for rec in index.records().filter(|r| r.is_live()) {
            let path = rec.path();
            let mut row = FileEntry::new(path.clone(), rec.media_type());
            row.role = Some(Role::Resource);
            row.size = Some(rec.size);
            row.digest = Some(EntryDigest::Blake3(rec.id.digest()));
            row.refcount = Some(rec.refcount);
            row.derived_from = rec.derived_from.map(|m| m.digest());
            row.derivation.clone_from(&rec.derivation);
            let payload = match &rec.data {
                ResourceData::Memory(b) => Payload::Bytes(b.clone()),
                ResourceData::Package => Payload::Raw,
            };
            self.entries.insert(path, Pending { payload, row });
        }
        let s = index.stats();
        self.dedup_hits = s.hits;
        self.dedup_bytes = s.bytes_saved;
        self.retained = index.retained_unreferenced_bytes();
    }

    /// Carries over, as raw copies with their manifest rows, every entry of
    /// `source` that this writer does not produce itself (§8.3): `history/`,
    /// `extensions/`, other `META-INF/` entries, unknown top-level entries,
    /// and anything under `resources/` that does not follow the hash-derived
    /// naming scheme. Also carries the manifest's own foreign attributes and
    /// children. Entries already set on this writer win.
    pub fn carry_from<R: Read + Seek>(&mut self, source: &XarastReader<R>) {
        let m = source.manifest();
        self.carry.foreign_attrs.clone_from(&m.foreign_attrs);
        self.carry.foreign_children.clone_from(&m.foreign_children);
        for info in source.entries() {
            if info.is_dir || self.entries.contains_key(&info.name) {
                continue;
            }
            let carry = match group_of(&info.name) {
                Group::History | Group::Extensions | Group::Other => true,
                Group::Resources => parse_resource_path(&info.name).is_none(),
                _ => false,
            };
            if !carry {
                continue;
            }
            let row = m.entry(&info.name).cloned().unwrap_or_else(|| {
                let mut r = FileEntry::new(info.name.clone(), "application/octet-stream");
                r.role = Some(Role::Unknown);
                r
            });
            self.entries.insert(
                info.name.clone(),
                Pending {
                    payload: Payload::Raw,
                    row,
                },
            );
        }
    }

    /// Writes the package, with no source package: every payload must be in
    /// memory.
    pub fn finish<W: Write + Seek>(self, out: W) -> Result<WriteReport, WriteError> {
        self.finish_with_source::<W, Cursor<&[u8]>>(out, None)
    }

    /// Writes the package, raw-copying from `source` where asked.
    pub fn finish_with_source<W: Write + Seek, R: Read + Seek>(
        self,
        out: W,
        mut source: Option<&mut XarastReader<R>>,
    ) -> Result<WriteReport, WriteError> {
        if self.opts.profile != Profile::Portable {
            return Err(WriteError::Zip(
                "the compact profile is not implemented yet".into(),
            ));
        }
        for required in [META_ENTRY, DOCUMENT_ENTRY] {
            if !self.entries.contains_key(required) {
                return Err(WriteError::MissingEntry(required));
            }
        }

        // Pass 1: sizes, digests and methods.
        let mut plan: Vec<(String, Pending, Method)> = Vec::with_capacity(self.entries.len());
        for (name, mut p) in self.entries {
            let method = match &p.payload {
                Payload::Bytes(b) => {
                    p.row.size = Some(b.len() as u64);
                    if p.row.digest.is_none() && needs_digest(&p.row) {
                        p.row.digest = Some(EntryDigest::Blake3(Digest::of(b)));
                    }
                    policy::choose(&p.row.media_type, b)
                }
                Payload::Raw => {
                    let src = source
                        .as_deref()
                        .ok_or_else(|| WriteError::MissingSource(name.clone()))?;
                    let info = src
                        .entry_info(&name)
                        .ok_or_else(|| WriteError::MissingSource(name.clone()))?;
                    p.row.size = Some(info.size);
                    // An unknown method is copied as-is; the manifest then
                    // omits `mf:method`, which is informative.
                    info.method.unwrap_or(Method::Stored)
                }
            };
            p.row.method = match &p.payload {
                Payload::Raw => source
                    .as_deref()
                    .and_then(|s| s.entry_info(&name))
                    .and_then(|i| i.method),
                Payload::Bytes(_) => Some(method),
            };
            plan.push((name, p, method));
        }
        plan.sort_by(|a, b| normative_cmp(&a.0, &b.0));

        // Pass 2: the manifest.
        let mut manifest = Manifest::new(self.opts.profile, Some(self.opts.generator.clone()));
        manifest.foreign_attrs = self.carry.foreign_attrs;
        manifest.foreign_children = self.carry.foreign_children;
        let mut mt = FileEntry::new(MIMETYPE_ENTRY, "text/plain");
        mt.role = Some(Role::Mimetype);
        mt.size = Some(MIME_TYPE.len() as u64);
        mt.method = Some(Method::Stored);
        manifest.entries.push(mt);
        let mut mf = FileEntry::new(MANIFEST_ENTRY, "application/xml");
        mf.role = Some(Role::Manifest);
        mf.method = Some(Method::Deflate);
        manifest.entries.push(mf);
        manifest
            .entries
            .extend(plan.iter().map(|(_, p, _)| p.row.clone()));
        let manifest_xml = manifest
            .to_xml()
            .map_err(|e| WriteError::Zip(format!("manifest: {e}")))?;

        // Pass 3: the ZIP.
        let mtime = if self.opts.deterministic {
            zip::DateTime::default()
        } else {
            crate::time::dos_datetime(self.opts.mtime.unwrap_or_else(SystemTime::now))
        };
        let base = SimpleFileOptions::default()
            .system(zip::System::Unix)
            .unix_permissions(0o644)
            .last_modified_time(mtime);
        let stored = base.compression_method(zip::CompressionMethod::Stored);
        let level = i64::from(self.opts.deflate_level.min(9));
        let deflated = |size: u64| {
            base.compression_method(zip::CompressionMethod::Deflated)
                .compression_level(Some(level))
                .large_file(size >= LARGE)
        };

        let mut zip = ZipWriter::new(out);
        let mut written = Vec::with_capacity(plan.len().saturating_add(2));

        zip.start_file(MIMETYPE_ENTRY, stored)?;
        zip.write_all(MIME_TYPE.as_bytes())?;
        written.push(WrittenEntry {
            name: MIMETYPE_ENTRY.into(),
            method: Method::Stored,
            size: MIME_TYPE.len() as u64,
            raw_copy: false,
        });

        zip.start_file(MANIFEST_ENTRY, deflated(manifest_xml.len() as u64))?;
        zip.write_all(manifest_xml.as_bytes())?;
        written.push(WrittenEntry {
            name: MANIFEST_ENTRY.into(),
            method: Method::Deflate,
            size: manifest_xml.len() as u64,
            raw_copy: false,
        });

        for (name, p, method) in plan {
            let size = p.row.size.unwrap_or(0);
            match p.payload {
                Payload::Bytes(b) => {
                    let opts = match method {
                        Method::Deflate => deflated(size),
                        _ => stored.large_file(size >= LARGE),
                    };
                    zip.start_file(name.as_str(), opts)?;
                    zip.write_all(&b)?;
                    written.push(WrittenEntry {
                        name,
                        method,
                        size,
                        raw_copy: false,
                    });
                }
                Payload::Raw => {
                    let src = source
                        .as_deref_mut()
                        .ok_or_else(|| WriteError::MissingSource(name.clone()))?;
                    let f = src.raw_file(&name)?;
                    zip.raw_copy_file_touch(f, mtime, Some(0o644))?;
                    written.push(WrittenEntry {
                        name,
                        method,
                        size,
                        raw_copy: true,
                    });
                }
            }
        }
        let mut out = zip.finish()?;
        let bytes_written = out.stream_position()?;
        out.flush()?;
        Ok(WriteReport {
            bytes_written,
            entries: written,
            resources_deduplicated: self.dedup_hits,
            bytes_saved_by_dedup: self.dedup_bytes,
            retained_unreferenced_bytes: self.retained,
        })
    }
}

/// Above this, an entry is written with a ZIP64 extra field. A little under
/// 4 GiB, because DEFLATE can expand incompressible input slightly.
const LARGE: u64 = 0xFFFF_FFFF - (64 << 20);

/// Every data entry gets a digest: the spec requires one for `document`,
/// `meta` and resources, allows omitting it for the rest, and it is what
/// lets a reader flag corruption.
fn needs_digest(row: &FileEntry) -> bool {
    !matches!(row.role, Some(Role::Mimetype) | Some(Role::Manifest))
}
