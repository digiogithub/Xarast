//! Paints: solids, the five gradient shapes, image fills, and the maths that
//! turns a device point into a ramp parameter.
//!
//! # What no library gave us
//!
//! Conical and diamond gradients, and perspective mapping, exist in none of
//! the candidate rasterisers (`research/03 §3.2`), so they are ours. Conical
//! is `atan2` normalised to `[0, 1)`; diamond is `max(|u|, |v|)`, the L∞
//! metric in the A/B/C frame. The GPU mirrors these in WGSL and the parity
//! test bounds the disagreement at 1/255, because WGSL's `atan2` is not
//! Rust's.
//!
//! # A deliberate deviation from the phase's sketch
//!
//! The phase document gives `Paint::Gradient` a single `ramp: RampId`, and
//! lists `Mesh3`/`Mesh4` among the shapes. A mesh has no ramp: it has three
//! or four corner colours. Rather than leave a field that is meaningless for
//! two of the six shapes, the ramp field is [`GradRamp`], which is either a
//! table or a mesh, and [`Paint::validate`] rejects a mismatch. The shape
//! enumeration is unchanged, so the gradient matrix still has its 6 × 4 × 2
//! cells.

use std::sync::Arc;

use xarast_color::Rgba8;

use crate::precision::Point64;
use crate::ramp::{Profile, RampCache, RampId};

/// The five gradient geometries plus the two meshes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GradShape {
    /// `s` is the coordinate along the A→C axis, in the A/B/C frame, so a
    /// skewed B gives a skewed gradient.
    Linear,
    /// Elliptical, with two independent radii: `s = ‖(u, v)‖₂`.
    Radial,
    /// Angular: `s = atan2(v, u)` normalised to `[0, 1)`.
    Conical,
    /// `s = max(|u|, |v|)`, the L∞ metric.
    Diamond,
    /// Three corner colours, barycentric.
    Mesh3,
    /// Four corner colours, bilinear.
    Mesh4,
}

/// Every shape, for the coverage matrix.
pub const ALL_SHAPES: [GradShape; 6] = [
    GradShape::Linear,
    GradShape::Radial,
    GradShape::Conical,
    GradShape::Diamond,
    GradShape::Mesh3,
    GradShape::Mesh4,
];

impl GradShape {
    /// Whether the shape evaluates to a scalar ramp parameter rather than
    /// interpolating corner colours.
    #[must_use]
    pub const fn is_scalar(self) -> bool {
        !matches!(self, GradShape::Mesh3 | GradShape::Mesh4)
    }
}

/// How the shape's frame is placed on the page.
///
/// `A` is the origin, `C` the end of the main axis and `B` the end of the
/// perpendicular axis — Xara's own control points, in that order. The
/// original reorders them on the way in (`grndrgn.cpp:2524`); we store them
/// already in this order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GradMapping {
    /// A parallelogram: two basis vectors from `a`.
    Affine {
        /// Origin.
        a: Point64,
        /// End of the perpendicular axis.
        b: Point64,
        /// End of the main axis.
        c: Point64,
    },
    /// A quadrilateral: the interpolation is projective, with `d` the fourth
    /// corner. `MouldPerspective::WillBeValid` in the original; here
    /// [`GradMapping::is_valid`] plays that part.
    Perspective {
        /// Origin, the image of `(0, 0)`.
        a: Point64,
        /// The image of `(0, 1)`.
        b: Point64,
        /// The image of `(1, 0)`.
        c: Point64,
        /// The image of `(1, 1)`.
        d: Point64,
    },
}

/// Every mapping kind, for the coverage matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MappingKind {
    /// [`GradMapping::Affine`].
    Affine,
    /// [`GradMapping::Perspective`].
    Perspective,
}

/// Both mapping kinds, for the coverage matrix.
pub const ALL_MAPPINGS: [MappingKind; 2] = [MappingKind::Affine, MappingKind::Perspective];

/// A 3×3 projective matrix, only ever used for perspective mappings.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Mat3([f64; 9]);

