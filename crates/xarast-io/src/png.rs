//! The PNG encoder (T11.2.3, T11.2.4).
//!
//! Written here rather than through the `png` crate for three reasons: the
//! `png` crate's writer cannot produce Adam7-interlaced data (it would set
//! the IHDR flag over progressive rows), it compresses with its own
//! `fdeflate` rather than the workspace's one DEFLATE backend
//! (`Cargo.toml`, `flate2`), and streaming strip by strip keeps a large
//! export's memory at one strip. The container is small: signature, IHDR,
//! `sRGB`, `pHYs`, `PLTE`/`tRNS`, IDAT, IEND, each with its CRC-32.
//!
//! Row filters are chosen per row by the usual minimum-sum-of-absolute-
//! differences heuristic, which depends on the pixels only, so the output
//! is byte-identical for identical input.

use std::io::Write;

use rayon::prelude::*;

use crate::deflate::ChunkedZlib;
use crate::options::{PngColour, PngDepth, PngOptions};

/// Bytes an IDAT chunk holds before it is written out.
const IDAT_CHUNK: usize = 1 << 18;

/// What the PNG header says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PngHeader {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Colour type, as options name it.
    pub colour: PngColour,
    /// Sample depth.
    pub depth: PngDepth,
    /// Adam7.
    pub interlace: bool,
    /// Pixels per metre for `pHYs`, or `None`.
    pub ppm: Option<u32>,
    /// zlib level.
    pub level: u32,
}

impl PngHeader {
    /// A header from options.
    #[must_use]
    pub fn from_options(width: u32, height: u32, o: &PngOptions, dpi: f64) -> PngHeader {
        PngHeader {
            width,
            height,
            colour: o.colour,
            depth: o.bit_depth,
            interlace: o.interlace,
            ppm: o.write_dpi.then(|| dpi_to_ppm(dpi)),
            level: o.compression.level(),
        }
    }

    const fn channels(&self) -> usize {
        match self.colour {
            PngColour::Rgba => 4,
            PngColour::Rgb => 3,
            PngColour::GreyAlpha => 2,
            PngColour::Grey | PngColour::Palette { .. } => 1,
        }
    }

    /// Bytes per complete pixel, for the filters (at least one).
    const fn bpp(&self) -> usize {
        match (self.colour, self.depth) {
            (PngColour::Palette { .. }, _) => 1,
            (_, PngDepth::Eight) => self.channels(),
            (_, PngDepth::Sixteen) => self.channels() * 2,
        }
    }

    const fn colour_type(&self) -> u8 {
        match self.colour {
            PngColour::Grey => 0,
            PngColour::Rgb => 2,
            PngColour::Palette { .. } => 3,
            PngColour::GreyAlpha => 4,
            PngColour::Rgba => 6,
        }
    }

    const fn bit_depth(&self) -> u8 {
        match (self.colour, self.depth) {
            (PngColour::Palette { .. }, _) | (_, PngDepth::Eight) => 8,
            (_, PngDepth::Sixteen) => 16,
        }
    }
}

/// Dots per inch to pixels per metre, rounded.
#[must_use]
pub fn dpi_to_ppm(dpi: f64) -> u32 {
    let v = (dpi / 0.0254).round();
    if v.is_finite() && v >= 1.0 && v <= f64::from(u32::MAX) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let v = v as u32;
        v
    } else {
        1
    }
}

/// BT.601 luma of an sRGB triple, in the integer form every grey export
/// uses.
#[must_use]
pub fn luma(r: u8, g: u8, b: u8) -> u8 {
    let y = (u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114 + 500) / 1000;
    #[allow(clippy::cast_possible_truncation)]
    let y = y as u8;
    y
}

/// Why a PNG could not be written.
#[derive(Debug, thiserror::Error)]
pub enum PngError {
    /// Writing failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The image has more colours than the palette allows.
    #[error(
        "the image has more than {0} colours; an indexed PNG needs palette \
         quantisation, which is not built yet (T11.2.8)"
    )]
    TooManyColours(u16),
}

