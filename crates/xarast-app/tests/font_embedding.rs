//! Font embedding end to end (XARA-T-0218, XARA-T-0228): a synthetic
//! document (the corpus's `embeddedFonts.xar` carries no font data) with
//! text in an embeddable face, in a face whose `OS/2.fsType` forbids
//! embedding, with a gradient and with an underline under a translucent
//! fill, exported to PDF and SVG and saved as `.xarast`, with the pinned
//! test fonts.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use xarast_app::fonts::FontService;
use xarast_app::svg_text::SvgTextPlacer;
use xarast_app::{DeviceSize, EditState, SceneWalker, Viewport};
use xarast_color::{Colour, ColourValue, Transparency};
use xarast_doc::builder::{BuildLimits, skeleton};
use xarast_doc::fill::{Paint, Ramp, TranspPaint};
use xarast_doc::{AttrValue, Document, NodeKind, TextItem, TextStoryNode, TypefaceRef};
use xarast_format::svg::Placer;
use xarast_geom::{Matrix, Mp, Point, Rect, Vector};
use xarast_io::{
    Compromise, ExportArea, ExportError, ExportRequest, ExportSource, Exporter, FormatOptions,
    NoProgress, PdfExporter, PdfOptions, SourceScene, SvgExporter, SvgOptions, TextOutput,
};
use xarast_render::{RenderQuality, Scene};

const RESTRICTED: &str = "Restricted Sans";

/// The pinned fonts plus a copy of Noto Sans whose licence forbids
/// embedding, registered as "Restricted Sans". One per test: a service
/// holds its own caches.
fn fonts() -> Arc<FontService> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../xarast-text/tests/fonts");
    let service = FontService::from_dir(&dir);
    let latin = std::fs::read(dir.join("NotoSans-Regular.subset.ttf")).unwrap();
    let restricted = xarast_text::embed::with_fs_type(&latin, 0x0002).unwrap();
    service
        .db()
        .register_embedded(Some(RESTRICTED), restricted)
        .unwrap();
    service
}

fn rgb(r: f32, g: f32, b: f32) -> Colour {
    Colour::Direct(ColourValue::rgb(r, g, b))
}

/// One point story per entry: its text, family and extra attributes.
fn doc(stories: &[(&str, &str, Vec<AttrValue>)]) -> Document {
    let mut b = skeleton(BuildLimits::default()).unwrap();
    for (i, (text, family, attrs)) in stories.iter().enumerate() {
        let y = 400_000 - 40_000 * i32::try_from(i).unwrap();
        b.node(NodeKind::TextStory(Box::new(TextStoryNode {
            transform: Matrix::translate(Vector::new(Mp::new(50_000), Mp::new(y))),
            ..TextStoryNode::default()
        })))
        .unwrap();
        b.push_scope().unwrap();
        b.attribute(AttrValue::FontTypeface(Arc::new(TypefaceRef {
            full_name: Arc::from(*family),
            family: Arc::from(*family),
            panose: None,
        })))
        .unwrap();
        b.attribute(AttrValue::FontSize(Mp::new(18_000))).unwrap();
        // Black fill, no line: the document default strokes text.
        b.attribute(AttrValue::Fill(Paint::Flat {
            value: rgb(0.0, 0.0, 0.0),
        }))
        .unwrap();
        b.attribute(AttrValue::StrokeColour(Paint::Flat {
            value: Colour::Direct(ColourValue::rgbt(0.0, 0.0, 0.0, 1.0)),
        }))
        .unwrap();
        for a in attrs {
            b.attribute(a.clone()).unwrap();
        }
        b.node(NodeKind::TextLine(Box::default())).unwrap();
        b.push_scope().unwrap();
        for c in text.chars() {
            b.node(NodeKind::TextItem(TextItem::Char(c))).unwrap();
        }
        b.node(NodeKind::TextItem(TextItem::LineBreak(true)))
            .unwrap();
        b.pop_scope();
        b.pop_scope();
    }
    b.finish().unwrap().0
}

fn gradient() -> AttrValue {
    AttrValue::Fill(Paint::Linear {
        start: Point::raw(50_000, 0),
        end: Point::raw(250_000, 0),
        persp: None,
        from: rgb(0.8, 0.1, 0.1),
        to: rgb(0.1, 0.2, 0.8),
        ramp: Ramp::new(),
    })
}

