//! The `PdfWriter` façade (T11.4.2): the only module that names
//! `pdf-writer`.
//!
//! Everything above it speaks in our own terms — a [`Canvas`] of path and
//! paint operators, [`Resource`] handles, [`GState`] values — so replacing
//! the crate means rewriting this file and nothing else. The T11.4.1 spike
//! that chose `pdf-writer` over `krilla` is recorded in
//! `docs/memory/export.md`.
//!
//! # Determinism
//!
//! Object numbers are allocated in call order, resource dictionaries are
//! written from ordered sets, graphics states are interned by value, no
//! date or random file identifier is written, and every stream is
//! compressed with one fixed zlib level. The same calls produce the same
//! bytes.
//!
//! # Numbers
//!
//! PDF numbers are written as `f32`. Callers pass `f64` in PDF points
//! relative to the page (or relative to a local origin, for strokes), so
//! the conversion loses nothing visible: a 14 400 pt page keeps
//! 0.001 pt. Non-finite values are written as zero rather than as the
//! `NaN` token, which is not PDF.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;

use kurbo::{BezPath, PathEl};
use pdf_writer::types::{
    BlendMode, CidFontType, FontFlags, FunctionShadingType, LineCapStyle, LineJoinStyle, MaskType,
    SystemInfo, TextRenderingMode, UnicodeCmap,
};
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, Str, TextStr};

/// Sanitises a coordinate: finite `f64` to `f32`, anything else to zero.
fn num(v: f64) -> f32 {
    if v.is_finite() {
        #[allow(clippy::cast_possible_truncation)]
        let f = v.clamp(-1.0e30, 1.0e30) as f32;
        f
    } else {
        0.0
    }
}

/// What kind of named resource a handle is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResourceKind {
    /// An `ExtGState`.
    GState,
    /// A shading, painted with `sh`.
    Shading,
    /// An image or form XObject, painted with `Do`.
    XObject,
    /// A font, selected with `Tf`.
    Font,
}

/// A named resource: its kind and its object number, which also makes
/// its name (`G7`, `S12`, `X3`) unique across the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Resource {
    kind: ResourceKind,
    id: i32,
}

impl Resource {
    fn name(self) -> String {
        let p = match self.kind {
            ResourceKind::GState => 'G',
            ResourceKind::Shading => 'S',
            ResourceKind::XObject => 'X',
            ResourceKind::Font => 'F',
        };
        format!("{p}{}", self.id)
    }

    /// The kind.
    #[must_use]
    pub const fn kind(self) -> ResourceKind {
        self.kind
    }
}

/// The blend modes the exporter writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Blend {
    /// Source over.
    #[default]
    Normal,
    /// `Multiply`.
    Multiply,
    /// `Screen`.
    Screen,
}

/// The unit of [`GState`] opacities: 255², so that a colour's alpha
/// times a transparency level is exact.
pub const OPACITY_ONE: u32 = 255 * 255;

/// A luminosity soft-mask group: a form whose grey levels become the
/// opacity of what is drawn under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SoftMask(i32);

/// A graphics-state parameter dictionary, interned by value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GState {
    /// Non-stroking opacity (`ca`), in 1 / [`OPACITY_ONE`].
    pub fill_alpha: u32,
    /// Stroking opacity (`CA`), in 1 / [`OPACITY_ONE`].
    pub stroke_alpha: u32,
    /// Blend mode (`BM`).
    pub blend: Blend,
    /// A luminosity soft mask (`SMask`), from [`PdfWriter::soft_mask`].
    pub mask: Option<SoftMask>,
}

impl GState {
    /// Opaque, normal blending: the state every page starts in.
    pub const DEFAULT: GState = GState {
        fill_alpha: OPACITY_ONE,
        stroke_alpha: OPACITY_ONE,
        blend: Blend::Normal,
        mask: None,
    };

    /// Whether it changes nothing.
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == GState::DEFAULT
    }
}

/// Which regions a fill or clip covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// Non-zero winding.
    NonZero,
    /// Even-odd.
    EvenOdd,
}

/// Cap style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cap {
    /// Butt.
    Butt,
    /// Round.
    Round,
    /// Projecting square.
    Square,
}

/// Join style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Join {
    /// Mitre.
    Mitre,
    /// Round.
    Round,
    /// Bevel.
    Bevel,
}

