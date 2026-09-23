//! The bitmap resource model: what a decoded image *is*, independent of how
//! it arrived.
//!
//! Pixels are always 8-bit **premultiplied** RGBA in non-linear sRGB — the
//! same convention as `xarast-render`'s surfaces — so that a bitmap can be
//! composited without a per-pixel conversion and hashed in one canonical
//! form. [`BitmapData::to_straight_rgba8`] is the one place that converts
//! back, for consumers (today: `xarast_render::ImageRef`) that want straight
//! alpha.

use std::sync::Arc;

/// The container format encoded bytes arrived in.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum ImageFormat {
    /// PNG, including the first frame of an APNG.
    Png,
    /// JPEG (baseline and progressive).
    Jpeg,
    /// WebP, lossy or lossless; first frame of an animation.
    WebP,
    /// GIF; first frame.
    Gif,
    /// TIFF.
    Tiff,
    /// Windows BMP with its `BM` file header.
    Bmp,
    /// A headerless Windows DIB (`BITMAPINFOHEADER` first), as the legacy
    /// `.xar` format embeds BMPs. Decoded by synthesising the file header.
    Dib,
    /// Netpbm: PBM, PGM, PPM, PAM.
    Pnm,
}

impl ImageFormat {
    /// A short lower-case name, for reports.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpeg",
            ImageFormat::WebP => "webp",
            ImageFormat::Gif => "gif",
            ImageFormat::Tiff => "tiff",
            ImageFormat::Bmp => "bmp",
            ImageFormat::Dib => "dib",
            ImageFormat::Pnm => "pnm",
        }
    }
}

/// The colour space a bitmap's samples are in.
///
/// Reserved from day one (`research/05 §6`): nothing converts yet — phase 15
/// does, through [`crate::to_working_space`] and nowhere else — but nothing
/// may assume sRGB at storage time either.
#[derive(Clone, PartialEq, Debug)]
pub enum ColourSpace {
    /// No colour information at all; sRGB is *assumed*. The flag matters
    /// for phase 15, which must not treat this as a declared sRGB.
    AssumedSrgb,
    /// Declared sRGB (a PNG `sRGB` chunk).
    Srgb,
    /// An embedded ICC profile. The bytes travel in
    /// [`crate::DecodedImage::icc`]; this is their BLAKE3, which is what the
    /// document's profile table will be keyed by.
    Icc {
        /// BLAKE3 of the profile bytes.
        hash: [u8; 32],
    },
    /// Greyscale with a declared gamma (a PNG `gAMA` chunk on a grey image).
    Grey {
        /// The decoding gamma, e.g. `2.2`.
        gamma: f32,
    },
}

/// How a bitmap is laid out. `BitmapInfo` of `bitmpinf.h:104`, plus the
/// colour-space slot.
#[derive(Clone, PartialEq, Debug)]
pub struct BitmapInfo {
    /// Width in pixels, after orientation normalisation.
    pub pixel_width: u32,
    /// Height in pixels, after orientation normalisation.
    pub pixel_height: u32,
    /// The *source* depth in bits per pixel: 1, 2, 4, 8, 16, 24, 32, 48 or
    /// 64. The stored pixels are always 32 bpp premultiplied.
    pub depth: u8,
    /// Palette entries in the source, 0 when it had none.
    pub palette_entries: u16,
    /// The natural width in millipoints, from the DPI.
    pub recommended_width: i32,
    /// Horizontal resolution in dots per inch.
    pub hdpi: u32,
    /// Vertical resolution in dots per inch.
    pub vdpi: u32,
    /// The colour space of the samples.
    pub colour_space: ColourSpace,
    /// Whether any pixel is not fully opaque.
    pub has_alpha: bool,
}

/// The resolution assumed when a file carries none. Screen resolution, the
/// same default every browser uses for an unannotated image.
pub const DEFAULT_DPI: u32 = 96;

/// Millipoints per inch.
const MP_PER_INCH: u64 = 72_000;

