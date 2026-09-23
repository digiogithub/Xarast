//! What the document holds that no export format carries (phase 11
//! T11.5.2, T11.5.3, T11.5.4): CMYK and spot colours become their sRGB
//! conversion and images' ICC profiles are not applied — and the report
//! says so, per format. SVG keeps an image's profile: passed through with
//! the original bytes, or in an `iCCP` chunk when the bitmap is re-encoded.

use std::sync::Arc;

use xarast_color::{Colour, ColourDef, ColourKind, ColourValue, Rgba8};
use xarast_doc::fill::Paint;
use xarast_doc::resources::{BitmapData, BitmapInfo, ImageFormat, OriginalEncoded};
use xarast_doc::{
    AttrValue, BitmapNode, BitmapResource, BuildLimits, Document, NodeKind, ShapeKind, ShapeNode,
};
use xarast_geom::{Point, Rect, Vector};
use xarast_io::fidelity::{Target, census, document_compromises};
use xarast_io::png::{PngHeader, encode_png, with_icc_profile};
use xarast_io::{
    Compromise, ExportArea, ExportError, ExportRequest, ExportSource, FormatId, FormatOptions,
    NoProgress, PngColour, PngDepth, Registry, SourceScene, SvgOptions,
};
use xarast_render::{RenderQuality, Resolver, Scene};

/// A document as a source, with an empty scene for the raster and PDF
/// exporters: only the report is under test here.
struct DocSource(Document);

impl ExportSource for DocSource {
    fn resolve_area(&self, _: &ExportArea) -> Result<Rect, ExportError> {
        Ok(Rect::raw(0, 0, 400_000, 400_000))
    }

    fn build_scene(&self, _: RenderQuality) -> Result<SourceScene, ExportError> {
        Ok(SourceScene {
            scene: Scene::new(),
            resolver: Resolver::default(),
            compromises: Vec::new(),
        })
    }

    fn document(&self) -> Option<&Document> {
        Some(&self.0)
    }
}

/// A fake but well-formed-enough profile: decoders hand ICC bytes through
/// without interpreting them.
fn profile() -> Vec<u8> {
    let mut p = b"XARAST-TEST-PROFILE".to_vec();
    p.extend((0..=255u8).cycle().take(600));
    p
}

fn png_with_profile() -> Vec<u8> {
    let rgba: Vec<u8> = (0..16u8)
        .flat_map(|i| [i * 16, 64, 255 - i * 16, 255])
        .collect();
    let mut png = Vec::new();
    let header = PngHeader {
        width: 4,
        height: 4,
        colour: PngColour::Rgba,
        depth: PngDepth::Eight,
        interlace: false,
        ppm: None,
        level: 6,
    };
    encode_png(&mut png, header, &rgba).unwrap();
    with_icc_profile(&png, &profile()).unwrap()
}

fn shape(x: i32) -> NodeKind {
    NodeKind::Shape(Box::new(ShapeNode {
        shape: ShapeKind::Rect,
        origin: Point::raw(x, 0),
        major: Vector::raw(50_000, 0),
        minor: Vector::raw(0, 50_000),
    }))
}

/// Three CMYK attributes (a direct fill, a palette line colour, a
/// gradient with one CMYK end), one spot fill, one RGB fill, and one
/// image with a profile. `palette_on_image` forces the SVG exporter to
/// re-encode the bitmap instead of passing its bytes through.
fn document(palette_on_image: bool) -> Document {
    let mut b = xarast_doc::builder::skeleton(BuildLimits::default()).unwrap();
    let cmyk = b.define_colour(ColourDef::normal(ColourValue::cmyk(0.1, 0.2, 0.3, 0.4)));
    let spot = b.define_colour(ColourDef {
        kind: ColourKind::Spot,
        ..ColourDef::normal(ColourValue::rgb(0.9, 0.5, 0.1))
    });
    let attrs: [Vec<AttrValue>; 4] = [
        vec![
            AttrValue::Fill(Paint::Flat {
                value: Colour::Direct(ColourValue::cmyk(1.0, 0.0, 0.0, 0.0)),
            }),
            AttrValue::StrokeColour(Paint::Flat {
                value: Colour::Indexed {
                    id: cmyk,
                    tint: None,
                },
            }),
        ],
        vec![AttrValue::Fill(Paint::Linear {
            start: Point::raw(0, 0),
            end: Point::raw(50_000, 0),
            persp: None,
            from: Colour::Direct(ColourValue::rgb(1.0, 0.0, 0.0)),
            to: Colour::Direct(ColourValue::cmyk(0.0, 0.0, 1.0, 0.0)),
            ramp: xarast_doc::fill::Ramp::new(),
        })],
        vec![AttrValue::Fill(Paint::Flat {
            value: Colour::Indexed {
                id: spot,
                tint: Some(0.5),
            },
        })],
        vec![AttrValue::Fill(Paint::Flat {
            value: Colour::Direct(ColourValue::rgb(0.0, 0.5, 0.0)),
        })],
    ];
    for (i, a) in attrs.into_iter().enumerate() {
        b.node(shape(i32::try_from(i).unwrap() * 60_000)).unwrap();
        b.push_scope().unwrap();
        for v in a {
            b.attribute(v).unwrap();
        }
        b.pop_scope();
    }
    let image = b.define_bitmap(BitmapResource {
        name: Arc::from("tagged"),
        info: BitmapInfo {
            width: 4,
            height: 4,
            bpp: 32,
            ..BitmapInfo::default()
        },
        pixels: Arc::new(BitmapData {
            pixels: Arc::from(Vec::new()),
            palette: Arc::from(if palette_on_image {
                vec![Rgba8::BLACK; 2]
            } else {
                Vec::new()
            }),
        }),
        original: Some(Arc::new(OriginalEncoded {
            format: ImageFormat::Png,
            bytes: Arc::from(png_with_profile()),
        })),
        procedural: None,
        transparent_index: None,
    });
    b.node(NodeKind::Bitmap(Box::new(BitmapNode {
        image,
        origin: Point::raw(0, 100_000),
        major: Vector::raw(100_000, 0),
        minor: Vector::raw(0, 100_000),
    })))
    .unwrap();
    b.finish().unwrap().0
}