/// Everything `w`, `J`, `j`, `M` and `d` set, in the canvas's current
/// user space.
#[derive(Debug, Clone, PartialEq)]
pub struct LineStyle {
    /// Width; zero is the thinnest line the device draws.
    pub width: f64,
    /// Cap.
    pub cap: Cap,
    /// Join.
    pub join: Join,
    /// Mitre limit, at least 1.
    pub mitre_limit: f64,
    /// Dash lengths (empty for solid) and phase.
    pub dash: (Vec<f64>, f64),
}

/// How text between [`Canvas::begin_text`] and [`Canvas::end_text`] is
/// rendered (`Tr`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextMode {
    /// Filled with the current fill colour (mode 0).
    Fill,
    /// Not painted at all, but selectable and searchable (mode 3).
    Invisible,
    /// Added to the clipping path at `ET` (mode 7).
    Clip,
}

/// A subset font to embed: the program and what its dictionaries say.
#[derive(Debug, Clone, Copy)]
pub struct FontSpec<'a> {
    /// The font program and metrics (`xarast_text::embed`).
    pub font: &'a xarast_text::PdfFont,
    /// The six-letter subset tag of the `BaseFont` name (`ABCDEF+Name`).
    pub tag: &'a str,
    /// The text each glyph (by CID, which is the subset's glyph id) stands
    /// for, for the `ToUnicode` map.
    pub to_unicode: &'a BTreeMap<u16, String>,
}

/// A content stream under construction, with the resources it uses.
pub struct Canvas {
    /// Content already flushed by [`Canvas::append`].
    flushed: Vec<u8>,
    content: Content,
    used: BTreeSet<Resource>,
    depth: usize,
    /// The font set in the open text object, if any.
    text_font: Option<Resource>,
}

impl std::fmt::Debug for Canvas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Canvas")
            .field("bytes", &self.len())
            .field("resources", &self.used.len())
            .field("depth", &self.depth)
            .finish()
    }
}

impl Default for Canvas {
    fn default() -> Canvas {
        Canvas::new()
    }
}

impl Canvas {
    /// An empty canvas.
    #[must_use]
    pub fn new() -> Canvas {
        Canvas {
            flushed: Vec::new(),
            content: Content::new(),
            used: BTreeSet::new(),
            depth: 0,
            text_font: None,
        }
    }

