//! Raster export: PNG, JPEG and WebP through one deterministic rasteriser
//! (T11.2.1, T11.2.3–T11.2.6).
//!
//! Every raster export goes through [`xarast_render::export`], which only
//! knows the CPU backend in its deterministic configuration. Pixels are
//! composited in encoded sRGB, 8 bits per channel, the same space as the
//! screen (`research/03 §2.10`).

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use xarast_color::Rgba8;
use xarast_geom::Rect;
use xarast_render::Surface;
use xarast_render::export::{
    DEFAULT_STRIP_BUDGET, ExportJob, ExportRenderError, render_export_strips,
};

use crate::model::{Background, ExportRequest};
use crate::options::{FormatId, FormatOptions, PngDepth, PngOptions};
use crate::png::{PngHeader, PngStream, encode_png};
use crate::registry::{Capabilities, Exporter};
use crate::report::{Compromise, ExportError, ExportReport};
use crate::source::{ExportSource, Progress, SourceScene, Stage};
use crate::webp::MAX_WEBP_SIDE;

/// Everything decided before the first pixel.
#[derive(Debug)]
pub struct RasterPlan {
    /// The document rectangle, bleed included.
    pub area: Rect,
    /// Output size.
    pub width: u32,
    /// Output size.
    pub height: u32,
    /// Output resolution.
    pub dpi: f64,
    /// What the surface is cleared to.
    pub clear: [u8; 4],
    /// Compromises known before rendering.
    pub compromises: Vec<Compromise>,
}

/// Resolves the area, the size and the background of a raster export.
///
/// # Errors
///
/// [`ExportError::Area`], [`ExportError::Sizing`], or `BadOptions` when
/// the size exceeds `max_side`.
pub fn plan(
    src: &dyn ExportSource,
    req: &ExportRequest,
    max_side: u32,
) -> Result<RasterPlan, ExportError> {
    let area = req.bled(src.resolve_area(&req.area)?)?;
    let mut sizing = req.sizing;
    sizing.resolve(area)?;
    let (width, height) = sizing.pixels;
    if width > max_side || height > max_side {
        return Err(ExportError::BadOptions {
            format: req.options.format(),
            reason: format!("{width}x{height} px exceeds {max_side} px a side"),
        });
    }
    let paper = src.paper_colour();
    let keeps_alpha = req.options.keeps_alpha();
    let clear = req.background.clear_colour(paper, keeps_alpha);
    let mut compromises = Vec::new();
    let wanted_alpha = match req.background {
        Background::Transparent => true,
        Background::Paper => paper.a < 255,
        Background::Colour(c) => c.a < 255,
    };
    if wanted_alpha && !keeps_alpha {
        compromises.push(Compromise::AlphaFlattened {
            onto: Rgba8 {
                r: clear[0],
                g: clear[1],
                b: clear[2],
                a: 255,
            },
        });
    }
    Ok(RasterPlan {
        area,
        width,
        height,
        dpi: sizing.dpi,
        clear,
        compromises,
    })
}

fn build(
    src: &dyn ExportSource,
    req: &ExportRequest,
    progress: &dyn Progress,
    report: &mut ExportReport,
) -> Result<SourceScene, ExportError> {
    progress.report(Stage::Scene, 0.0);
    let t = Instant::now();
    let built = src.build_scene(req.quality)?;
    report.scene_time = t.elapsed();
    progress.report(Stage::Scene, 1.0);
    if progress.cancelled() {
        return Err(ExportError::Cancelled);
    }
    Ok(built)
}

fn job<'a>(built: &'a SourceScene, p: &RasterPlan, req: &ExportRequest) -> ExportJob<'a> {
    ExportJob {
        scene: &built.scene,
        resolver: &built.resolver,
        area: p.area,
        width: p.width,
        height: p.height,
        quality: req.quality,
        dpi: p.dpi,
        background: p.clear,
        strip_budget_bytes: DEFAULT_STRIP_BUDGET,
    }
}

fn render_error<E: std::fmt::Display>(e: ExportRenderError<E>) -> ExportError {
    match e {
        ExportRenderError::Cancelled => ExportError::Cancelled,
        ExportRenderError::Sink(s) => ExportError::Encode(s.to_string()),
        other => ExportError::Render(other.to_string()),
    }
}

/// Renders the whole export into one surface.
///
/// # Errors
///
/// [`ExportError`].
pub fn render_for_export(
    built: &SourceScene,
    p: &RasterPlan,
    req: &ExportRequest,
    progress: &dyn Progress,
    report: &mut ExportReport,
) -> Result<Surface, ExportError> {
    let t = Instant::now();
    let j = job(built, p, req);
    let mut out = Surface::new(p.width, p.height);
    let stride = p.width as usize * 4;
    let cancelled = || progress.cancelled();
    let stats = render_export_strips(
        &j,
        &cancelled,
        &mut |done, total| progress.report(Stage::Render, frac(done, total)),
        &mut |y0, strip: &Surface| -> Result<(), std::convert::Infallible> {
            let o = y0 as usize * stride;
            out.data_mut()[o..o + strip.data().len()].copy_from_slice(strip.data());
            Ok(())
        },
    )
    .map_err(render_error)?;
    report.commands = stats.commands;
    report.render_time = t.elapsed();
    Ok(out)
}

