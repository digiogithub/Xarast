//! Reading embedded fonts (XARA-T-0276): a `.xarast` whose face is not
//! installed draws with the WOFF2 subset it carries, through a
//! per-document overlay that no other document sees; the machine's face
//! wins where it has the same face; a character the subset lacks comes
//! from a real face.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use xarast_app::fonts::{self, FontService};
use xarast_app::headless::{self, HeadlessFrame, HeadlessOptions};
use xarast_app::svg_text::SvgTextPlacer;
use xarast_app::{DeviceSize, DocRect, DocumentId, Session};
use xarast_color::{Colour, ColourValue};
use xarast_doc::builder::{BuildLimits, skeleton};
use xarast_doc::fill::Paint;
use xarast_doc::{
    AttrValue, Document, EmbeddedFont, NodeKind, TextItem, TextStoryNode, TypefaceRef,
};
use xarast_format::svg::Placer;
use xarast_geom::{Matrix, Mp, Vector};
use xarast_text::{FontQuery, StoryInput, StyleRange};

const PINNED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../xarast-text/tests/fonts");
/// The face the second machine lacks.
const REMOVED: &str = "NotoSans-Bold.subset.ttf";

/// A directory holding the pinned fonts except `REMOVED`: a machine
/// without the bold face. Unique per test and process.
fn fonts_without_bold(tag: &str) -> (Arc<FontService>, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "xarast-embedded-fonts-read-{}-{tag}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for e in std::fs::read_dir(PINNED).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_name().unwrap().to_str().unwrap().to_owned();
        let font = [".ttf", ".otf"].iter().any(|x| name.ends_with(x));
        if font && name != REMOVED {
            std::fs::copy(&p, dir.join(&name)).unwrap();
        }
    }
    (FontService::from_dir(&dir), dir)
}

fn pinned() -> Arc<FontService> {
    FontService::from_dir(Path::new(PINNED))
}

