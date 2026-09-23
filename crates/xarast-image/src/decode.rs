//! The `Decoder` façade: sniff, read the header, check every guard, and only
//! then allocate and decode.
//!
//! The order is the security property. [`decode`] never allocates a pixel
//! buffer before the header has passed [`DecodeLimits`]; a file that trips a
//! guard returns a typed error and leaves nothing behind.

use std::io::{self, BufRead, Cursor, Read, Seek, SeekFrom};
use std::sync::Arc;
use std::time::Instant;

use image::{ImageDecoder, ImageError, ImageReader, Limits, error::LimitErrorKind};

use crate::limits::{DecodeError, DecodeLimits, SizeLimit};
use crate::model::{BitmapData, BitmapInfo, ColourSpace, DEFAULT_DPI, ImageFormat};
use crate::orient::{self, Orientation};
use crate::sniff::{self, HeaderFacts};

/// What the header says, without decoding a pixel.
#[derive(Clone, PartialEq, Debug)]
pub struct Probe {
    /// The container.
    pub format: ImageFormat,
    /// The layout, as it will be **after** orientation normalisation.
    pub info: BitmapInfo,
    /// The EXIF orientation the decode will apply, if the file has one.
    pub exif_orientation: Option<Orientation>,
    /// `width × height × 4`: the size of the premultiplied output.
    pub estimated_decoded_bytes: u64,
    /// The size of the decoder's native buffer (before conversion).
    pub native_decoded_bytes: u64,
}

/// Something worth telling the user that did not stop the decode.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecodeWarning {
    /// Decoded ÷ encoded above `warn_ratio` but below `refuse_ratio`.
    HighRatio {
        /// The ratio, rounded down.
        ratio: u64,
    },
    /// The bitmap is not in the working space and was not converted
    /// ([`to_working_space`] is a no-op until phase 15).
    ColourSpaceNotConverted,
}

/// A decoded, upright image.
#[derive(Clone, PartialEq, Debug)]
pub struct DecodedImage {
    /// Premultiplied RGBA8, orientation already applied.
    pub data: BitmapData,
    /// The layout.
    pub info: BitmapInfo,
    /// The container it came from.
    pub format: ImageFormat,
    /// The orientation that was applied, so export can re-emit it.
    pub applied_orientation: Orientation,
    /// The raw EXIF block (TIFF structure, no `Exif\0\0` prefix), retained.
    pub exif: Option<Arc<[u8]>>,
    /// The raw ICC profile, retained.
    pub icc: Option<Arc<[u8]>>,
    /// Non-fatal findings.
    pub warnings: Vec<DecodeWarning>,
}

/// Header inspection without decoding. Cheap: no pixel buffer is allocated.
///
/// # Errors
///
/// [`DecodeError::UnknownFormat`] or [`DecodeError::Corrupt`] when the header
/// cannot be read.
pub fn probe(bytes: &[u8]) -> Result<Probe, DecodeError> {
    let format = sniff::sniff(bytes).ok_or(DecodeError::UnknownFormat)?;
    probe_as(bytes, format)
}

/// [`probe`] with the format already known.
///
/// # Errors
///
/// As [`probe`].
pub fn probe_as(bytes: &[u8], format: ImageFormat) -> Result<Probe, DecodeError> {
    if format == ImageFormat::Jpeg {
        return probe_jpeg(bytes);
    }
    let owned;
    let (bytes, codec_format) = if format == ImageFormat::Dib {
        owned = crate::xar::dib_to_bmp(bytes)?;
        (&owned[..], ImageFormat::Bmp)
    } else {
        (bytes, format)
    };
    let deadline = Instant::now() + std::time::Duration::from_secs(3600);
    let limits = DecodeLimits::default();
    let mut dec = open(bytes, codec_format, &limits, deadline)?;
    let mut head = read_header(&mut *dec, bytes, codec_format, false);
    head.probe.format = format;
    Ok(head.probe)
}

/// Decodes with limits and EXIF orientation applied. Returns upright,
/// premultiplied pixels.
///
/// # Errors
///
/// Any [`DecodeError`]; see its variants for which guard raised it.
pub fn decode(bytes: &[u8], limits: &DecodeLimits) -> Result<DecodedImage, DecodeError> {
    let format = sniff::sniff(bytes).ok_or(DecodeError::UnknownFormat)?;
    decode_as(bytes, format, limits)
}

