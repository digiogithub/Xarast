//! The physical layer: magic, record framing, raw DEFLATE, CRC.
//!
//! # The shape of the file
//!
//! Eight bytes of magic, then a flat sequence of records — `tag: u32 LE`,
//! `size: u32 LE`, then exactly `size` payload bytes. No alignment, no
//! padding, no table of contents, no offsets. The only pointer the format
//! has is the **record number**: the 1-based ordinal of a record, counting
//! every single one including `UP`, `DOWN`, the header and everything inside
//! the compressed blocks. Definitions are referenced by it, so the count has
//! to be exact.
//!
//! # The compression, and the one subtle thing in it
//!
//! From immediately after `TAG_STARTCOMPRESSION`'s four payload bytes, the
//! stream is **raw DEFLATE** — RFC 1951 with no zlib and no gzip wrapper,
//! `windowBits = -15`. Using a zlib-wrapped inflater here fails on the first
//! byte of every file in existence, and is the classic way to lose a day on
//! this format.
//!
//! `TAG_ENDCOMPRESSION` is split across the two streams:
//!
//! 1. its eight-byte record *header* is written **inside** the deflate
//!    stream, so the reader meets it as an ordinary record while inflating;
//! 2. its eight bytes of *payload* are **not** compressed — they sit in the
//!    physical file immediately after the last byte the inflater consumed;
//! 3. they are `u32 LE crc32` and `u32 LE uncompressed_length` over the
//!    block's plain bytes, *including* that record header itself.
//!
//! So on meeting tag 31 the reader drives the inflater to the end of the
//! stream, asks it how many physical bytes it consumed
//! ([`flate2::Decompress::total_in`]), seeks there, switches back to plain
//! mode and reads the trailer. The original performs the same adjustment.
//!
//! A file has as many blocks as it has streamed records plus one: bitmap and
//! sound definitions close the compressed block, write themselves
//! uncompressed, and open a new one. That is why 59 corpus files contain 103
//! blocks. The reader needs no special case for streamed records at all —
//! implement start and end correctly and it falls out.
//!
//! # Bounded allocation
//!
//! Every allocation here is a function of bytes *actually read*, never of a
//! declared length. A twelve-byte file whose one record claims `0xFFFFFFFF`
//! bytes must cost nothing, and a test asserts exactly that. The payload
//! buffer grows as bytes arrive, the inflated window is compacted as it is
//! consumed, and three caps in [`ReaderLimits`] bound the rest.

use crate::diag::{DiagCode, DiagSink, Diagnostic, Severity};
use crate::error::XarError;
use crate::header::{FileHeader, XAR_MAGIC};
use crate::tags::{TAG_ENDCOMPRESSION, TAG_ENDOFFILE, TAG_FILEHEADER, TAG_STARTCOMPRESSION};

/// How much work a single file is allowed to cost.
///
/// These are the numbers that make the fuzz invariants achievable rather
/// than hoped for: without them "never OOM" is a wish, and with them it is
/// arithmetic.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct ReaderLimits {
    /// Floor on how much one compressed block may inflate to, before the
    /// ratio below is applied. 64 MiB.
    pub max_inflated_per_block: usize,
    /// A block may inflate to at most `max(max_inflated_per_block, ratio ×
    /// compressed bytes available)`. This is the decompression-bomb
    /// defence: 200.
    pub inflation_ratio: usize,
    /// Total inflated bytes across the whole file.
    pub max_total_inflated: usize,
    /// The largest record payload that will be read.
    pub max_record_size: usize,
    /// How many records one file may contain.
    ///
    /// The effective budget is the smaller of this and what the input could
    /// possibly hold: a record costs at least its eight header bytes, and
    /// the stream is at most `inflation_ratio` times the file, so a
    /// kilobyte of input can never be millions of records however it
    /// inflates. Without that second bound a tiny decompression bomb of
    /// empty records would still cost a gigabyte of tree.
    pub max_records: u32,
    /// How deep `DOWN` may nest before deeper nodes are flattened. Bounds
    /// the recursion that dropping a [`RecordTree`](crate::RecordTree)
    /// would otherwise do. The deepest corpus file nests 13.
    pub max_tree_depth: usize,
}

impl Default for ReaderLimits {
    fn default() -> ReaderLimits {
        ReaderLimits {
            max_inflated_per_block: 64 << 20,
            inflation_ratio: 200,
            max_total_inflated: 256 << 20,
            max_record_size: 64 << 20,
            max_records: 8_000_000,
            max_tree_depth: 1024,
        }
    }
}