/// The document every test exports: plain, gradient, restricted, and an
/// underlined run under a half-transparent fill.
fn sample() -> Document {
    doc(&[
        ("Xarast embeds fonts", "Noto Sans", vec![]),
        ("Gradient text", "Noto Sans", vec![gradient()]),
        ("Licensed elsewhere", RESTRICTED, vec![]),
        (
            "Underlined",
            "Noto Sans",
            vec![
                AttrValue::Underline(true),
                AttrValue::Fill(Paint::Flat {
                    value: rgb(0.1, 0.5, 0.2),
                }),
                AttrValue::TranspFill(TranspPaint::Flat {
                    value: Transparency::mix(128),
                }),
            ],
        ),
    ])
}

/// An export source over a document, walked with `fonts`.
struct Src {
    doc: Document,
    fonts: Arc<FontService>,
}

impl ExportSource for Src {
    fn resolve_area(&self, _: &ExportArea) -> Result<Rect, ExportError> {
        Ok(Rect::raw(0, 200_000, 500_000, 450_000))
    }

    fn build_scene(&self, quality: RenderQuality) -> Result<SourceScene, ExportError> {
        let mut w = SceneWalker::with_fonts(Arc::clone(&self.fonts));
        let mut scene = Scene::new();
        let edit = EditState::for_document(&self.doc);
        let vp = Viewport::new(DeviceSize::new(800, 600));
        w.rebuild(&self.doc, &edit, &vp, quality, None, &mut scene)
            .map_err(|e| ExportError::Scene(e.to_string()))?;
        let text = w.scene_text();
        Ok(SourceScene {
            scene,
            resolver: w.into_resolver(),
            compromises: Vec::new(),
            text,
        })
    }

    fn document(&self) -> Option<&Document> {
        Some(&self.doc)
    }

    fn svg_text_placer(&self) -> Option<Placer> {
        Some(Placer(Arc::new(SvgTextPlacer::new(Arc::clone(
            &self.fonts,
        )))))
    }