impl Mat3 {
    fn invert(self) -> Option<Mat3> {
        let m = self.0;
        let c0 = m[4] * m[8] - m[5] * m[7];
        let c1 = m[5] * m[6] - m[3] * m[8];
        let c2 = m[3] * m[7] - m[4] * m[6];
        let det = m[0] * c0 + m[1] * c1 + m[2] * c2;
        if !det.is_finite() || det.abs() < 1e-12 {
            return None;
        }
        let id = 1.0 / det;
        Some(Mat3([
            c0 * id,
            (m[2] * m[7] - m[1] * m[8]) * id,
            (m[1] * m[5] - m[2] * m[4]) * id,
            c1 * id,
            (m[0] * m[8] - m[2] * m[6]) * id,
            (m[2] * m[3] - m[0] * m[5]) * id,
            c2 * id,
            (m[1] * m[6] - m[0] * m[7]) * id,
            (m[0] * m[4] - m[1] * m[3]) * id,
        ]))
    }

    fn apply(self, p: Point64) -> Option<(f64, f64)> {
        let m = self.0;
        let w = m[6] * p.x + m[7] * p.y + m[8];
        if !w.is_finite() || w.abs() < 1e-12 {
            return None;
        }
        Some((
            (m[0] * p.x + m[1] * p.y + m[2]) / w,
            (m[3] * p.x + m[4] * p.y + m[5]) / w,
        ))
    }
}

impl GradMapping {
    /// Which kind this is.
    #[must_use]
    pub const fn kind(self) -> MappingKind {
        match self {
            GradMapping::Affine { .. } => MappingKind::Affine,
            GradMapping::Perspective { .. } => MappingKind::Perspective,
        }
    }

    /// A unit-square affine mapping, handy in tests.
    #[must_use]
    pub fn unit() -> GradMapping {
        GradMapping::Affine {
            a: Point64::new(0.0, 0.0),
            b: Point64::new(0.0, 1.0),
            c: Point64::new(1.0, 0.0),
        }
    }

    /// Whether the mapping is non-degenerate: an affine one needs
    /// independent axes, a projective one a convex, non-collapsed quad.
    #[must_use]
    pub fn is_valid(self) -> bool {
        self.inverse().is_some()
    }

    /// The map from device space back to the `(u, v)` frame.
    fn inverse(self) -> Option<Mat3> {
        match self {
            GradMapping::Affine { a, b, c } => {
                let (ux, uy) = (c.x - a.x, c.y - a.y);
                let (vx, vy) = (b.x - a.x, b.y - a.y);
                Mat3([ux, vx, a.x, uy, vy, a.y, 0.0, 0.0, 1.0]).invert()
            }
            GradMapping::Perspective { a, b, c, d } => {
                // Corners of the unit square in the order (0,0), (1,0),
                // (1,1), (0,1) -> a, c, d, b.
                let p = [a, c, d, b];
                let sx = p[0].x - p[1].x + p[2].x - p[3].x;
                let sy = p[0].y - p[1].y + p[2].y - p[3].y;
                let m = if sx.abs() < 1e-12 && sy.abs() < 1e-12 {
                    Mat3([
                        p[1].x - p[0].x,
                        p[3].x - p[0].x,
                        p[0].x,
                        p[1].y - p[0].y,
                        p[3].y - p[0].y,
                        p[0].y,
                        0.0,
                        0.0,
                        1.0,
                    ])
                } else {
                    let dx1 = p[1].x - p[2].x;
                    let dx2 = p[3].x - p[2].x;
                    let dy1 = p[1].y - p[2].y;
                    let dy2 = p[3].y - p[2].y;
                    let den = dx1 * dy2 - dx2 * dy1;
                    if !den.is_finite() || den.abs() < 1e-12 {
                        return None;
                    }
                    let g = (sx * dy2 - dx2 * sy) / den;
                    let h = (dx1 * sy - sx * dy1) / den;
                    Mat3([
                        p[1].x - p[0].x + g * p[1].x,
                        p[3].x - p[0].x + h * p[3].x,
                        p[0].x,
                        p[1].y - p[0].y + g * p[1].y,
                        p[3].y - p[0].y + h * p[3].y,
                        p[0].y,
                        g,
                        h,
                        1.0,
                    ])
                };
                m.invert()
            }
        }
    }

    /// Maps a device point into the `(u, v)` frame, or `None` if the
    /// mapping is degenerate or the point is behind the horizon of a
    /// projective one.
    #[must_use]
    pub fn to_frame(self, p: Point64) -> Option<(f64, f64)> {
        self.inverse()?.apply(p)
    }