/// One record, as the physical layer hands it over.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Record {
    /// 1-based ordinal over **every** record in the file. The format's only
    /// pointer, so an off-by-one here silently mis-resolves every colour.
    pub number: u32,
    /// The tag.
    pub tag: u32,
    /// The payload, exactly as stored.
    pub data: Vec<u8>,
    /// Byte offset of the record header in the physical file. For
    /// diagnostics; inside a compressed block it is the offset of the
    /// enclosing block's consumed input, which is as precise as the format
    /// allows.
    pub file_offset: u64,
    /// Whether the record was read from inside a compressed block.
    pub compressed: bool,
}

impl Record {
    /// The payload size.
    #[must_use]
    pub fn size(&self) -> usize {
        self.data.len()
    }
}

/// What became of one `START`/`END` compression pair.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct BlockReport {
    /// Physical offset where the deflate stream begins.
    pub start_offset: u64,
    /// How many physical bytes the inflater consumed.
    pub compressed_bytes: u64,
    /// How many plain bytes the block yielded.
    pub inflated_bytes: u32,
    /// The CRC-32 the trailer states.
    pub crc_file: u32,
    /// The CRC-32 over the bytes actually read.
    pub crc_computed: u32,
    /// The uncompressed length the trailer states.
    pub length_file: u32,
    /// The uncompressed length actually read.
    pub length_computed: u32,
    /// Whether both matched.
    pub ok: bool,
}

/// How many bytes are inflated in one go.
const INFLATE_CHUNK: usize = 16 * 1024;
/// How far the inflated window may drift before it is compacted.
const COMPACT_THRESHOLD: usize = 64 * 1024;
/// How much of a record payload is read at a time.
const PAYLOAD_CHUNK: usize = 64 * 1024;

struct Block {
    inflate: flate2::Decompress,
    /// Physical offset of the first byte of the deflate stream.
    stream_start: usize,
    /// Cap on this block's inflated output.
    cap: usize,
    buf: Vec<u8>,
    bpos: usize,
    crc: flate2::Crc,
    /// Plain bytes handed to the caller. This, and not what the inflater
    /// produced, is what the trailer's length and CRC cover.
    consumed: u64,
    produced: usize,
    eof: bool,
}

impl core::fmt::Debug for Block {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Block")
            .field("stream_start", &self.stream_start)
            .field("consumed", &self.consumed)
            .field("produced", &self.produced)
            .field("eof", &self.eof)
            .finish()
    }
}

#[derive(Debug)]
enum Mode {
    Plain,
    Deflate(Box<Block>),
}

/// The streaming record reader.
///
/// Handles raw deflate, the CRC trailer and streamed records transparently:
/// the caller sees one flat record sequence and never has to know which
/// stream a record came out of.
#[derive(Debug)]
pub struct RecordReader<'a> {
    raw: &'a [u8],
    pos: usize,
    header: FileHeader,
    limits: ReaderLimits,
    mode: Mode,
    records: u32,
    record_budget: u32,
    finished: bool,
    diags: DiagSink,
    blocks: Vec<BlockReport>,
    total_inflated: usize,
    trailing: u64,
}