#[allow(clippy::cast_precision_loss)]
fn frac(done: u32, total: u32) -> f32 {
    done as f32 / total.max(1) as f32
}

/// A file written next to its destination and renamed into place, so a
/// failed or cancelled export never leaves a partial file.
struct AtomicFile {
    tmp: PathBuf,
    dest: PathBuf,
    done: bool,
}

impl AtomicFile {
    fn new(dest: &Path) -> AtomicFile {
        let name = dest
            .file_name()
            .map_or_else(|| "export".into(), |n| n.to_string_lossy().into_owned());
        let tmp = dest.with_file_name(format!(".{name}.{}.part", std::process::id()));
        AtomicFile {
            tmp,
            dest: dest.to_path_buf(),
            done: false,
        }
    }

    fn create(&self) -> std::io::Result<BufWriter<File>> {
        Ok(BufWriter::with_capacity(1 << 20, File::create(&self.tmp)?))
    }

    fn commit(mut self, w: BufWriter<File>) -> std::io::Result<u64> {
        let f = w
            .into_inner()
            .map_err(std::io::IntoInnerError::into_error)?;
        f.sync_all()?;
        let len = f.metadata()?.len();
        drop(f);
        std::fs::rename(&self.tmp, &self.dest)?;
        self.done = true;
        Ok(len)
    }

    fn write_all(self, bytes: &[u8]) -> std::io::Result<u64> {
        let mut w = self.create()?;
        w.write_all(bytes)?;
        self.commit(w)
    }
}

impl Drop for AtomicFile {
    fn drop(&mut self) {
        if !self.done {
            let _ = std::fs::remove_file(&self.tmp);
        }
    }
}

fn finish(
    mut report: ExportReport,
    built: SourceScene,
    p: RasterPlan,
    t0: Instant,
) -> ExportReport {
    report.pixels = (p.width, p.height);
    report.dpi = p.dpi;
    report.compromises.extend(p.compromises);
    report.compromises.extend(built.compromises);
    report.duration = t0.elapsed();
    report
}

fn bad(format: FormatId, reason: impl Into<String>) -> ExportError {
    ExportError::BadOptions {
        format,
        reason: reason.into(),
    }
}

/// PNG.
#[derive(Debug, Clone, Copy, Default)]
pub struct PngExporter;

impl PngExporter {
    fn options(req: &ExportRequest) -> Result<PngOptions, ExportError> {
        match req.options {
            FormatOptions::Png(o) => {
                if o.optimise && !cfg!(feature = "oxipng") {
                    return Err(ExportError::FeatureNotBuilt("the `oxipng` feature"));
                }
                Ok(o)
            }
            _ => Err(bad(FormatId::Png, "options are not PNG options")),
        }
    }
}

impl Exporter for PngExporter {
    fn id(&self) -> FormatId {
        FormatId::Png
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["png"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            vector: false,
            alpha: true,
            multipage: false,
            embeds_fonts: false,
            has_dpi: true,
            lossy: false,
            deterministic: true,
            max_side: xarast_render::export::MAX_EXPORT_SIDE,
            streams: true,
        }
    }

    fn export(
        &self,
        src: &dyn ExportSource,
        req: &ExportRequest,
        progress: &dyn Progress,
    ) -> Result<ExportReport, ExportError> {
        let t0 = Instant::now();
        let o = Self::options(req)?;
        let p = plan(src, req, self.capabilities().max_side)?;
        let mut report = ExportReport::default();
        if o.bit_depth == PngDepth::Sixteen {
            report.compromises.push(Compromise::WidenedFrom8Bit);
        }
        let built = build(src, req, progress, &mut report)?;
        let header = PngHeader::from_options(p.width, p.height, &o, p.dpi);
        let streams = !o.interlace
            && !o.optimise
            && !matches!(o.colour, crate::options::PngColour::Palette { .. });
        let file = AtomicFile::new(&req.destination);
        if streams {
            // Render and encode strip by strip: memory is one strip.
            let t = Instant::now();
            let mut w = file.create()?;
            let mut encode = Duration::ZERO;
            let stats = {
                let mut png = PngStream::begin(&mut w, header)
                    .map_err(|e| ExportError::Encode(e.to_string()))?;
                let j = job(&built, &p, req);
                let cancelled = || progress.cancelled();
                let stats = render_export_strips(
                    &j,
                    &cancelled,
                    &mut |done, total| progress.report(Stage::Render, frac(done, total)),
                    &mut |_, strip: &Surface| {
                        let te = Instant::now();
                        let r = png.write_rgba_rows(strip.data());
                        encode += te.elapsed();
                        r
                    },
                )
                .map_err(render_error)?;
                let te = Instant::now();
                png.finish()
                    .map_err(|e| ExportError::Encode(e.to_string()))?;
                encode += te.elapsed();
                stats
            };
            report.commands = stats.commands;
            report.render_time = t.elapsed().saturating_sub(encode);
            let te = Instant::now();
            report.bytes_written = file.commit(w)?;
            report.encode_time = encode + te.elapsed();
        } else {
            let surface = render_for_export(&built, &p, req, progress, &mut report)?;
            progress.report(Stage::Encode, 0.0);
            let te = Instant::now();
            let mut bytes = Vec::new();
            encode_png(&mut bytes, header, surface.data())
                .map_err(|e| bad(FormatId::Png, e.to_string()))?;
            drop(surface);
            if o.optimise {
                bytes = optimise(bytes, progress)?;
            }
            report.bytes_written = file.write_all(&bytes)?;
            report.encode_time = te.elapsed();
        }
        Ok(finish(report, built, p, t0))
    }
}