/// [`decode`] with the format already known — the only way to decode a
/// headerless [`ImageFormat::Dib`].
///
/// # Errors
///
/// As [`decode`].
pub fn decode_as(
    bytes: &[u8],
    format: ImageFormat,
    limits: &DecodeLimits,
) -> Result<DecodedImage, DecodeError> {
    let start = Instant::now();
    let deadline = start + limits.max_duration;
    let owned;
    let (bytes, codec_format) = if format == ImageFormat::Dib {
        owned = crate::xar::dib_to_bmp(bytes)?;
        (&owned[..], ImageFormat::Bmp)
    } else {
        (bytes, format)
    };
    let result = decode_inner(bytes, codec_format, limits, deadline);
    // Whatever the decoder said, a decode that overran is a timeout: an
    // I/O error raised by the deadline reader may arrive wrapped as
    // "corrupt", and a result that arrived late is discarded, never
    // returned half-trusted.
    if Instant::now() > deadline {
        return Err(DecodeError::Timeout);
    }
    let mut img = result?;
    img.format = format;
    Ok(img)
}

/// [`decode`] on a worker thread with a **hard** wall-clock ceiling: if
/// `max_duration` elapses the caller gets [`DecodeError::Timeout`] at once,
/// even from a decoder that never looks at the clock. The abandoned thread
/// runs to completion under the same memory limits and its result is
/// dropped.
///
/// # Errors
///
/// As [`decode`].
pub fn decode_on_worker(
    bytes: Arc<[u8]>,
    limits: DecodeLimits,
) -> Result<DecodedImage, DecodeError> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let spawned = std::thread::Builder::new()
        .name("xarast-image-decode".into())
        .spawn(move || {
            let _ = tx.send(decode(&bytes, &limits));
        });
    if spawned.is_err() {
        return Err(DecodeError::corrupt("could not start the decode thread"));
    }
    match rx.recv_timeout(limits.max_duration) {
        Ok(r) => r,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(DecodeError::Timeout),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err(DecodeError::corrupt("the decoder panicked"))
        }
    }
}

/// The single choke point where a bitmap not in the working space would be
/// converted (`T10.8.4`). Today it returns the pixels unchanged and reports
/// whether a conversion was owed; phase 15 fills it in, and nothing else in
/// the code base may convert colour spaces.
#[must_use]
pub fn to_working_space(data: BitmapData, space: &ColourSpace) -> (BitmapData, bool) {
    let owed = !matches!(space, ColourSpace::AssumedSrgb | ColourSpace::Srgb);
    (data, owed)
}

// ── internals ───────────────────────────────────────────────────────────────

/// A cursor that refuses to hand out bytes after the deadline, so that a
/// streaming decoder aborts cooperatively.
struct DeadlineReader<'a> {
    inner: Cursor<&'a [u8]>,
    deadline: Instant,
}

impl DeadlineReader<'_> {
    fn check(&self) -> io::Result<()> {
        if Instant::now() > self.deadline {
            Err(io::Error::new(io::ErrorKind::TimedOut, "decode deadline"))
        } else {
            Ok(())
        }
    }
}

impl Read for DeadlineReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.check()?;
        self.inner.read(buf)
    }
}

impl BufRead for DeadlineReader<'_> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.check()?;
        self.inner.fill_buf()
    }

    fn consume(&mut self, amt: usize) {
        self.inner.consume(amt);
    }
}

impl Seek for DeadlineReader<'_> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}

fn codec(format: ImageFormat) -> image::ImageFormat {
    match format {
        ImageFormat::Png => image::ImageFormat::Png,
        ImageFormat::Jpeg => image::ImageFormat::Jpeg,
        ImageFormat::WebP => image::ImageFormat::WebP,
        ImageFormat::Gif => image::ImageFormat::Gif,
        ImageFormat::Tiff => image::ImageFormat::Tiff,
        ImageFormat::Bmp | ImageFormat::Dib => image::ImageFormat::Bmp,
        ImageFormat::Pnm => image::ImageFormat::Pnm,
    }
}

fn image_limits(max_dimension: Option<u32>, max_alloc: u64) -> Limits {
    let mut l = Limits::default();
    l.max_image_width = max_dimension;
    l.max_image_height = max_dimension;
    l.max_alloc = Some(max_alloc);
    l
}

fn map_err(e: ImageError, format: ImageFormat, alloc_ceiling: u64) -> DecodeError {
    match e {
        ImageError::Limits(l) => match l.kind() {
            LimitErrorKind::DimensionError => DecodeError::TooLarge {
                declared: (0, 0),
                limit: SizeLimit::Dimension,
            },
            LimitErrorKind::InsufficientMemory => DecodeError::AllocationRefused {
                wanted: alloc_ceiling,
            },
            _ => DecodeError::Corrupt(Box::new(l)),
        },
        ImageError::IoError(io) if io.kind() == io::ErrorKind::TimedOut => DecodeError::Timeout,
        ImageError::Unsupported(_) => DecodeError::Unsupported(format),
        other => DecodeError::Corrupt(Box::new(other)),
    }
}