    /// Bytes of content so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.flushed.len() + self.content.len()
    }

    /// Whether nothing has been drawn.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Appends another canvas's operators and resources. `other` closes
    /// its own `q`s first, so this canvas's nesting is unchanged.
    pub fn append(&mut self, other: Canvas) {
        let (bytes, used) = other.finish();
        if bytes.is_empty() {
            return;
        }
        let mine = std::mem::replace(&mut self.content, Content::new()).finish();
        self.flushed.extend_from_slice(&mine);
        if !self.flushed.is_empty() {
            self.flushed.push(b'\n');
        }
        self.flushed.extend_from_slice(&bytes);
        self.flushed.push(b'\n');
        self.used.extend(used);
    }

    /// The `q` nesting depth.
    #[must_use]
    pub const fn depth(&self) -> usize {
        self.depth
    }

    /// `q`.
    pub fn save(&mut self) {
        self.content.save_state();
        self.depth += 1;
    }

    /// `Q`; ignored when nothing is saved, so a corrupt stream stays
    /// balanced.
    pub fn restore(&mut self) {
        if self.depth > 0 {
            self.content.restore_state();
            self.depth -= 1;
        }
    }

    /// `cm`.
    pub fn transform(&mut self, m: [f64; 6]) {
        self.content.transform(m.map(num));
    }

    /// Appends a path: `m`, `l`, `c` and `h`. Quadratic segments are
    /// raised to cubics exactly.
    pub fn path(&mut self, p: &BezPath) {
        let mut last = kurbo::Point::ZERO;
        for el in p.elements() {
            match *el {
                PathEl::MoveTo(a) => {
                    self.content.move_to(num(a.x), num(a.y));
                    last = a;
                }
                PathEl::LineTo(a) => {
                    self.content.line_to(num(a.x), num(a.y));
                    last = a;
                }
                PathEl::QuadTo(q, a) => {
                    let c1 = last + (q - last) * (2.0 / 3.0);
                    let c2 = a + (q - a) * (2.0 / 3.0);
                    self.content.cubic_to(
                        num(c1.x),
                        num(c1.y),
                        num(c2.x),
                        num(c2.y),
                        num(a.x),
                        num(a.y),
                    );
                    last = a;
                }
                PathEl::CurveTo(c1, c2, a) => {
                    self.content.cubic_to(
                        num(c1.x),
                        num(c1.y),
                        num(c2.x),
                        num(c2.y),
                        num(a.x),
                        num(a.y),
                    );
                    last = a;
                }
                PathEl::ClosePath => {
                    self.content.close_path();
                }
            }
        }
    }

    /// `re`.
    pub fn rect(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        self.content
            .rect(num(x0), num(y0), num(x1 - x0), num(y1 - y0));
    }

    /// `f` or `f*`.
    pub fn fill(&mut self, rule: Rule) {
        match rule {
            Rule::NonZero => self.content.fill_nonzero(),
            Rule::EvenOdd => self.content.fill_even_odd(),
        };
    }

    /// `S`.
    pub fn stroke(&mut self) {
        self.content.stroke();
    }

    /// `W n` or `W* n`.
    pub fn clip(&mut self, rule: Rule) {
        match rule {
            Rule::NonZero => self.content.clip_nonzero(),
            Rule::EvenOdd => self.content.clip_even_odd(),
        };
        self.content.end_path();
    }

    /// `rg`.
    pub fn fill_rgb(&mut self, c: [u8; 3]) {
        let f = |v: u8| f32::from(v) / 255.0;
        self.content.set_fill_rgb(f(c[0]), f(c[1]), f(c[2]));
    }

    /// `RG`.
    pub fn stroke_rgb(&mut self, c: [u8; 3]) {
        let f = |v: u8| f32::from(v) / 255.0;
        self.content.set_stroke_rgb(f(c[0]), f(c[1]), f(c[2]));
    }

    /// `w`, `J`, `j`, `M` and `d`.
    pub fn line_style(&mut self, s: &LineStyle) {
        self.content.set_line_width(num(s.width.max(0.0)));
        self.content.set_line_cap(match s.cap {
            Cap::Butt => LineCapStyle::ButtCap,
            Cap::Round => LineCapStyle::RoundCap,
            Cap::Square => LineCapStyle::ProjectingSquareCap,
        });
        self.content.set_line_join(match s.join {
            Join::Mitre => LineJoinStyle::MiterJoin,
            Join::Round => LineJoinStyle::RoundJoin,
            Join::Bevel => LineJoinStyle::BevelJoin,
        });
        self.content.set_miter_limit(num(s.mitre_limit.max(1.0)));
        if !s.dash.0.is_empty() {
            self.content
                .set_dash_pattern(s.dash.0.iter().map(|&d| num(d.max(0.0))), num(s.dash.1));
        }
    }

    /// `gs`.
    pub fn gstate(&mut self, r: Resource) {
        self.used.insert(r);
        self.content.set_parameters(Name(r.name().as_bytes()));
    }

    /// `sh`: paints the shading over the current clip.
    pub fn shading(&mut self, r: Resource) {
        self.used.insert(r);
        self.content.shading(Name(r.name().as_bytes()));
    }

    /// `Do`.
    pub fn xobject(&mut self, r: Resource) {
        self.used.insert(r);
        self.content.x_object(Name(r.name().as_bytes()));
    }

    /// `BT` and the rendering mode. Every glyph until [`Canvas::end_text`]
    /// is placed by its own text matrix.
    pub fn begin_text(&mut self, mode: TextMode) {
        self.content.begin_text();
        self.text_font = None;
        self.content.set_text_rendering_mode(match mode {
            TextMode::Fill => TextRenderingMode::Fill,
            TextMode::Invisible => TextRenderingMode::Invisible,
            TextMode::Clip => TextRenderingMode::Clip,
        });
    }

    /// One glyph of a font from [`PdfWriter::font`]: `m` maps the glyph's
    /// em square (1 unit = 1 em) to the current user space.
    pub fn glyph(&mut self, font: Resource, m: [f64; 6], cid: u16) {
        if self.text_font != Some(font) {
            self.used.insert(font);
            self.content.set_font(Name(font.name().as_bytes()), 1.0);
            self.text_font = Some(font);
        }
        self.content.set_text_matrix(m.map(num));
        self.content.show(Str(&cid.to_be_bytes()));
    }

    /// `ET`.
    pub fn end_text(&mut self) {
        self.content.end_text();
        self.text_font = None;
    }

    /// Closes every open `q`, then hands the stream and its resources over.
    fn finish(mut self) -> (Vec<u8>, BTreeSet<Resource>) {
        while self.depth > 0 {
            self.restore();
        }
        let mut out = self.flushed;
        out.extend_from_slice(&self.content.finish());
        while out.last() == Some(&b'\n') {
            out.pop();
        }
        (out, self.used)
    }
}

