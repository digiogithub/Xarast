//! SVG export end to end (W11.3): the interchange dialect of the `.xarast`
//! profile's mapper. Checks what a browser and Inkscape rely on — the file
//! is well-formed and carries no `xarast` vocabulary, the root frames the
//! area, bitmaps are inline or in a sidecar folder — plus the report and
//! the determinism contract. The render comparison against our raster
//! export runs over the corpus through `cargo xtask svg-render`
//! (`docs/memory/export.md`, "SVG").

use std::path::Path;
use std::sync::Arc;

use quick_xml::events::Event;
use xarast_color::{Colour, ColourValue, Rgba8};
use xarast_doc::fill::Paint;
use xarast_doc::resources::{BitmapData, BitmapInfo, ImageFormat, OriginalEncoded};
use xarast_doc::{
    AttrValue, BitmapNode, BitmapResource, BuildLimits, Document, ForeignAttr, ForeignBaggage,
    ForeignMarks, NodeKind, ShapeKind, ShapeNode,
};
use xarast_geom::{Point, Rect, Vector};
use xarast_io::png::{PngHeader, encode_png};
use xarast_io::{
    Background, CancelFlag, Compromise, ExportArea, ExportError, ExportRequest, ExportSource,
    FormatOptions, NoProgress, PngColour, PngDepth, Registry, SceneSource, SourceScene, SvgOptions,
    SvgResources,
};
use xarast_render::{RenderQuality, Resolver, Scene};

/// A document as a source: the area is always `area`.
struct DocSource {
    doc: Document,
    area: Rect,
}

impl ExportSource for DocSource {
    fn resolve_area(&self, area: &ExportArea) -> Result<Rect, ExportError> {
        match area {
            ExportArea::Rect(r) => Ok(*r),
            _ => Ok(self.area),
        }
    }

    fn build_scene(&self, _q: RenderQuality) -> Result<SourceScene, ExportError> {
        Err(ExportError::Scene(
            "SVG export must not build a scene".into(),
        ))
    }

    fn document(&self) -> Option<&Document> {
        Some(&self.doc)
    }
}

fn rect(x: i32, y: i32, w: i32, h: i32) -> NodeKind {
    NodeKind::Shape(Box::new(ShapeNode {
        shape: ShapeKind::Rect,
        origin: Point::raw(x, y),
        major: Vector::raw(w, 0),
        minor: Vector::raw(0, h),
    }))
}

fn rgba_png(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let header = PngHeader {
        width: w,
        height: h,
        colour: PngColour::Rgba,
        depth: PngDepth::Eight,
        interlace: false,
        ppm: None,
        level: 6,
    };
    encode_png(&mut out, header, rgba).unwrap();
    out
}

fn bitmap_node(image: xarast_doc::BitmapId, x: i32) -> NodeKind {
    NodeKind::Bitmap(Box::new(BitmapNode {
        image,
        origin: Point::raw(x, 400_000),
        major: Vector::raw(100_000, 0),
        minor: Vector::raw(0, 100_000),
    }))
}

/// A red rectangle carrying foreign baggage, a PNG bitmap placed twice and
/// a bitmap that exists only as pixels.
fn fixture() -> Document {
    let mut b = xarast_doc::builder::skeleton(BuildLimits::default()).unwrap();
    let red = b.node(rect(100_000, 100_000, 200_000, 100_000)).unwrap();
    b.push_scope().unwrap();
    b.attribute(AttrValue::Fill(Paint::Flat {
        value: Colour::Direct(ColourValue::rgb(0.8, 0.1, 0.1)),
    }))
    .unwrap();
    b.pop_scope();
    b.foreign(
        red,
        ForeignBaggage {
            attrs: vec![ForeignAttr {
                ns: Arc::from("urn:test:future"),
                prefix: Some(Arc::from("fut")),
                local: Arc::from("state"),
                value: Arc::from("kept in .xarast only"),
            }],
            children: Vec::new(),
            marks: ForeignMarks::empty(),
        },
    );
    let pixels: Vec<u8> = (0..16u8)
        .flat_map(|i| [i * 16, 255 - i * 16, 64, 255])
        .collect();
    let png = b.define_bitmap(BitmapResource {
        name: Arc::from("png"),
        info: BitmapInfo {
            width: 4,
            height: 4,
            bpp: 32,
            ..BitmapInfo::default()
        },
        pixels: Arc::new(BitmapData::default()),
        original: Some(Arc::new(OriginalEncoded {
            format: ImageFormat::Png,
            bytes: Arc::from(rgba_png(4, 4, &pixels)),
        })),
        procedural: None,
        transparent_index: None,
    });
    let raw = b.define_bitmap(BitmapResource {
        name: Arc::from("raw"),
        info: BitmapInfo {
            width: 2,
            height: 2,
            bpp: 32,
            ..BitmapInfo::default()
        },
        pixels: Arc::new(BitmapData {
            pixels: Arc::from(vec![255u8; 16]),
            palette: Arc::from(Vec::new()),
        }),
        original: None,
        procedural: None,
        transparent_index: None,
    });
    b.node(bitmap_node(png, 100_000)).unwrap();
    b.node(bitmap_node(png, 250_000)).unwrap();
    b.node(bitmap_node(raw, 400_000)).unwrap();
    b.finish().unwrap().0
}