fn write_chunk(w: &mut dyn Write, kind: &[u8; 4], data: &[u8]) -> std::io::Result<()> {
    let len = u32::try_from(data.len()).map_err(std::io::Error::other)?;
    w.write_all(&len.to_be_bytes())?;
    w.write_all(kind)?;
    w.write_all(data)?;
    let mut crc = flate2::Crc::new();
    crc.update(kind);
    crc.update(data);
    w.write_all(&crc.sum().to_be_bytes())
}

/// Replaces a PNG's `sRGB` chunk with an `iCCP` chunk carrying `icc`
/// (phase 11 T11.5.2): the image's own profile travels with pixels that
/// were decoded without converting them. PNG allows only one of the two
/// (PNG 1.2 §4.2.2.4), so `sRGB` goes; `iCCP` goes right after `IHDR`,
/// before `PLTE` and `IDAT` as the specification requires. The profile
/// is compressed at zlib level 6 through the workspace's one DEFLATE
/// backend, so the bytes are a function of the input only.
///
/// Returns `None` when `png` is not a well-formed chunk stream starting
/// with `IHDR`, or `icc` is empty.
#[must_use]
pub fn with_icc_profile(png: &[u8], icc: &[u8]) -> Option<Vec<u8>> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if icc.is_empty() || !png.starts_with(&SIGNATURE) {
        return None;
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
    z.write_all(icc).ok()?;
    // Profile name, its NUL, compression method 0.
    let mut iccp = b"ICC profile\0\0".to_vec();
    iccp.extend_from_slice(&z.finish().ok()?);
    let mut out = Vec::with_capacity(png.len() + iccp.len() + 12);
    out.extend_from_slice(&SIGNATURE);
    let mut at = SIGNATURE.len();
    let mut first = true;
    while at < png.len() {
        let len = u32::from_be_bytes(png.get(at..at + 4)?.try_into().ok()?) as usize;
        let end = at.checked_add(12)?.checked_add(len)?;
        let chunk = png.get(at..end)?;
        let kind = &chunk[4..8];
        if first && kind != b"IHDR" {
            return None;
        }
        if kind != b"sRGB" && kind != b"iCCP" {
            out.extend_from_slice(chunk);
        }
        if first {
            write_chunk(&mut out, b"iCCP", &iccp).ok()?;
            first = false;
        }
        at = end;
    }
    Some(out)
}

/// Splits a zlib stream into IDAT chunks.
struct IdatWriter<'w> {
    out: &'w mut dyn Write,
    buf: Vec<u8>,
}

