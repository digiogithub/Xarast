//! Per-format options (T11.1.4): serialisable, with defaults.
//!
//! They serialise with `serde` so that export hints can be stored in the
//! document (T11.1.5) and passed on a command line or in a batch file.
//! Every struct is `#[serde(default)]`: a hint written by an older build,
//! missing a newer field, still reads.

use serde::{Deserialize, Serialize};

/// The export formats. There is no `.xar` here and there must never be
/// one: architecture §3.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FormatId {
    /// Portable Network Graphics.
    Png,
    /// JPEG (JFIF).
    Jpeg,
    /// WebP.
    WebP,
}

impl FormatId {
    /// The usual file extension.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            FormatId::Png => "png",
            FormatId::Jpeg => "jpg",
            FormatId::WebP => "webp",
        }
    }

    /// A human name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            FormatId::Png => "PNG",
            FormatId::Jpeg => "JPEG",
            FormatId::WebP => "WebP",
        }
    }
}

/// PNG sample depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PngDepth {
    /// Eight bits per sample.
    #[default]
    Eight,
    /// Sixteen bits per sample: the 8-bit render widened exactly
    /// (`v × 257`), for pipelines that want 16-bit input.
    Sixteen,
}

/// PNG colour type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PngColour {
    /// Colour with alpha.
    #[default]
    Rgba,
    /// Colour, flattened onto the background.
    Rgb,
    /// Grey (BT.601 luma), flattened onto the background.
    Grey,
    /// Grey with alpha.
    GreyAlpha,
    /// Indexed colour with at most `max_colours` entries (≤ 256). Exact
    /// only: an image with more colours is refused until palette
    /// quantisation lands (T11.2.8).
    Palette {
        /// The most palette entries allowed.
        max_colours: u16,
    },
}

impl PngColour {
    /// Whether the colour type carries alpha.
    #[must_use]
    pub const fn has_alpha(self) -> bool {
        matches!(
            self,
            PngColour::Rgba | PngColour::GreyAlpha | PngColour::Palette { .. }
        )
    }
}

/// DEFLATE effort for PNG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PngCompression {
    /// zlib level 1.
    Fast,
    /// zlib level 6.
    #[default]
    Balanced,
    /// zlib level 9.
    Best,
}

impl PngCompression {
    /// The zlib level.
    #[must_use]
    pub const fn level(self) -> u32 {
        match self {
            PngCompression::Fast => 1,
            PngCompression::Balanced => 6,
            PngCompression::Best => 9,
        }
    }
}

/// PNG options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PngOptions {
    /// Sample depth.
    pub bit_depth: PngDepth,
    /// Colour type.
    pub colour: PngColour,
    /// Adam7 interlacing. Needs the whole image in memory.
    pub interlace: bool,
    /// Write the resolution as a `pHYs` chunk.
    pub write_dpi: bool,
    /// DEFLATE effort.
    pub compression: PngCompression,
    /// Run `oxipng` over the result ("optimise, slower"). Needs the
    /// `oxipng` feature; without it the export fails with
    /// `ExportError::FeatureNotBuilt`.
    pub optimise: bool,
}

impl Default for PngOptions {
    /// RGBA, 8-bit, not interlaced, resolution written, balanced
    /// compression, no optimisation pass.
    fn default() -> PngOptions {
        PngOptions {
            bit_depth: PngDepth::Eight,
            colour: PngColour::Rgba,
            interlace: false,
            write_dpi: true,
            compression: PngCompression::Balanced,
            optimise: false,
        }
    }
}

/// JPEG chroma subsampling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Subsampling {
    /// No subsampling: sharpest colour edges, largest files.
    #[serde(rename = "4:4:4")]
    S444,
    /// Half horizontal chroma resolution.
    #[serde(rename = "4:2:2")]
    S422,
    /// Half horizontal and vertical chroma resolution: the common default.
    #[default]
    #[serde(rename = "4:2:0")]
    S420,
}

/// JPEG options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct JpegOptions {
    /// 1 to 100.
    pub quality: u8,
    /// Progressive rather than baseline.
    pub progressive: bool,
    /// Chroma subsampling.
    pub subsampling: Subsampling,
    /// Write the resolution into the JFIF header.
    pub write_dpi: bool,
}

