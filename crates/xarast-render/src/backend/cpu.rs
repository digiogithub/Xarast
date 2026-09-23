//! The CPU backend: the deterministic reference.
//!
//! # Why this one is the oracle
//!
//! Export and every golden-image test go through the CPU backend
//! (`docs/10-architecture.md` §3.3), so it has to produce the same bytes on
//! every run and on every machine of the same architecture. Two things make
//! that true and both are load-bearing:
//!
//! * The SIMD level handed to `vello_cpu` is **pinned**, not detected. The
//!   W0 spike measured three of 786,432 channel samples differing between
//!   the AVX2 and the baseline paths — never by more than 1/255, but never
//!   is not the same as zero, and a golden baseline cannot be "nearly".
//!   [`CpuConfig::deterministic`] pins it; [`CpuConfig::interactive`] does
//!   not, and is about 15 % faster.
//! * Bands are rasterised in parallel but merged by band index, and no
//!   accumulation crosses a band boundary.
//!
//! # How a frame is drawn
//!
//! `vello_cpu` is used as a **coverage rasteriser and nothing else**: every
//! primitive is rasterised as opaque white into a scratch pixmap the size of
//! its clipped bounds, and the alpha channel of that scratch is the
//! antialiasing coverage. The paint is evaluated by [`crate::paint`] and
//! the coverage is composited by [`crate::blend`]. That is what lets Xara's
//! twelve families, its conical and diamond gradients and its perspective
//! mapping exist at all, none of which any rasteriser library offers.

use std::time::Instant;

use rayon::prelude::*;
use vello_cpu::color::{AlphaColor, PremulRgba8, Srgb};
use vello_cpu::kurbo::{
    Affine, BezPath, Cap as KCap, Join as KJoin, Stroke as KStroke, StrokeOpts,
};
use vello_cpu::peniko::Fill;
use vello_cpu::{Level, Pixmap, PixmapMut, RenderContext, RenderMode, RenderSettings, Resources};
use xarast_color::Rgba8;
use xarast_geom::{Cap, FillRule, Join, StrokeStyle};

use crate::backend::{BackendError, FrameTimings, LayerId, Rasterizer, RasterizerCaps};
use crate::blend::{BlendFamily, BlendLuts, LumaWeights, TranspSource, Transparency, composite};
use crate::display_list::{DisplayList, DrawCmd, DrawItem, ListParts, SCENE_PAINT};
use crate::paint::{GradMapping, ImageId, ImageRegistry, Paint, eval_paint};
use crate::path::PathRef;
use crate::precision::{Point64, Transform2D};
use crate::ramp::RampCache;
use crate::scene::{LayerKind, RenderQuality, SceneOp};
use crate::surface::{DeviceRect, DirtyRect, Surface};
use crate::tiling::{MIN_BAND_SCANLINES, band_height};

/// Rows of geometry rasterised beyond a band's edge before the coverage is
/// used.
///
/// Without it the band height changes the picture: a shape clipped exactly
/// at a band boundary produces coverage differing by 1/255 from the same
/// shape rasterised whole, because the rasteriser's strips start at the
/// viewport edge. Antialiasing influence is local to one pixel, so two rows
/// of guard make a band's pixels identical to the unbanded ones — which is
/// what `determinism::the_band_height_does_not_change_the_pixels` asserts.
const BAND_GUARD: i32 = 2;

/// How the CPU backend is configured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CpuConfig {
    /// Pin the SIMD level so that output is byte-identical everywhere, at
    /// about 15 % of the throughput.
    pub pin_simd: bool,
    /// How many bands to rasterise at once. Zero means "one per core".
    pub threads: usize,
    /// Working memory per band, which sets the band height exactly as the
    /// original's `GRenderDIB::SetFirstBand` heuristic does.
    pub band_budget_bytes: usize,
    /// The luminance weights every blend family's `Y(c)` uses.
    pub weights: LumaWeights,
}

impl Default for CpuConfig {
    fn default() -> CpuConfig {
        CpuConfig::interactive()
    }
}

impl CpuConfig {
    /// The configuration export and the golden tests use: bit-reproducible
    /// across machines of the same architecture.
    #[must_use]
    pub fn deterministic() -> CpuConfig {
        CpuConfig {
            pin_simd: true,
            threads: 1,
            band_budget_bytes: 1 << 20,
            weights: LumaWeights::BT601,
        }
    }

    /// The configuration the canvas uses: runtime SIMD detection and every
    /// core.
    ///
    /// Bands are half the deterministic height: at 1920 px wide a 1 MiB
    /// band is 136 rows, eight bands per 1080p frame, which leaves most of
    /// a many-core machine idle. Sixteen bands halved the 100 000-object
    /// frame on the reference machine; see `docs/memory/perf.md`.
    #[must_use]
    pub fn interactive() -> CpuConfig {
        CpuConfig {
            pin_simd: false,
            threads: 0,
            band_budget_bytes: 1 << 19,
            weights: LumaWeights::BT601,
        }
    }

    fn level(&self) -> Level {
        if self.pin_simd {
            Level::baseline()
        } else {
            Level::try_detect().unwrap_or(Level::baseline())
        }
    }
}