#[cfg(feature = "oxipng")]
fn optimise(bytes: Vec<u8>, progress: &dyn Progress) -> Result<Vec<u8>, ExportError> {
    progress.report(Stage::Optimise, 0.0);
    let r = crate::png::optimise(bytes, &|| progress.cancelled());
    progress.report(Stage::Optimise, 1.0);
    r.map_err(|e| e.map_or(ExportError::Cancelled, ExportError::Encode))
}

#[cfg(not(feature = "oxipng"))]
fn optimise(_bytes: Vec<u8>, _progress: &dyn Progress) -> Result<Vec<u8>, ExportError> {
    Err(ExportError::FeatureNotBuilt("the `oxipng` feature"))
}

/// JPEG.
#[derive(Debug, Clone, Copy, Default)]
pub struct JpegExporter;

impl Exporter for JpegExporter {
    fn id(&self) -> FormatId {
        FormatId::Jpeg
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["jpg", "jpeg", "jpe", "jfif"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            vector: false,
            alpha: false,
            multipage: false,
            embeds_fonts: false,
            has_dpi: true,
            lossy: true,
            deterministic: true,
            max_side: xarast_render::export::MAX_EXPORT_SIDE,
            streams: false,
        }
    }

    fn export(
        &self,
        src: &dyn ExportSource,
        req: &ExportRequest,
        progress: &dyn Progress,
    ) -> Result<ExportReport, ExportError> {
        let t0 = Instant::now();
        let FormatOptions::Jpeg(o) = req.options else {
            return Err(bad(FormatId::Jpeg, "options are not JPEG options"));
        };
        if !(1..=100).contains(&o.quality) {
            return Err(bad(
                FormatId::Jpeg,
                format!("quality {} is outside 1..=100", o.quality),
            ));
        }
        let p = plan(src, req, self.capabilities().max_side)?;
        let mut report = ExportReport::default();
        let built = build(src, req, progress, &mut report)?;
        let surface = render_for_export(&built, &p, req, progress, &mut report)?;
        progress.report(Stage::Encode, 0.0);
        let te = Instant::now();
        let bytes = crate::jpeg::encode_jpeg(surface.data(), p.width, p.height, &o, p.dpi)
            .map_err(ExportError::Encode)?;
        drop(surface);
        report.bytes_written = AtomicFile::new(&req.destination).write_all(&bytes)?;
        report.encode_time = te.elapsed();
        Ok(finish(report, built, p, t0))
    }
}

/// WebP, lossless.
#[derive(Debug, Clone, Copy, Default)]
pub struct WebPExporter;

impl Exporter for WebPExporter {
    fn id(&self) -> FormatId {
        FormatId::WebP
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["webp"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            vector: false,
            alpha: true,
            multipage: false,
            embeds_fonts: false,
            // WebP stores no resolution (only in optional EXIF/XMP).
            has_dpi: false,
            lossy: false,
            deterministic: true,
            max_side: MAX_WEBP_SIDE,
            streams: false,
        }
    }

    fn export(
        &self,
        src: &dyn ExportSource,
        req: &ExportRequest,
        progress: &dyn Progress,
    ) -> Result<ExportReport, ExportError> {
        let t0 = Instant::now();
        let FormatOptions::WebP(o) = req.options else {
            return Err(bad(FormatId::WebP, "options are not WebP options"));
        };
        if let crate::options::WebPMode::Lossy { .. } = o.mode {
            return Err(ExportError::FeatureNotBuilt(
                "a lossy WebP encoder (none is pure Rust; see docs/memory/export.md)",
            ));
        }
        let p = plan(src, req, self.capabilities().max_side)?;
        let mut report = ExportReport::default();
        let built = build(src, req, progress, &mut report)?;
        let surface = render_for_export(&built, &p, req, progress, &mut report)?;
        progress.report(Stage::Encode, 0.0);
        let te = Instant::now();
        let opaque = p.clear[3] == 255 && surface.data().as_chunks::<4>().0.iter().all(|px| px[3] == 255);
        let bytes = crate::webp::encode_webp(surface.data(), p.width, p.height, &o, opaque)
            .map_err(|e| ExportError::Encode(e.to_string()))?;
        drop(surface);
        report.bytes_written = AtomicFile::new(&req.destination).write_all(&bytes)?;
        report.encode_time = te.elapsed();
        Ok(finish(report, built, p, t0))
    }
}