impl<'a> RecordReader<'a> {
    /// Checks the magic, reads `TAG_FILEHEADER` and stops there.
    ///
    /// # Errors
    ///
    /// [`XarError::BadMagic`] when the signature is wrong,
    /// [`XarError::MissingHeader`] when the first record is not tag 2, and
    /// whatever [`FileHeader::parse`] rejects.
    pub fn new(bytes: &'a [u8], limits: ReaderLimits) -> Result<RecordReader<'a>, XarError> {
        if !bytes.starts_with(&XAR_MAGIC) {
            return Err(XarError::BadMagic);
        }
        let mut r = RecordReader {
            raw: bytes,
            pos: XAR_MAGIC.len(),
            header: FileHeader {
                file_type: crate::header::FileType::Native,
                declared_size: 0,
                native_web_link_id: 0,
                precompression_flags: 0,
                producer: None,
                producer_version: None,
                producer_build: None,
            },
            limits,
            mode: Mode::Plain,
            records: 0,
            record_budget: record_budget(bytes.len(), limits),
            finished: false,
            diags: DiagSink::new(),
            blocks: Vec::new(),
            total_inflated: 0,
            trailing: 0,
        };
        // The header is parsed without consuming it, so that `next_record`
        // hands it out as record 1 and the numbering every reference in the
        // file depends on starts where the format says it does.
        let head = bytes
            .get(XAR_MAGIC.len()..XAR_MAGIC.len().saturating_add(8))
            .and_then(|s| <[u8; 8]>::try_from(s).ok())
            .ok_or(XarError::MissingHeader)?;
        let tag = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
        let size = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
        if tag != TAG_FILEHEADER {
            return Err(XarError::MissingHeader);
        }
        let from = XAR_MAGIC.len().saturating_add(8);
        let payload = bytes
            .get(from..from.saturating_add(size))
            .ok_or(XarError::Truncated(from as u64))?;
        r.header = FileHeader::parse(payload)?;
        Ok(r)
    }

    /// The parsed `TAG_FILEHEADER`.
    #[must_use]
    pub const fn header(&self) -> &FileHeader {
        &self.header
    }

    /// The limits in force.
    #[must_use]
    pub const fn limits(&self) -> &ReaderLimits {
        &self.limits
    }

    /// Everything recoverable that has been found so far.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        self.diags.items()
    }

    /// The diagnostic sink, for callers that want the counts.
    #[must_use]
    pub const fn diag_sink(&self) -> &DiagSink {
        &self.diags
    }

    /// One entry per `START`/`END` pair, with its verification result.
    #[must_use]
    pub fn blocks(&self) -> &[BlockReport] {
        &self.blocks
    }

    /// How many records have been handed out, `TAG_FILEHEADER` included.
    #[must_use]
    pub const fn records_read(&self) -> u32 {
        self.records
    }

    /// How many physical bytes sat after `TAG_ENDOFFILE`.
    ///
    /// The corpus has zero in all 59 files, but the format does not promise
    /// it and the reader must not assume `pos == len`.
    #[must_use]
    pub const fn trailing_bytes(&self) -> u64 {
        self.trailing
    }

    /// Whether `TAG_ENDOFFILE` (or the end of the input) has been reached.
    #[must_use]
    pub const fn is_finished(&self) -> bool {
        self.finished
    }

    /// The next record, or `None` once `TAG_ENDOFFILE` or the end of input
    /// has been passed.
    ///
    /// `TAG_ENDOFFILE` itself *is* handed out — it is a record, and the
    /// corpus totals count it — and the call after that returns `None`.
    pub fn next_record(&mut self) -> Option<Result<Record, XarError>> {
        if self.finished {
            return None;
        }
        let file_offset = self.physical_pos();
        let head = match self.read_header_bytes() {
            Ok(Some(h)) => h,
            Ok(None) => {
                self.finished = true;
                self.trailing = 0;
                return None;
            }
            Err(e) => {
                self.finished = true;
                return Some(Err(e));
            }
        };
        let tag = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
        let size = u32::from_le_bytes([head[4], head[5], head[6], head[7]]);
        if self.records >= self.record_budget {
            self.finished = true;
            return Some(Err(XarError::Limit("record count")));
        }
        self.records = self.records.saturating_add(1);
        let number = self.records;
        let compressed = matches!(self.mode, Mode::Deflate(_));

        let result = match tag {
            TAG_STARTCOMPRESSION => self.begin_block(number, size),
            TAG_ENDCOMPRESSION => self.end_block(number, size),
            _ => self.read_payload(size as usize),
        };
        let data = match result {
            Ok(d) => d,
            Err(e) => {
                self.finished = true;
                return Some(Err(e));
            }
        };
        if tag == TAG_ENDOFFILE {
            self.finished = true;
            self.trailing = (self.raw.len() as u64).saturating_sub(self.physical_pos());
            if self.trailing != 0 {
                self.diags.push(
                    Diagnostic::new(DiagCode::TrailingBytes)
                        .at(number, tag)
                        .with_detail(self.trailing),
                );
            }
        }
        Some(Ok(Record {
            number,
            tag,
            data,
            file_offset,
            compressed,
        }))
    }

    /// Drains the reader, returning every record.
    ///
    /// # Errors
    ///
    /// The first [`XarError`] the stream produces.
    pub fn collect_records(mut self) -> Result<Vec<Record>, XarError> {
        let mut out = Vec::new();
        while let Some(r) = self.next_record() {
            out.push(r?);
        }
        Ok(out)
    }