fn open<'a>(
    bytes: &'a [u8],
    format: ImageFormat,
    limits: &DecodeLimits,
    deadline: Instant,
) -> Result<Box<dyn ImageDecoder + 'a>, DecodeError> {
    let reader = DeadlineReader {
        inner: Cursor::new(bytes),
        deadline,
    };
    let mut r = ImageReader::with_format(reader, codec(format));
    // Header-time ceiling: nothing a header parse legitimately needs comes
    // near the output limit. No dimension limit yet: `check_size` reports
    // the declared dimensions in its error, which `image`'s check does not.
    r.limits(image_limits(None, limits.max_decoded_bytes));
    let dec = r
        .into_decoder()
        .map_err(|e| map_err(e, format, limits.max_decoded_bytes))?;
    Ok(Box::new(dec))
}

struct Header {
    probe: Probe,
    orientation: Orientation,
    exif: Option<Vec<u8>>,
    icc: Option<Vec<u8>>,
    facts: HeaderFacts,
}

fn read_header(
    dec: &mut dyn ImageDecoder,
    bytes: &[u8],
    format: ImageFormat,
    count_scans: bool,
) -> Header {
    let (w, h) = dec.dimensions();
    let facts = sniff::header_facts(format, bytes, count_scans);
    // Metadata errors are not fatal: a broken EXIF block must not make an
    // otherwise good photograph unopenable.
    let exif = dec.exif_metadata().ok().flatten();
    let icc = dec.icc_profile().ok().flatten();
    let bpp = dec.original_color_type().bits_per_pixel();
    let has_alpha = dec.color_type().has_alpha();
    assemble(
        format,
        (w, h),
        facts,
        exif,
        icc,
        bpp,
        has_alpha,
        dec.total_bytes(),
    )
}

/// The JPEG probe fast path: the `image` JPEG decoder copies the whole file
/// before it reads a header, which alone breaks the 200 µs probe budget on
/// a 24 Mpx photograph. The segments before the first scan hold everything
/// a probe reports.
fn probe_jpeg(bytes: &[u8]) -> Result<Probe, DecodeError> {
    let facts = sniff::header_facts(ImageFormat::Jpeg, bytes, false);
    let (w, h) = facts
        .jpeg_dims
        .filter(|(w, h)| *w > 0 && *h > 0)
        .ok_or_else(|| DecodeError::corrupt("JPEG has no frame header"))?;
    let (exif, icc) = sniff::jpeg_metadata(bytes);
    let bpp = u16::from(facts.depth.unwrap_or(24));
    let native = u64::from(w) * u64::from(h) * u64::from(bpp.div_ceil(8).max(1));
    Ok(assemble(
        ImageFormat::Jpeg,
        (w, h),
        facts,
        exif,
        icc,
        bpp,
        false,
        native,
    )
    .probe)
}

#[allow(
    clippy::too_many_arguments,
    reason = "the header facts come from two different sources"
)]
fn assemble(
    format: ImageFormat,
    (w, h): (u32, u32),
    facts: HeaderFacts,
    exif: Option<Vec<u8>>,
    icc: Option<Vec<u8>>,
    bits_per_pixel: u16,
    has_alpha: bool,
    native_decoded_bytes: u64,
) -> Header {
    let exif_orientation = exif
        .as_deref()
        .and_then(image::metadata::Orientation::from_exif_chunk)
        .and_then(|o| Orientation::from_exif(o.to_exif()));
    let orientation = exif_orientation.unwrap_or_default();
    let (ow, oh) = if orientation.swaps_axes() {
        (h, w)
    } else {
        (w, h)
    };
    let (hdpi, vdpi) = facts.dpi.unwrap_or((DEFAULT_DPI, DEFAULT_DPI));
    let (hdpi, vdpi) = if orientation.swaps_axes() {
        (vdpi, hdpi)
    } else {
        (hdpi, vdpi)
    };
    let colour_space = if let Some(p) = &icc {
        ColourSpace::Icc {
            hash: *blake3::hash(p).as_bytes(),
        }
    } else if facts.srgb {
        ColourSpace::Srgb
    } else if let (true, Some(gamma)) = (facts.grey, facts.gamma) {
        ColourSpace::Grey { gamma }
    } else {
        ColourSpace::AssumedSrgb
    };
    let depth = facts
        .depth
        .unwrap_or_else(|| u8::try_from(bits_per_pixel).unwrap_or(u8::MAX));
    let info = BitmapInfo {
        pixel_width: ow,
        pixel_height: oh,
        depth,
        palette_entries: facts.palette.unwrap_or(0),
        recommended_width: BitmapInfo::natural_width(ow, hdpi),
        hdpi,
        vdpi,
        colour_space,
        has_alpha,
    };
    Header {
        probe: Probe {
            format,
            info,
            exif_orientation,
            estimated_decoded_bytes: u64::from(w) * u64::from(h) * 4,
            native_decoded_bytes,
        },
        orientation,
        exif,
        icc,
        facts,
    }
}