/// One vertex of a Gouraud-shaded triangle: a point and its colour.
pub type Vertex = (kurbo::Point, [u8; 3]);

/// Document information.
#[derive(Debug, Clone, Default)]
pub struct DocInfo {
    /// `/Title`, when there is one.
    pub title: Option<String>,
    /// `/Producer`.
    pub producer: String,
}

/// A page's boxes, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageBoxes {
    /// The whole sheet, bleed included.
    pub media: [f64; 4],
    /// The finished page, when a bleed makes it smaller than the sheet.
    pub trim: Option<[f64; 4]>,
}

fn rect(r: [f64; 4]) -> Rect {
    Rect::new(num(r[0]), num(r[1]), num(r[2]), num(r[3]))
}

/// The document under construction.
pub struct PdfWriter {
    pdf: Pdf,
    next: i32,
    compress: bool,
    catalog: Ref,
    pages: Ref,
    page_refs: Vec<Ref>,
    gstates: BTreeMap<GState, Resource>,
    /// Sampled functions by (domain, size, samples): a ramp shared by many
    /// objects is written once.
    functions: BTreeMap<FunctionKey, Ref>,
    /// Function-based shadings by (type, function, coords, domain, extend).
    shadings: BTreeMap<ShadingKey, Resource>,
}

impl std::fmt::Debug for PdfWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PdfWriter")
            .field("objects", &(self.next - 1))
            .field("pages", &self.page_refs.len())
            .finish_non_exhaustive()
    }
}

/// What identifies a sampled function: the `f32` bits of its domain, its
/// size and its samples.
type FunctionKey = (Vec<u32>, Vec<u32>, Vec<u8>);

/// What identifies a function-based shading: its type, function, and the
/// `f32` bits of its coordinates and domain as written, and its extend.
type ShadingKey = (i32, i32, Vec<u32>, Vec<u32>, Option<[bool; 2]>);

/// The `f32` bits a list of numbers is written as.
fn bits(v: &[f64]) -> Vec<u32> {
    v.iter().map(|&x| num(x).to_bits()).collect()
}

/// The zlib level every stream is compressed with. Fixed, so the bytes
/// are too.
const ZLIB_LEVEL: u32 = 6;

impl PdfWriter {
    /// An empty PDF 1.7 document. `compress` FlateDecodes every stream.
    #[must_use]
    pub fn new(compress: bool) -> PdfWriter {
        let mut pdf = Pdf::new();
        pdf.set_version(1, 7);
        PdfWriter {
            pdf,
            next: 3,
            compress,
            catalog: Ref::new(1),
            pages: Ref::new(2),
            page_refs: Vec::new(),
            gstates: BTreeMap::new(),
            functions: BTreeMap::new(),
            shadings: BTreeMap::new(),
        }
    }

    fn alloc(&mut self) -> Ref {
        let r = Ref::new(self.next);
        self.next += 1;
        r
    }

    /// The bytes of a stream, compressed when the writer compresses.
    fn pack(&self, data: &[u8]) -> (Vec<u8>, bool) {
        if !self.compress || data.is_empty() {
            return (data.to_vec(), false);
        }
        let mut z = flate2::write::ZlibEncoder::new(
            Vec::with_capacity(data.len() / 2 + 64),
            flate2::Compression::new(ZLIB_LEVEL),
        );
        // Writing into a `Vec` cannot fail.
        if z.write_all(data).is_err() {
            return (data.to_vec(), false);
        }
        match z.finish() {
            Ok(v) => (v, true),
            Err(_) => (data.to_vec(), false),
        }
    }

    /// An `ExtGState`, written once per distinct value.
    pub fn gstate(&mut self, g: GState) -> Resource {
        if let Some(r) = self.gstates.get(&g) {
            return *r;
        }
        let id = self.alloc();
        let mut e = self.pdf.ext_graphics(id);
        #[allow(clippy::cast_precision_loss)]
        let a = |v: u32| (f64::from(v.min(OPACITY_ONE)) / f64::from(OPACITY_ONE)) as f32;
        e.non_stroking_alpha(a(g.fill_alpha));
        e.stroking_alpha(a(g.stroke_alpha));
        e.blend_mode(match g.blend {
            Blend::Normal => BlendMode::Normal,
            Blend::Multiply => BlendMode::Multiply,
            Blend::Screen => BlendMode::Screen,
        });
        if let Some(SoftMask(m)) = g.mask {
            e.soft_mask()
                .subtype(MaskType::Luminosity)
                .group(Ref::new(m));
        }
        e.finish();
        let r = Resource {
            kind: ResourceKind::GState,
            id: id.get(),
        };
        self.gstates.insert(g, r);
        r
    }