/// One point story per entry: text, family, bold.
fn doc(stories: &[(&str, &str, bool)], embedded: &[EmbeddedFont]) -> Document {
    let mut b = skeleton(BuildLimits::default()).unwrap();
    for f in embedded {
        b.define_font(f.clone());
    }
    for (i, (text, family, bold)) in stories.iter().enumerate() {
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
        b.attribute(AttrValue::FontSize(Mp::new(24_000))).unwrap();
        b.attribute(AttrValue::Bold(*bold)).unwrap();
        b.attribute(AttrValue::Fill(Paint::Flat {
            value: Colour::Direct(ColourValue::rgb(0.0, 0.0, 0.0)),
        }))
        .unwrap();
        b.attribute(AttrValue::StrokeColour(Paint::Flat {
            value: Colour::Direct(ColourValue::rgbt(0.0, 0.0, 0.0, 1.0)),
        }))
        .unwrap();
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

fn open(package: &[u8]) -> Document {
    xarast_format::open_reader(
        Cursor::new(package.to_vec()),
        &xarast_format::open::OpenOptions::default(),
    )
    .unwrap()
    .document
}

/// Renders `doc` with `fonts` into a fixed frame, so that the pixels
/// depend on the layout alone.
fn render(doc: &mut Document, fonts: &Arc<FontService>) -> Vec<u8> {
    let s = Session::adopt(DocumentId(1), std::mem::take(doc), None);
    let opts = HeadlessOptions {
        size: DeviceSize::new(640, 240),
        frame: HeadlessFrame::Fit(DocRect::raw(40_000, 300_000, 360_000, 440_000)),
        ..HeadlessOptions::default()
    };
    let px = headless::render_with_fonts(&s, &opts, Some(Arc::clone(fonts)))
        .expect("render")
        .surface
        .data()
        .to_vec();
    *doc = s.doc;
    px
}

fn ink(px: &[u8]) -> usize {
    px.as_chunks::<4>().0.iter().filter(|p| p[0] < 128).count()
}

const SAMPLE: &[(&str, &str, bool)] = &[
    ("Embedded bold 0123", "Noto Sans", true),
    ("Regular stays machine", "Noto Sans", false),
];

#[test]
fn a_missing_face_draws_with_its_embedded_subset_exactly_as_installed() {
    let full = pinned();
    let (without, dir) = fonts_without_bold("render");
    let mut original = doc(SAMPLE, &[]);
    let package = save(&original, &full);
    let mut opened = open(&package);
    // Both faces the text draws with come back as the document's.
    assert_eq!(opened.resources.fonts().len(), 2, "regular and bold");
    assert!(
        opened
            .resources
            .fonts()
            .iter()
            .all(|f| &*f.family == "Noto Sans" && f.data.starts_with(b"wOF2"))
    );

    let installed = render(&mut opened, &full);
    assert!(ink(&installed) > 500, "the text drew");
    let missing = render(&mut opened, &without);
    assert_eq!(
        installed, missing,
        "the embedded bold draws exactly as the installed one"
    );
    // Without the embedded faces the machine synthesises the bold, which
    // looks different: the comparison above is not vacuous.
    let substituted = render(&mut original, &without);
    assert_ne!(installed, substituted);

    // The rule: the machine's face wins where it has the same face.
    let q = FontQuery::new("Noto Sans");
    let over_full = fonts::for_document(&full, &opened);
    let m = over_full.db().query(&q).unwrap();
    assert!(!over_full.db().is_document_face(m.face), "regular: machine");
    let m = over_full.db().query(&q.clone().with_weight(700)).unwrap();
    assert!(!over_full.db().is_document_face(m.face), "bold: machine");
    let over = fonts::for_document(&without, &opened);
    let m = over.db().query(&q.clone().with_weight(700)).unwrap();
    assert!(over.db().is_document_face(m.face), "bold: the document's");
    assert_eq!(&*m.family, "Noto Sans");
    assert!(m.substitution.is_none());
    assert!(!m.synthesis.embolden);
    let m = over.db().query(&q).unwrap();
    assert!(!over.db().is_document_face(m.face), "regular: machine");
    // The document's faces are not offered for editing.
    assert_eq!(over.db().families(), without.db().families());
    // Walks, tools and saves of the document share one overlay.
    assert!(Arc::ptr_eq(&over, &fonts::for_document(&without, &opened)));

    // A re-save on the machine without the face embeds it again, from the
    // document's own subset.
    let again = save(&opened, &without);
    let mut reopened = open(&again);
    assert_eq!(reopened.resources.fonts().len(), 2);
    assert_eq!(render(&mut reopened, &without), installed);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn typing_outside_the_subset_uses_a_real_face() {
    let full = pinned();
    let (without, dir) = fonts_without_bold("typing");
    let opened = open(&save(&doc(SAMPLE, &[]), &full));
    let over = fonts::for_document(&without, &opened);
    // "Q", "z" and "!" are not in the text, so not in the subset.
    let text = "bold Qz!";
    let runs = [StyleRange::new(
        0..text.len(),
        FontQuery::new("Noto Sans").with_weight(700),
        Mp::new(24_000),
    )];
    let layout = over.ready().layout(&StoryInput {
        text,
        runs: &runs,
        paragraphs: &[],
        kerns: &[],
        mode: xarast_text::StoryMode::Point,
    });
    let mut seen = Vec::new();
    for line in &layout.lines {
        for run in &line.runs {
            let doc_face = over.db().is_document_face(run.face);
            for g in &run.glyphs {
                let c = text[g.cluster..].chars().next().unwrap();
                assert_ne!(g.id, 0, "{c:?} drew .notdef");
                seen.push((c, doc_face));
            }
        }
    }
    seen.sort_unstable();
    let drawn: String = seen.iter().map(|(c, _)| *c).filter(|c| *c != ' ').collect();
    assert_eq!(drawn, "!Qbdloz", "every character drew once");
    for (c, doc_face) in seen {
        match c {
            'b' | 'o' | 'l' | 'd' => assert!(doc_face, "{c:?} from the subset"),
            'Q' | 'z' | '!' => assert!(!doc_face, "{c:?} from a real face"),
            _ => {}
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn documents_embedding_different_faces_of_one_family_do_not_interfere() {
    let base = pinned();
    let read =
        |f: &str| -> Arc<[u8]> { Arc::from(std::fs::read(Path::new(PINNED).join(f)).unwrap()) };
    let family = "Private Family";
    let face = |file: &str| EmbeddedFont {
        family: Arc::from(family),
        weight: 400,
        italic: false,
        data: read(file),
    };
    let stories = [("Same family", family, false)];
    let mut a = doc(&stories, &[face("NotoSans-Regular.subset.ttf")]);
    let mut b = doc(&stories, &[face("NotoSans-Italic.subset.ttf")]);
    let mut plain = doc(&stories, &[]);

    let first_a = render(&mut a, &base);
    let rb = render(&mut b, &base);
    let again_a = render(&mut a, &base);
    let rp = render(&mut plain, &base);
    assert_eq!(first_a, again_a, "b's face did not reach a");
    assert_ne!(first_a, rb, "each document draws its own face");
    assert!(ink(&rb) > 200);

    let q = FontQuery::new(family);
    let fa = fonts::for_document(&base, &a);
    let fb = fonts::for_document(&base, &b);
    assert!(!Arc::ptr_eq(&fa, &fb));
    let ma = fa.db().query(&q).unwrap();
    let mb = fb.db().query(&q).unwrap();
    assert!(fa.db().is_document_face(ma.face) && fb.db().is_document_face(mb.face));
    assert_eq!(
        fa.db().face_data(ma.face).unwrap().bytes(),
        &*read("NotoSans-Regular.subset.ttf")
    );
    assert_eq!(
        fb.db().face_data(mb.face).unwrap().bytes(),
        &*read("NotoSans-Italic.subset.ttf")
    );
    // Nothing reached the base service either: there the family is
    // substituted, and the plain document draws as before.
    let m = base.db().query(&q).unwrap();
    assert!(m.substitution.is_some());
    assert!(!base.db().families().iter().any(|f| &**f == family));
    assert_eq!(rp, render(&mut plain, &base));
}