impl Write for IdatWriter<'_> {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(data);
        while self.buf.len() >= IDAT_CHUNK {
            write_chunk(self.out, b"IDAT", &self.buf[..IDAT_CHUNK])?;
            self.buf.drain(..IDAT_CHUNK);
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Converts straight RGBA8 pixels into the header's sample layout.
fn convert_row(h: &PngHeader, rgba: &[u8], palette: Option<&PaletteMap>, out: &mut Vec<u8>) {
    out.clear();
    let wide = h.depth == PngDepth::Sixteen && !matches!(h.colour, PngColour::Palette { .. });
    let mut push = |v: u8| {
        out.push(v);
        if wide {
            out.push(v);
        }
    };
    for p in rgba.as_chunks::<4>().0.iter() {
        match h.colour {
            PngColour::Rgba => {
                push(p[0]);
                push(p[1]);
                push(p[2]);
                push(p[3]);
            }
            PngColour::Rgb => {
                push(p[0]);
                push(p[1]);
                push(p[2]);
            }
            PngColour::Grey => push(luma(p[0], p[1], p[2])),
            PngColour::GreyAlpha => {
                push(luma(p[0], p[1], p[2]));
                push(p[3]);
            }
            PngColour::Palette { .. } => {
                // Palette samples are 8-bit whatever the depth, so `wide`
                // is false here and `push` writes exactly one byte.
                push(palette.and_then(|m| m.index(p)).unwrap_or(0));
            }
        }
    }
}

/// Filters one row into `out` (type byte first) with the filter whose
/// output has the smallest sum of absolute signed bytes. `cand` is
/// scratch.
fn filter_row(bpp: usize, prev: &[u8], row: &[u8], cand: &mut [Vec<u8>; 5], out: &mut Vec<u8>) {
    for c in cand.iter_mut() {
        c.clear();
    }
    for i in 0..row.len() {
        let x = row[i];
        let a = if i >= bpp { row[i - bpp] } else { 0 };
        let b = prev[i];
        let c = if i >= bpp { prev[i - bpp] } else { 0 };
        cand[0].push(x);
        cand[1].push(x.wrapping_sub(a));
        cand[2].push(x.wrapping_sub(b));
        #[allow(clippy::cast_possible_truncation)]
        let avg = ((u16::from(a) + u16::from(b)) / 2) as u8;
        cand[3].push(x.wrapping_sub(avg));
        cand[4].push(x.wrapping_sub(paeth(a, b, c)));
    }
    let score =
        |v: &Vec<u8>| -> u64 { v.iter().map(|&b| u64::from((b as i8).unsigned_abs())).sum() };
    let mut best = 0;
    let mut best_score = score(&cand[0]);
    for (t, c) in cand.iter().enumerate().skip(1) {
        let s = score(c);
        if s < best_score {
            best = t;
            best_score = s;
        }
    }
    #[allow(clippy::cast_possible_truncation)]
    out.push(best as u8);
    out.extend_from_slice(&cand[best]);
}

/// Filters `rows` (each `row_len` bytes; `prev` is the row above the
/// first) in parallel. Each row depends only on itself and the unfiltered
/// row above, so the result is the same on any thread count.
fn filter_rows(bpp: usize, prev: &[u8], rows: &[u8], row_len: usize) -> Vec<u8> {
    if row_len == 0 {
        return vec![0; rows.len()];
    }
    let n = rows.len() / row_len;
    let mut out = vec![0u8; n * (row_len + 1)];
    out.par_chunks_mut(row_len + 1).enumerate().for_each_init(
        || {
            (
                std::array::from_fn(|_| Vec::with_capacity(row_len)),
                Vec::with_capacity(row_len + 1),
            )
        },
        |(cand, buf): &mut ([Vec<u8>; 5], Vec<u8>), (i, dst)| {
            let row = &rows[i * row_len..(i + 1) * row_len];
            let above = if i == 0 {
                prev
            } else {
                &rows[(i - 1) * row_len..i * row_len]
            };
            buf.clear();
            filter_row(bpp, above, row, cand, buf);
            dst.copy_from_slice(buf);
        },
    );
    out
}

/// Converts rows of straight RGBA8 (`stride` bytes each) in parallel.
fn convert_rows(
    h: &PngHeader,
    rgba: &[u8],
    stride: usize,
    palette: Option<&PaletteMap>,
) -> Vec<u8> {
    let row_len = h.width as usize * h.bpp();
    let n = rgba.len().checked_div(stride).unwrap_or(0);
    let mut out = vec![0u8; n * row_len];
    if row_len == 0 {
        return out;
    }
    out.par_chunks_mut(row_len).enumerate().for_each_init(
        || Vec::with_capacity(row_len),
        |buf, (i, dst)| {
            convert_row(h, &rgba[i * stride..(i + 1) * stride], palette, buf);
            dst.copy_from_slice(buf);
        },
    );
    out
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let pa = (p - i16::from(a)).abs();
    let pb = (p - i16::from(b)).abs();
    let pc = (p - i16::from(c)).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// An exact palette: colour → index, in first-seen order.
#[derive(Debug)]
struct PaletteMap {
    colours: Vec<[u8; 4]>,
    lookup: std::collections::HashMap<[u8; 4], u8>,
}

impl PaletteMap {
    fn build(rgba: &[u8], max: u16) -> Result<PaletteMap, PngError> {
        let max = usize::from(max.clamp(1, 256));
        let mut m = PaletteMap {
            colours: Vec::new(),
            lookup: std::collections::HashMap::new(),
        };
        for p in rgba.as_chunks::<4>().0.iter() {
            let k = [p[0], p[1], p[2], p[3]];
            if !m.lookup.contains_key(&k) {
                if m.colours.len() == max {
                    #[allow(clippy::cast_possible_truncation)]
                    return Err(PngError::TooManyColours(max as u16));
                }
                #[allow(clippy::cast_possible_truncation)]
                m.lookup.insert(k, m.colours.len() as u8);
                m.colours.push(k);
            }
        }
        if m.colours.is_empty() {
            m.colours.push([0, 0, 0, 0]);
        }
        Ok(m)
    }

    fn index(&self, p: &[u8]) -> Option<u8> {
        self.lookup.get(&[p[0], p[1], p[2], p[3]]).copied()
    }
}

/// A PNG being written row by row.
///
/// Non-interlaced only: an interlaced or indexed image needs every pixel
/// before the first byte of image data, so it goes through
/// [`encode_png`].
pub struct PngStream<'w> {
    header: PngHeader,
    z: ChunkedZlib<IdatWriter<'w>>,
    /// The last converted row, for the next row's filters.
    prev: Vec<u8>,
    rows_done: u32,
}

impl std::fmt::Debug for PngStream<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PngStream")
            .field("header", &self.header)
            .field("rows_done", &self.rows_done)
            .finish_non_exhaustive()
    }
}