    /// Where we are in the physical file. Inside a block this is the input
    /// the inflater has consumed, which is the best the format allows.
    fn physical_pos(&self) -> u64 {
        match &self.mode {
            Mode::Plain => self.pos as u64,
            Mode::Deflate(b) => (b.stream_start as u64).saturating_add(b.inflate.total_in()),
        }
    }

    /// Reads the eight bytes of a record header, or `None` at a clean end of
    /// input in plain mode.
    fn read_header_bytes(&mut self) -> Result<Option<[u8; 8]>, XarError> {
        let mut head = [0u8; 8];
        let mut got = 0usize;
        while got < 8 {
            let want = 8usize.saturating_sub(got);
            let taken = {
                let chunk = self.read_chunk(want)?;
                let n = chunk.len();
                if n != 0 {
                    let end = got.saturating_add(n);
                    if let Some(dst) = head.get_mut(got..end) {
                        dst.copy_from_slice(chunk);
                    }
                }
                n
            };
            if taken == 0 {
                if got == 0 {
                    return Ok(None);
                }
                return Err(XarError::Truncated(self.physical_pos()));
            }
            got = got.saturating_add(taken);
        }
        Ok(Some(head))
    }

    /// Reads exactly `size` bytes of payload, allocating as they arrive.
    fn read_payload(&mut self, size: usize) -> Result<Vec<u8>, XarError> {
        if size > self.limits.max_record_size {
            self.diags
                .push(Diagnostic::new(DiagCode::LimitExceeded).with_detail(size as u64));
            return Err(XarError::Limit("record size"));
        }
        let mut out: Vec<u8> = Vec::new();
        let mut left = size;
        while left > 0 {
            let chunk = self.read_chunk(left.min(PAYLOAD_CHUNK))?;
            if chunk.is_empty() {
                return Err(XarError::Truncated(self.physical_pos()));
            }
            left = left.saturating_sub(chunk.len());
            out.extend_from_slice(chunk);
        }
        Ok(out)
    }

    /// Hands out up to `want` contiguous bytes from whichever stream is
    /// active, consuming them. An empty slice means end of stream.
    fn read_chunk(&mut self, want: usize) -> Result<&[u8], XarError> {
        if matches!(self.mode, Mode::Deflate(_)) {
            return self.read_chunk_inflated(want);
        }
        let end = self.pos.saturating_add(want).min(self.raw.len());
        let start = self.pos;
        self.pos = end;
        Ok(self.raw.get(start..end).unwrap_or(&[]))
    }

    fn read_chunk_inflated(&mut self, want: usize) -> Result<&[u8], XarError> {
        // Compact first: the window must never grow with the block, and the
        // slice handed out below has to survive until the next call.
        if let Mode::Deflate(b) = &mut self.mode
            && b.bpos >= COMPACT_THRESHOLD
        {
            b.buf.drain(..b.bpos);
            b.bpos = 0;
        }
        // Top the window up before borrowing it.
        loop {
            let need = match &self.mode {
                Mode::Deflate(b) => b.buf.len().saturating_sub(b.bpos) == 0 && !b.eof,
                Mode::Plain => false,
            };
            if !need {
                break;
            }
            self.inflate_more()?;
        }
        let (start, end) = {
            let Mode::Deflate(b) = &mut self.mode else {
                return Ok(&[]);
            };
            let avail = b.buf.len().saturating_sub(b.bpos);
            let n = want.min(avail);
            let start = b.bpos;
            let end = start.saturating_add(n);
            let slice = b.buf.get(start..end).unwrap_or(&[]);
            b.crc.update(slice);
            b.consumed = b.consumed.saturating_add(slice.len() as u64);
            b.bpos = end;
            (start, end)
        };
        let Mode::Deflate(b) = &self.mode else {
            return Ok(&[]);
        };
        Ok(b.buf.get(start..end).unwrap_or(&[]))
    }

