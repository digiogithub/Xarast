//! Synthetic `.xar` byte streams, for tests and fuzz seeds.
//!
//! This is **not** a `.xar` writer and never will be: writing the format is a
//! permanent non-goal (`docs/10-architecture.md §3.5`). What it is, is the
//! smallest thing that can put a valid header, a correct deflate block and a
//! correct CRC trailer in front of an arbitrary record sequence, so that the
//! physical layer can be tested exhaustively — every truncation, both
//! imbalances, a bad CRC, a bomb — without any corpus file.
//!
//! That distinction matters for more than pedantry. The 59 real files are
//! Xara's artwork and may not be copied into this repository
//! (`docs/11-licensing-and-clean-room.md §3.2`), so the committed fuzz seed
//! corpus has to be synthetic. This module is how it is generated.

use std::io::Write;

use crate::header::XAR_MAGIC;

/// Builds a `.xar` byte stream record by record.
#[derive(Clone, Debug)]
pub struct XarBuilder {
    out: Vec<u8>,
}

impl Default for XarBuilder {
    fn default() -> XarBuilder {
        XarBuilder::new()
    }
}

impl XarBuilder {
    /// A stream with the magic and a valid `CXN` `TAG_FILEHEADER`.
    #[must_use]
    pub fn new() -> XarBuilder {
        let mut b = XarBuilder {
            out: XAR_MAGIC.to_vec(),
        };
        let mut payload = b"CXN".to_vec();
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(b"Xarast synth\0");
        payload.extend_from_slice(b"0\0");
        payload.extend_from_slice(b"0\0");
        b.push_record(2, &payload);
        b
    }

    /// A stream with the magic but no header record, for negative tests.
    #[must_use]
    pub fn headerless() -> XarBuilder {
        XarBuilder {
            out: XAR_MAGIC.to_vec(),
        }
    }

    /// Appends an uncompressed record.
    #[must_use]
    pub fn record(mut self, tag: u32, payload: &[u8]) -> XarBuilder {
        self.push_record(tag, payload);
        self
    }

    /// Appends `TAG_DOWN`.
    #[must_use]
    pub fn down(self) -> XarBuilder {
        self.record(crate::tags::TAG_DOWN, &[])
    }

    /// Appends `TAG_UP`.
    #[must_use]
    pub fn up(self) -> XarBuilder {
        self.record(crate::tags::TAG_UP, &[])
    }

    /// Appends `TAG_ENDOFFILE`.
    #[must_use]
    pub fn end_of_file(self) -> XarBuilder {
        self.record(crate::tags::TAG_ENDOFFILE, &[])
    }

    /// Appends arbitrary bytes, for building deliberately broken files.
    #[must_use]
    pub fn raw(mut self, bytes: &[u8]) -> XarBuilder {
        self.out.extend_from_slice(bytes);
        self
    }

    /// Appends a complete compressed block: `TAG_STARTCOMPRESSION`, the raw
    /// deflate stream containing the records `f` writes plus the
    /// `TAG_ENDCOMPRESSION` header, and the uncompressed CRC trailer.
    #[must_use]
    pub fn compressed(self, f: impl FnOnce(&mut BlockBuilder)) -> XarBuilder {
        self.compressed_tweaked(f, 99, 0, 0)
    }

    /// As [`XarBuilder::compressed`], but lets a test corrupt the version
    /// word, the CRC or the length so that the verification path can be
    /// exercised.
    #[must_use]
    pub fn compressed_tweaked(
        mut self,
        f: impl FnOnce(&mut BlockBuilder),
        version: u32,
        crc_xor: u32,
        len_delta: u32,
    ) -> XarBuilder {
        let mut block = BlockBuilder { plain: Vec::new() };
        f(&mut block);
        // The END record's header is the last thing inside the stream, and
        // it declares the eight trailer bytes that are NOT in the stream.
        block.header_only(crate::tags::TAG_ENDCOMPRESSION, 8);
        let plain = block.plain;
        let mut crc = flate2::Crc::new();
        crc.update(&plain);
        let crc_value = crc.sum() ^ crc_xor;
        let length = u32::try_from(plain.len())
            .unwrap_or(u32::MAX)
            .wrapping_add(len_delta);

        self.push_record(crate::tags::TAG_STARTCOMPRESSION, &version.to_le_bytes());
        let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
        let _ = enc.write_all(&plain);
        if let Ok(compressed) = enc.finish() {
            self.out.extend_from_slice(&compressed);
        }
        self.out.extend_from_slice(&crc_value.to_le_bytes());
        self.out.extend_from_slice(&length.to_le_bytes());
        self
    }