impl BitmapInfo {
    /// The natural width in millipoints for `width` pixels at `dpi`,
    /// saturating at `i32::MAX`. A zero DPI falls back to [`DEFAULT_DPI`].
    #[must_use]
    pub fn natural_width(width: u32, dpi: u32) -> i32 {
        let dpi = if dpi == 0 { DEFAULT_DPI } else { dpi };
        let mp = u64::from(width) * MP_PER_INCH / u64::from(dpi);
        i32::try_from(mp).unwrap_or(i32::MAX)
    }
}

/// Decoded pixels: 8-bit premultiplied RGBA, row-major, top row first,
/// `width * height * 4` bytes, no row padding.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct BitmapData {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Premultiplied RGBA8.
    pub pixels: Box<[u8]>,
    /// An optional 16-bit companion for ops that need the precision.
    /// Not populated by the decoder in phase 10.
    pub deep: Option<Box<[u16]>>,
}

impl BitmapData {
    /// The number of bytes the pixels occupy.
    #[must_use]
    pub fn byte_len(&self) -> usize {
        self.pixels.len()
    }

    /// The pixels converted to **straight** (non-premultiplied) RGBA8, the
    /// layout `xarast_render::ImageRef::new` takes.
    ///
    /// Exact for every opaque pixel (alpha 255 is the identity both ways).
    /// For a translucent pixel the premultiplied value already lost the low
    /// bits; this returns the nearest straight value that premultiplies back
    /// to the same bytes.
    #[must_use]
    pub fn to_straight_rgba8(&self) -> Vec<u8> {
        let mut out = self.pixels.to_vec();
        for px in out.as_chunks_mut::<4>().0 {
            let a = px[3];
            if a != 255 {
                for c in &mut px[..3] {
                    *c = crate::pixels::unpremultiply(*c, a);
                }
            }
        }
        out
    }

    /// BLAKE3 over the dimensions and the premultiplied pixels: the
    /// deduplication key of phase 10 (`T10.1.2`). Because the pixels are
    /// already orientation-normalised, the same picture stored with two
    /// different EXIF orientations hashes the same.
    #[must_use]
    pub fn content_hash(&self) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        h.update(b"xarast-image/rgba8-premul/v1");
        h.update(&self.width.to_le_bytes());
        h.update(&self.height.to_le_bytes());
        h.update(&self.pixels);
        *h.finalize().as_bytes()
    }
}

/// The encoded source bytes of a bitmap, kept so that saving re-emits them
/// verbatim: an embedded JPEG is never recompressed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OriginalEncoded {
    /// The container format.
    pub format: ImageFormat,
    /// The bytes, exactly as they arrived.
    pub bytes: Arc<[u8]>,
}

/// A decoded bitmap resource, as `xarast-image` produces it.
///
/// `xarast-doc` owns the document's resource table; this is the value an
/// importer hands it. The two will be reconciled when `xarast-doc` takes a
/// dependency on this crate (`T10.1.2`).
#[derive(Clone, PartialEq, Debug)]
pub struct BitmapResource {
    /// The name shown in the bitmap gallery.
    pub name: Arc<str>,
    /// The layout.
    pub info: BitmapInfo,
    /// The pixels, copy-on-write.
    pub pixels: Arc<BitmapData>,
    /// The encoded source, when there is one.
    pub original: Option<Arc<OriginalEncoded>>,
    /// The palette index that is transparent, GIF style.
    pub transparent_index: Option<u8>,
    /// [`BitmapData::content_hash`] of `pixels`, computed once.
    pub content_hash: [u8; 32],
}

impl BitmapResource {
    /// Wraps a decoded image, hashing its pixels.
    #[must_use]
    pub fn from_decoded(
        name: Arc<str>,
        decoded: crate::DecodedImage,
        original: Option<Arc<OriginalEncoded>>,
    ) -> BitmapResource {
        let content_hash = decoded.data.content_hash();
        BitmapResource {
            name,
            info: decoded.info,
            pixels: Arc::new(decoded.data),
            original,
            transparent_index: None,
            content_hash,
        }
    }
}