    /// Translates the mapping, used when a paint follows a transformed node.
    #[must_use]
    pub fn transformed(self, xf: crate::precision::Transform2D) -> GradMapping {
        match self {
            GradMapping::Affine { a, b, c } => GradMapping::Affine {
                a: xf.apply(a),
                b: xf.apply(b),
                c: xf.apply(c),
            },
            GradMapping::Perspective { a, b, c, d } => GradMapping::Perspective {
                a: xf.apply(a),
                b: xf.apply(b),
                c: xf.apply(c),
                d: xf.apply(d),
            },
        }
    }
}

/// How a gradient's parameter behaves outside `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Repeat {
    /// Clamp to the end colours.
    #[default]
    Simple,
    /// Tile.
    Repeat,
    /// Tile, mirroring alternate tiles.
    Mirror,
    /// Tile, with the long ramp table: the "repeating high quality" mode,
    /// which exists precisely to stop a tiled gradient from banding.
    RepeatHq,
}

/// Every repeat mode, for the coverage matrix.
pub const ALL_REPEATS: [Repeat; 4] = [
    Repeat::Simple,
    Repeat::Repeat,
    Repeat::Mirror,
    Repeat::RepeatHq,
];

/// Folds a raw parameter into `0..=1` according to the repeat mode.
#[must_use]
pub fn apply_repeat(s: f64, repeat: Repeat) -> f64 {
    if !s.is_finite() {
        return 0.0;
    }
    match repeat {
        Repeat::Simple => s.clamp(0.0, 1.0),
        Repeat::Repeat | Repeat::RepeatHq => s.rem_euclid(1.0),
        Repeat::Mirror => {
            let m = s.rem_euclid(2.0);
            if m > 1.0 { 2.0 - m } else { m }
        }
    }
}

/// Evaluates a scalar gradient shape at a device point, before the repeat
/// mode is applied.
///
/// Returns `None` for a mesh shape or a degenerate mapping.
#[must_use]
pub fn grad_param(shape: GradShape, mapping: GradMapping, p: Point64) -> Option<f64> {
    let (u, v) = mapping.to_frame(p)?;
    Some(match shape {
        GradShape::Linear => u,
        GradShape::Radial => (u * u + v * v).sqrt(),
        // atan2 is measured from the main axis and normalised to one turn.
        GradShape::Conical => (v.atan2(u) / std::f64::consts::TAU).rem_euclid(1.0),
        GradShape::Diamond => u.abs().max(v.abs()),
        GradShape::Mesh3 | GradShape::Mesh4 => return None,
    })
}

/// A gradient's colour source: a ramp table, or mesh corner colours.
#[derive(Debug, Clone, PartialEq)]
pub enum GradRamp {
    /// A 256- or 2048-entry table held in a [`RampCache`].
    Table(RampId),
    /// Three corner colours at `(0,0)`, `(1,0)`, `(0,1)` in the frame.
    Mesh3([Rgba8; 3]),
    /// Four corner colours at `(0,0)`, `(1,0)`, `(0,1)`, `(1,1)`.
    Mesh4([Rgba8; 4]),
}

/// An image in the renderer's registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImageId(u32);

impl ImageId {
    /// The index into the registry.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}

/// A decoded, straight (non-premultiplied) RGBA8 image.
///
/// Decoding is Phase 10's; this crate only samples what it is handed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRef {
    width: u32,
    height: u32,
    data: Arc<Vec<u8>>,
}

impl ImageRef {
    /// Wraps straight RGBA8 pixel data.
    ///
    /// # Panics
    ///
    /// Panics if `data` is not exactly `width * height * 4` bytes.
    #[must_use]
    pub fn new(width: u32, height: u32, data: Vec<u8>) -> ImageRef {
        assert_eq!(
            data.len(),
            width as usize * height as usize * 4,
            "image data must be width * height * 4 bytes"
        );
        ImageRef {
            width,
            height,
            data: Arc::new(data),
        }
    }

    /// Width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Reads one texel with the given repeat mode applied to both axes.
    #[must_use]
    pub fn texel(&self, x: i64, y: i64, repeat: Repeat) -> Rgba8 {
        if self.width == 0 || self.height == 0 {
            return Rgba8::TRANSPARENT;
        }
        let wrap = |v: i64, n: i64| -> i64 {
            match repeat {
                Repeat::Simple => v.clamp(0, n - 1),
                Repeat::Repeat | Repeat::RepeatHq => v.rem_euclid(n),
                Repeat::Mirror => {
                    let m = v.rem_euclid(2 * n);
                    if m >= n { 2 * n - 1 - m } else { m }
                }
            }
        };
        let x = wrap(x, i64::from(self.width));
        let y = wrap(y, i64::from(self.height));
        let o = (y as usize * self.width as usize + x as usize) * 4;
        Rgba8 {
            r: self.data[o],
            g: self.data[o + 1],
            b: self.data[o + 2],
            a: self.data[o + 3],
        }
    }
}