    /// The finished byte stream.
    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        self.out
    }

    fn push_record(&mut self, tag: u32, payload: &[u8]) {
        self.out.extend_from_slice(&tag.to_le_bytes());
        let size = u32::try_from(payload.len()).unwrap_or(u32::MAX);
        self.out.extend_from_slice(&size.to_le_bytes());
        self.out.extend_from_slice(payload);
    }
}

/// The record sequence inside one compressed block.
///
/// The `TAG_ENDCOMPRESSION` header is appended automatically; writing one
/// here would produce two.
#[derive(Clone, Debug)]
pub struct BlockBuilder {
    plain: Vec<u8>,
}

impl BlockBuilder {
    /// Appends a record to the block.
    pub fn record(&mut self, tag: u32, payload: &[u8]) -> &mut BlockBuilder {
        self.plain.extend_from_slice(&tag.to_le_bytes());
        let size = u32::try_from(payload.len()).unwrap_or(u32::MAX);
        self.plain.extend_from_slice(&size.to_le_bytes());
        self.plain.extend_from_slice(payload);
        self
    }

    /// Appends only a record header, with the size the record declares.
    ///
    /// `TAG_ENDCOMPRESSION` is the one record whose header is inside the
    /// deflate stream while its payload is not.
    pub fn header_only(&mut self, tag: u32, size: u32) -> &mut BlockBuilder {
        self.plain.extend_from_slice(&tag.to_le_bytes());
        self.plain.extend_from_slice(&size.to_le_bytes());
        self
    }

    /// Appends `TAG_DOWN`.
    pub fn down(&mut self) -> &mut BlockBuilder {
        self.record(crate::tags::TAG_DOWN, &[])
    }

    /// Appends `TAG_UP`.
    pub fn up(&mut self) -> &mut BlockBuilder {
        self.record(crate::tags::TAG_UP, &[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ReaderLimits, RecordReader};

    #[test]
    fn what_the_builder_writes_the_reader_reads() {
        let bytes = XarBuilder::new()
            .compressed(|b| {
                b.record(40, &[]).down().record(104, &[]).up();
            })
            .end_of_file()
            .finish();
        let r = RecordReader::new(&bytes, ReaderLimits::default()).unwrap();
        let tags: Vec<u32> = r.collect_records().unwrap().iter().map(|r| r.tag).collect();
        assert_eq!(tags, vec![2, 30, 40, 1, 104, 0, 31, 3]);
    }

    #[test]
    fn a_tweaked_crc_is_detected() {
        let bytes = XarBuilder::new()
            .compressed_tweaked(|b| _ = b.record(40, &[]), 99, 0xFFFF_FFFF, 0)
            .end_of_file()
            .finish();
        let mut r = RecordReader::new(&bytes, ReaderLimits::default()).unwrap();
        while let Some(rec) = r.next_record() {
            rec.unwrap();
        }
        assert!(!r.blocks()[0].ok);
    }

    #[test]
    fn a_tweaked_length_is_detected() {
        let bytes = XarBuilder::new()
            .compressed_tweaked(|b| _ = b.record(40, &[]), 99, 0, 7)
            .end_of_file()
            .finish();
        let mut r = RecordReader::new(&bytes, ReaderLimits::default()).unwrap();
        while let Some(rec) = r.next_record() {
            rec.unwrap();
        }
        assert!(!r.blocks()[0].ok);
    }

    #[test]
    fn a_non_zero_compression_type_warns_but_still_reads() {
        let bytes = XarBuilder::new()
            .compressed_tweaked(|b| _ = b.record(40, &[]), 0x0100_0063, 0, 0)
            .end_of_file()
            .finish();
        let mut r = RecordReader::new(&bytes, ReaderLimits::default()).unwrap();
        while let Some(rec) = r.next_record() {
            rec.unwrap();
        }
        assert!(r.blocks()[0].ok);
        assert!(
            r.diagnostics()
                .iter()
                .any(|d| d.code == crate::DiagCode::UnknownCompressionType)
        );
    }
}