    /// A sampled function (type 0) from `domain` (one `[lo, hi]` pair per
    /// input) to RGB, 8 bits per sample, multilinear interpolation.
    /// `size` has one entry per input; `samples` holds
    /// `3 × Π size` bytes with the first input varying fastest.
    pub fn sampled_rgb_function(&mut self, domain: &[f64], size: &[u32], samples: &[u8]) -> Ref {
        let key = (bits(domain), size.to_vec(), samples.to_vec());
        if let Some(r) = self.functions.get(&key) {
            return *r;
        }
        let id = self.alloc();
        self.functions.insert(key, id);
        let (data, packed) = self.pack(samples);
        let mut f = self.pdf.sampled_function(id, &data);
        f.domain(domain.iter().map(|&v| num(v)));
        f.range([0.0, 1.0, 0.0, 1.0, 0.0, 1.0]);
        f.size(size.iter().map(|&s| i32::try_from(s).unwrap_or(i32::MAX)));
        f.bits_per_sample(8);
        if packed {
            f.filter(Filter::FlateDecode);
        }
        f.finish();
        id
    }

    fn function_shading(
        &mut self,
        kind: FunctionShadingType,
        function: Ref,
        coords: &[f64],
        domain: &[f64],
        extend: Option<[bool; 2]>,
    ) -> Resource {
        let tag = match kind {
            FunctionShadingType::Function => 1,
            FunctionShadingType::Axial => 2,
            FunctionShadingType::Radial => 3,
        };
        let key = (tag, function.get(), bits(coords), bits(domain), extend);
        if let Some(r) = self.shadings.get(&key) {
            return *r;
        }
        let id = self.alloc();
        self.shadings.insert(
            key,
            Resource {
                kind: ResourceKind::Shading,
                id: id.get(),
            },
        );
        let mut s = self.pdf.function_shading(id);
        s.shading_type(kind);
        s.color_space().device_rgb();
        s.function(function);
        match kind {
            FunctionShadingType::Function => {
                let d = [domain[0], domain[1], domain[2], domain[3]];
                s.domain(d.map(num));
            }
            _ => {
                s.coords(coords.iter().map(|&v| num(v)));
                s.insert(Name(b"Domain"))
                    .array()
                    .items(domain.iter().map(|&v| num(v)));
            }
        }
        if let Some(e) = extend {
            s.extend(e);
        }
        s.anti_alias(true);
        s.finish();
        Resource {
            kind: ResourceKind::Shading,
            id: id.get(),
        }
    }

    /// An axial shading (type 2) from `(x0, y0)` to `(x1, y1)` whose
    /// parameter runs over `domain`.
    pub fn axial(
        &mut self,
        function: Ref,
        coords: [f64; 4],
        domain: [f64; 2],
        extend: [bool; 2],
    ) -> Resource {
        self.function_shading(
            FunctionShadingType::Axial,
            function,
            &coords,
            &domain,
            Some(extend),
        )
    }

    /// A radial shading (type 3) between two circles `(x, y, r)`.
    pub fn radial(
        &mut self,
        function: Ref,
        coords: [f64; 6],
        domain: [f64; 2],
        extend: [bool; 2],
    ) -> Resource {
        self.function_shading(
            FunctionShadingType::Radial,
            function,
            &coords,
            &domain,
            Some(extend),
        )
    }

    /// A function-based shading (type 1) over `[x0, x1, y0, y1]`, where
    /// `function` takes the point itself.
    pub fn function_based(&mut self, function: Ref, domain: [f64; 4]) -> Resource {
        self.function_shading(FunctionShadingType::Function, function, &[], &domain, None)
    }

