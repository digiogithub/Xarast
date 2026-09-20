//! The one error type the importer returns.
//!
//! Anything recoverable is a [`Diagnostic`](crate::Diagnostic) instead: an
//! error here means the byte stream cannot be walked any further, which for a
//! sequential format with no index is the only unrecoverable condition.

/// Why a `.xar` byte stream could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum XarError {
    /// The first eight bytes are not `58 41 52 41 A3 A3 0D 0A`.
    ///
    /// The `A3 A3 0D 0A` tail is the format's anti-ASCII-transfer trap: an FTP
    /// client in text mode rewrites `0D 0A`, and the signature stops matching.
    #[error("not a Xara file: bad magic")]
    BadMagic,
    /// The stream ended in the middle of something.
    #[error("truncated at offset {0}")]
    Truncated(u64),
    /// The first record after the magic is not `TAG_FILEHEADER` (2).
    #[error("first record is not TAG_FILEHEADER")]
    MissingHeader,
    /// `TAG_FILEHEADER`'s type field is not `CXN`, `CXW` or `CXM`.
    #[error("unknown file type identifier")]
    BadFileType,
    /// `precompression_flags` at offset 11 of the header is non-zero. The
    /// original aborts here too (`IDS_UNKNOWN_COMPRESSION`), because a
    /// non-zero value means the payloads are pre-transformed in a way this
    /// reader has never seen.
    #[error("unsupported precompression flags {0:#x}")]
    BadPrecompression(u32),
    /// The raw DEFLATE stream of a compressed block is malformed.
    #[error("deflate error at offset {offset}")]
    Inflate {
        /// Offset of the compressed block in the physical file.
        offset: u64,
    },
    /// A record payload ran out of bytes while a field was being decoded.
    ///
    /// Distinct from [`XarError::Truncated`], which is about the file; this
    /// one is about one record, and callers usually turn it into a
    /// [`DiagCode::TruncatedRecord`](crate::DiagCode::TruncatedRecord)
    /// diagnostic and carry on with the next record.
    #[error("record payload exhausted at offset {0}")]
    ShortRecord(usize),
    /// A tag on the essential list has no handler, so the file cannot be
    /// represented faithfully and the import must stop.
    #[error("essential tag {0} has no handler; the file cannot be represented")]
    EssentialTag(u32),
    /// A [`ReaderLimits`](crate::ReaderLimits) cap was hit. The `&'static str`
    /// names which one; it never carries anything read from the file.
    #[error("limit exceeded: {0}")]
    Limit(&'static str),
    /// The document model refused to build the document the records
    /// describe. Only the mapping stage
    /// ([`crate::import::import`]) can produce this.
    #[error(transparent)]
    Build(#[from] xarast_doc::BuildError),
}