fn write_head(
    out: &mut dyn Write,
    h: &PngHeader,
    palette: Option<&PaletteMap>,
) -> std::io::Result<()> {
    out.write_all(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a])?;
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&h.width.to_be_bytes());
    ihdr.extend_from_slice(&h.height.to_be_bytes());
    ihdr.push(h.bit_depth());
    ihdr.push(h.colour_type());
    ihdr.push(0); // deflate
    ihdr.push(0); // adaptive filtering
    ihdr.push(u8::from(h.interlace));
    write_chunk(out, b"IHDR", &ihdr)?;
    // Composited in encoded sRGB (research/03 §2.10): say so, with the
    // perceptual intent, so that viewers do not guess.
    write_chunk(out, b"sRGB", &[0])?;
    if let Some(ppm) = h.ppm {
        let mut phys = Vec::with_capacity(9);
        phys.extend_from_slice(&ppm.to_be_bytes());
        phys.extend_from_slice(&ppm.to_be_bytes());
        phys.push(1); // metres
        write_chunk(out, b"pHYs", &phys)?;
    }
    if let Some(pal) = palette {
        let plte: Vec<u8> = pal
            .colours
            .iter()
            .flat_map(|c| [c[0], c[1], c[2]])
            .collect();
        write_chunk(out, b"PLTE", &plte)?;
        if pal.colours.iter().any(|c| c[3] != 255) {
            let trns: Vec<u8> = pal.colours.iter().map(|c| c[3]).collect();
            write_chunk(out, b"tRNS", &trns)?;
        }
    }
    Ok(())
}