    /// A free-form Gouraud-shaded triangle mesh (type 4).
    pub fn gouraud(&mut self, triangles: &[[Vertex; 3]]) -> Resource {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for v in triangles.iter().flatten() {
            x0 = x0.min(v.0.x);
            y0 = y0.min(v.0.y);
            x1 = x1.max(v.0.x);
            y1 = y1.max(v.0.y);
        }
        if !(x0.is_finite() && y0.is_finite() && x1 > x0 && y1 > y0) {
            (x0, y0, x1, y1) = (0.0, 0.0, 1.0, 1.0);
        }
        let q = |v: f64, lo: f64, hi: f64| -> u32 {
            let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let u = (t * f64::from(u32::MAX)).round() as u32;
            u
        };
        let mut data = Vec::with_capacity(triangles.len() * 3 * 12);
        for t in triangles {
            for v in t {
                data.push(0u8);
                data.extend_from_slice(&q(v.0.x, x0, x1).to_be_bytes());
                data.extend_from_slice(&q(v.0.y, y0, y1).to_be_bytes());
                data.extend_from_slice(&v.1);
            }
        }
        let id = self.alloc();
        let (bytes, packed) = self.pack(&data);
        let mut s = self.pdf.stream_shading(id, &bytes);
        // `StreamShadingType` is not re-exported by pdf-writer 0.15: the
        // key is written by hand. 4 is the free-form Gouraud mesh.
        s.pair(Name(b"ShadingType"), 4);
        s.color_space().device_rgb();
        s.bits_per_coordinate(32);
        s.bits_per_component(8);
        s.bits_per_flag(8);
        s.decode([
            num(x0),
            num(x1),
            num(y0),
            num(y1),
            0.0,
            1.0,
            0.0,
            1.0,
            0.0,
            1.0,
        ]);
        s.anti_alias(true);
        if packed {
            s.filter(Filter::FlateDecode);
        }
        s.finish();
        Resource {
            kind: ResourceKind::Shading,
            id: id.get(),
        }
    }

    /// An RGB image XObject, 8 bits per sample, rows top to bottom, with
    /// an optional grey soft mask of the same size.
    pub fn image(&mut self, width: u32, height: u32, rgb: &[u8], alpha: Option<&[u8]>) -> Resource {
        let w = i32::try_from(width).unwrap_or(i32::MAX);
        let h = i32::try_from(height).unwrap_or(i32::MAX);
        let mask = alpha.map(|a| {
            let id = self.alloc();
            let (bytes, packed) = self.pack(a);
            let mut m = self.pdf.image_xobject(id, &bytes);
            m.width(w);
            m.height(h);
            m.color_space().device_gray();
            m.bits_per_component(8);
            if packed {
                m.filter(Filter::FlateDecode);
            }
            m.finish();
            id
        });
        let id = self.alloc();
        let (bytes, packed) = self.pack(rgb);
        let mut img = self.pdf.image_xobject(id, &bytes);
        img.width(w);
        img.height(h);
        img.color_space().device_rgb();
        img.bits_per_component(8);
        img.interpolate(false);
        if let Some(m) = mask {
            img.s_mask(m);
        }
        if packed {
            img.filter(Filter::FlateDecode);
        }
        img.finish();
        Resource {
            kind: ResourceKind::XObject,
            id: id.get(),
        }
    }

    /// A form XObject drawing `canvas` in page space, clipped to `bbox`,
    /// as a transparency group (isolated when asked).
    pub fn group(&mut self, canvas: Canvas, bbox: [f64; 4], isolated: bool) -> Resource {
        let (content, used) = canvas.finish();
        let id = self.alloc();
        let (bytes, packed) = self.pack(&content);
        let mut form = self.pdf.form_xobject(id, &bytes);
        form.bbox(rect(bbox));
        form.group()
            .transparency()
            .isolated(isolated)
            .knockout(false)
            .color_space()
            .device_rgb();
        write_resources(&mut form.resources(), &used);
        if packed {
            form.filter(Filter::FlateDecode);
        }
        form.finish();
        Resource {
            kind: ResourceKind::XObject,
            id: id.get(),
        }
    }

    /// A luminosity soft mask drawing `canvas` in the coordinate space
    /// current where the graphics state using it is set (the page, for
    /// every caller here), over `bbox`. Outside the box the mask is black:
    /// fully transparent.
    pub fn soft_mask(&mut self, canvas: Canvas, bbox: [f64; 4]) -> SoftMask {
        SoftMask(self.group(canvas, bbox, true).id)
    }