    fn text_as_outlines(&self, all: bool) -> Option<(Document, Vec<Arc<str>>)> {
        xarast_app::convert::text_as_outlines(&self.doc, &self.fonts, all)
    }
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "xarast-font-embedding-{}-{name}",
        std::process::id()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn pdf(src: &Src, dir: &Path, compress: bool) -> (Vec<u8>, xarast_io::ExportReport) {
    let dest = dir.join(if compress { "a.pdf" } else { "plain.pdf" });
    let req = ExportRequest::new(
        FormatOptions::Pdf(PdfOptions {
            compress,
            ..PdfOptions::default()
        }),
        dest.clone(),
    );
    let report = PdfExporter.export(src, &req, &NoProgress).unwrap();
    (std::fs::read(dest).unwrap(), report)
}

fn tool(name: &str) -> bool {
    std::process::Command::new(name).arg("-v").output().is_ok()
}

#[test]
fn pdf_text_is_text_in_embedded_subsets_except_where_the_licence_forbids() {
    let src = Src {
        doc: sample(),
        fonts: fonts(),
    };
    let dir = scratch("pdf");
    let (bytes, report) = pdf(&src, &dir, false);
    let s = String::from_utf8_lossy(&bytes);
    // One embedded font: Noto Sans, a TrueType subset with its ToUnicode
    // map. The restricted copy is never embedded.
    assert_eq!(s.matches("/FontFile2").count(), 1, "one font program");
    assert_eq!(s.matches("/Subtype /Type0").count(), 1);
    assert!(s.contains("/CIDFontType2"));
    assert!(s.contains("/Identity-H"));
    assert!(s.contains("/ToUnicode"));
    let base = s
        .split("/BaseFont /")
        .nth(1)
        .and_then(|t| t.split_whitespace().next())
        .unwrap();
    assert_eq!(base.len(), "ABCDEF+NotoSans-Regular".len(), "{base}");
    assert_eq!(&base[6..], "+NotoSans-Regular");
    assert!(base[..6].chars().all(|c| c.is_ascii_uppercase()));
    // Painted as text: filled (mode 0), the gradient through a text clip
    // (mode 7), the translucent underlined run as outlines plus invisible
    // text (mode 3).
    assert!(s.contains("0 Tr"), "filled text");
    assert!(s.contains("7 Tr"), "gradient text clip");
    assert!(s.contains("3 Tr"), "invisible text over outlines");
    assert!(s.contains(" Tj"));
    // The report names the refused face, once.
    let refused: Vec<_> = report
        .compromises
        .iter()
        .filter_map(|c| match c {
            Compromise::FontNotEmbedded { family, reason } => Some((family, reason)),
            _ => None,
        })
        .collect();
    assert_eq!(refused.len(), 1, "{:?}", report.compromises);
    assert_eq!(&**refused[0].0, RESTRICTED);
    assert!(refused[0].1.contains("fsType"), "{}", refused[0].1);
    assert!(PdfExporter.capabilities().embeds_fonts);

    // Deterministic, compressed or not.
    assert_eq!(pdf(&src, &dir, false).0, bytes);
    let (a, _) = pdf(&src, &dir, true);
    assert_eq!(pdf(&src, &dir, true).0, a);

    // Poppler reads the text back: every embedded story, not the refused
    // one (its outlines carry no text).
    if tool("pdftotext") {
        let out = std::process::Command::new("pdftotext")
            .arg(dir.join("a.pdf"))
            .arg("-")
            .output()
            .unwrap();
        assert!(out.status.success());
        let text = String::from_utf8_lossy(&out.stdout);
        for want in ["Xarast embeds fonts", "Gradient text", "Underlined"] {
            assert!(text.contains(want), "{want:?} in {text:?}");
        }
        assert!(!text.contains("Licensed"), "{text:?}");
    }
    if tool("pdffonts") {
        let out = std::process::Command::new("pdffonts")
            .arg(dir.join("a.pdf"))
            .output()
            .unwrap();
        let t = String::from_utf8_lossy(&out.stdout);
        let rows: Vec<&str> = t.lines().skip(2).collect();
        assert_eq!(rows.len(), 1, "{t}");
        // emb, sub, uni all "yes".
        assert_eq!(rows[0].matches(" yes").count(), 3, "{t}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

fn svg(src: &Src, dir: &Path, text: TextOutput) -> (String, Vec<(String, String)>) {
    let dest = dir.join("a.svg");
    let req = ExportRequest::new(
        FormatOptions::Svg(SvgOptions {
            text,
            ..SvgOptions::default()
        }),
        dest.clone(),
    );
    let report = SvgExporter.export(src, &req, &NoProgress).unwrap();
    let refused = report
        .compromises
        .iter()
        .filter_map(|c| match c {
            Compromise::FontNotEmbedded { family, reason } => {
                Some((family.to_string(), reason.to_string()))
            }
            _ => None,
        })
        .collect();
    (std::fs::read_to_string(&dest).unwrap(), refused)
}

#[test]
fn svg_export_embeds_woff2_subsets_and_outlines_the_refused_face() {
    let src = Src {
        doc: sample(),
        fonts: fonts(),
    };
    let dir = scratch("svg");
    let (text_svg, refused) = svg(&src, &dir, TextOutput::Text);
    assert_eq!(
        text_svg.matches("@font-face{").count(),
        1,
        "one face embedded"
    );
    assert!(text_svg.contains("font-family:'Noto Sans';font-weight:400;font-style:normal;"));
    assert!(text_svg.contains("src:url(data:font/woff2;base64,d09GMg"));
    assert!(!text_svg.contains("xarast:"));
    // T9.6.5: the story in the refused face is its glyph outlines; the
    // three others stay live text.
    assert_eq!(text_svg.matches("<text").count(), 3, "{text_svg}");
    assert!(!text_svg.contains("Licensed elsewhere"));
    assert!(text_svg.contains("Xarast embeds fonts"));
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert_eq!(refused[0].0, RESTRICTED);
    assert!(refused[0].1.contains("outlines"), "{}", refused[0].1);
    assert!(SvgExporter.capabilities().embeds_fonts);
    // The document itself is untouched.
    let stories = src
        .doc
        .tree
        .preorder(src.doc.tree.root())
        .filter(|n| matches!(src.doc.tree.kind(*n), Some(NodeKind::TextStory(_))))
        .count();
    assert_eq!(stories, 4);

    // Text as outlines: no text, no fonts, nothing to report.
    let (outlined, refused) = svg(&src, &dir, TextOutput::Outlines);
    assert!(!outlined.contains("<text"));
    assert!(!outlined.contains("@font-face"));
    assert!(refused.is_empty(), "{refused:?}");
    assert!(outlined.matches("<path").count() >= 4);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pdf_text_as_outlines_embeds_nothing() {
    let src = Src {
        doc: sample(),
        fonts: fonts(),
    };
    let dir = scratch("pdf-outlines");
    let dest = dir.join("o.pdf");
    let req = ExportRequest::new(
        FormatOptions::Pdf(PdfOptions {
            compress: false,
            text: TextOutput::Outlines,
            ..PdfOptions::default()
        }),
        dest.clone(),
    );
    let report = PdfExporter.export(&src, &req, &NoProgress).unwrap();
    let s = String::from_utf8_lossy(&std::fs::read(&dest).unwrap()).into_owned();
    assert!(!s.contains("/Font"), "no font at all");
    assert!(!s.contains(" Tj"));
    assert!(
        !report
            .compromises
            .iter()
            .any(|c| matches!(c, Compromise::FontNotEmbedded { .. })),
        "{:?}",
        report.compromises
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Saves `doc` as `.xarast` with the placer over `fonts`.
fn save(doc: &Document, fonts: &Arc<FontService>) -> Vec<u8> {
    let opts = xarast_format::SaveOptions {
        svg: xarast_format::svg::SvgOptions {
            text: Some(Placer(Arc::new(SvgTextPlacer::new(Arc::clone(fonts))))),
            ..xarast_format::svg::SvgOptions::default()
        },
        write: xarast_format::WriteOptions {
            deterministic: true,
            ..xarast_format::WriteOptions::default()
        },
        ..xarast_format::SaveOptions::default()
    };
    let mut out = Cursor::new(Vec::new());
    xarast_format::save_to(doc, &mut out, &opts).unwrap();
    out.into_inner()
}

#[test]
fn xarast_embeds_woff2_resources_and_marks_the_refused_face() {
    let fonts = fonts();
    let doc = sample();
    let package = save(&doc, &fonts);
    let mut r = xarast_format::XarastReader::open(Cursor::new(package.clone())).unwrap();
    let names: Vec<String> = r.entries().iter().map(|e| e.name.clone()).collect();
    let files: Vec<&String> = names
        .iter()
        .filter(|n| n.starts_with("resources/fonts/"))
        .collect();
    // Criterion 13 of US-0050: exactly the embeddable face has a file;
    // the refused one has none and its runs say so.
    assert_eq!(files.len(), 1, "{names:?}");
    assert!(files[0].ends_with(".woff2"));
    let woff2 = r.entry(files[0]).unwrap();
    let ttf = xarast_text::embed::woff2::decode(&woff2).unwrap();
    let probe = xarast_text::FontDb::new_isolated();
    let face = probe.register_embedded(None, ttf).unwrap()[0];
    assert!(
        !probe.embedding_denied(face),
        "the file is not the refused face"
    );
    let svg = String::from_utf8(r.document_bytes().unwrap()).unwrap();
    assert!(svg.contains(&format!("src:url({}) format('woff2')", files[0])));
    assert!(svg.contains("xarast:font-embed=\"denied\""));
    assert_eq!(svg.matches("xarast:font-embed=\"denied\"").count(), 1);
    let meta = String::from_utf8(r.meta_bytes().unwrap()).unwrap();
    assert!(meta.contains("xarast:fonts=\"1\""), "{meta}");

    // The reader ignores the rules and the marks: the model comes back
    // the same, and saving the reopened document gives the same bytes,
    // fresh or through the package's raw copies.
    let opened = xarast_format::open_reader(
        Cursor::new(package.clone()),
        &xarast_format::open::OpenOptions::default(),
    )
    .unwrap();
    assert!(
        opened
            .diagnostics
            .iter()
            .all(|d| !d.message.contains("CSS")),
        "{:?}",
        opened.diagnostics
    );
    assert_eq!(
        xarast_format::svg::normal_form(&opened.document),
        xarast_format::svg::normal_form(&doc)
    );
    assert_eq!(save(&opened.document, &fonts), package);
    let mut source = opened.package;
    let mut again = Cursor::new(Vec::new());
    xarast_format::save_opened_to(
        &opened.document,
        &mut source,
        &mut again,
        &xarast_format::SaveOptions {
            svg: xarast_format::svg::SvgOptions {
                text: Some(Placer(Arc::new(SvgTextPlacer::new(Arc::clone(&fonts))))),
                ..xarast_format::svg::SvgOptions::default()
            },
            write: xarast_format::WriteOptions {
                deterministic: true,
                ..xarast_format::WriteOptions::default()
            },
            ..xarast_format::SaveOptions::default()
        },
    )
    .unwrap();
    assert_eq!(again.into_inner(), package);
}