/// Everything a display list needs looked up while it is rendered.
///
/// `Clone` so that a snapshot can travel to the render thread with the
/// display list it belongs to: images are shared by `Arc`, and ramps are
/// small tables.
#[derive(Debug, Clone, Default)]
pub struct Resolver {
    /// Interned gradient ramps.
    pub ramps: RampCache,
    /// Registered images.
    pub images: ImageRegistry,
    /// Interned transparency ramps, indexed the same way as `ramps`.
    pub transparency_ramps: Vec<Vec<u8>>,
}

impl Resolver {
    /// An empty resolver.
    #[must_use]
    pub fn new() -> Resolver {
        Resolver::default()
    }
}

/// The CPU backend.
#[derive(Debug)]
pub struct CpuBackend {
    cfg: CpuConfig,
    luts: BlendLuts,
}

impl CpuBackend {
    /// Builds a backend. Constructing the twelve blend tables costs about a
    /// millisecond and happens once.
    #[must_use]
    pub fn new(cfg: CpuConfig) -> CpuBackend {
        CpuBackend {
            luts: BlendLuts::build(cfg.weights),
            cfg,
        }
    }

    /// The configuration in force.
    #[must_use]
    pub const fn config(&self) -> &CpuConfig {
        &self.cfg
    }

    /// What this backend can do.
    #[must_use]
    pub fn capabilities(&self) -> RasterizerCaps {
        RasterizerCaps {
            deterministic: self.cfg.pin_simd,
            max_texture_dim: u32::from(u16::MAX),
            supports_dst_read: true,
            tile_size: MIN_BAND_SCANLINES,
        }
    }

    /// Renders a display list into a surface.
    ///
    /// # Errors
    ///
    /// Fails if the surface is bigger than the rasteriser accepts.
    pub fn render(
        &mut self,
        dl: &DisplayList,
        res: &Resolver,
        target: &mut Surface,
    ) -> Result<FrameTimings, BackendError> {
        let max = u32::from(u16::MAX);
        if target.width() > max || target.height() > max {
            return Err(BackendError::SurfaceTooLarge {
                width: target.width(),
                height: target.height(),
                max,
            });
        }
        let t_build = Instant::now();
        let area = dl.bounds().intersection(target.bounds());
        if area.is_empty() {
            return Ok(FrameTimings::default());
        }
        // The same height `plan_bands` gives its first band, without the
        // binning, which the band loop does not use.
        let target_rect = target.bounds();
        let band_lines = band_height(target_rect, self.cfg.band_budget_bytes)
            .min(target_rect.height())
            .max(1);
        let build_us = elapsed_us(t_build);

        let stride = target.width() as usize * 4;
        let width = target.width();
        let rows_per_band = band_lines as usize;
        let cfg = self.cfg;
        let luts = &self.luts;

        let t_raster = Instant::now();
        // Bands are independent and are written back by index, which is
        // what keeps a parallel render byte-identical to a serial one.
        let chunk = rows_per_band * stride;
        let results: Vec<BandStats> = if cfg.threads == 1 {
            target
                .data_mut()
                .chunks_mut(chunk)
                .enumerate()
                .map(|(i, rows)| {
                    render_band(dl, res, luts, &cfg, width, i * rows_per_band, rows, area)
                })
                .collect()
        } else {
            target
                .data_mut()
                .par_chunks_mut(chunk)
                .enumerate()
                .map(|(i, rows)| {
                    render_band(dl, res, luts, &cfg, width, i * rows_per_band, rows, area)
                })
                .collect()
        };
        let raster_us = elapsed_us(t_raster);

        let mut timings = FrameTimings {
            build_us,
            raster_us,
            tiles: u32::try_from(results.iter().filter(|r| r.drew).count()).unwrap_or(u32::MAX),
            ..FrameTimings::default()
        };
        for r in &results {
            timings.rasterised_pixels += r.pixels;
        }
        Ok(timings)
    }
}

#[derive(Debug, Default)]
struct BandStats {
    drew: bool,
    pixels: u64,
}

fn elapsed_us(t: Instant) -> u32 {
    u32::try_from(t.elapsed().as_micros()).unwrap_or(u32::MAX)
}

/// One layer of the compositing stack, band-local.
#[derive(Debug)]
struct Layer {
    /// Straight RGBA8, band-sized.
    pixels: Vec<u8>,
    kind: LayerKind,
}