/// The images a display list refers to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImageRegistry {
    images: Vec<ImageRef>,
}

impl ImageRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> ImageRegistry {
        ImageRegistry::default()
    }

    /// Adds an image and returns its id.
    pub fn insert(&mut self, image: ImageRef) -> ImageId {
        let id = ImageId(u32::try_from(self.images.len()).expect("image registry overflow"));
        self.images.push(image);
        id
    }

    /// Looks an image up.
    #[must_use]
    pub fn get(&self, id: ImageId) -> Option<&ImageRef> {
        self.images.get(id.0 as usize)
    }

    /// How many images are registered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.images.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }
}

/// How an image fill samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Filter {
    /// Point sampling. Draft quality uses this for every image.
    #[default]
    Nearest,
    /// Bilinear, the original's `SetTileSmoothingFlag`.
    Bilinear,
    /// The maximum-quality filter, the original's `SetTileFilteringFlag`,
    /// used for printing and export.
    HighQuality,
}

/// Per-bitmap adjustments the original applies inside the fill.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BitmapAdjust {
    /// −1.0 to 1.0, 0 neutral.
    pub brightness: f32,
    /// −1.0 to 1.0, 0 neutral.
    pub contrast: f32,
    /// Above zero; 1.0 neutral.
    pub gamma: f32,
    /// −1.0 to 1.0, 0 neutral.
    pub saturation: f32,
    /// The bias/gain profile applied to the transparency channel.
    pub profile: Profile,
}

impl Default for BitmapAdjust {
    fn default() -> BitmapAdjust {
        BitmapAdjust {
            brightness: 0.0,
            contrast: 0.0,
            gamma: 1.0,
            saturation: 0.0,
            profile: Profile::IDENTITY,
        }
    }
}

impl BitmapAdjust {
    /// Whether the adjustment is the identity and can be skipped.
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.brightness == 0.0
            && self.contrast == 0.0
            && (self.gamma - 1.0).abs() < f32::EPSILON
            && self.saturation == 0.0
            && self.profile == Profile::IDENTITY
    }
}

/// The parameters of a plasma or clouds fill.
///
/// The generator itself is Phase 13's (`fracfill.cpp` is application code,
/// not CDraw's); this crate only carries the parameters and expects the
/// fill to have been materialised into an [`ImageRef`] before rasterisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FractalParams {
    /// The pseudorandom seed.
    pub seed: u32,
    /// Whether the result tiles seamlessly.
    pub tileable: bool,
    /// Vertical squash, in the original's units.
    pub squash: u16,
    /// 0..=32.
    pub graininess: u8,
    /// 0..=255: the pull of each midpoint towards the centre.
    pub gravity: u8,
}

/// What fills a shape.
#[derive(Debug, Clone, PartialEq)]
pub enum Paint {
    /// One colour.
    Solid(Rgba8),
    /// A gradient or a mesh.
    Gradient {
        /// Which geometry.
        shape: GradShape,
        /// Where its frame sits.
        mapping: GradMapping,
        /// What happens outside `0..=1`.
        repeat: Repeat,
        /// Where the colours come from.
        ramp: GradRamp,
    },
    /// A bitmap fill.
    Image {
        /// The image.
        image: ImageId,
        /// The parallelogram or quadrilateral it is mapped onto.
        mapping: GradMapping,
        /// What happens outside the tile.
        repeat: Repeat,
        /// How it is sampled.
        filter: Filter,
        /// A two-colour remap of the image's luminance, with the effect
        /// space the ramp is built in.
        contone: Option<(Rgba8, Rgba8, crate::ramp::EffectSpace)>,
        /// Brightness, contrast, gamma, saturation.
        adjust: BitmapAdjust,
    },
    /// Materialised into an [`Paint::Image`] by the Phase 13 generator
    /// before it reaches a backend.
    Fractal(FractalParams),
}

