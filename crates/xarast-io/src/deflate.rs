//! A zlib stream compressed in parallel, byte-identical on any thread
//! count.
//!
//! DEFLATE dominates a PNG export: on an A4 page at 300 dpi, level 6 took
//! about four times as long as rasterising. The stream is therefore cut
//! into fixed [`DEFLATE_CHUNK`]-byte pieces of *uncompressed* input, each
//! compressed on its own (no shared dictionary) and ended with a sync
//! flush, so that the pieces are byte-aligned and concatenate into one
//! valid stream; the last piece carries the final block. The zlib header
//! is fixed by the level and the Adler-32 trailer is combined from the
//! pieces' checksums.
//!
//! The cut points are at fixed offsets of the input, never where a strip
//! or a batch happened to end, and each piece is compressed by the same
//! single-threaded backend (`zlib-rs`, the workspace's one): the bytes
//! depend on the input and the level only. The price is the dictionary
//! lost at each cut, well under 1 % at 1 MiB pieces.

use std::io::Write;

use flate2::{Compress, Compression, FlushCompress};
use rayon::prelude::*;

/// Uncompressed bytes per independently compressed piece.
pub const DEFLATE_CHUNK: usize = 1 << 20;

/// Pieces gathered before a parallel batch runs.
const BATCH: usize = 32;

const ADLER_BASE: u32 = 65_521;

/// Adler-32 of `data`, continuing from `adler`.
#[must_use]
pub fn adler32(adler: u32, data: &[u8]) -> u32 {
    let (mut a, mut b) = (adler & 0xffff, adler >> 16);
    // 5552 is the largest block for which `b` cannot overflow a u32.
    for block in data.chunks(5552) {
        for &x in block {
            a += u32::from(x);
            b += a;
        }
        a %= ADLER_BASE;
        b %= ADLER_BASE;
    }
    (b << 16) | a
}

/// The Adler-32 of `A ‖ B` from `adler(A)`, `adler(B)` and `|B|`.
#[must_use]
pub fn adler32_combine(a1: u32, a2: u32, len2: u64) -> u32 {
    let base = u64::from(ADLER_BASE);
    let rem = len2 % base;
    let s1a = u64::from(a1 & 0xffff);
    let s2a = u64::from(a1 >> 16);
    let s1b = u64::from(a2 & 0xffff);
    let s2b = u64::from(a2 >> 16);
    // A ‖ B: sum1 = s1a + s1b − 1, sum2 = s2a + s2b + |B|·s1a − |B|.
    let sum1 = (s1a + s1b + base - 1) % base;
    let sum2 = (s2a + s2b + (rem * s1a) % base + base - rem) % base;
    #[allow(clippy::cast_possible_truncation)]
    let v = (sum1 | (sum2 << 16)) as u32;
    v
}

/// Compresses one piece to raw DEFLATE, ending with a sync flush or, for
/// the last piece, the final block.
fn compress_piece(level: u32, data: &[u8], last: bool) -> std::io::Result<(Vec<u8>, u32)> {
    let mut c = Compress::new(Compression::new(level), false);
    let mut out = Vec::with_capacity(data.len() / 2 + 64);
    let flush = if last {
        FlushCompress::Finish
    } else {
        FlushCompress::Sync
    };
    loop {
        let consumed = usize::try_from(c.total_in()).unwrap_or(usize::MAX);
        if out.capacity() - out.len() < 64 {
            out.reserve(out.capacity().max(4096));
        }
        let status = c
            .compress_vec(&data[consumed.min(data.len())..], &mut out, flush)
            .map_err(std::io::Error::other)?;
        let done_in = usize::try_from(c.total_in()).unwrap_or(usize::MAX) >= data.len();
        match status {
            flate2::Status::StreamEnd => break,
            // A sync flush is complete once all input is in and the
            // output buffer was not filled to the brim.
            _ if !last && done_in && out.len() < out.capacity() => break,
            _ => {}
        }
    }
    Ok((out, adler32(1, data)))
}

/// A zlib writer that compresses fixed-size pieces in parallel.
pub struct ChunkedZlib<W: Write> {
    out: W,
    level: u32,
    pending: Vec<u8>,
    adler: u32,
}

impl<W: Write> std::fmt::Debug for ChunkedZlib<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChunkedZlib")
            .field("level", &self.level)
            .field("pending", &self.pending.len())
            .finish_non_exhaustive()
    }
}