fn source() -> DocSource {
    DocSource {
        doc: fixture(),
        area: Rect::raw(50_000, 50_000, 550_000, 550_000),
    }
}

fn svg_request(dir: &Path, name: &str, o: SvgOptions) -> ExportRequest {
    ExportRequest::new(FormatOptions::Svg(o), dir.join(name))
}

/// Parses the file: well-formed or panic. Returns every element name and
/// every attribute name.
fn parse(svg: &str) -> (Vec<String>, Vec<String>) {
    let mut r = quick_xml::Reader::from_str(svg);
    r.config_mut().check_end_names = true;
    let (mut names, mut attrs) = (Vec::new(), Vec::new());
    loop {
        match r.read_event().expect("well-formed XML") {
            Event::Start(e) | Event::Empty(e) => {
                names.push(String::from_utf8_lossy(e.name().as_ref()).into_owned());
                for a in e.attributes() {
                    let a = a.expect("well-formed attribute");
                    attrs.push(String::from_utf8_lossy(a.key.as_ref()).into_owned());
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    (names, attrs)
}

fn attr<'a>(svg: &'a str, name: &str) -> &'a str {
    let pat = format!(" {name}=\"");
    let start = svg.find(&pat).unwrap() + pat.len();
    &svg[start..start + svg[start..].find('"').unwrap()]
}

#[test]
fn an_interchange_svg_has_no_private_vocabulary_and_frames_the_area() {
    let dir = tempfile::tempdir().unwrap();
    let src = source();
    let req = svg_request(dir.path(), "a.svg", SvgOptions::default());
    let report = Registry::with_builtin()
        .export(&src, &req, &NoProgress)
        .unwrap();
    let svg = std::fs::read_to_string(&req.destination).unwrap();
    let (names, attrs) = parse(&svg);
    assert!(!svg.contains("xarast"), "{svg}");
    assert!(!svg.contains("urn:test:future") && !svg.contains("fut:"));
    // Only SVG and the standard metadata / Inkscape vocabularies.
    let known = [
        "", "xmlns", "xml", "xlink", "inkscape", "sodipodi", "rdf", "dc", "cc",
    ];
    for n in names.iter().chain(&attrs) {
        let prefix = n.split_once(':').map_or("", |(p, _)| p);
        assert!(known.contains(&prefix), "{n}");
    }
    assert_eq!(names.iter().filter(|n| *n == "image").count(), 3);
    // 500 x 500 pt, in the first spread's frame; 500 pt = 176.389 mm.
    let vb: Vec<f64> = attr(&svg, "viewBox")
        .split(' ')
        .map(|v| v.parse().unwrap())
        .collect();
    assert_eq!(&vb[2..], [500.0, 500.0]);
    assert_eq!(attr(&svg, "width"), "176.389mm");
    assert_eq!(report.pixels, (500, 500));
    assert_eq!(report.bytes_written, svg.len() as u64);
    assert!(
        report
            .compromises
            .contains(&Compromise::UnknownDataDropped { count: 1 }),
        "{:?}",
        report.compromises
    );
    // The PNG passes through; the pixel-only bitmap becomes a PNG too.
    assert_eq!(svg.matches("data:image/png;base64,").count(), 3);
}

#[test]
fn sidecar_resources_are_written_once_beside_the_svg() {
    let dir = tempfile::tempdir().unwrap();
    let req = svg_request(
        dir.path(),
        "my drawing.svg",
        SvgOptions {
            resources: SvgResources::Sidecar,
            ..SvgOptions::default()
        },
    );
    let report = Registry::with_builtin()
        .export(&source(), &req, &NoProgress)
        .unwrap();
    let svg = std::fs::read_to_string(&req.destination).unwrap();
    assert!(!svg.contains("data:"));
    // One reference per image, as `xlink:href` (SVG 1.1).
    assert_eq!(
        svg.matches(" xlink:href=\"my_drawing_files/image-1.png\"")
            .count(),
        2
    );
    assert_eq!(
        svg.matches(" xlink:href=\"my_drawing_files/image-2.png\"")
            .count(),
        1
    );
    assert!(!svg.contains(" href="));
    let side = dir.path().join("my_drawing_files");
    let one = std::fs::read(side.join("image-1.png")).unwrap();
    let two = std::fs::read(side.join("image-2.png")).unwrap();
    assert!(one.starts_with(b"\x89PNG") && two.starts_with(b"\x89PNG"));
    assert_eq!(
        report.bytes_written,
        (svg.len() + one.len() + two.len()) as u64
    );
}

#[test]
fn the_same_request_gives_the_same_bytes() {
    let dir = tempfile::tempdir().unwrap();
    for o in [
        SvgOptions::default(),
        SvgOptions {
            minify: true,
            ..SvgOptions::default()
        },
        SvgOptions {
            pretty: true,
            ..SvgOptions::default()
        },
    ] {
        let a = svg_request(dir.path(), "a.svg", o);
        let b = svg_request(dir.path(), "b.svg", o);
        Registry::with_builtin()
            .export(&source(), &a, &NoProgress)
            .unwrap();
        Registry::with_builtin()
            .export(&source(), &b, &NoProgress)
            .unwrap();
        let (x, y) = (
            std::fs::read(&a.destination).unwrap(),
            std::fs::read(&b.destination).unwrap(),
        );
        assert_eq!(x, y, "{o:?}");
        parse(std::str::from_utf8(&x).unwrap());
    }
}

#[test]
fn minify_drops_unreferenced_ids_and_background_paints_the_area() {
    let dir = tempfile::tempdir().unwrap();
    let mut req = svg_request(
        dir.path(),
        "m.svg",
        SvgOptions {
            minify: true,
            ..SvgOptions::default()
        },
    );
    req.background = Background::Colour(Rgba8 {
        r: 0x12,
        g: 0x34,
        b: 0x56,
        a: 255,
    });
    Registry::with_builtin()
        .export(&source(), &req, &NoProgress)
        .unwrap();
    let svg = std::fs::read_to_string(&req.destination).unwrap();
    // Only the layer Inkscape opens on keeps its id.
    assert!(svg.matches(" id=\"x").count() <= 1, "{svg}");
    assert!(!svg.contains("\n "), "no indentation");
    let bg = svg.find("fill=\"#123456\"").expect("background rect");
    assert!(bg < svg.find("<image").unwrap(), "under everything");
}

#[test]
fn a_scene_only_source_is_refused_and_cancelling_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (scene, resolver) = (Scene::new(), Resolver::default());
    let scene_only = SceneSource {
        scene: &scene,
        resolver: &resolver,
        area: Rect::raw(0, 0, 1000, 1000),
        paper: Rgba8::WHITE,
    };
    let req = svg_request(dir.path(), "s.svg", SvgOptions::default());
    let e = Registry::with_builtin()
        .export(&scene_only, &req, &NoProgress)
        .unwrap_err();
    assert!(matches!(e, ExportError::UnsupportedFormat { .. }), "{e}");
    let cancel = CancelFlag::default();
    cancel.cancel();
    let e = Registry::with_builtin()
        .export(&source(), &req, &cancel)
        .unwrap_err();
    assert!(matches!(e, ExportError::Cancelled));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}