/// Renders one horizontal band. `y0` is the band's first row in device
/// space, `rows` is the band's slice of the target.
#[allow(
    clippy::too_many_arguments,
    reason = "a band's state is genuinely this wide"
)]
fn render_band(
    dl: &DisplayList,
    res: &Resolver,
    luts: &BlendLuts,
    cfg: &CpuConfig,
    width: u32,
    y0: usize,
    rows: &mut [u8],
    area: DeviceRect,
) -> BandStats {
    let mut stats = BandStats::default();
    let height = u32::try_from(rows.len() / (width as usize * 4)).unwrap_or(0);
    if width == 0 || height == 0 {
        return stats;
    }
    let band = DeviceRect::new(
        0,
        i32::try_from(y0).unwrap_or(0),
        i32::try_from(width).unwrap_or(i32::MAX),
        i32::try_from(y0 + height as usize).unwrap_or(i32::MAX),
    );
    // `area` is the display list's bounds, already intersected with the
    // dirty region: nothing outside it may be touched, which is what makes
    // an incremental redraw cheap rather than merely correct.
    let draw = band.intersection(area);
    if draw.is_empty() {
        return stats;
    }

    let tol_doc = dl.view().tolerance();
    let settings = RenderSettings {
        level: cfg.level(),
        num_threads: 0,
    };
    let mut ctx = RenderContext::new_with(1, 1, settings);
    let mut scratch = Scratch {
        pixmap: Pixmap::new(1, 1),
        flat: BezPath::new(),
    };
    let mut resources = Resources::new();

    // The compositing stack. Index 0 is the target itself; deeper entries
    // are offscreen layers, which is the Capture equivalent.
    let mut layers: Vec<Layer> = Vec::new();
    let mut layer_blend: Vec<(BlendFamily, u8)> = Vec::new();
    // Clip coverage, band-sized; `None` means no clip.
    let mut clips: Vec<Vec<u8>> = Vec::new();

    for cmd in dl.commands() {
        // Reject a primitive that misses the band on its precomputed bounds
        // before looking up its payload: resolving touches the scene op, and
        // at 100 000 commands per band that would be most of a band's time.
        if let DrawCmd::Fill { bounds, .. }
        | DrawCmd::Stroke { bounds, .. }
        | DrawCmd::Image { bounds, .. } = cmd
            && !bounds.intersects(draw)
        {
            stats.drew = true;
            continue;
        }
        match dl.item(cmd) {
            DrawItem::PushClip { path, rule, xf } => {
                let mask = rasterise_clip(
                    &mut ctx,
                    &mut scratch,
                    &mut resources,
                    path,
                    rule,
                    xf,
                    band,
                    tol_doc,
                );
                let merged = match clips.last() {
                    Some(prev) => prev
                        .iter()
                        .zip(mask.iter())
                        .map(|(a, b)| crate::blend::mul(*a, *b))
                        .collect(),
                    None => mask,
                };
                clips.push(merged);
            }
            DrawItem::PopClip => {
                clips.pop();
            }
            DrawItem::PushLayer { kind, .. } => {
                layers.push(Layer {
                    pixels: vec![0u8; rows.len()],
                    kind,
                });
                layer_blend.push((BlendFamily::Mix, 0));
            }
            DrawItem::PopLayer { blend, opacity } => {
                let Some(layer) = layers.pop() else { continue };
                layer_blend.pop();
                let t = flat_level(opacity);
                let (dst_slice, _) = split_target(&mut layers, rows);
                composite_layer(dst_slice, &layer, blend, t, luts, cfg.weights);
            }
            DrawItem::Fill {
                node: _,
                path,
                rule,
                paint,
                xf,
                transparency,
                bounds,
            } => {
                let (dst, _) = split_target(&mut layers, rows);
                stats.pixels += draw_primitive(
                    &mut ctx,
                    &mut scratch,
                    &mut resources,
                    Primitive::Fill { path, rule, xf },
                    paint,
                    transparency,
                    bounds,
                    band,
                    draw,
                    tol_doc,
                    width,
                    clips.last().map(Vec::as_slice),
                    dst,
                    luts,
                    res,
                    cfg,
                );
                stats.drew = true;
            }
            DrawItem::Stroke {
                node: _,
                path,
                style,
                paint,
                xf,
                transparency,
                bounds,
            } => {
                let (dst, _) = split_target(&mut layers, rows);
                stats.pixels += draw_primitive(
                    &mut ctx,
                    &mut scratch,
                    &mut resources,
                    Primitive::Stroke { path, style, xf },
                    paint,
                    transparency,
                    bounds,
                    band,
                    draw,
                    tol_doc,
                    width,
                    clips.last().map(Vec::as_slice),
                    dst,
                    luts,
                    res,
                    cfg,
                );
                stats.drew = true;
            }
            DrawItem::Image {
                node: _,
                image,
                mapping,
                paint,
                transparency,
                bounds,
            } => {
                let (dst, _) = split_target(&mut layers, rows);
                stats.pixels += draw_image_cmd(
                    image,
                    mapping,
                    paint,
                    transparency,
                    bounds.intersection(draw),
                    band,
                    width,
                    clips.last().map(Vec::as_slice),
                    dst,
                    luts,
                    res,
                    cfg,
                );
                stats.drew = true;
            }
            DrawItem::CachedSurface { .. } | DrawItem::Invalid => {
                // Blitting a cached surface is the cache's job and is
                // exercised through `RenderCache`; a display list that has
                // one has already had its pixels produced.
            }
        }
    }

    // Anything left open is a scene-builder bug, which `SceneBuilder::finish`
    // rejects; unwinding here keeps the band from leaking it.
    while let Some(layer) = layers.pop() {
        let (dst, _) = split_target(&mut layers, rows);
        composite_layer(dst, &layer, BlendFamily::Mix, 0, luts, cfg.weights);
    }
    stats
}