impl<'w> PngStream<'w> {
    /// Writes the header chunks and gets ready for rows.
    ///
    /// # Errors
    ///
    /// When writing fails, or the header asks for interlacing or a
    /// palette.
    pub fn begin(out: &'w mut dyn Write, header: PngHeader) -> Result<PngStream<'w>, PngError> {
        if header.interlace || matches!(header.colour, PngColour::Palette { .. }) {
            return Err(PngError::Io(std::io::Error::other(
                "interlaced and indexed PNGs are encoded whole",
            )));
        }
        write_head(out, &header, None)?;
        let row_len = header.width as usize * header.bpp();
        Ok(PngStream {
            header,
            z: ChunkedZlib::new(
                IdatWriter {
                    out,
                    buf: Vec::with_capacity(IDAT_CHUNK),
                },
                header.level,
            )?,
            prev: vec![0; row_len],
            rows_done: 0,
        })
    }

    /// Appends rows of straight RGBA8, `width × 4` bytes each.
    ///
    /// # Errors
    ///
    /// When writing fails.
    pub fn write_rgba_rows(&mut self, rgba: &[u8]) -> Result<(), PngError> {
        let stride = self.header.width as usize * 4;
        let row_len = self.prev.len();
        let conv = convert_rows(&self.header, rgba, stride, None);
        let filtered = filter_rows(self.header.bpp(), &self.prev, &conv, row_len);
        self.z.write_all(&filtered)?;
        let n = rgba.len() / stride.max(1);
        if n > 0 && row_len > 0 {
            self.prev.copy_from_slice(&conv[(n - 1) * row_len..]);
        }
        #[allow(clippy::cast_possible_truncation)]
        {
            self.rows_done += n as u32;
        }
        Ok(())
    }

    /// Finishes the image data and writes IEND.
    ///
    /// # Errors
    ///
    /// When writing fails or fewer rows than the height were written.
    pub fn finish(self) -> Result<(), PngError> {
        if self.rows_done != self.header.height {
            return Err(PngError::Io(std::io::Error::other(format!(
                "{} of {} rows written",
                self.rows_done, self.header.height
            ))));
        }
        let out = self.z.finish()?.finish_into()?;
        write_chunk(out, b"IEND", &[])?;
        Ok(())
    }
}

impl<'w> IdatWriter<'w> {
    fn finish_into(self) -> std::io::Result<&'w mut dyn Write> {
        if !self.buf.is_empty() {
            write_chunk(self.out, b"IDAT", &self.buf)?;
        }
        Ok(self.out)
    }
}

