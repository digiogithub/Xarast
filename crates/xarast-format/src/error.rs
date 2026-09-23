//! Errors (the package cannot be used) and diagnostics (it can, with a warning).
//!
//! The split follows `research/06`: a zip-slip name, a duplicate entry or a
//! zip bomb is an error — the file is refused. A manifest that disagrees with
//! the ZIP, a newer `min-reader` or a missing capability is a
//! [`Diagnostic`]: the file opens, the user is told, and
//! [`Diagnostic::suggests_read_only`] says whether read-only mode should be
//! offered (§3.4 rule 9, §7.4). The reader never opens silently past one.

use std::io;

use thiserror::Error;

use crate::Version;
use crate::manifest::ManifestError;
use crate::name::NameError;

/// Why a package could not be opened or an entry could not be read.
#[derive(Debug, Error)]
pub enum ReadError {
    /// An I/O error from the underlying reader.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    /// The first 64 bytes are not the `.xarast` signature.
    #[error("not a .xarast package (signature mismatch)")]
    NotXarast,
    /// The ZIP structure is damaged or unsupported.
    #[error("corrupt container: {0}")]
    Corrupt(&'static str),
    /// The ZIP layer reported an error.
    #[error("ZIP error: {0}")]
    Zip(String),
    /// An entry name breaks `research/06 §3.1`.
    #[error("invalid entry name {name:?}: {reason}")]
    BadName {
        /// The name, lossily decoded.
        name: String,
        /// Why it was rejected.
        reason: NameError,
    },
    /// Two entries share a name.
    #[error("duplicate entries in the archive")]
    DuplicateEntries,
    /// More entries than [`crate::Limits::max_entries`].
    #[error("{declared} entries declared, limit {limit}")]
    TooManyEntries {
        /// Declared by the end record.
        declared: u64,
        /// The limit in force.
        limit: u64,
    },
    /// An entry's declared ratio exceeds [`crate::Limits::max_entry_ratio`],
    /// or it inflated past its declared size.
    #[error("entry {name:?} exceeds the decompression limits")]
    ZipBomb {
        /// The entry.
        name: String,
    },
    /// The sum of declared sizes exceeds
    /// [`crate::Limits::max_total_uncompressed`].
    #[error("total uncompressed size {total} exceeds the limit {limit}")]
    TooLarge {
        /// The declared total.
        total: u64,
        /// The limit in force.
        limit: u64,
    },
    /// ZIP encryption is forbidden in v1.0 (N4).
    #[error("entry {name:?} is encrypted")]
    Encrypted {
        /// The entry.
        name: String,
    },
    /// A mandatory entry is absent.
    #[error("mandatory entry {0:?} is missing")]
    MissingEntry(&'static str),
    /// The `mimetype` entry is not first, not STORED or has the wrong content.
    #[error("the mimetype entry is not conformant")]
    BadMimetype,
    /// The manifest is malformed.
    #[error("manifest: {0}")]
    Manifest(#[from] ManifestError),
    /// The package was written by an incompatible major version.
    #[error("format major version {0} is not supported")]
    UnsupportedMajor(Version),
    /// No such entry.
    #[error("no entry named {0:?}")]
    NoSuchEntry(String),
    /// The entry uses a compression method this build cannot decode. It can
    /// still be preserved (raw-copied) on save.
    #[error("entry {name:?} uses unsupported compression method {method}")]
    UnsupportedMethod {
        /// The entry.
        name: String,
        /// The ZIP method id.
        method: u16,
    },
    /// The entry's BLAKE3-256 digest does not match the manifest.
    #[error("entry {name:?} does not match its manifest digest")]
    DigestMismatch {
        /// The entry.
        name: String,
    },
}

impl From<zip::result::ZipError> for ReadError {
    fn from(e: zip::result::ZipError) -> Self {
        match e {
            zip::result::ZipError::Io(io) => ReadError::Io(io),
            other => ReadError::Zip(other.to_string()),
        }
    }
}

/// Why a package could not be written.
#[derive(Debug, Error)]
pub enum WriteError {
    /// An I/O error from the underlying writer.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    /// The ZIP layer reported an error.
    #[error("ZIP error: {0}")]
    Zip(String),
    /// An entry name breaks `research/06 §3.1`.
    #[error("invalid entry name {name:?}: {reason}")]
    BadName {
        /// The name.
        name: String,
        /// Why it was rejected.
        reason: NameError,
    },
    /// The name is reserved for an entry the writer produces itself
    /// (`mimetype`, the manifest) or has a dedicated setter.
    #[error("entry name {0:?} is reserved")]
    ReservedName(String),
    /// A mandatory entry was never supplied.
    #[error("mandatory entry {0:?} was not supplied")]
    MissingEntry(&'static str),
    /// The same name was added twice with different content.
    #[error("entry {0:?} added twice")]
    Duplicate(String),
    /// A raw copy was requested but no source package was given, or the
    /// source does not hold the entry.
    #[error("raw copy of {0:?} needs the source package")]
    MissingSource(String),
    /// A thumbnail or preview is not a conformant PNG.
    #[error("invalid thumbnail: {0}")]
    BadThumbnail(&'static str),
    /// A resource extension is not one `research/06 §3.2.7` allows.
    #[error("resource extension {0:?} is not allowed")]
    BadExtension(String),
    /// Reading from the source package failed.
    #[error("source package: {0}")]
    Source(#[from] ReadError),
}

impl From<zip::result::ZipError> for WriteError {
    fn from(e: zip::result::ZipError) -> Self {
        match e {
            zip::result::ZipError::Io(io) => WriteError::Io(io),
            other => WriteError::Zip(other.to_string()),
        }
    }
}

/// A problem that does not prevent opening, and that the user must be told
/// about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diagnostic {
    /// A ZIP entry has no manifest row (§3.4 rule 2).
    UnlistedEntry {
        /// The entry.
        path: String,
    },
    /// A manifest row names an entry the ZIP does not hold.
    MissingEntry {
        /// The row's path.
        path: String,
    },
    /// Two manifest rows share a path.
    DuplicateRow {
        /// The path.
        path: String,
    },
    /// The manifest's `mf:size` disagrees with the ZIP.
    SizeMismatch {
        /// The entry.
        path: String,
        /// What the manifest says.
        manifest: u64,
        /// What the ZIP says.
        actual: u64,
    },
    /// A `document`, `meta` or `resource` row has no digest (§3.4 rule 4).
    MissingDigest {
        /// The entry.
        path: String,
    },
    /// An entry's content does not match its manifest digest.
    DigestMismatch {
        /// The entry.
        path: String,
    },
    /// The root `/` row is missing, duplicated or names another media type
    /// (§3.4 rule 1).
    BadRootEntry,
    /// An entry is out of the normative order (§3.2). Harmless to a reader.
    OutOfOrder {
        /// The first entry found out of place.
        path: String,
    },
    /// `mf:min-reader` is newer than this reader (§7.4 rule 2).
    NewerFormat {
        /// The declared minimum reader version.
        min_reader: Version,
    },
    /// A declared capability is not supported by this reader.
    MissingCapability {
        /// The capability.
        name: String,
        /// `mf:optional="true"`: things look worse, nothing is at risk.
        optional: bool,
    },
    /// A resource's hash-derived name disagrees with its manifest digest.
    ResourceNameMismatch {
        /// The entry.
        path: String,
    },
    /// An entry uses a compression method this build cannot decode.
    UnsupportedMethod {
        /// The entry.
        path: String,
        /// The ZIP method id.
        method: u16,
    },
}

impl Diagnostic {
    /// Whether this diagnostic, on its own, should make the application offer
    /// read-only mode (`research/06 §3.4` rule 9, `§7.4` rule 2).
    pub fn suggests_read_only(&self) -> bool {
        match self {
            Diagnostic::UnlistedEntry { .. }
            | Diagnostic::MissingEntry { .. }
            | Diagnostic::DuplicateRow { .. }
            | Diagnostic::SizeMismatch { .. }
            | Diagnostic::DigestMismatch { .. }
            | Diagnostic::BadRootEntry
            | Diagnostic::NewerFormat { .. }
            | Diagnostic::ResourceNameMismatch { .. } => true,
            Diagnostic::MissingCapability { optional, .. } => !optional,
            Diagnostic::MissingDigest { .. }
            | Diagnostic::OutOfOrder { .. }
            | Diagnostic::UnsupportedMethod { .. } => false,
        }
    }
}