/// The destination the next primitive draws into: the innermost layer, or
/// the target rows.
fn split_target<'a>(layers: &'a mut [Layer], rows: &'a mut [u8]) -> (&'a mut [u8], bool) {
    match layers.last_mut() {
        Some(l) => (&mut l.pixels, true),
        None => (rows, false),
    }
}

/// A band's reusable buffers: the coverage pixmap and the flattened path,
/// kept across primitives so that neither is reallocated per command.
struct Scratch {
    pixmap: Pixmap,
    flat: BezPath,
}

enum Primitive<'a> {
    Fill {
        path: &'a PathRef,
        rule: FillRule,
        xf: Transform2D,
    },
    Stroke {
        path: &'a PathRef,
        style: &'a StrokeStyle,
        xf: Transform2D,
    },
}

fn to_fill(rule: FillRule) -> Fill {
    match rule {
        FillRule::EvenOdd => Fill::EvenOdd,
        // NonZero, Positive and Negative all fill by winding sign; vello
        // offers nonzero only, and the sign variants differ from it just in
        // orientation, which the path already carries.
        _ => Fill::NonZero,
    }
}

fn to_cap(c: Cap) -> KCap {
    match c {
        Cap::Butt => KCap::Butt,
        Cap::Round => KCap::Round,
        Cap::Square => KCap::Square,
    }
}

fn to_join(j: Join) -> KJoin {
    match j {
        Join::Mitre => KJoin::Miter,
        Join::Round => KJoin::Round,
        Join::Bevel => KJoin::Bevel,
    }
}

/// Produces a coverage mask for one primitive over `rect`, a device
/// rectangle already clipped to the band.
fn rasterise_coverage(
    ctx: &mut RenderContext,
    scratch: &mut Scratch,
    resources: &mut Resources,
    prim: &Primitive<'_>,
    rect: DeviceRect,
    tol_doc: f64,
) -> bool {
    let (w, h) = (rect.width(), rect.height());
    let Ok(w16) = u16::try_from(w) else {
        return false;
    };
    let Ok(h16) = u16::try_from(h) else {
        return false;
    };
    if w16 == 0 || h16 == 0 {
        return false;
    }
    ctx.reset_and_resize(w16, h16);
    scratch.pixmap.resize(w16, h16);
    ctx.set_paint(AlphaColor::<Srgb>::new([1.0, 1.0, 1.0, 1.0]));
    let to_origin = Affine::translate((-f64::from(rect.x0), -f64::from(rect.y0)));
    // Flatness is **ours**, not the rasteriser's. The original sets it too
    // (`grndrgn.cpp:5626`), and it is what `RenderQuality` actually
    // controls: Draft multiplies the tolerance by five, Final does not.
    // Leaving it to the rasteriser's internal default would make the two
    // quality levels identical and would cap curve fidelity at whatever
    // that default happens to be.
    match prim {
        Primitive::Fill { path, rule, xf } => {
            ctx.set_fill_rule(to_fill(*rule));
            ctx.set_transform(to_origin * xf.to_affine());
            if path.is_polyline() {
                // Flattening a polyline reproduces it element for element,
                // so skipping it changes nothing but the time.
                ctx.fill_path(path.bez());
            } else {
                flatten_into(path.bez(), tol_doc, &mut scratch.flat);
                ctx.fill_path(&scratch.flat);
            }
        }
        Primitive::Stroke { path, style, xf } => {
            let scale = xf.max_scale().max(1e-12);
            // A hairline has no document-space outline: it is one device
            // pixel at any zoom, so its width is expressed back in document
            // units against the current scale.
            let width_doc = if style.width == xarast_geom::Mp::ZERO {
                1.0 / scale
            } else {
                style.width.to_f64()
            };
            let mut stroke = KStroke::new(width_doc)
                .with_caps(to_cap(style.cap_start))
                .with_join(to_join(style.join))
                .with_miter_limit(style.mitre_limit.max(1.0));
            if let Some(d) = &style.dash {
                let pattern = d.resolved(style.width);
                if !pattern.is_empty() {
                    stroke = stroke.with_dashes(0.0, pattern);
                }
            }
            // Expanded to an outline at our tolerance and filled, rather
            // than handed to the rasteriser's stroker, so that the same
            // flatness rule governs strokes and fills.
            let outline = kurbo_stroke(path.bez(), &stroke, tol_doc);
            ctx.set_fill_rule(Fill::NonZero);
            ctx.set_transform(to_origin * xf.to_affine());
            flatten_into(&outline, tol_doc, &mut scratch.flat);
            ctx.fill_path(&scratch.flat);
        }
    }
    ctx.flush();
    ctx.render(&mut scratch.pixmap, resources);
    true
}

/// Flattens a path to line segments within `tol` document units, into a
/// reused buffer.
fn flatten_into(path: &BezPath, tol: f64, out: &mut BezPath) {
    out.truncate(0);
    vello_cpu::kurbo::flatten(path.iter(), tol.max(1e-6), |el| out.push(el));
}