/// Adam7: (x0, y0, dx, dy) per pass.
const ADAM7: [(usize, usize, usize, usize); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

/// Encodes a whole straight-RGBA8 image, any colour type, interlaced or
/// not.
///
/// # Errors
///
/// [`PngError`].
pub fn encode_png(out: &mut dyn Write, header: PngHeader, rgba: &[u8]) -> Result<(), PngError> {
    let (w, h) = (header.width as usize, header.height as usize);
    let palette = match header.colour {
        PngColour::Palette { max_colours } => Some(PaletteMap::build(rgba, max_colours)?),
        _ => None,
    };
    if !header.interlace && palette.is_none() {
        let mut s = PngStream::begin(out, header)?;
        s.write_rgba_rows(rgba)?;
        return s.finish();
    }
    write_head(out, &header, palette.as_ref())?;
    let bpp = header.bpp();
    let mut z = ChunkedZlib::new(
        IdatWriter {
            out,
            buf: Vec::with_capacity(IDAT_CHUNK),
        },
        header.level,
    )?;
    let passes: &[(usize, usize, usize, usize)] = if header.interlace {
        &ADAM7
    } else {
        &[(0, 0, 1, 1)]
    };
    for &(x0, y0, dx, dy) in passes {
        if x0 >= w || y0 >= h {
            continue;
        }
        let pw = (w - x0).div_ceil(dx);
        // The pass's pixels, gathered into rows of `pw` RGBA pixels.
        let mut px = Vec::with_capacity(pw * 4 * (h - y0).div_ceil(dy));
        for y in (y0..h).step_by(dy) {
            for x in (x0..w).step_by(dx) {
                let o = (y * w + x) * 4;
                px.extend_from_slice(&rgba[o..o + 4]);
            }
        }
        let sub = PngHeader {
            #[allow(clippy::cast_possible_truncation)]
            width: pw as u32,
            ..header
        };
        let row_len = pw * bpp;
        let conv = convert_rows(&sub, &px, pw * 4, palette.as_ref());
        z.write_all(&filter_rows(bpp, &vec![0; row_len], &conv, row_len))?;
    }
    let out = z.finish()?.finish_into()?;
    write_chunk(out, b"IEND", &[])?;
    Ok(())
}

/// Runs `oxipng` over an encoded PNG on its own thread, polling
/// `cancelled` every 20 ms (T11.2.4).
///
/// `oxipng` has no cancellation of its own and its `timeout` would make
/// the output depend on the clock, so a cancelled pass is **abandoned**:
/// this returns at once and the worker finishes its current file and
/// drops the result. No partial output is ever written.
///
/// # Errors
///
/// `Err(None)` when cancelled, `Err(Some(message))` when `oxipng` fails.
#[cfg(feature = "oxipng")]
pub fn optimise(png: Vec<u8>, cancelled: &dyn Fn() -> bool) -> Result<Vec<u8>, Option<String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("xarast-oxipng".into())
        .spawn(move || {
            let mut opts = oxipng::Options::from_preset(2);
            // Our sRGB and pHYs chunks are metadata the export promised.
            opts.strip = oxipng::StripChunks::None;
            let _ = tx.send(oxipng::optimize_from_memory(&png, &opts));
        })
        .map_err(|e| Some(e.to_string()))?;
    loop {
        match rx.recv_timeout(std::time::Duration::from_millis(20)) {
            Ok(r) => return r.map_err(|e| Some(e.to_string())),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if cancelled() {
                    return Err(None);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(Some("the optimiser stopped".into()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(bytes: &[u8]) -> (png::OutputInfo, Vec<u8>, png::Info<'static>) {
        let mut dec = png::Decoder::new(std::io::Cursor::new(bytes.to_vec()));
        dec.set_transformations(png::Transformations::IDENTITY);
        let mut r = dec.read_info().expect("header");
        let mut buf = vec![0; r.output_buffer_size().expect("size")];
        let info = r.next_frame(&mut buf).expect("frame");
        buf.truncate(info.buffer_size());
        let meta = r.info().clone();
        (info, buf, meta)
    }

    fn pattern(w: u32, h: u32) -> Vec<u8> {
        let mut v = Vec::new();
        for y in 0..h {
            for x in 0..w {
                #[allow(clippy::cast_possible_truncation)]
                v.extend_from_slice(&[
                    (x * 7) as u8,
                    (y * 13) as u8,
                    ((x ^ y) * 3) as u8,
                    if (x + y) % 5 == 0 {
                        0
                    } else {
                        200 + (x % 50) as u8
                    },
                ]);
            }
        }
        v
    }

    fn header(w: u32, h: u32, colour: PngColour, depth: PngDepth, interlace: bool) -> PngHeader {
        PngHeader {
            width: w,
            height: h,
            colour,
            depth,
            interlace,
            ppm: Some(dpi_to_ppm(300.0)),
            level: 6,
        }
    }

    #[test]
    fn every_colour_type_round_trips_interlaced_or_not() {
        let (w, h) = (37u32, 23u32);
        let src = pattern(w, h);
        for interlace in [false, true] {
            for depth in [PngDepth::Eight, PngDepth::Sixteen] {
                for colour in [
                    PngColour::Rgba,
                    PngColour::Rgb,
                    PngColour::Grey,
                    PngColour::GreyAlpha,
                ] {
                    let hd = header(w, h, colour, depth, interlace);
                    let mut out = Vec::new();
                    encode_png(&mut out, hd, &src).unwrap();
                    let (info, data, meta) = decode(&out);
                    assert_eq!((info.width, info.height), (w, h));
                    assert_eq!(meta.interlaced, interlace);
                    assert!(meta.srgb.is_some(), "sRGB chunk");
                    let dims = meta.pixel_dims.expect("pHYs");
                    assert_eq!(dims.xppu, 11_811);
                    let mut want = Vec::new();
                    convert_row(&hd, &src, None, &mut want);
                    assert_eq!(data, want, "{colour:?} {depth:?} interlace={interlace}");
                }
            }
        }
    }

    #[test]
    fn a_palette_is_exact_or_refused() {
        let (w, h) = (16u32, 9u32);
        let mut src = Vec::new();
        for i in 0..w * h {
            let c = [[255u8, 0, 0, 255], [0, 0, 255, 128], [0, 0, 0, 0]][(i % 3) as usize];
            src.extend_from_slice(&c);
        }
        let hd = header(
            w,
            h,
            PngColour::Palette { max_colours: 4 },
            PngDepth::Eight,
            true,
        );
        let mut out = Vec::new();
        encode_png(&mut out, hd, &src).unwrap();
        let (_, data, meta) = decode(&out);
        let pal = meta.palette.expect("PLTE");
        let trns = meta.trns.expect("tRNS");
        for (i, idx) in data.iter().enumerate() {
            let k = usize::from(*idx);
            let got = [pal[k * 3], pal[k * 3 + 1], pal[k * 3 + 2], trns[k]];
            assert_eq!(&got[..], &src[i * 4..i * 4 + 4]);
        }
        let hd = header(
            w,
            h,
            PngColour::Palette { max_colours: 2 },
            PngDepth::Eight,
            false,
        );
        assert!(matches!(
            encode_png(&mut Vec::new(), hd, &src),
            Err(PngError::TooManyColours(2))
        ));
    }

    #[test]
    fn streaming_matches_the_whole_image_encoder() {
        let (w, h) = (300u32, 200u32);
        let src = pattern(w, h);
        let hd = header(w, h, PngColour::Rgba, PngDepth::Eight, false);
        let mut whole = Vec::new();
        encode_png(&mut whole, hd, &src).unwrap();
        let mut streamed = Vec::new();
        let mut s = PngStream::begin(&mut streamed, hd).unwrap();
        for chunk in src.chunks(w as usize * 4 * 17) {
            s.write_rgba_rows(chunk).unwrap();
        }
        s.finish().unwrap();
        assert_eq!(whole, streamed);
        let (_, data, _) = decode(&streamed);
        assert_eq!(data, src);
    }

    #[test]
    fn a_short_stream_is_an_error() {
        let hd = header(4, 4, PngColour::Rgb, PngDepth::Eight, false);
        let mut out = Vec::new();
        let mut s = PngStream::begin(&mut out, hd).unwrap();
        s.write_rgba_rows(&[0; 16]).unwrap();
        assert!(s.finish().is_err());
    }

    #[test]
    fn an_icc_profile_replaces_the_srgb_chunk_and_keeps_the_pixels() {
        let (w, h) = (5, 3);
        let src = pattern(w, h);
        let hd = PngHeader {
            width: w,
            height: h,
            colour: PngColour::Rgba,
            depth: PngDepth::Eight,
            interlace: false,
            ppm: Some(2835),
            level: 6,
        };
        let mut plain = Vec::new();
        encode_png(&mut plain, hd, &src).unwrap();
        let profile: Vec<u8> = (0..=255u8).cycle().take(3000).collect();
        let tagged = with_icc_profile(&plain, &profile).expect("a PNG we wrote");
        let (_, pixels, meta) = decode(&tagged);
        assert_eq!(pixels, src);
        assert!(meta.srgb.is_none(), "sRGB and iCCP are exclusive");
        assert_eq!(meta.icc_profile.as_deref(), Some(&profile[..]));
        assert!(meta.pixel_dims.is_some(), "the other chunks stay");
        // iCCP right after IHDR.
        assert_eq!(&tagged[33 + 4..33 + 8], b"iCCP");
        // Deterministic, and refuses what is not a PNG.
        assert_eq!(with_icc_profile(&plain, &profile).unwrap(), tagged);
        assert!(with_icc_profile(b"GIF89a", &profile).is_none());
        assert!(with_icc_profile(&plain, &[]).is_none());
        assert!(with_icc_profile(&plain[..40], &profile).is_none());
    }
}
