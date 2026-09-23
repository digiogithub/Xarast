//! Decode limits and the typed errors a guard raises.
//!
//! Decompression bombs are a security issue: every limit here is checked
//! against the **header** before a single pixel buffer is allocated.

use std::time::Duration;

use crate::ImageFormat;

/// Limits enforced before and during a decode. The defaults are the table of
/// `docs/phases/phase-10-bitmaps-and-photo.md` §W10.2.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct DecodeLimits {
    /// Largest width or height, in pixels. Default 65 535.
    pub max_dimension: u32,
    /// Largest `width × height`. Default 256 Mpx.
    pub max_pixels: u64,
    /// Largest decoded output (`width × height × 4`). Default 1 GiB.
    pub max_decoded_bytes: u64,
    /// Wall-clock ceiling. Default 20 s.
    pub max_duration: Duration,
    /// The decoder's own allocations may reach this multiple of the
    /// header's native decoded size. Default 1.25.
    pub max_alloc_factor: f32,
    /// Decoded ÷ encoded ratio above which a warning is recorded. 2 000.
    pub warn_ratio: u32,
    /// Decoded ÷ encoded ratio above which the file is refused. 20 000.
    pub refuse_ratio: u32,
    /// The ratio is only consulted when the native decoded size is at least
    /// this many bytes: a 1 × 4000 white strip in 60 bytes is not a bomb.
    /// Default 16 MiB.
    pub ratio_floor_bytes: u64,
    /// Most JPEG scans (`SOS` markers) accepted. Decode time is roughly
    /// scans × pixels, and a progressive JPEG has about ten; a file with
    /// thousands of scans is a time bomb, not a photograph. Default 128.
    pub max_jpeg_scans: u32,
    /// GIF/APNG frames considered. Always 1 in phase 10: the first frame.
    pub max_frames: u32,
}

impl Default for DecodeLimits {
    fn default() -> DecodeLimits {
        DecodeLimits {
            max_dimension: 65_535,
            max_pixels: 256 << 20,
            max_decoded_bytes: 1 << 30,
            max_duration: Duration::from_secs(20),
            max_alloc_factor: 1.25,
            warn_ratio: 2_000,
            refuse_ratio: 20_000,
            ratio_floor_bytes: 16 << 20,
            max_jpeg_scans: 128,
            max_frames: 1,
        }
    }
}

impl DecodeLimits {
    /// Tight limits for fuzzing and for untrusted previews: 4096², 16 Mpx,
    /// 64 MiB, 2 s.
    #[must_use]
    pub fn tight() -> DecodeLimits {
        DecodeLimits {
            max_dimension: 4096,
            max_pixels: 16 << 20,
            max_decoded_bytes: 64 << 20,
            max_duration: Duration::from_secs(2),
            ..DecodeLimits::default()
        }
    }
}

/// Which limit a [`DecodeError::TooLarge`] tripped.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SizeLimit {
    /// `max_dimension`.
    Dimension,
    /// `max_pixels`.
    Pixels,
    /// `max_decoded_bytes`.
    DecodedBytes,
}

/// Why a decode was refused or failed.
///
/// A guard never leaves a partial result behind: every error means no
/// pixels at all.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    /// The header alone shows this cannot be decoded within the limits.
    /// The only overridable error: the UI may offer "import anyway" with
    /// raised limits.
    #[error("image of {}×{} exceeds the {limit:?} limit", declared.0, declared.1)]
    TooLarge {
        /// The dimensions the header declares.
        declared: (u32, u32),
        /// Which limit.
        limit: SizeLimit,
    },
    /// Decoded-to-encoded ratio above `refuse_ratio`. Never overridable.
    #[error("decoded-to-encoded ratio {ratio}:1 is a decompression bomb")]
    SuspiciousRatio {
        /// The ratio, rounded down.
        ratio: u64,
    },
    /// The decode would take unbounded time (e.g. a JPEG with thousands of
    /// scans). Never overridable.
    #[error("{what}: {count} exceeds the limit")]
    ExcessiveWork {
        /// What was counted.
        what: &'static str,
        /// How many there were.
        count: u64,
    },
    /// `max_duration` elapsed.
    #[error("decode exceeded its wall-clock limit")]
    Timeout,
    /// The decoder wanted more memory than the allocation ceiling allows.
    /// Never overridable.
    #[error("decoder allocation refused ({wanted} bytes allowed)")]
    AllocationRefused {
        /// The ceiling that was in force.
        wanted: u64,
    },
    /// A format we recognise but do not decode.
    #[error("unsupported {} variant", .0.name())]
    Unsupported(ImageFormat),
    /// The bytes are not an image we recognise.
    #[error("unrecognised image format")]
    UnknownFormat,
    /// The file is damaged.
    #[error("corrupt image: {0}")]
    Corrupt(Box<dyn std::error::Error + Send + Sync>),
}

impl DecodeError {
    /// True only for [`DecodeError::TooLarge`]: the UI may offer "import
    /// anyway". The ratio, work and allocation limits are never overridable.
    #[must_use]
    pub fn is_overridable(&self) -> bool {
        matches!(self, DecodeError::TooLarge { .. })
    }

    pub(crate) fn corrupt(msg: impl Into<String>) -> DecodeError {
        DecodeError::Corrupt(msg.into().into())
    }
}