/// Expands a stroke into its outline at our tolerance.
fn kurbo_stroke(path: &BezPath, style: &KStroke, tol: f64) -> BezPath {
    vello_cpu::kurbo::stroke(path.iter(), style, &StrokeOpts::default(), tol.max(1e-6))
}

/// Rasterises a clip path into a band-sized coverage mask.
#[allow(
    clippy::too_many_arguments,
    reason = "a band's state is genuinely this wide"
)]
fn rasterise_clip(
    ctx: &mut RenderContext,
    scratch: &mut Scratch,
    resources: &mut Resources,
    path: &PathRef,
    rule: FillRule,
    xf: Transform2D,
    band: DeviceRect,
    tol_doc: f64,
) -> Vec<u8> {
    let mut mask = vec![0u8; band.area() as usize];
    let guarded = DeviceRect::new(band.x0, band.y0 - BAND_GUARD, band.x1, band.y1 + BAND_GUARD);
    let prim = Primitive::Fill { path, rule, xf };
    if !rasterise_coverage(ctx, scratch, resources, &prim, guarded, tol_doc) {
        return mask;
    }
    let w = band.width() as usize;
    let skip = (band.y0 - guarded.y0) as usize * w;
    for (dst, px) in mask.iter_mut().zip(scratch.pixmap.data()[skip..].iter()) {
        *dst = px.a;
    }
    mask
}

/// Composites one primitive.
#[allow(
    clippy::too_many_arguments,
    reason = "a band's state is genuinely this wide"
)]
fn draw_primitive(
    ctx: &mut RenderContext,
    scratch: &mut Scratch,
    resources: &mut Resources,
    prim: Primitive<'_>,
    paint: &Paint,
    transparency: &Transparency,
    bounds: DeviceRect,
    band: DeviceRect,
    area: DeviceRect,
    tol_doc: f64,
    width: u32,
    clip: Option<&[u8]>,
    dst: &mut [u8],
    luts: &BlendLuts,
    res: &Resolver,
    cfg: &CpuConfig,
) -> u64 {
    let rect = bounds.intersection(band).intersection(area);
    if rect.is_empty() || transparency.family == BlendFamily::None {
        return 0;
    }
    // Rasterise with a guard band, composite without one.
    let guarded = bounds.intersection(DeviceRect::new(
        rect.x0,
        rect.y0 - BAND_GUARD,
        rect.x1,
        rect.y1 + BAND_GUARD,
    ));
    if !rasterise_coverage(ctx, scratch, resources, &prim, guarded, tol_doc) {
        return 0;
    }
    composite_coverage(
        scratch.pixmap.data(),
        guarded,
        rect,
        band,
        width,
        clip,
        dst,
        paint,
        transparency,
        luts,
        res,
        cfg,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "a band's state is genuinely this wide"
)]
fn draw_image_cmd(
    image: ImageId,
    mapping: &GradMapping,
    paint: &Paint,
    transparency: &Transparency,
    bounds: DeviceRect,
    band: DeviceRect,
    width: u32,
    clip: Option<&[u8]>,
    dst: &mut [u8],
    luts: &BlendLuts,
    res: &Resolver,
    cfg: &CpuConfig,
) -> u64 {
    let rect = bounds.intersection(band);
    if rect.is_empty() || transparency.family == BlendFamily::None {
        return 0;
    }
    // The image occupies its whole mapped quadrilateral, so coverage comes
    // from the mapping rather than from a rasterised path: a point maps
    // inside the unit square or it does not.
    let paint = match paint {
        Paint::Image { .. } => paint.clone(),
        _ => Paint::Image {
            image,
            mapping: *mapping,
            repeat: crate::paint::Repeat::Simple,
            filter: crate::paint::Filter::Bilinear,
            contone: None,
            adjust: crate::paint::BitmapAdjust::default(),
        },
    };
    let mut touched = 0u64;
    for y in rect.y0..rect.y1 {
        for x in rect.x0..rect.x1 {
            let p = Point64::new(f64::from(x) + 0.5, f64::from(y) + 0.5);
            let Some((u, v)) = mapping.to_frame(p) else {
                continue;
            };
            if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
                continue;
            }
            let mut cov = 255u8;
            if let Some(mask) = clip {
                cov = crate::blend::mul(cov, mask_at(mask, band, width, x, y));
            }
            if cov == 0 {
                continue;
            }
            let src = eval_paint(&paint, &res.ramps, &res.images, p);
            let t = level_at(transparency, res, p);
            blend_into(
                dst,
                band,
                width,
                x,
                y,
                src,
                t,
                cov,
                transparency.family,
                luts,
                cfg,
            );
            touched += 1;
        }
    }
    touched
}