    /// Embeds a subset font as a `Type0` font over a `CIDFont` (identity
    /// encoding, CID = subset glyph id), with its program, descriptor,
    /// widths and `ToUnicode` map.
    pub fn font(&mut self, spec: &FontSpec<'_>) -> Resource {
        let f = spec.font;
        let type0 = self.alloc();
        let cid_font = self.alloc();
        let descriptor = self.alloc();
        let file = self.alloc();
        let cmap_id = self.alloc();
        let base = format!("{}+{}", spec.tag, f.postscript_name);
        let cff = f.format == xarast_text::ProgramFormat::Cff;
        let system = SystemInfo {
            registry: Str(b"Adobe"),
            ordering: Str(b"Identity"),
            supplement: 0,
        };

        self.pdf
            .type0_font(type0)
            .base_font(Name(base.as_bytes()))
            .encoding_predefined(Name(b"Identity-H"))
            .descendant_font(cid_font)
            .to_unicode(cmap_id);

        {
            let mut c = self.pdf.cid_font(cid_font);
            c.subtype(if cff {
                CidFontType::Type0
            } else {
                CidFontType::Type2
            });
            c.base_font(Name(base.as_bytes()));
            c.system_info(system);
            c.font_descriptor(descriptor);
            c.default_width(0.0);
            if !cff {
                c.cid_to_gid_map_predefined(Name(b"Identity"));
            }
            c.widths()
                .consecutive(0, f.widths.iter().map(|w| num(f64::from(*w))));
            c.finish();
        }

        let mut flags = FontFlags::SYMBOLIC;
        if f.monospace {
            flags |= FontFlags::FIXED_PITCH;
        }
        if f.italic {
            flags |= FontFlags::ITALIC;
        }
        {
            let mut d = self.pdf.font_descriptor(descriptor);
            d.name(Name(base.as_bytes()))
                .flags(flags)
                .bbox(Rect::new(f.bbox[0], f.bbox[1], f.bbox[2], f.bbox[3]))
                .italic_angle(f.italic_angle)
                .ascent(f.ascent)
                .descent(f.descent)
                .cap_height(f.cap_height)
                // Not in the font; the usual value for regular text.
                .stem_v(80.0);
            if cff {
                d.font_file3(file);
            } else {
                d.font_file2(file);
            }
            d.finish();
        }

        let (bytes, packed) = self.pack(&f.program);
        {
            let mut s = self.pdf.stream(file, &bytes);
            if packed {
                s.filter(Filter::FlateDecode);
            }
            if cff {
                s.pair(Name(b"Subtype"), Name(b"CIDFontType0C"));
            } else {
                s.pair(
                    Name(b"Length1"),
                    i32::try_from(f.program.len()).unwrap_or(i32::MAX),
                );
            }
            s.finish();
        }

        let mut cmap = UnicodeCmap::new(
            Name(b"Xarast-UCS"),
            SystemInfo {
                registry: Str(b"Adobe"),
                ordering: Str(b"UCS"),
                supplement: 0,
            },
        );
        for (cid, text) in spec.to_unicode {
            if !text.is_empty() {
                cmap.pair_with_multiple(*cid, text.chars());
            }
        }
        let data = cmap.finish();
        let (bytes, packed) = self.pack(data.as_slice());
        let mut s = self.pdf.cmap(cmap_id, &bytes);
        if packed {
            s.filter(Filter::FlateDecode);
        }
        s.finish();

        Resource {
            kind: ResourceKind::Font,
            id: type0.get(),
        }
    }

    /// Adds a page drawing `canvas`.
    pub fn page(&mut self, canvas: Canvas, boxes: PageBoxes) {
        let (content, used) = canvas.finish();
        let contents = self.alloc();
        let (bytes, packed) = self.pack(&content);
        let mut s = self.pdf.stream(contents, &bytes);
        if packed {
            s.filter(Filter::FlateDecode);
        }
        s.finish();
        let id = self.alloc();
        let pages = self.pages;
        let mut page = self.pdf.page(id);
        page.parent(pages);
        page.media_box(rect(boxes.media));
        if let Some(t) = boxes.trim {
            page.bleed_box(rect(boxes.media));
            page.trim_box(rect(t));
        }
        page.contents(contents);
        // Every page is a transparency group in DeviceRGB, so blending on
        // the page happens in the space the renderer composites in.
        page.group()
            .transparency()
            .isolated(false)
            .knockout(false)
            .color_space()
            .device_rgb();
        write_resources(&mut page.resources(), &used);
        page.finish();
        self.page_refs.push(id);
    }