    /// Runs the inflater once, appending to the window.
    fn inflate_more(&mut self) -> Result<(), XarError> {
        let raw = self.raw;
        let total_cap = self.limits.max_total_inflated;
        let produced_now;
        {
            let Mode::Deflate(b) = &mut self.mode else {
                return Ok(());
            };
            let consumed_in = usize::try_from(b.inflate.total_in()).unwrap_or(usize::MAX);
            let from = b.stream_start.saturating_add(consumed_in).min(raw.len());
            let input = raw.get(from..).unwrap_or(&[]);
            let mut scratch = [0u8; INFLATE_CHUNK];
            let before_out = b.inflate.total_out();
            let status = b
                .inflate
                .decompress(input, &mut scratch, flate2::FlushDecompress::None)
                .map_err(|_| XarError::Inflate {
                    offset: b.stream_start as u64,
                })?;
            let produced =
                usize::try_from(b.inflate.total_out().saturating_sub(before_out)).unwrap_or(0);
            produced_now = produced;
            if produced > 0 {
                let new = scratch.get(..produced).unwrap_or(&[]);
                b.buf.extend_from_slice(new);
                b.produced = b.produced.saturating_add(produced);
            }
            if status == flate2::Status::StreamEnd {
                b.eof = true;
            } else if input.is_empty() && produced == 0 {
                // No input left and the stream never ended: truncated.
                b.eof = true;
                return Err(XarError::Truncated(
                    (b.stream_start as u64).saturating_add(b.inflate.total_in()),
                ));
            }
            if b.produced > b.cap {
                return Err(XarError::Limit("inflation ratio"));
            }
        }
        self.total_inflated = self.total_inflated.saturating_add(produced_now);
        if self.total_inflated > total_cap {
            return Err(XarError::Limit("total inflated bytes"));
        }
        Ok(())
    }

    /// `TAG_STARTCOMPRESSION`: read the version word, then switch streams.
    fn begin_block(&mut self, number: u32, size: u32) -> Result<Vec<u8>, XarError> {
        let payload = self.read_payload(size as usize)?;
        if matches!(self.mode, Mode::Deflate(_)) {
            self.diags.push(
                Diagnostic::new(DiagCode::UnexpectedCompressionRecord)
                    .at(number, TAG_STARTCOMPRESSION),
            );
            return Ok(payload);
        }
        let version = payload
            .get(..4)
            .and_then(|s| <[u8; 4]>::try_from(s).ok())
            .map_or(0, u32::from_le_bytes);
        if version >> 24 != 0 {
            self.diags.push(
                Diagnostic::new(DiagCode::UnknownCompressionType)
                    .at(number, TAG_STARTCOMPRESSION)
                    .with_detail(u64::from(version >> 24)),
            );
        }
        let available = self.raw.len().saturating_sub(self.pos);
        let cap = self
            .limits
            .max_inflated_per_block
            .max(available.saturating_mul(self.limits.inflation_ratio));
        self.mode = Mode::Deflate(Box::new(Block {
            inflate: flate2::Decompress::new(false),
            stream_start: self.pos,
            cap,
            buf: Vec::new(),
            bpos: 0,
            crc: flate2::Crc::new(),
            consumed: 0,
            produced: 0,
            eof: false,
        }));
        Ok(payload)
    }

    /// `TAG_ENDCOMPRESSION`: close the stream, seek to the uncompressed
    /// trailer and verify it.
    fn end_block(&mut self, number: u32, size: u32) -> Result<Vec<u8>, XarError> {
        if !matches!(self.mode, Mode::Deflate(_)) {
            self.diags.push(
                Diagnostic::new(DiagCode::UnexpectedCompressionRecord)
                    .at(number, TAG_ENDCOMPRESSION),
            );
            return self.read_payload(size as usize);
        }
        // Drive the inflater to the end of the stream so that `total_in` is
        // final; there should be no output left to produce.
        loop {
            let done = match &self.mode {
                Mode::Deflate(b) => b.eof,
                Mode::Plain => true,
            };
            if done {
                break;
            }
            self.inflate_more()?;
        }
        let (start_offset, compressed_bytes, crc_computed, length_computed, leftover) =
            match &self.mode {
                Mode::Deflate(b) => (
                    b.stream_start as u64,
                    b.inflate.total_in(),
                    b.crc.sum(),
                    b.consumed,
                    b.buf.len().saturating_sub(b.bpos),
                ),
                Mode::Plain => (0, 0, 0, 0, 0),
            };
        if leftover != 0 {
            self.diags.push(
                Diagnostic::new(DiagCode::UnconsumedBlockBytes)
                    .at(number, TAG_ENDCOMPRESSION)
                    .with_detail(leftover as u64),
            );
        }
        self.mode = Mode::Plain;
        self.pos = usize::try_from(start_offset.saturating_add(compressed_bytes))
            .unwrap_or(usize::MAX)
            .min(self.raw.len());

        let trailer = self.read_payload(size as usize)?;
        let crc_file = le_u32(&trailer, 0);
        let length_file = le_u32(&trailer, 4);
        let length_computed32 = u32::try_from(length_computed).unwrap_or(u32::MAX);
        let ok = crc_file == crc_computed && length_file == length_computed32;
        if !ok {
            if crc_file != crc_computed {
                self.diags.push(
                    Diagnostic::new(DiagCode::CrcMismatch)
                        .at(number, TAG_ENDCOMPRESSION)
                        .with_detail(u64::from(crc_computed)),
                );
            }
            if length_file != length_computed32 {
                self.diags.push(
                    Diagnostic::new(DiagCode::BlockLengthMismatch)
                        .at(number, TAG_ENDCOMPRESSION)
                        .with_detail(length_computed),
                );
            }
        }
        self.blocks.push(BlockReport {
            start_offset,
            compressed_bytes,
            inflated_bytes: length_computed32,
            crc_file,
            crc_computed,
            length_file,
            length_computed: length_computed32,
            ok,
        });
        Ok(trailer)
    }