#[allow(
    clippy::too_many_arguments,
    reason = "a band's state is genuinely this wide"
)]
fn composite_coverage(
    coverage: &[PremulRgba8],
    coverage_rect: DeviceRect,
    rect: DeviceRect,
    band: DeviceRect,
    width: u32,
    clip: Option<&[u8]>,
    dst: &mut [u8],
    paint: &Paint,
    transparency: &Transparency,
    luts: &BlendLuts,
    res: &Resolver,
    cfg: &CpuConfig,
) -> u64 {
    let solid = match paint {
        Paint::Solid(c) => Some(*c),
        _ => None,
    };
    let flat = match &transparency.source {
        TranspSource::Flat(t) => Some(*t),
        _ => None,
    };
    // An opaque solid colour mixed at zero transparency replaces a fully
    // covered pixel outright, whatever was under it: `composite` gives the
    // source with alpha 255 there, and premultiplying that is the identity.
    // Most pixels of a flat-filled scene take this path.
    // `opaque_replace_matches_the_general_path` pins the equivalence.
    let replace = match (solid, flat) {
        (Some(c), Some(0)) if c.a == 255 && transparency.family == BlendFamily::Mix => Some(c),
        _ => None,
    };
    let mut touched = 0u64;
    let cw = coverage_rect.width() as usize;
    for y in rect.y0..rect.y1 {
        let row = (y - coverage_rect.y0) as usize;
        for x in rect.x0..rect.x1 {
            let col = (x - coverage_rect.x0) as usize;
            let mut cov = coverage[row * cw + col].a;
            if cov == 0 {
                continue;
            }
            if let Some(mask) = clip {
                cov = crate::blend::mul(cov, mask_at(mask, band, width, x, y));
                if cov == 0 {
                    continue;
                }
            }
            if cov == 255
                && let Some(c) = replace
            {
                write_opaque(dst, band, width, x, y, c);
                touched += 1;
                continue;
            }
            let p = Point64::new(f64::from(x) + 0.5, f64::from(y) + 0.5);
            let src = match solid {
                Some(c) => c,
                None => eval_paint(paint, &res.ramps, &res.images, p),
            };
            let t = match flat {
                Some(t) => t,
                None => level_at(transparency, res, p),
            };
            blend_into(
                dst,
                band,
                width,
                x,
                y,
                src,
                t,
                cov,
                transparency.family,
                luts,
                cfg,
            );
            touched += 1;
        }
    }
    touched
}

fn mask_at(mask: &[u8], band: DeviceRect, width: u32, x: i32, y: i32) -> u8 {
    let ix = (y - band.y0) as usize * width as usize + (x - band.x0) as usize;
    mask.get(ix).copied().unwrap_or(0)
}

/// The transparency level at a point, from a flat value, a graduated ramp
/// or a bitmap.
fn level_at(t: &Transparency, res: &Resolver, p: Point64) -> u8 {
    match &t.source {
        TranspSource::Flat(v) => *v,
        TranspSource::Gradient {
            shape,
            mapping,
            repeat,
            ramp,
        } => {
            let Some(table) = res.transparency_ramps.get(ramp.index() as usize) else {
                return 0;
            };
            if table.is_empty() {
                return 0;
            }
            let Some(s) = crate::paint::grad_param(*shape, *mapping, p) else {
                return 0;
            };
            let s = crate::paint::apply_repeat(s, *repeat);
            let idx = (s * (table.len() - 1) as f64)
                .round()
                .clamp(0.0, (table.len() - 1) as f64);
            table[idx as usize]
        }
        TranspSource::Image {
            image,
            mapping,
            repeat,
        } => {
            let Some(img) = res.images.get(*image) else {
                return 0;
            };
            let Some((u, v)) = mapping.to_frame(p) else {
                return 0;
            };
            let x = (u * f64::from(img.width()) - 0.5).round();
            let y = (v * f64::from(img.height()) - 0.5).round();
            // The original reads the transparency out of the luminance of
            // the tile pattern.
            LumaWeights::BT601.luma(img.texel(x as i64, y as i64, *repeat))
        }
    }
}

/// Reads a straight colour out of a premultiplied band buffer, blends, and
/// writes it back premultiplied.
#[allow(
    clippy::too_many_arguments,
    reason = "the pixel address is four of these"
)]
fn blend_into(
    dst: &mut [u8],
    band: DeviceRect,
    width: u32,
    x: i32,
    y: i32,
    src: Rgba8,
    t: u8,
    cov: u8,
    family: BlendFamily,
    luts: &BlendLuts,
    cfg: &CpuConfig,
) {
    let o = ((y - band.y0) as usize * width as usize + (x - band.x0) as usize) * 4;
    if o + 4 > dst.len() {
        return;
    }
    let d = unpremultiply(Rgba8 {
        r: dst[o],
        g: dst[o + 1],
        b: dst[o + 2],
        a: dst[o + 3],
    });
    // A source with its own alpha attenuates coverage: an image texel with
    // alpha 128 covers half as much as an opaque one.
    let cov = crate::blend::mul(cov, src.a);
    let out = composite(family, luts, cfg.weights, src, t, cov, d);
    let out = premultiply(out);
    dst[o] = out.r;
    dst[o + 1] = out.g;
    dst[o + 2] = out.b;
    dst[o + 3] = out.a;
}

/// Writes an opaque colour over a pixel: [`blend_into`] for a fully
/// covered, opaque, zero-transparency Mix.
fn write_opaque(dst: &mut [u8], band: DeviceRect, width: u32, x: i32, y: i32, c: Rgba8) {
    let o = ((y - band.y0) as usize * width as usize + (x - band.x0) as usize) * 4;
    if let Some(px) = dst.get_mut(o..o + 4) {
        px.copy_from_slice(&[c.r, c.g, c.b, 255]);
    }
}