impl<W: Write> ChunkedZlib<W> {
    /// Writes the zlib header for `level` (0–9) and starts a stream.
    ///
    /// # Errors
    ///
    /// When writing the header fails.
    pub fn new(mut out: W, level: u32) -> std::io::Result<ChunkedZlib<W>> {
        let level = level.min(9);
        // CMF 0x78: DEFLATE, 32 KiB window. FLG: the level hint, and the
        // check bits that make CMF·256 + FLG a multiple of 31.
        let flevel: u8 = match level {
            0 | 1 => 0,
            2..=5 => 1,
            6 => 2,
            _ => 3,
        };
        let mut flg = flevel << 6;
        let rem = (0x78u16 * 256 + u16::from(flg)) % 31;
        if rem != 0 {
            #[allow(clippy::cast_possible_truncation)]
            {
                flg += (31 - rem) as u8;
            }
        }
        out.write_all(&[0x78, flg])?;
        Ok(ChunkedZlib {
            out,
            level,
            pending: Vec::with_capacity(DEFLATE_CHUNK * BATCH),
            adler: 1,
        })
    }

    fn emit(&mut self, pieces: &[&[u8]], last_is_final: bool) -> std::io::Result<()> {
        let n = pieces.len();
        let level = self.level;
        let done: Vec<std::io::Result<(Vec<u8>, u32)>> = pieces
            .par_iter()
            .enumerate()
            .map(|(i, p)| compress_piece(level, p, last_is_final && i + 1 == n))
            .collect();
        for (piece, r) in pieces.iter().zip(done) {
            let (bytes, adler) = r?;
            self.out.write_all(&bytes)?;
            self.adler = adler32_combine(self.adler, adler, piece.len() as u64);
        }
        Ok(())
    }

    fn drain_full(&mut self) -> std::io::Result<()> {
        let full = self.pending.len() / DEFLATE_CHUNK * DEFLATE_CHUNK;
        if full == 0 {
            return Ok(());
        }
        let pending = std::mem::take(&mut self.pending);
        let pieces: Vec<&[u8]> = pending[..full].chunks(DEFLATE_CHUNK).collect();
        self.emit(&pieces, false)?;
        self.pending = pending;
        self.pending.drain(..full);
        Ok(())
    }

    /// Compresses what is left, writes the Adler-32 trailer and returns
    /// the writer.
    ///
    /// # Errors
    ///
    /// When compressing or writing fails.
    pub fn finish(mut self) -> std::io::Result<W> {
        let pending = std::mem::take(&mut self.pending);
        // Every full piece, then the remainder — possibly empty — as the
        // final piece: the cut points stay at multiples of the chunk.
        let full = pending.len() / DEFLATE_CHUNK * DEFLATE_CHUNK;
        let mut pieces: Vec<&[u8]> = pending[..full].chunks(DEFLATE_CHUNK).collect();
        pieces.push(&pending[full..]);
        self.emit(&pieces, true)?;
        let adler = self.adler;
        self.out.write_all(&adler.to_be_bytes())?;
        Ok(self.out)
    }
}

impl<W: Write> Write for ChunkedZlib<W> {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.pending.extend_from_slice(data);
        if self.pending.len() >= DEFLATE_CHUNK * BATCH {
            self.drain_full()?;
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn adler_matches_the_reference_value() {
        assert_eq!(adler32(1, b"Wikipedia"), 0x11E6_0398);
        let data: Vec<u8> = (0..100_000u32).map(|i| (i * 31 % 251) as u8).collect();
        for cut in [0, 1, 5552, 77_777, 100_000] {
            let (a, b) = data.split_at(cut);
            assert_eq!(
                adler32_combine(adler32(1, a), adler32(1, b), b.len() as u64),
                adler32(1, &data),
                "cut at {cut}"
            );
        }
    }

    fn roundtrip(data: &[u8], level: u32, writes: usize) -> Vec<u8> {
        let mut z = ChunkedZlib::new(Vec::new(), level).unwrap();
        for part in data.chunks(data.len().div_ceil(writes.max(1)).max(1)) {
            z.write_all(part).unwrap();
        }
        let bytes = z.finish().unwrap();
        let mut back = Vec::new();
        flate2::read::ZlibDecoder::new(&bytes[..])
            .read_to_end(&mut back)
            .expect("a valid zlib stream with a correct checksum");
        assert_eq!(back, data);
        bytes
    }

    #[test]
    fn streams_decode_and_do_not_depend_on_how_they_were_fed() {
        let mut data = Vec::new();
        let mut s = 0x1234_5678u32;
        for i in 0..(3 * DEFLATE_CHUNK + 12_345) {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            #[allow(clippy::cast_possible_truncation)]
            data.push(if i % 3 == 0 {
                (s >> 24) as u8
            } else {
                (i / 97) as u8
            });
        }
        for level in [1, 6, 9] {
            let a = roundtrip(&data, level, 1);
            let b = roundtrip(&data, level, 37);
            assert_eq!(a, b, "level {level}");
        }
        roundtrip(&[], 6, 1);
        roundtrip(&data[..DEFLATE_CHUNK], 6, 1);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap();
        let one = pool.install(|| roundtrip(&data, 6, 3));
        assert_eq!(
            one,
            roundtrip(&data, 6, 3),
            "thread count changed the bytes"
        );
    }
}