    /// Adds a diagnostic from a later stage of the pipeline.
    pub fn push_diag(&mut self, d: Diagnostic) {
        self.diags.push(d);
    }

    /// Whether any stored diagnostic is an error.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diags.count(Severity::Error) > 0
    }

    /// Consumes the reader and returns its diagnostics and block reports.
    #[must_use]
    pub fn into_parts(self) -> (FileHeader, DiagSink, Vec<BlockReport>, u32, u64) {
        (
            self.header,
            self.diags,
            self.blocks,
            self.records,
            self.trailing,
        )
    }
}

/// The most records this input could hold, however it inflates.
fn record_budget(input_len: usize, limits: ReaderLimits) -> u32 {
    let ratio = limits.inflation_ratio.max(1);
    let possible = input_len.saturating_mul(ratio) / 8;
    u32::try_from(possible.saturating_add(16))
        .unwrap_or(u32::MAX)
        .min(limits.max_records)
}

fn le_u32(bytes: &[u8], at: usize) -> u32 {
    bytes
        .get(at..at.saturating_add(4))
        .and_then(|s| <[u8; 4]>::try_from(s).ok())
        .map_or(0, u32::from_le_bytes)
}

/// Reads only the magic and `TAG_FILEHEADER`, inflating nothing.
///
/// # Errors
///
/// As [`RecordReader::new`].
pub fn probe(bytes: &[u8]) -> Result<FileHeader, XarError> {
    let r = RecordReader::new(bytes, ReaderLimits::default())?;
    Ok(r.header().clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::XarBuilder;

    #[test]
    fn a_minimal_file_reads() {
        let bytes = XarBuilder::new().end_of_file().finish();
        let mut r = RecordReader::new(&bytes, ReaderLimits::default()).unwrap();
        let first = r.next_record().unwrap().unwrap();
        assert_eq!(first.tag, TAG_FILEHEADER);
        assert_eq!(first.number, 1);
        let rec = r.next_record().unwrap().unwrap();
        assert_eq!(rec.tag, TAG_ENDOFFILE);
        assert_eq!(rec.number, 2);
        assert!(r.next_record().is_none());
        assert_eq!(r.trailing_bytes(), 0);
        assert_eq!(r.records_read(), 2);
    }

    #[test]
    fn bad_magic_is_rejected() {
        assert_eq!(
            RecordReader::new(b"not a xar file at all", ReaderLimits::default()).unwrap_err(),
            XarError::BadMagic
        );
    }

    #[test]
    fn a_compressed_block_verifies() {
        let bytes = XarBuilder::new()
            .compressed(|b| {
                b.record(40, &[]);
                b.record(1, &[]);
                b.record(0, &[]);
            })
            .end_of_file()
            .finish();
        let mut r = RecordReader::new(&bytes, ReaderLimits::default()).unwrap();
        let mut tags = Vec::new();
        while let Some(rec) = r.next_record() {
            tags.push(rec.unwrap().tag);
        }
        assert_eq!(tags, vec![2, 30, 40, 1, 0, 31, 3]);
        assert_eq!(r.blocks().len(), 1);
        assert!(r.blocks()[0].ok);
        assert_eq!(r.trailing_bytes(), 0);
    }

    #[test]
    fn two_blocks_with_a_streamed_record_between_them() {
        let bytes = XarBuilder::new()
            .compressed(|b| {
                b.record(40, &[]);
            })
            .record(68, b"\x89PNG fake")
            .compressed(|b| {
                b.record(41, &[]);
            })
            .end_of_file()
            .finish();
        let mut r = RecordReader::new(&bytes, ReaderLimits::default()).unwrap();
        let mut tags = Vec::new();
        while let Some(rec) = r.next_record() {
            tags.push(rec.unwrap().tag);
        }
        assert_eq!(tags, vec![2, 30, 40, 31, 68, 30, 41, 31, 3]);
        assert_eq!(r.blocks().len(), 2);
        assert!(r.blocks().iter().all(|b| b.ok));
    }

    #[test]
    fn a_bad_crc_is_a_diagnostic_not_a_failure() {
        let mut bytes = XarBuilder::new()
            .compressed(|b| {
                b.record(40, &[]);
            })
            .end_of_file()
            .finish();
        // Corrupt the trailer's first byte: it is the last 8 bytes before
        // the ENDOFFILE record header.
        let n = bytes.len();
        bytes[n - 8 - 1] ^= 0xFF;
        let mut r = RecordReader::new(&bytes, ReaderLimits::default()).unwrap();
        while let Some(rec) = r.next_record() {
            rec.unwrap();
        }
        assert!(!r.blocks()[0].ok);
        assert!(r.has_errors());
    }

    #[test]
    fn a_truncated_deflate_stream_errors_without_panicking() {
        let full = XarBuilder::new()
            .compressed(|b| {
                for _ in 0..50 {
                    b.record(40, &[1, 2, 3, 4]);
                }
            })
            .end_of_file()
            .finish();
        for cut in (10..full.len()).step_by(7) {
            let mut r = match RecordReader::new(&full[..cut], ReaderLimits::default()) {
                Ok(r) => r,
                Err(_) => continue,
            };
            while let Some(rec) = r.next_record() {
                if rec.is_err() {
                    break;
                }
            }
        }
    }

    #[test]
    fn a_record_declaring_four_gigabytes_costs_nothing() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&XAR_MAGIC);
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&15u32.to_le_bytes());
        bytes.extend_from_slice(b"CXN");
        bytes.extend_from_slice(&[0u8; 12]);
        bytes.extend_from_slice(&104u32.to_le_bytes());
        bytes.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        let mut r = RecordReader::new(&bytes, ReaderLimits::default()).unwrap();
        assert_eq!(r.next_record().unwrap().unwrap().tag, TAG_FILEHEADER);
        let e = r.next_record().unwrap().unwrap_err();
        assert!(matches!(e, XarError::Truncated(_) | XarError::Limit(_)));
    }

    #[test]
    fn trailing_bytes_after_end_of_file_are_reported() {
        let mut bytes = XarBuilder::new().end_of_file().finish();
        bytes.extend_from_slice(b"junk");
        let mut r = RecordReader::new(&bytes, ReaderLimits::default()).unwrap();
        while let Some(rec) = r.next_record() {
            rec.unwrap();
        }
        assert_eq!(r.trailing_bytes(), 4);
    }

    #[test]
    fn a_short_record_size_cap_is_enforced() {
        let limits = ReaderLimits {
            max_record_size: 40,
            ..ReaderLimits::default()
        };
        let bytes = XarBuilder::new()
            .record(104, &[0u8; 64])
            .end_of_file()
            .finish();
        let mut r = RecordReader::new(&bytes, limits).unwrap();
        assert_eq!(r.next_record().unwrap().unwrap().tag, TAG_FILEHEADER);
        assert_eq!(
            r.next_record().unwrap().unwrap_err(),
            XarError::Limit("record size")
        );
    }

    #[test]
    fn every_truncation_of_a_real_looking_file_is_handled() {
        let full = XarBuilder::new()
            .compressed(|b| {
                b.record(40, &[]);
                b.record(1, &[]);
                b.record(51, &[0u8; 31]);
                b.record(0, &[]);
            })
            .end_of_file()
            .finish();
        for cut in 0..full.len() {
            let Ok(mut r) = RecordReader::new(&full[..cut], ReaderLimits::default()) else {
                continue;
            };
            while let Some(rec) = r.next_record() {
                if rec.is_err() {
                    break;
                }
            }
        }
    }
}