    /// Writes the catalog, the page tree and the information dictionary,
    /// and returns the file.
    #[must_use]
    pub fn finish(mut self, info: &DocInfo) -> Vec<u8> {
        let info_id = self.alloc();
        {
            let mut d = self.pdf.document_info(info_id);
            if let Some(t) = &info.title {
                d.title(TextStr(t));
            }
            d.producer(TextStr(&info.producer));
            d.finish();
        }
        self.pdf.catalog(self.catalog).pages(self.pages);
        let count = i32::try_from(self.page_refs.len()).unwrap_or(i32::MAX);
        self.pdf
            .pages(self.pages)
            .kids(self.page_refs.iter().copied())
            .count(count);
        self.pdf.finish()
    }
}

fn write_resources(res: &mut pdf_writer::writers::Resources<'_>, used: &BTreeSet<Resource>) {
    let of = |k: ResourceKind| used.iter().filter(move |r| r.kind == k);
    if of(ResourceKind::GState).next().is_some() {
        let mut d = res.ext_g_states();
        for r in of(ResourceKind::GState) {
            d.pair(Name(r.name().as_bytes()), Ref::new(r.id));
        }
    }
    if of(ResourceKind::Shading).next().is_some() {
        let mut d = res.shadings();
        for r in of(ResourceKind::Shading) {
            d.pair(Name(r.name().as_bytes()), Ref::new(r.id));
        }
    }
    if of(ResourceKind::XObject).next().is_some() {
        let mut d = res.x_objects();
        for r in of(ResourceKind::XObject) {
            d.pair(Name(r.name().as_bytes()), Ref::new(r.id));
        }
    }
    if of(ResourceKind::Font).next().is_some() {
        let mut d = res.fonts();
        for r in of(ResourceKind::Font) {
            d.pair(Name(r.name().as_bytes()), Ref::new(r.id));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).into_owned()
    }

    #[test]
    fn a_page_is_a_well_formed_pdf() {
        let mut w = PdfWriter::new(false);
        let g = w.gstate(GState {
            fill_alpha: OPACITY_ONE / 2,
            ..GState::DEFAULT
        });
        assert_eq!(
            g,
            w.gstate(GState {
                fill_alpha: OPACITY_ONE / 2,
                ..GState::DEFAULT
            }),
            "graphics states are interned"
        );
        let mut c = Canvas::new();
        c.save();
        c.gstate(g);
        c.fill_rgb([255, 0, 0]);
        c.rect(10.0, 10.0, 50.0, 40.0);
        c.fill(Rule::NonZero);
        c.save(); // left open: finishing closes it
        w.page(
            c,
            PageBoxes {
                media: [0.0, 0.0, 200.0, 100.0],
                trim: None,
            },
        );
        let bytes = w.finish(&DocInfo {
            title: None,
            producer: "test".into(),
        });
        let s = text(&bytes);
        assert!(s.starts_with("%PDF-1.7"), "{s}");
        assert!(s.contains("/MediaBox [0 0 200 100]"), "{s}");
        assert!(s.contains("/ExtGState"), "{s}");
        assert!(s.contains("1 0 0 rg"), "{s}");
        assert_eq!(
            s.matches(" q\n").count() + s.matches("\nq\n").count(),
            2,
            "{s}"
        );
        assert!(s.trim_end().ends_with("%%EOF"));
    }

    #[test]
    fn quadratics_become_cubics_and_nan_becomes_zero() {
        let mut p = BezPath::new();
        p.move_to((0.0, 0.0));
        p.quad_to((3.0, 3.0), (6.0, 0.0));
        p.line_to((f64::NAN, 1.0));
        let mut c = Canvas::new();
        c.path(&p);
        let (bytes, _) = c.finish();
        let s = text(&bytes);
        assert!(s.contains("2 2 4 2 6 0 c"), "{s}");
        assert!(s.contains("0 1 l"), "{s}");
        assert!(!s.contains("NaN"));
    }

    #[test]
    fn the_same_calls_make_the_same_bytes() {
        let make = || {
            let mut w = PdfWriter::new(true);
            let f = w.sampled_rgb_function(&[0.0, 1.0], &[2], &[0, 0, 0, 255, 255, 255]);
            let sh = w.axial(f, [0.0, 0.0, 1.0, 0.0], [0.0, 1.0], [true, true]);
            let mut c = Canvas::new();
            c.save();
            c.rect(0.0, 0.0, 10.0, 10.0);
            c.clip(Rule::EvenOdd);
            c.shading(sh);
            c.restore();
            w.page(
                c,
                PageBoxes {
                    media: [0.0, 0.0, 10.0, 10.0],
                    trim: Some([1.0, 1.0, 9.0, 9.0]),
                },
            );
            w.finish(&DocInfo::default())
        };
        assert_eq!(make(), make());
    }
}