fn check_size(w: u32, h: u32, limits: &DecodeLimits) -> Result<(), DecodeError> {
    let declared = (w, h);
    if w > limits.max_dimension || h > limits.max_dimension {
        return Err(DecodeError::TooLarge {
            declared,
            limit: SizeLimit::Dimension,
        });
    }
    let pixels = u64::from(w) * u64::from(h);
    if pixels > limits.max_pixels {
        return Err(DecodeError::TooLarge {
            declared,
            limit: SizeLimit::Pixels,
        });
    }
    if pixels.saturating_mul(4) > limits.max_decoded_bytes {
        return Err(DecodeError::TooLarge {
            declared,
            limit: SizeLimit::DecodedBytes,
        });
    }
    Ok(())
}

fn decode_inner(
    bytes: &[u8],
    format: ImageFormat,
    limits: &DecodeLimits,
    deadline: Instant,
) -> Result<DecodedImage, DecodeError> {
    let mut dec = open(bytes, format, limits, deadline)?;
    let head = read_header(&mut *dec, bytes, format, true);
    let (w, h) = dec.dimensions();

    // Guard 1: declared size, before any allocation.
    check_size(w, h, limits)?;
    if w == 0 || h == 0 {
        return Err(DecodeError::corrupt("zero-sized image"));
    }
    let color = dec.color_type();
    if !crate::pixels::supported(color) {
        return Err(DecodeError::Unsupported(format));
    }
    let native = dec.total_bytes();
    if native > limits.max_decoded_bytes.saturating_mul(4) {
        return Err(DecodeError::TooLarge {
            declared: (w, h),
            limit: SizeLimit::DecodedBytes,
        });
    }

    // Guard 2: the compression ratio, from the header and the byte count.
    let mut warnings = Vec::new();
    let ratio = native / (bytes.len() as u64).max(1);
    if native >= limits.ratio_floor_bytes {
        if ratio > u64::from(limits.refuse_ratio) {
            return Err(DecodeError::SuspiciousRatio { ratio });
        }
        if ratio > u64::from(limits.warn_ratio) {
            warnings.push(DecodeWarning::HighRatio { ratio });
        }
    }

    // Guard 3: work that grows without bound in the pixel count.
    if format == ImageFormat::Jpeg && head.facts.jpeg_scans > limits.max_jpeg_scans {
        return Err(DecodeError::ExcessiveWork {
            what: "JPEG scans",
            count: u64::from(head.facts.jpeg_scans),
        });
    }

    // Guard 4: the decoder's own allocations, bounded by the estimate. The
    // megabyte of slack covers Huffman/LZW tables and row scratch, which do
    // not scale with the image.
    let ceiling = ((native as f64) * f64::from(limits.max_alloc_factor)).ceil() as u64 + (1 << 20);
    dec.set_limits(image_limits(Some(limits.max_dimension), ceiling))
        .map_err(|e| map_err(e, format, ceiling))?;

    let native_len =
        usize::try_from(native).map_err(|_| DecodeError::AllocationRefused { wanted: ceiling })?;
    let mut buf = Vec::new();
    buf.try_reserve_exact(native_len)
        .map_err(|_| DecodeError::AllocationRefused { wanted: ceiling })?;
    buf.resize(native_len, 0);
    dec.read_image(&mut buf)
        .map_err(|e| map_err(e, format, ceiling))?;
    if Instant::now() > deadline {
        return Err(DecodeError::Timeout);
    }

    let px = (w as usize) * (h as usize);
    let mut out = Vec::new();
    out.try_reserve_exact(px * 4)
        .map_err(|_| DecodeError::AllocationRefused { wanted: ceiling })?;
    out.resize(px * 4, 0);
    let translucent = crate::pixels::to_premul_rgba8(color, &buf, &mut out);
    drop(buf);

    let (ow, oh, pixels) = orient::apply(head.orientation, w, h, out.into_boxed_slice());
    let mut info = head.probe.info;
    info.has_alpha = translucent;
    let data = BitmapData {
        width: ow,
        height: oh,
        pixels,
        deep: None,
    };
    let (data, owed) = to_working_space(data, &info.colour_space);
    if owed {
        warnings.push(DecodeWarning::ColourSpaceNotConverted);
    }
    Ok(DecodedImage {
        data,
        info,
        format,
        applied_orientation: head.orientation,
        exif: head.exif.map(Arc::from),
        icc: head.icc.map(Arc::from),
        warnings,
    })
}
