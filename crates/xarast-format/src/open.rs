//! Document-level open, and the re-save of an opened package
//! (`research/06 §3`, `§8.3`, `§10.1`; phase 6 W4).
//!
//! [`open`] is the read half of [`save()`](crate::save()): the container
//! (never trusting it beyond its limits), `meta.xml`, then `document.svg`
//! through the SVG reader, with every `resources/…` reference fetched from
//! the package and checked against its digest. The package reader is kept
//! in the result, because a later [`save_opened`] copies the entries it
//! does not rewrite — unknown ZIP entries, `extensions/`, unchanged
//! resources — raw from it (§8.3).
//!
//! Opening writes nothing (F4.10): no lock, no temporary, no touch of the
//! file. Locking is the application's decision
//! ([`DocumentLock`](crate::DocumentLock)).

use std::fs::File;
use std::io::{BufReader, Read, Seek, Write};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use xarast_doc::{DiagCode, Document, DocumentMeta, Severity};

use crate::durability::write_atomic_with;
use crate::error::{ReadError, WriteError};
use crate::limits::Limits;
use crate::reader::XarastReader;
use crate::resource::ResourceIndex;
use crate::save::{SaveOptions, SaveReport, meta_xml};
use crate::svg::read::dom::{self, Child, XmlLimits};
use crate::svg::read::{Preservation, ReadOptions, ReadStats, SvgReadError, read_svg};
use crate::svg::{NS_DC, NS_XARAST, write_svg};
use crate::writer::PackageWriter;

/// Options for [`open`].
#[derive(Debug, Clone, Default)]
pub struct OpenOptions {
    /// Container limits.
    pub limits: Limits,
    /// SVG reader options (its `limits` are replaced by the ones above).
    pub read: ReadOptions,
}

