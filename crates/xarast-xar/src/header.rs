//! The eight magic bytes and `TAG_FILEHEADER`.

use crate::cur::Cur;
use crate::error::XarError;

/// The eight bytes every `.xar` file starts with: `"XARA"` then
/// `A3 A3 0D 0A`.
///
/// That tail is deliberate. An FTP client transferring in text mode rewrites
/// `0D 0A`, and the signature stops matching — the file announces its own
/// corruption instead of half-loading.
pub const XAR_MAGIC: [u8; 8] = [0x58, 0x41, 0x52, 0x41, 0xA3, 0xA3, 0x0D, 0x0A];

/// Which of the three `.xar` dialects a file declares itself to be.
///
/// The binary structure is identical for all three; only *which* records get
/// written differs, so one reader serves them all.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum FileType {
    /// `CXN`: the native `.xar` format.
    Native,
    /// `CXW`: the web format.
    Web,
    /// `CXM`: the "minimal" web format, which wants a default template on
    /// load.
    MinimalWeb,
}

impl FileType {
    /// The three ASCII characters this type is stored as.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            FileType::Native => "CXN",
            FileType::Web => "CXW",
            FileType::MinimalWeb => "CXM",
        }
    }
}

/// `TAG_FILEHEADER` (2), the first record of every file.
///
/// The three producer strings are `Option` because they really can be
/// missing: `Templates/animation.xar` has a 36-byte header where all three
/// are empty or truncated (`research/01 §12.2`). Requiring them rejects a
/// file the original reads happily.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FileHeader {
    /// `CXN`, `CXW` or `CXM`.
    pub file_type: FileType,
    /// The total uncompressed size the writer recorded, for its progress
    /// bar. Advisory only, and often patched in after the fact.
    pub declared_size: u32,
    /// Links a native file to its web counterpart. Always 0 in the corpus.
    pub native_web_link_id: u32,
    /// Must be 0. Any other value means the payloads are pre-transformed in
    /// some way, and the original aborts too.
    pub precompression_flags: u32,
    /// e.g. `"Xara Xtreme"`.
    pub producer: Option<String>,
    /// e.g. `"3.0"`.
    pub producer_version: Option<String>,
    /// e.g. `"0.4366 (Xara)"`.
    pub producer_build: Option<String>,
}

impl FileHeader {
    /// Parses a `TAG_FILEHEADER` payload.
    ///
    /// # Errors
    ///
    /// [`XarError::ShortRecord`] if the fixed 15-byte prefix is not there,
    /// [`XarError::BadFileType`] for an unknown type identifier, and
    /// [`XarError::BadPrecompression`] for a non-zero precompression word.
    pub fn parse(payload: &[u8]) -> Result<FileHeader, XarError> {
        let mut c = Cur::new(payload);
        let ty = c.take(3)?;
        let file_type = match ty {
            b"CXN" => FileType::Native,
            b"CXW" => FileType::Web,
            b"CXM" => FileType::MinimalWeb,
            _ => return Err(XarError::BadFileType),
        };
        let declared_size = c.u32()?;
        let native_web_link_id = c.u32()?;
        let precompression_flags = c.u32()?;
        if precompression_flags != 0 {
            return Err(XarError::BadPrecompression(precompression_flags));
        }
        // Tolerated, not required: see the struct documentation.
        let producer = c.ascii_z().ok().filter(|s| !s.is_empty());
        let producer_version = c.ascii_z().ok().filter(|s| !s.is_empty());
        let producer_build = c.ascii_z().ok().filter(|s| !s.is_empty());
        Ok(FileHeader {
            file_type,
            declared_size,
            native_web_link_id,
            precompression_flags,
            producer,
            producer_version,
            producer_build,
        })
    }
}

/// Whether a byte slice starts with the `.xar` signature.
#[must_use]
pub fn has_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(&XAR_MAGIC)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_bytes(ty: &[u8], precomp: u32, tail: &[u8]) -> Vec<u8> {
        let mut v = ty.to_vec();
        v.extend_from_slice(&3996u32.to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(&precomp.to_le_bytes());
        v.extend_from_slice(tail);
        v
    }

    #[test]
    fn the_worked_example_parses() {
        let h = FileHeader::parse(&header_bytes(b"CXN", 0, b"Xara X\0" as &[u8])).unwrap();
        assert_eq!(h.file_type, FileType::Native);
        assert_eq!(h.declared_size, 3996);
        assert_eq!(h.producer.as_deref(), Some("Xara X"));
        // The two strings that are not there are absent, not an error.
        assert_eq!(h.producer_version, None);
    }

    #[test]
    fn a_header_with_no_strings_is_accepted() {
        let h = FileHeader::parse(&header_bytes(b"CXM", 0, b"")).unwrap();
        assert_eq!(h.file_type, FileType::MinimalWeb);
        assert_eq!(h.producer, None);
    }

    #[test]
    fn non_zero_precompression_is_fatal() {
        let e = FileHeader::parse(&header_bytes(b"CXN", 1, b"")).unwrap_err();
        assert_eq!(e, XarError::BadPrecompression(1));
    }

    #[test]
    fn an_unknown_type_identifier_is_rejected() {
        assert_eq!(
            FileHeader::parse(&header_bytes(b"XXX", 0, b"")).unwrap_err(),
            XarError::BadFileType
        );
    }

    #[test]
    fn a_short_header_does_not_panic() {
        for n in 0..15usize {
            let mut v = b"CXN".to_vec();
            v.resize(n, 0);
            let _ = FileHeader::parse(&v);
        }
    }

    #[test]
    fn the_crlf_trap_rejects_a_text_mode_transfer() {
        let mut mangled = XAR_MAGIC;
        mangled[6] = 0x0A;
        mangled[7] = 0x0D;
        assert!(!has_magic(&mangled));
        assert!(has_magic(&XAR_MAGIC));
    }
}