impl Default for Paint {
    fn default() -> Paint {
        Paint::Solid(Rgba8::BLACK)
    }
}

/// Why a paint cannot be rendered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PaintError {
    /// A mesh shape was given a ramp table, or a scalar shape a mesh.
    #[error("gradient shape {shape} does not take this kind of ramp")]
    ShapeRampMismatch {
        /// The offending shape.
        shape: &'static str,
    },
    /// The control points are collinear or the quadrilateral is degenerate.
    #[error("the gradient mapping is degenerate")]
    DegenerateMapping,
    /// A fractal fill reached a backend without being materialised.
    #[error("a fractal fill must be materialised into an image before rasterisation")]
    UnmaterialisedFractal,
}

impl Paint {
    /// Checks that the paint can be evaluated at all.
    ///
    /// # Errors
    ///
    /// Returns the reason the paint is unrenderable.
    pub fn validate(&self) -> Result<(), PaintError> {
        match self {
            Paint::Solid(_) => Ok(()),
            Paint::Gradient {
                shape,
                mapping,
                ramp,
                ..
            } => {
                let ok = match (shape, ramp) {
                    (GradShape::Mesh3, GradRamp::Mesh3(_))
                    | (GradShape::Mesh4, GradRamp::Mesh4(_)) => true,
                    (s, GradRamp::Table(_)) if s.is_scalar() => true,
                    _ => false,
                };
                if !ok {
                    return Err(PaintError::ShapeRampMismatch {
                        shape: match shape {
                            GradShape::Linear => "Linear",
                            GradShape::Radial => "Radial",
                            GradShape::Conical => "Conical",
                            GradShape::Diamond => "Diamond",
                            GradShape::Mesh3 => "Mesh3",
                            GradShape::Mesh4 => "Mesh4",
                        },
                    });
                }
                if mapping.is_valid() {
                    Ok(())
                } else {
                    Err(PaintError::DegenerateMapping)
                }
            }
            Paint::Image { mapping, .. } => {
                if mapping.is_valid() {
                    Ok(())
                } else {
                    Err(PaintError::DegenerateMapping)
                }
            }
            Paint::Fractal(_) => Err(PaintError::UnmaterialisedFractal),
        }
    }

    /// Whether the paint is the same colour everywhere, which lets the
    /// rasteriser take the solid fast path.
    #[must_use]
    pub fn is_solid(&self) -> bool {
        matches!(self, Paint::Solid(_))
    }
}