/// Why a package could not be opened as a document.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    /// The container refused.
    #[error(transparent)]
    Package(#[from] ReadError),
    /// `document.svg` could not be read.
    #[error("document.svg: {0}")]
    Svg(#[from] SvgReadError),
}

/// An opened document and everything that came with it.
#[derive(Debug)]
pub struct OpenedDocument<R: Read + Seek> {
    /// The document.
    pub document: Document,
    /// What the SVG reader and the model had to say.
    pub diagnostics: Vec<xarast_doc::Diagnostic>,
    /// What the container had to say (manifest mismatches, a newer
    /// `min-reader`, digest failures).
    pub container: Vec<crate::error::Diagnostic>,
    /// Whether the container suggests opening read-only (§3.4 rule 9).
    pub read_only_suggested: bool,
    /// What was read.
    pub stats: ReadStats,
    /// The preservation digest check (§8.4).
    pub preservation: Preservation,
    /// The package, kept for raw copies on the next save.
    pub package: XarastReader<R>,
    /// Time spent parsing `document.svg` into the model.
    pub parse_time: Duration,
}

/// Opens a `.xarast` file.
///
/// # Errors
///
/// [`OpenError`] when the container or `document.svg` is unusable.
pub fn open(path: &Path) -> Result<OpenedDocument<BufReader<File>>, OpenError> {
    open_with(path, &OpenOptions::default())
}

/// [`open`] with options.
///
/// # Errors
///
/// As [`open`].
pub fn open_with(
    path: &Path,
    opts: &OpenOptions,
) -> Result<OpenedDocument<BufReader<File>>, OpenError> {
    let f = File::open(path).map_err(ReadError::Io)?;
    open_reader(BufReader::new(f), opts)
}

/// Opens a package from any seekable reader.
///
/// # Errors
///
/// As [`open`].
pub fn open_reader<R: Read + Seek>(
    r: R,
    opts: &OpenOptions,
) -> Result<OpenedDocument<R>, OpenError> {
    let mut package = XarastReader::open_with(r, opts.limits)?;
    let container = package.diagnostics().to_vec();
    let read_only_suggested = package.suggests_read_only();
    let svg = package.document_bytes()?;
    let meta = package.meta_bytes().ok();
    let mut read = opts.read.clone();
    read.limits = opts.limits;
    let mut missing: Vec<String> = Vec::new();
    let t = Instant::now();
    let result = {
        let mut fetch = |p: &str| -> Option<Arc<[u8]>> {
            match package.entry(p) {
                Ok(b) => Some(Arc::from(b)),
                Err(_) => {
                    missing.push(p.to_owned());
                    None
                }
            }
        };
        read_svg(&svg, &read, &mut fetch)?
    };
    let parse_time = t.elapsed();
    let mut document = result.document;
    let mut diagnostics = result.diagnostics;
    if let Some(m) = meta {
        read_meta(&m, &mut document.meta, &mut diagnostics);
    }
    for p in missing {
        diagnostics.push(xarast_doc::Diagnostic::new(
            Severity::Warning,
            DiagCode::ChecksumMismatch,
            format!("{p} is missing or corrupt in the package"),
        ));
    }
    Ok(OpenedDocument {
        document,
        diagnostics,
        container,
        read_only_suggested,
        stats: result.stats,
        preservation: result.preservation,
        package,
        parse_time,
    })
}

/// Reads what the model keeps of `meta.xml` (`research/06 §7.2`): title,
/// dates, origin and comment. `meta.xml` is authoritative over the copies
/// in `document.svg`.
fn read_meta(bytes: &[u8], meta: &mut DocumentMeta, diags: &mut Vec<xarast_doc::Diagnostic>) {
    let limits = XmlLimits {
        max_depth: 64,
        max_elements: 100_000,
        max_bytes: 16 << 20,
    };
    let dom = match dom::parse(bytes, limits) {
        Ok(d) => d,
        Err(e) => {
            diags.push(xarast_doc::Diagnostic::new(
                Severity::Warning,
                DiagCode::TruncatedRecord,
                format!("meta.xml is unreadable ({e}); the copy in document.svg is used"),
            ));
            return;
        }
    };
    let Some(root) = dom.root().filter(|r| r.is(NS_XARAST, "meta")) else {
        return;
    };
    let elem = |c: &Child| match c {
        Child::Elem(k) => dom.elem(*k),
        _ => None,
    };
    for e in root.children.iter().filter_map(elem) {
        if e.is(NS_DC, "title") {
            meta.title = Some(e.text());
        } else if e.is(NS_XARAST, "comment") {
            meta.comment = Some(e.text());
        } else if e.is(NS_XARAST, "dates") {
            meta.created = None;
            meta.modified = None;
            for d in e.children.iter().filter_map(elem) {
                let v = crate::svg::read::unix_of_rfc3339(&d.text());
                if d.is(NS_XARAST, "created") {
                    meta.created = v;
                } else if d.is(NS_XARAST, "modified") {
                    meta.modified = v;
                }
            }
        } else if e.is(NS_XARAST, "origin") {
            let g = |n: &str| e.get(NS_XARAST, n).map(str::to_owned);
            meta.producer = g("producer");
            meta.producer_version = g("producer-version");
            meta.producer_build = g("producer-build");
        }
    }
}

/// Saves a document that was opened from `source`, atomically: the SVG and
/// `meta.xml` are written anew; resources the document still uses and that
/// are unchanged are raw-copied from the source package, as are unknown
/// entries (§8.3), and a resource referenced only from foreign data is
/// kept (§8.3 rule 3).
///
/// # Errors
///
/// [`WriteError`] on any I/O or container failure; the target is then
/// untouched.
pub fn save_opened<R: Read + Seek>(
    doc: &Document,
    source: &mut XarastReader<R>,
    path: &Path,
    opts: &SaveOptions,
) -> Result<SaveReport, WriteError> {
    let (writer, mut partial) = prepare_from(doc, source, opts)?;
    let t = Instant::now();
    let package = write_atomic_with(path, opts.atomic, |f| {
        writer.finish_with_source(f, Some(&mut *source))
    })?;
    partial.package_time = t.elapsed();
    Ok(SaveReport { package, ..partial })
}

/// [`save_opened`] to any seekable writer (not atomically).
///
/// # Errors
///
/// As [`save_opened`].
pub fn save_opened_to<W: Write + Seek, R: Read + Seek>(
    doc: &Document,
    source: &mut XarastReader<R>,
    out: W,
    opts: &SaveOptions,
) -> Result<SaveReport, WriteError> {
    let (writer, mut partial) = prepare_from(doc, source, opts)?;
    let t = Instant::now();
    let package = writer.finish_with_source(out, Some(source))?;
    partial.package_time = t.elapsed();
    Ok(SaveReport { package, ..partial })
}

fn prepare_from<R: Read + Seek>(
    doc: &Document,
    source: &XarastReader<R>,
    opts: &SaveOptions,
) -> Result<(PackageWriter, SaveReport), WriteError> {
    let t = Instant::now();
    let mut resources = ResourceIndex::from_package(source);
    resources.begin_recount();
    let svg = write_svg(doc, &mut resources, &opts.svg);
    for (_, b) in doc.tree.foreign_iter() {
        for a in &b.attrs {
            resources.mark_referenced_in(&a.value);
        }
        for c in &b.children {
            resources.mark_referenced_in(&c.raw);
        }
    }
    let meta = meta_xml(doc, &svg.stats, &opts.write.generator);
    let serialise = t.elapsed();
    resources.gc();
    let mut w = PackageWriter::new(opts.write.clone());
    w.set_document(svg.svg.into_bytes());
    w.set_meta(meta.into_bytes());
    w.add_resources(&resources);
    if let Some(png) = &opts.thumbnail {
        w.set_thumbnail(png.clone())?;
    }
    w.carry_from(source);
    Ok((
        w,
        SaveReport {
            package: crate::writer::WriteReport::default(),
            svg: svg.stats,
            foreign_count: svg.foreign_count,
            serialise,
            package_time: Duration::ZERO,
        },
    ))
}