#[test]
fn the_census_counts_attributes_not_stops() {
    let c = census(&document(false));
    assert_eq!((c.cmyk, c.spot, c.icc_images), (3, 1, 1));
    // A document with nothing to report reports nothing.
    let plain = xarast_doc::builder::skeleton(BuildLimits::default())
        .unwrap()
        .finish()
        .unwrap()
        .0;
    assert!(document_compromises(&DocSource(plain), Target::Raster).is_empty());
}

#[test]
fn every_format_reports_converted_colours_and_raster_and_pdf_the_profile() {
    let src = DocSource(document(false));
    let dir = tempfile::tempdir().unwrap();
    let cmyk = Compromise::ColourConverted {
        model: Arc::from("CMYK"),
        count: 3,
    };
    let spot = Compromise::ColourConverted {
        model: Arc::from("spot"),
        count: 1,
    };
    let dropped = Compromise::ProfileDropped { images: 1 };
    for id in [
        FormatId::Png,
        FormatId::Jpeg,
        FormatId::WebP,
        FormatId::Pdf,
        FormatId::Svg,
    ] {
        let req = ExportRequest::new(
            FormatOptions::default_for(id),
            dir.path().join(format!("out.{}", id.extension())),
        );
        let r = Registry::with_builtin()
            .export(&src, &req, &NoProgress)
            .unwrap();
        assert!(r.compromises.contains(&cmyk), "{id:?}: {:?}", r.compromises);
        assert!(r.compromises.contains(&spot), "{id:?}");
        assert_eq!(
            r.compromises.contains(&dropped),
            id != FormatId::Svg,
            "{id:?}: SVG keeps the profile, the others do not apply it"
        );
    }
    assert!(
        cmyk.to_string()
            .contains("CMYK colours written as their sRGB conversion")
    );
}

/// The PNG inside the first `data:` URI of an SVG.
fn embedded_png(svg: &str) -> Vec<u8> {
    let start = svg.find("data:image/png;base64,").unwrap() + "data:image/png;base64,".len();
    let b64 = &svg[start..start + svg[start..].find('"').unwrap()];
    decode_base64(b64)
}

fn decode_base64(s: &str) -> Vec<u8> {
    let val = |c: u8| -> u32 {
        match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("base64 byte {c}"),
        }
    };
    let bytes: Vec<u8> = s.bytes().filter(|&c| c != b'=').collect();
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let mut acc = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            acc |= val(c) << (18 - 6 * i);
        }
        let n = chunk.len() * 6 / 8;
        for i in 0..n {
            out.push(((acc >> (16 - 8 * i)) & 0xFF) as u8);
        }
    }
    out
}

fn icc_of(png: &[u8]) -> Option<Vec<u8>> {
    let dec = png::Decoder::new(std::io::Cursor::new(png.to_vec()));
    let r = dec.read_info().unwrap();
    r.info().icc_profile.as_ref().map(|p| p.to_vec())
}

#[test]
fn svg_keeps_an_images_profile_passed_through_or_re_encoded() {
    let dir = tempfile::tempdir().unwrap();
    for re_encoded in [false, true] {
        let src = DocSource(document(re_encoded));
        let req = ExportRequest::new(
            FormatOptions::Svg(SvgOptions::default()),
            dir.path().join("i.svg"),
        );
        Registry::with_builtin()
            .export(&src, &req, &NoProgress)
            .unwrap();
        let svg = std::fs::read_to_string(&req.destination).unwrap();
        let png = embedded_png(&svg);
        // Both come out byte for byte the original: passed through, or
        // decoded and written again by the same encoder, with the same
        // profile spliced in the same place — which is the point.
        assert_eq!(png, png_with_profile(), "re-encoded: {re_encoded}");
        assert_eq!(icc_of(&png), Some(profile()), "re-encoded: {re_encoded}");
    }
}