fn lerp_rgba(a: Rgba8, b: Rgba8, t: f64) -> Rgba8 {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| -> u8 {
        (f64::from(x) + (f64::from(y) - f64::from(x)) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Rgba8 {
        r: m(a.r, b.r),
        g: m(a.g, b.g),
        b: m(a.b, b.b),
        a: m(a.a, b.a),
    }
}

/// Evaluates a paint at one device-space point.
///
/// This is the CPU path and the definition the WGSL shader mirrors. An
/// unevaluable paint (degenerate mapping, missing image, unmaterialised
/// fractal) yields transparent rather than panicking: a corrupt document
/// must not take the renderer down.
#[must_use]
pub fn eval_paint(paint: &Paint, ramps: &RampCache, images: &ImageRegistry, p: Point64) -> Rgba8 {
    match paint {
        Paint::Solid(c) => *c,
        Paint::Gradient {
            shape,
            mapping,
            repeat,
            ramp,
        } => match ramp {
            GradRamp::Table(id) => {
                let Some(table) = ramps.try_get(*id) else {
                    return Rgba8::TRANSPARENT;
                };
                let Some(s) = grad_param(*shape, *mapping, p) else {
                    return Rgba8::TRANSPARENT;
                };
                let s = apply_repeat(s, *repeat);
                let idx = (s * (table.len() - 1) as f64).round();
                let idx = (idx.clamp(0.0, (table.len() - 1) as f64)) as usize;
                table[idx]
            }
            GradRamp::Mesh3(c) => {
                let Some((u, v)) = mapping.to_frame(p) else {
                    return Rgba8::TRANSPARENT;
                };
                let (u, v) = (u.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
                let w0 = (1.0 - u - v).max(0.0);
                let sum = w0 + u + v;
                if sum <= 0.0 {
                    return c[0];
                }
                let ch = |f: fn(&Rgba8) -> u8| -> u8 {
                    ((w0 * f64::from(f(&c[0])) + u * f64::from(f(&c[1])) + v * f64::from(f(&c[2])))
                        / sum)
                        .round()
                        .clamp(0.0, 255.0) as u8
                };
                Rgba8 {
                    r: ch(|c| c.r),
                    g: ch(|c| c.g),
                    b: ch(|c| c.b),
                    a: ch(|c| c.a),
                }
            }
            GradRamp::Mesh4(c) => {
                let Some((u, v)) = mapping.to_frame(p) else {
                    return Rgba8::TRANSPARENT;
                };
                let (u, v) = (u.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
                let top = lerp_rgba(c[0], c[1], u);
                let bottom = lerp_rgba(c[2], c[3], u);
                lerp_rgba(top, bottom, v)
            }
        },
        Paint::Image {
            image,
            mapping,
            repeat,
            filter,
            contone,
            adjust,
        } => {
            let Some(img) = images.get(*image) else {
                return Rgba8::TRANSPARENT;
            };
            let Some((u, v)) = mapping.to_frame(p) else {
                return Rgba8::TRANSPARENT;
            };
            let (fw, fh) = (f64::from(img.width()), f64::from(img.height()));
            let (x, y) = (u * fw - 0.5, v * fh - 0.5);
            let sample = match filter {
                Filter::Nearest => img.texel(x.round() as i64, y.round() as i64, *repeat),
                Filter::Bilinear | Filter::HighQuality => {
                    let (x0, y0) = (x.floor(), y.floor());
                    let (fx, fy) = (x - x0, y - y0);
                    let (x0, y0) = (x0 as i64, y0 as i64);
                    let t00 = img.texel(x0, y0, *repeat);
                    let t10 = img.texel(x0 + 1, y0, *repeat);
                    let t01 = img.texel(x0, y0 + 1, *repeat);
                    let t11 = img.texel(x0 + 1, y0 + 1, *repeat);
                    lerp_rgba(lerp_rgba(t00, t10, fx), lerp_rgba(t01, t11, fx), fy)
                }
            };
            let sample = apply_contone(sample, *contone);
            apply_adjust(sample, *adjust)
        }
        Paint::Fractal(_) => Rgba8::TRANSPARENT,
    }
}

fn apply_contone(c: Rgba8, contone: Option<(Rgba8, Rgba8, crate::ramp::EffectSpace)>) -> Rgba8 {
    let Some((start, end, space)) = contone else {
        return c;
    };
    // The original remaps the bitmap's luminance onto a two-colour ramp.
    let y = crate::blend::LumaWeights::BT601.luma(c);
    let t = f32::from(y) / 255.0;
    let mixed = xarast_color::interpolate(
        xarast_color::ColourValue::from_rgba8(start),
        xarast_color::ColourValue::from_rgba8(end),
        t,
        match space {
            crate::ramp::EffectSpace::Rgb => xarast_color::FillEffect::Fade,
            crate::ramp::EffectSpace::HsvShort => xarast_color::FillEffect::Rainbow,
            crate::ramp::EffectSpace::HsvLong => xarast_color::FillEffect::AltRainbow,
        },
    );
    let out = mixed.to_rgba8();
    Rgba8 { a: c.a, ..out }
}

fn apply_adjust(c: Rgba8, adj: BitmapAdjust) -> Rgba8 {
    if adj.is_identity() {
        return c;
    }
    let f = |v: u8| -> f32 {
        let mut x = f32::from(v) / 255.0;
        if adj.gamma > 0.0 && (adj.gamma - 1.0).abs() > f32::EPSILON {
            x = x.powf(1.0 / adj.gamma);
        }
        x = (x + adj.brightness).clamp(0.0, 1.0);
        x = ((x - 0.5) * (1.0 + adj.contrast) + 0.5).clamp(0.0, 1.0);
        x
    };
    let (r, g, b) = (f(c.r), f(c.g), f(c.b));
    let (r, g, b) = if adj.saturation == 0.0 {
        (r, g, b)
    } else {
        let y = 0.299 * r + 0.587 * g + 0.114 * b;
        let k = 1.0 + adj.saturation;
        (
            (y + (r - y) * k).clamp(0.0, 1.0),
            (y + (g - y) * k).clamp(0.0, 1.0),
            (y + (b - y) * k).clamp(0.0, 1.0),
        )
    };
    Rgba8 {
        r: (r * 255.0).round() as u8,
        g: (g * 255.0).round() as u8,
        b: (b * 255.0).round() as u8,
        a: c.a,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ramp::{EffectSpace, RampLength, Stop};

    fn rgb(r: u8, g: u8, b: u8) -> Rgba8 {
        Rgba8 { r, g, b, a: 255 }
    }

    fn unit_square_perspective() -> GradMapping {
        GradMapping::Perspective {
            a: Point64::new(0.0, 0.0),
            b: Point64::new(0.0, 1.0),
            c: Point64::new(1.0, 0.0),
            d: Point64::new(1.0, 1.0),
        }
    }

    #[test]
    fn linear_is_the_coordinate_along_the_main_axis() {
        let m = GradMapping::unit();
        assert_eq!(
            grad_param(GradShape::Linear, m, Point64::new(0.0, 0.0)),
            Some(0.0)
        );
        assert_eq!(
            grad_param(GradShape::Linear, m, Point64::new(1.0, 0.0)),
            Some(1.0)
        );
        assert_eq!(
            grad_param(GradShape::Linear, m, Point64::new(0.5, 9.0)),
            Some(0.5)
        );
    }

    #[test]
    fn radial_is_the_euclidean_radius_in_the_frame() {
        let m = GradMapping::Affine {
            a: Point64::new(0.0, 0.0),
            b: Point64::new(0.0, 2.0),
            c: Point64::new(4.0, 0.0),
        };
        // Two independent radii: 4 across, 2 down.
        let s = grad_param(GradShape::Radial, m, Point64::new(4.0, 0.0)).unwrap();
        assert!((s - 1.0).abs() < 1e-12);
        let s = grad_param(GradShape::Radial, m, Point64::new(0.0, 2.0)).unwrap();
        assert!((s - 1.0).abs() < 1e-12);
    }

    #[test]
    fn conical_sweeps_one_turn() {
        let m = GradMapping::unit();
        let a = grad_param(GradShape::Conical, m, Point64::new(1.0, 0.0)).unwrap();
        let b = grad_param(GradShape::Conical, m, Point64::new(0.0, 1.0)).unwrap();
        let c = grad_param(GradShape::Conical, m, Point64::new(-1.0, 0.0)).unwrap();
        assert!((a - 0.0).abs() < 1e-12);
        assert!((b - 0.25).abs() < 1e-12);
        assert!((c - 0.5).abs() < 1e-12);
        // Never outside [0, 1).
        for k in 0..64 {
            let th = k as f64 / 64.0 * std::f64::consts::TAU;
            let s = grad_param(GradShape::Conical, m, Point64::new(th.cos(), th.sin())).unwrap();
            assert!((0.0..1.0).contains(&s), "{s}");
        }
    }

    #[test]
    fn diamond_is_the_l_infinity_metric() {
        let m = GradMapping::unit();
        let s = grad_param(GradShape::Diamond, m, Point64::new(0.3, -0.7)).unwrap();
        assert!((s - 0.7).abs() < 1e-12);
    }

    #[test]
    fn a_mesh_shape_has_no_scalar_parameter() {
        assert_eq!(
            grad_param(GradShape::Mesh3, GradMapping::unit(), Point64::ORIGIN),
            None
        );
    }

    #[test]
    fn a_degenerate_mapping_is_rejected_rather_than_dividing_by_zero() {
        let m = GradMapping::Affine {
            a: Point64::new(0.0, 0.0),
            b: Point64::new(1.0, 1.0),
            c: Point64::new(2.0, 2.0),
        };
        assert!(!m.is_valid());
        assert_eq!(
            grad_param(GradShape::Linear, m, Point64::new(1.0, 0.0)),
            None
        );
    }

    #[test]
    fn an_unskewed_perspective_agrees_with_the_affine_mapping() {
        let affine = GradMapping::unit();
        let persp = unit_square_perspective();
        for (x, y) in [(0.1, 0.2), (0.9, 0.4), (0.5, 0.5)] {
            let a = grad_param(GradShape::Linear, affine, Point64::new(x, y)).unwrap();
            let b = grad_param(GradShape::Linear, persp, Point64::new(x, y)).unwrap();
            assert!((a - b).abs() < 1e-12, "{a} vs {b}");
        }
    }

    #[test]
    fn a_real_perspective_quad_is_projective_not_linear() {
        let m = GradMapping::Perspective {
            a: Point64::new(0.0, 0.0),
            b: Point64::new(0.0, 1.0),
            c: Point64::new(1.0, 0.2),
            d: Point64::new(1.0, 0.8),
        };
        assert!(m.is_valid());
        let mid = grad_param(GradShape::Linear, m, Point64::new(0.5, 0.5)).unwrap();
        assert!(mid > 0.0 && mid < 1.0, "{mid}");
    }

    #[test]
    fn repeat_modes_fold_the_parameter() {
        assert_eq!(apply_repeat(1.25, Repeat::Simple), 1.0);
        assert_eq!(apply_repeat(-0.25, Repeat::Simple), 0.0);
        assert!((apply_repeat(1.25, Repeat::Repeat) - 0.25).abs() < 1e-12);
        assert!((apply_repeat(1.25, Repeat::Mirror) - 0.75).abs() < 1e-12);
        assert!((apply_repeat(2.25, Repeat::Mirror) - 0.25).abs() < 1e-12);
        assert!((apply_repeat(-0.25, Repeat::Repeat) - 0.75).abs() < 1e-12);
        assert_eq!(apply_repeat(f64::NAN, Repeat::Repeat), 0.0);
    }

    #[test]
    fn a_gradient_paint_evaluates_through_its_ramp() {
        let mut ramps = RampCache::new();
        let id = ramps.intern(
            &[
                Stop::new(0.0, rgb(0, 0, 0)),
                Stop::new(1.0, rgb(255, 255, 255)),
            ],
            Profile::IDENTITY,
            EffectSpace::Rgb,
            RampLength::Short,
        );
        let paint = Paint::Gradient {
            shape: GradShape::Linear,
            mapping: GradMapping::unit(),
            repeat: Repeat::Simple,
            ramp: GradRamp::Table(id),
        };
        paint.validate().unwrap();
        let images = ImageRegistry::new();
        assert_eq!(
            eval_paint(&paint, &ramps, &images, Point64::new(0.0, 0.0)),
            rgb(0, 0, 0)
        );
        assert_eq!(
            eval_paint(&paint, &ramps, &images, Point64::new(1.0, 0.0)),
            rgb(255, 255, 255)
        );
    }

    #[test]
    fn a_mesh_paint_hits_its_corner_colours() {
        let ramps = RampCache::new();
        let images = ImageRegistry::new();
        let paint = Paint::Gradient {
            shape: GradShape::Mesh4,
            mapping: GradMapping::unit(),
            repeat: Repeat::Simple,
            ramp: GradRamp::Mesh4([
                rgb(255, 0, 0),
                rgb(0, 255, 0),
                rgb(0, 0, 255),
                rgb(255, 255, 0),
            ]),
        };
        paint.validate().unwrap();
        assert_eq!(
            eval_paint(&paint, &ramps, &images, Point64::new(0.0, 0.0)),
            rgb(255, 0, 0)
        );
        assert_eq!(
            eval_paint(&paint, &ramps, &images, Point64::new(1.0, 0.0)),
            rgb(0, 255, 0)
        );
        assert_eq!(
            eval_paint(&paint, &ramps, &images, Point64::new(0.0, 1.0)),
            rgb(0, 0, 255)
        );
    }

    #[test]
    fn a_shape_ramp_mismatch_is_an_error_not_a_panic() {
        let paint = Paint::Gradient {
            shape: GradShape::Mesh3,
            mapping: GradMapping::unit(),
            repeat: Repeat::Simple,
            ramp: GradRamp::Mesh4([Rgba8::BLACK; 4]),
        };
        assert!(matches!(
            paint.validate(),
            Err(PaintError::ShapeRampMismatch { .. })
        ));
    }

    #[test]
    fn image_texels_wrap_according_to_the_repeat_mode() {
        let img = ImageRef::new(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]);
        assert_eq!(img.texel(-1, 0, Repeat::Simple), rgb(255, 0, 0));
        assert_eq!(img.texel(2, 0, Repeat::Repeat), rgb(255, 0, 0));
        assert_eq!(img.texel(2, 0, Repeat::Mirror), rgb(0, 0, 255));
    }

    #[test]
    fn a_fractal_must_be_materialised_first() {
        let p = Paint::Fractal(FractalParams {
            seed: 1,
            tileable: true,
            squash: 0,
            graininess: 8,
            gravity: 0,
        });
        assert_eq!(p.validate(), Err(PaintError::UnmaterialisedFractal));
    }
}