impl Default for JpegOptions {
    /// Quality 90, baseline, 4:2:0, resolution written.
    fn default() -> JpegOptions {
        JpegOptions {
            quality: 90,
            progressive: false,
            subsampling: Subsampling::S420,
            write_dpi: true,
        }
    }
}

/// WebP compression mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "mode")]
pub enum WebPMode {
    /// Lossless (VP8L).
    #[default]
    Lossless,
    /// Lossy (VP8). No acceptable pure-Rust encoder exists, so this is
    /// refused with `ExportError::FeatureNotBuilt` (`docs/memory/export.md`).
    Lossy {
        /// 1 to 100.
        quality: u8,
    },
}

/// WebP options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WebPOptions {
    /// Lossless or lossy.
    pub mode: WebPMode,
}

/// One entry per format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "format")]
pub enum FormatOptions {
    /// PNG.
    Png(PngOptions),
    /// JPEG.
    Jpeg(JpegOptions),
    /// WebP.
    #[serde(rename = "webp")]
    WebP(WebPOptions),
}

impl Default for FormatOptions {
    fn default() -> FormatOptions {
        FormatOptions::Png(PngOptions::default())
    }
}

impl FormatOptions {
    /// The format these options are for.
    #[must_use]
    pub const fn format(&self) -> FormatId {
        match self {
            FormatOptions::Png(_) => FormatId::Png,
            FormatOptions::Jpeg(_) => FormatId::Jpeg,
            FormatOptions::WebP(_) => FormatId::WebP,
        }
    }

    /// The defaults for a format.
    #[must_use]
    pub fn default_for(id: FormatId) -> FormatOptions {
        match id {
            FormatId::Png => FormatOptions::Png(PngOptions::default()),
            FormatId::Jpeg => FormatOptions::Jpeg(JpegOptions::default()),
            FormatId::WebP => FormatOptions::WebP(WebPOptions::default()),
        }
    }

    /// Whether the output, with these options, keeps an alpha channel.
    #[must_use]
    pub const fn keeps_alpha(&self) -> bool {
        match self {
            FormatOptions::Png(p) => p.colour.has_alpha(),
            FormatOptions::Jpeg(_) => false,
            FormatOptions::WebP(_) => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_round_trip_through_json() {
        let all = [
            FormatOptions::Png(PngOptions {
                bit_depth: PngDepth::Sixteen,
                colour: PngColour::Palette { max_colours: 16 },
                interlace: true,
                write_dpi: false,
                compression: PngCompression::Best,
                optimise: true,
            }),
            FormatOptions::Jpeg(JpegOptions {
                quality: 42,
                progressive: true,
                subsampling: Subsampling::S444,
                write_dpi: false,
            }),
            FormatOptions::WebP(WebPOptions {
                mode: WebPMode::Lossy { quality: 70 },
            }),
            FormatOptions::default(),
        ];
        for o in all {
            let j = serde_json::to_string(&o).unwrap();
            let back: FormatOptions = serde_json::from_str(&j).unwrap();
            assert_eq!(back, o, "{j}");
        }
    }

    #[test]
    fn a_partial_hint_reads_with_defaults() {
        let o: FormatOptions = serde_json::from_str(r#"{"format":"jpeg","quality":75}"#).unwrap();
        assert_eq!(
            o,
            FormatOptions::Jpeg(JpegOptions {
                quality: 75,
                ..JpegOptions::default()
            })
        );
        let o: FormatOptions =
            serde_json::from_str(r#"{"format":"jpeg","subsampling":"4:4:4"}"#).unwrap();
        assert!(matches!(
            o,
            FormatOptions::Jpeg(JpegOptions {
                subsampling: Subsampling::S444,
                ..
            })
        ));
    }

    #[test]
    fn alpha_follows_the_options() {
        assert!(FormatOptions::default().keeps_alpha());
        assert!(!FormatOptions::default_for(FormatId::Jpeg).keeps_alpha());
        assert!(
            !FormatOptions::Png(PngOptions {
                colour: PngColour::Rgb,
                ..PngOptions::default()
            })
            .keeps_alpha()
        );
    }
}