fn unpremultiply(c: Rgba8) -> Rgba8 {
    if c.a == 0 {
        return Rgba8 {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        };
    }
    if c.a == 255 {
        return c;
    }
    let un = |v: u8| -> u8 {
        u8::try_from((u32::from(v) * 255 + u32::from(c.a) / 2) / u32::from(c.a)).unwrap_or(255)
    };
    Rgba8 {
        r: un(c.r),
        g: un(c.g),
        b: un(c.b),
        a: c.a,
    }
}

fn premultiply(c: Rgba8) -> Rgba8 {
    if c.a == 255 {
        return c;
    }
    Rgba8 {
        r: crate::blend::mul(c.r, c.a),
        g: crate::blend::mul(c.g, c.a),
        b: crate::blend::mul(c.b, c.a),
        a: c.a,
    }
}

fn flat_level(t: &Transparency) -> u8 {
    match &t.source {
        TranspSource::Flat(v) => *v,
        _ => 0,
    }
}

/// Composites a finished offscreen layer back onto what is beneath it.
fn composite_layer(
    dst: &mut [u8],
    layer: &Layer,
    blend: BlendFamily,
    t: u8,
    luts: &BlendLuts,
    weights: LumaWeights,
) {
    if blend == BlendFamily::None {
        return;
    }
    for (d, s) in dst
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(layer.pixels.as_chunks::<4>().0.iter())
    {
        if s[3] == 0 {
            continue;
        }
        let src = unpremultiply(Rgba8 {
            r: s[0],
            g: s[1],
            b: s[2],
            a: s[3],
        });
        let under = unpremultiply(Rgba8 {
            r: d[0],
            g: d[1],
            b: d[2],
            a: d[3],
        });
        // An isolated layer carries its own alpha, which becomes the
        // coverage of the composite.
        let cov = match layer.kind {
            LayerKind::Plain => 255,
            LayerKind::Isolated | LayerKind::DestinationReading => src.a,
        };
        let out = premultiply(composite(blend, luts, weights, src, t, cov, under));
        d[0] = out.r;
        d[1] = out.g;
        d[2] = out.b;
        d[3] = out.a;
    }
}

/// A vector index as a command index; see `display_list::idx`.
fn idx(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

/// The immediate-mode facade over the CPU backend.
///
/// It records into a display list and renders it on [`Rasterizer::end_frame`],
/// which keeps one compositor rather than two.
#[derive(Debug)]
pub struct CpuRasterizer {
    backend: CpuBackend,
    resolver: Resolver,
    parts: ListParts,
    clip: DeviceRect,
    target: Option<Surface>,
    dirty: DirtyRect,
    next_layer: u32,
}

impl CpuRasterizer {
    /// Builds a facade over a freshly configured CPU backend.
    #[must_use]
    pub fn new(cfg: CpuConfig) -> CpuRasterizer {
        CpuRasterizer {
            backend: CpuBackend::new(cfg),
            resolver: Resolver::new(),
            parts: ListParts::default(),
            clip: DeviceRect::EMPTY,
            target: None,
            dirty: DirtyRect::NONE,
            next_layer: 0,
        }
    }

    /// The resolver holding ramps and images, so a caller can register them.
    pub fn resolver_mut(&mut self) -> &mut Resolver {
        &mut self.resolver
    }
}

impl Rasterizer for CpuRasterizer {
    fn begin_frame(&mut self, target: &mut Surface, clip: DeviceRect) {
        self.parts = ListParts::default();
        self.dirty = DirtyRect::NONE;
        self.clip = clip.intersection(target.bounds());
        self.target = Some(std::mem::replace(target, Surface::new(0, 0)));
        self.next_layer = 0;
    }

    fn fill_path(&mut self, path: &PathRef, rule: FillRule, paint: &Paint, xf: &Transform2D) {
        let bounds = crate::display_list::device_bounds_of(path, *xf, 0.0);
        self.dirty = self
            .dirty
            .union(DirtyRect::of(bounds.intersection(self.clip)));
        let p = &mut self.parts;
        let op = idx(p.ops.len());
        p.ops.push(SceneOp::Fill {
            id: crate::scene::SceneNodeId(0),
            path: path.clone(),
            rule,
            paint: paint.clone(),
            transparency: Transparency::OPAQUE,
        });
        let xf_i = idx(p.xforms.len());
        p.xforms.push(*xf);
        p.cmds.push(DrawCmd::Fill {
            op,
            xf: xf_i,
            paint: SCENE_PAINT,
            dst_read: false,
            bounds,
        });
    }

    fn stroke_path(
        &mut self,
        path: &PathRef,
        style: &StrokeStyle,
        paint: &Paint,
        xf: &Transform2D,
    ) {
        let pad = style.width.to_f64() * 0.5 * style.mitre_limit.max(1.0);
        let bounds = crate::display_list::device_bounds_of(path, *xf, pad).inflated(1);
        self.dirty = self
            .dirty
            .union(DirtyRect::of(bounds.intersection(self.clip)));
        let p = &mut self.parts;
        let op = idx(p.ops.len());
        p.ops.push(SceneOp::Stroke {
            id: crate::scene::SceneNodeId(0),
            path: path.clone(),
            style: style.clone(),
            paint: paint.clone(),
            transparency: Transparency::OPAQUE,
        });
        let xf_i = idx(p.xforms.len());
        p.xforms.push(*xf);
        p.cmds.push(DrawCmd::Stroke {
            op,
            xf: xf_i,
            paint: SCENE_PAINT,
            dst_read: false,
            bounds,
        });
    }

    fn draw_image(&mut self, image: ImageId, mapping: &GradMapping, paint: &Paint) {
        let bounds = self.clip;
        self.dirty = self.dirty.union(DirtyRect::of(bounds));
        let p = &mut self.parts;
        let op = idx(p.ops.len());
        p.ops.push(SceneOp::Image {
            id: crate::scene::SceneNodeId(0),
            image,
            mapping: *mapping,
            paint: paint.clone(),
            transparency: Transparency::OPAQUE,
        });
        let m = idx(p.mappings.len());
        p.mappings.push(*mapping);
        p.cmds.push(DrawCmd::Image {
            op,
            mapping: m,
            paint: SCENE_PAINT,
            dst_read: false,
            bounds,
        });
    }

    fn push_layer(&mut self, kind: LayerKind, bounds: DeviceRect) -> LayerId {
        self.parts.cmds.push(DrawCmd::PushLayer {
            kind,
            bounds,
            needs_dst_read: kind == LayerKind::DestinationReading,
        });
        self.next_layer += 1;
        LayerId(self.next_layer - 1)
    }

    fn pop_layer(&mut self, _id: LayerId, blend: BlendFamily, opacity: u8) {
        // The pop's transparency needs an op to live in; this one is
        // storage only and never drawn.
        let p = &mut self.parts;
        let layer = idx(p.ops.len());
        p.ops.push(SceneOp::PushLayer {
            kind: LayerKind::Plain,
            transparency: Transparency::flat(blend, opacity),
        });
        p.cmds.push(DrawCmd::PopLayer { blend, layer });
    }

    fn end_frame(&mut self) -> DirtyRect {
        let Some(mut target) = self.target.take() else {
            return DirtyRect::NONE;
        };
        let dl = crate::display_list::DisplayList::from_parts(
            std::mem::take(&mut self.parts),
            self.clip,
            crate::display_list::ViewParams {
                viewport: self.clip,
                ..crate::display_list::ViewParams::default()
            },
        );
        let _ = self.backend.render(&dl, &self.resolver, &mut target);
        self.target = Some(target);
        self.dirty
    }

    fn capabilities(&self) -> RasterizerCaps {
        self.backend.capabilities()
    }
}

impl CpuRasterizer {
    /// Takes back the surface handed to [`Rasterizer::begin_frame`].
    ///
    /// The facade owns the target between `begin_frame` and here, because
    /// the commands it records refer to it.
    #[must_use]
    pub fn take_target(&mut self) -> Option<Surface> {
        self.target.take()
    }
}

/// Wraps a band of a surface as something `vello_cpu` can render into.
///
/// Unused by the coverage path, but the GPU-parity harness and any future
/// fast path need it, and it is the one place that asserts the two crates
/// agree on the pixel format.
#[must_use]
pub fn band_pixmap(rows: &mut [u8], width: u16, height: u16) -> Option<PixmapMut<'_>> {
    PixmapMut::new(width, height, rows)
}

/// The render mode a quality level asks `vello_cpu` for.
#[must_use]
pub const fn render_mode(q: RenderQuality) -> RenderMode {
    match q {
        RenderQuality::Draft => RenderMode::OptimizeSpeed,
        RenderQuality::Final => RenderMode::OptimizeQuality,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_replace_matches_the_general_path() {
        let luts = BlendLuts::build(LumaWeights::BT601);
        let cfg = CpuConfig::deterministic();
        let band = DeviceRect::new(0, 0, 1, 1);
        let mut s: u64 = 0x9e37_79b9;
        let mut next = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        let check = |src: Rgba8, under: [u8; 4]| {
            let mut slow = under;
            blend_into(
                &mut slow,
                band,
                1,
                0,
                0,
                src,
                0,
                255,
                BlendFamily::Mix,
                &luts,
                &cfg,
            );
            let mut fast = under;
            write_opaque(&mut fast, band, 1, 0, 0, src);
            assert_eq!(slow, fast, "{src:?} over {under:?}");
        };
        // Every premultiplied destination alpha, with channels at and
        // below it, and random opaque sources.
        for a in 0..=255u8 {
            for _ in 0..64 {
                let r = next();
                let under = [
                    (r as u8).min(a),
                    ((r >> 8) as u8).min(a),
                    ((r >> 16) as u8).min(a),
                    a,
                ];
                let src = Rgba8 {
                    r: (r >> 24) as u8,
                    g: (r >> 32) as u8,
                    b: (r >> 40) as u8,
                    a: 255,
                };
                check(src, under);
            }
        }
    }
}
