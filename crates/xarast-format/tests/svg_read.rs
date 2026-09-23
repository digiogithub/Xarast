//! The SVG profile reader (W4) against `research/06 §5`, `§6` and `§8`.
//!
//! Every fixture here is synthetic: documents built through the model's
//! builder, or SVG text written in the test. What is checked:
//!
//! - what the writer writes reads back to the same normal form and saves
//!   back to the same bytes (XARA-T-0105);
//! - hoisted paint and CSS classes (passes 4–5) read exactly like inline
//!   paint (XARA-T-0107);
//! - foreign data — attributes, elements, comments, PIs — survives a read
//!   and a save in place, and the preservation digest notices a loss (§8);
//! - active content is stripped, hostile XML refused, duplicate ids
//!   reassigned, orphan baked geometry kept (F4.3), a plain SVG read;
//! - opening writes nothing (F4.10); unknown ZIP entries and resources only
//!   foreign data refers to survive a re-save (§8.3).

use std::io::Cursor;
use std::sync::Arc;

use xarast_color::{
    Colour, ColourDef, ColourKind, ColourModel, ColourValue, Rgba8, TranspMode, Transparency,
};
use xarast_doc::attr::resolve_uncached;
use xarast_doc::fill::{FillGeometry, Paint, Ramp, RampStop, TranspPaint};
use xarast_doc::{
    AttrSlot, AttrValue, BitmapData, BitmapInfo, BitmapNode, BitmapResource, BuildLimits,
    ClipViewMode, ClipViewNode, Document, ForeignAttr, ForeignBaggage, ForeignChild,
    ForeignChildKind, ForeignMarks, GroupNode, GuidelineNode, ImageFormat, LayerNode, LiveKind,
    LiveNode, LiveRole, NodeFlags, NodeKind, OpaqueNode, OriginalEncoded, PathNode, RegenState,
    ShapeKind, ShapeNode, TextItem, TextStoryNode,
};
use xarast_format::svg::read::{ReadOptions, read_svg};
use xarast_format::svg::{SvgOptions, normal_form};
use xarast_format::{
    OpenOptions, PackageWriter, ResourceIndex, SaveOptions, WriteOptions, XarastReader,
    open_reader, save_opened_to, save_to,
};
use xarast_geom::{BiasGain, Matrix, Mp, Path, Point, Vector};

/// A colour 8-bit sRGB holds exactly: what a key stop can carry.
fn rgb(r: f32, g: f32, b: f32) -> Colour {
    let q = |v: f32| (v * 255.0).round() as u8;
    Colour::Direct(ColourValue::from_rgba8(Rgba8 {
        r: q(r),
        g: q(g),
        b: q(b),
        a: 255,
    }))
}

fn flat(c: Colour) -> AttrValue {
    AttrValue::Fill(Paint::Flat { value: c })
}

fn shape(kind: ShapeKind, o: (i32, i32), u: (i32, i32), v: (i32, i32)) -> NodeKind {
    NodeKind::Shape(Box::new(ShapeNode {
        shape: kind,
        origin: Point::raw(o.0, o.1),
        major: Vector::raw(u.0, u.1),
        minor: Vector::raw(v.0, v.1),
    }))
}

fn triangle(x: i32, y: i32) -> NodeKind {
    let mut pb = Path::builder();
    pb.move_to(Point::raw(x, y))
        .line_to(Point::raw(x + 100_000, y))
        .cubic_to(
            Point::raw(x + 120_000, y + 30_000),
            Point::raw(x + 80_000, y + 90_000),
            Point::raw(x + 50_000, y + 100_000),
        )
        .close();
    NodeKind::Path(Box::new(PathNode::new(pb.build())))
}

fn baggage() -> ForeignBaggage {
    ForeignBaggage {
        attrs: vec![
            ForeignAttr {
                ns: Arc::from("urn:test:future"),
                prefix: Some(Arc::from("fut")),
                local: Arc::from("review-state"),
                value: Arc::from("approved & <done>"),
            },
            ForeignAttr {
                ns: Arc::from("https://xarast.org/ns/document/1.0"),
                prefix: Some(Arc::from("xarast")),
                local: Arc::from("mesh-warp"),
                value: Arc::from("3 .5 .2"),
            },
        ],
        children: vec![
            ForeignChild {
                position: 0,
                kind: ForeignChildKind::Element,
                raw: Arc::from(
                    "<fut:note xmlns:fut=\"urn:test:future\" v='1'>keep <b>this</b></fut:note>",
                ),
            },
            ForeignChild {
                position: 0,
                kind: ForeignChildKind::Comment,
                raw: Arc::from("<!-- a person's note -->"),
            },
            ForeignChild {
                position: 0,
                kind: ForeignChildKind::ProcessingInstruction,
                raw: Arc::from("<?tool keep me?>"),
            },
        ],
        marks: ForeignMarks::DIRTY,
    }
}

/// A document touching every path of the reader: palette colours (a tint
/// among them), every shape spelling, gradients with and without twins, a
/// conical twin, flat, graduated and twin transparencies, strokes with
/// dashes, a bitmap and a bitmap fill, text, a group, a ClipView, a live
/// effect with generated geometry, a guide layer, flags, names and foreign
/// baggage on a container and on a leaf.
fn fixture() -> Document {
    let mut b = xarast_doc::builder::skeleton(BuildLimits::default()).unwrap();
    let red = b.define_colour(ColourDef::normal(ColourValue::rgb(0.8, 0.1, 0.1)).named("Red"));
    let mut tint = ColourDef::normal(ColourValue::rgb(0.8, 0.1, 0.1)).named("Red tint");
    tint.kind = ColourKind::Tint { factor: 0.3 };
    tint.model = ColourModel::Rgbt;
    tint.components = [None, None, None, None];
    let tint = b.define_colour(tint);
    b.colour_parent(tint, Some(red));
    let png: Arc<[u8]> = Arc::from(&b"\x89PNG\r\n\x1a\nnot really a png"[..]);
    let bitmap = b.define_bitmap(BitmapResource {
        name: Arc::from(""),
        info: BitmapInfo::default(),
        pixels: Arc::new(BitmapData::default()),
        original: Some(Arc::new(OriginalEncoded {
            format: ImageFormat::Png,
            bytes: Arc::clone(&png),
        })),
        procedural: None,
        transparent_index: None,
    });

    // Palette fills and a stroke.
    b.node(shape(
        ShapeKind::Rect,
        (50_000, 50_000),
        (100_000, 0),
        (0, 60_000),
    ))
    .unwrap();
    b.push_scope().unwrap();
    b.attribute(flat(Colour::Indexed {
        id: red,
        tint: None,
    }))
    .unwrap();
    b.attribute(AttrValue::StrokeColour(Paint::Flat {
        value: Colour::Indexed {
            id: tint,
            tint: None,
        },
    }))
    .unwrap();
    b.attribute(AttrValue::LineWidth(Mp::new(3_000))).unwrap();
    b.attribute(AttrValue::DashPattern(Arc::new(xarast_geom::DashPattern {
        elements: vec![Mp::new(4_000), Mp::new(2_000)],
        offset: Mp::new(1_000),
        reference_width: None,
    })))
    .unwrap();
    b.attribute(AttrValue::ObjectName(Arc::from("first")))
        .unwrap();
    b.attribute(AttrValue::ObjectName(Arc::from("second name")))
        .unwrap();
    b.pop_scope();
    // A circle, an odd ellipse (a path twin) and a rotated rectangle.
    b.node(shape(
        ShapeKind::Ellipse,
        (200_000, 50_000),
        (60_000, 0),
        (0, 60_000),
    ))
    .unwrap();
    b.node(shape(
        ShapeKind::Ellipse,
        (300_000, 50_000),
        (61_001, 0),
        (0, 40_000),
    ))
    .unwrap();
    b.node(shape(
        ShapeKind::Rect,
        (400_000, 50_000),
        (60_000, 30_000),
        (-20_000, 40_000),
    ))
    .unwrap();
    // A linear gradient with a profile (twin) and a radial circle.
    b.node(triangle(50_000, 200_000)).unwrap();
    b.push_scope().unwrap();
    let mut ramp = Ramp::new();
    ramp.profile = BiasGain::new(0.3, -0.2);
    ramp.insert(RampStop {
        pos: 0.4,
        value: rgb(0.1, 0.9, 0.1),
    });
    b.attribute(AttrValue::Fill(FillGeometry::Linear {
        start: Point::raw(50_000, 200_000),
        end: Point::raw(150_000, 250_000),
        persp: None,
        from: rgb(1.0, 0.0, 0.0),
        to: rgb(0.0, 0.0, 1.0),
        ramp,
    }))
    .unwrap();
    b.pop_scope();
    b.node(triangle(200_000, 200_000)).unwrap();
    b.push_scope().unwrap();
    b.attribute(AttrValue::Fill(FillGeometry::Radial {
        centre: Point::raw(250_000, 250_000),
        major: Point::raw(290_000, 250_000),
        minor: Point::raw(250_000, 290_000),
        aspect_locked: true,
        persp: None,
        from: rgb(1.0, 1.0, 0.0),
        to: rgb(0.0, 0.5, 0.5),
        ramp: Ramp::new(),
    }))
    .unwrap();
    b.attribute(AttrValue::TranspFill(TranspPaint::Flat {
        value: Transparency {
            level: 100,
            mode: TranspMode::StainedGlass,
        },
    }))
    .unwrap();
    b.pop_scope();
    // A conical fill (twin) with a graduated transparency (a mask).
    b.node(triangle(350_000, 200_000)).unwrap();
    b.push_scope().unwrap();
    b.attribute(AttrValue::Fill(FillGeometry::Conical {
        centre: Point::raw(400_000, 250_000),
        zero_dir: Point::raw(450_000, 250_000),
        from: rgb(0.2, 0.2, 0.9),
        to: rgb(0.9, 0.9, 0.2),
        ramp: Ramp::new(),
    }))
    .unwrap();
    b.attribute(AttrValue::TranspFill(TranspPaint::Linear {
        start: Point::raw(350_000, 200_000),
        end: Point::raw(450_000, 200_000),
        persp: None,
        from: Transparency::mix(0),
        to: Transparency::mix(200),
        ramp: Ramp::new(),
    }))
    .unwrap();
    b.pop_scope();
    // A bitmap and a bitmap-filled path.
    b.node(NodeKind::Bitmap(Box::new(BitmapNode {
        image: bitmap,
        origin: Point::raw(50_000, 500_000),
        major: Vector::raw(100_000, 0),
        minor: Vector::raw(0, -80_000),
    })))
    .unwrap();
    b.node(triangle(200_000, 400_000)).unwrap();
    b.push_scope().unwrap();
    b.attribute(AttrValue::Fill(FillGeometry::Bitmap {
        image: bitmap,
        origin: Point::raw(200_000, 400_000),
        axis_x: Point::raw(260_000, 400_000),
        axis_y: Point::raw(200_000, 460_000),
        persp: None,
        tiling: xarast_doc::fill::Tiling::Repeat,
        dpi: 96,
        contone: None,
        profile: BiasGain::IDENTITY,
    }))
    .unwrap();
    b.pop_scope();
    // A named, soft group holding a locked path with baggage.
    let g = b
        .node(NodeKind::Group(Box::new(GroupNode {
            name: Some(Arc::from("The group")),
            soft: true,
        })))
        .unwrap();
    b.push_scope().unwrap();
    b.attribute(flat(rgb(0.1, 0.5, 0.9))).unwrap();
    let p = b.node(triangle(400_000, 400_000)).unwrap();
    b.flags(p, NodeFlags::LOCKED | NodeFlags::MAGNETIC);
    b.foreign(p, baggage());
    b.node(triangle(450_000, 450_000)).unwrap();
    b.pop_scope();
    b.foreign(
        g,
        ForeignBaggage {
            attrs: Vec::new(),
            children: vec![ForeignChild {
                position: 1,
                kind: ForeignChildKind::Element,
                raw: Arc::from("<fut:between xmlns:fut=\"urn:test:future\"/>"),
            }],
            marks: ForeignMarks::STALE,
        },
    );
    // A ClipView, clipping outside.
    b.node(NodeKind::ClipView(ClipViewNode {
        mode: ClipViewMode::Outside,
    }))
    .unwrap();
    b.push_scope().unwrap();
    b.node(shape(
        ShapeKind::Rect,
        (50_000, 600_000),
        (80_000, 0),
        (0, 80_000),
    ))
    .unwrap();
    b.node(triangle(60_000, 610_000)).unwrap();
    b.pop_scope();
    // A blend with its sources and its baked steps.
    let blend = LiveKind::Blend(Box::new(xarast_doc::BlendParams {
        steps: 3,
        step_distance: None,
        one_to_one: false,
        antialias: true,
        tangential: false,
        reverse: false,
        profile: BiasGain::IDENTITY,
    }));
    b.node(NodeKind::Live(Box::new(LiveNode {
        role: LiveRole::Controller,
        kind: blend.clone(),
        regen: RegenState::Clean,
        name: Some(Arc::from("Blend")),
    })))
    .unwrap();
    b.push_scope().unwrap();
    b.node(triangle(300_000, 600_000)).unwrap();
    b.node(NodeKind::Live(Box::new(LiveNode {
        role: LiveRole::Generated,
        kind: blend,
        regen: RegenState::Clean,
        name: None,
    })))
    .unwrap();
    b.push_scope().unwrap();
    b.node(triangle(320_000, 620_000)).unwrap();
    b.pop_scope();
    b.node(triangle(340_000, 640_000)).unwrap();
    b.pop_scope();
    // Text: two lines, two styles.
    b.node(NodeKind::TextStory(Box::new(TextStoryNode {
        transform: Matrix {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: Mp::new(100_000),
            f: Mp::new(750_000),
        },
        ..TextStoryNode::default()
    })))
    .unwrap();
    b.push_scope().unwrap();
    for (i, word) in ["Hello", "World"].iter().enumerate() {
        b.node(NodeKind::TextLine(Box::default())).unwrap();
        b.push_scope().unwrap();
        b.attribute(AttrValue::FontSize(Mp::new(18_000))).unwrap();
        if i == 1 {
            b.attribute(AttrValue::Bold(true)).unwrap();
            b.attribute(flat(rgb(0.8, 0.0, 0.0))).unwrap();
        }
        for ch in word.chars() {
            b.node(NodeKind::TextItem(TextItem::Char(ch))).unwrap();
        }
        b.pop_scope();
    }
    b.pop_scope();
    // Something from a newer producer.
    b.node(NodeKind::Opaque(Box::new(OpaqueNode {
        tag: 9999,
        payload: Arc::from(&b"\x00\x01binary"[..]),
    })))
    .unwrap();
    // …keeping the objects of its own subtree (XARA-T-0108).
    b.push_scope().unwrap();
    b.node(triangle(450_000, 450_000)).unwrap();
    b.push_scope().unwrap();
    b.attribute(flat(rgb(0.2, 0.4, 0.6))).unwrap();
    b.pop_scope();
    b.node(NodeKind::Group(Box::default())).unwrap();
    b.push_scope().unwrap();
    b.node(triangle(470_000, 470_000)).unwrap();
    b.pop_scope();
    b.pop_scope();
    // A guide layer with a guideline in a palette colour.
    b.pop_scope();
    b.node(NodeKind::Layer(Box::new(LayerNode {
        name: Arc::from("Guides"),
        guide: true,
        active: false,
        guide_colour: Some(red),
        ..LayerNode::default()
    })))
    .unwrap();
    b.push_scope().unwrap();
    b.node(NodeKind::Guideline(Box::new(GuidelineNode {
        horizontal: true,
        position: Mp::new(300_000),
        colour: Some(tint),
    })))
    .unwrap();
    b.pop_scope();
    let mut doc = b.finish().unwrap().0;
    doc.meta.title = Some("Fixture & <title>".into());
    doc.meta.created = Some(1_700_000_000);
    doc.meta.modified = Some(1_700_000_100);
    doc.meta.comment = Some("a comment".into());
    doc
}

fn deterministic() -> SaveOptions {
    SaveOptions {
        write: WriteOptions::deterministic(),
        ..SaveOptions::default()
    }
}

fn package(doc: &Document, svg: SvgOptions) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    let opts = SaveOptions {
        svg,
        ..deterministic()
    };
    save_to(doc, &mut out, &opts).expect("save");
    out.into_inner()
}

fn open(bytes: &[u8]) -> xarast_format::OpenedDocument<Cursor<Vec<u8>>> {
    open_reader(Cursor::new(bytes.to_vec()), &OpenOptions::default()).expect("open")
}

fn resave(o: &mut xarast_format::OpenedDocument<Cursor<Vec<u8>>>) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    save_opened_to(&o.document, &mut o.package, &mut out, &deterministic()).expect("re-save");
    out.into_inner()
}

/// Asserts two texts are equal, showing where they first differ.
fn same_text(a: &str, b: &str) {
    if a == b {
        return;
    }
    for (i, (x, y)) in a.lines().zip(b.lines()).enumerate() {
        assert_eq!(x, y, "line {i} differs");
    }
    panic!(
        "line counts differ: {} vs {}",
        a.lines().count(),
        b.lines().count()
    );
}

fn svg_of(bytes: &[u8]) -> String {
    let mut r = XarastReader::open(Cursor::new(bytes.to_vec())).unwrap();
    String::from_utf8(r.document_bytes().unwrap()).unwrap()
}

#[test]
fn the_fixture_reads_back_to_the_same_normal_form_and_bytes() {
    let doc = fixture();
    let first = package(&doc, SvgOptions::default());
    let mut o = open(&first);
    assert!(o.diagnostics.is_empty(), "{:?}", o.diagnostics);
    assert!(o.preservation.intact(), "{:?}", o.preservation);
    let (a, b) = (normal_form(&doc), normal_form(&o.document));
    for (x, y) in a.lines().zip(b.lines()) {
        assert_eq!(x, y);
    }
    assert_eq!(a, b);
    let second = resave(&mut o);
    same_text(&svg_of(&first), &svg_of(&second));
    assert_eq!(first, second, "the first re-save is a fixed point");
    // The meta survives through meta.xml.
    assert_eq!(o.document.meta.title.as_deref(), Some("Fixture & <title>"));
    assert_eq!(o.document.meta.created, Some(1_700_000_000));
    assert_eq!(o.document.meta.comment.as_deref(), Some("a comment"));
}

#[test]
fn an_opaque_node_keeps_its_subtree_where_a_browser_does_not_draw_it() {
    let doc = fixture();
    let svg = svg_of(&package(&doc, SvgOptions::default()));
    let start = svg.find("<xarast:opaque").expect("written");
    let end = svg[start..].find("</xarast:opaque>").expect("closed") + start;
    let inside = &svg[start..end];
    // The payload first, then the subtree: two paths, one in a group.
    assert!(inside.contains(">AAFiaW5hcnk=\n"), "{inside}");
    assert_eq!(inside.matches("<path").count(), 2, "{inside}");
    assert!(inside.contains("<g "), "{inside}");
    let o = open(&package(&doc, SvgOptions::default()));
    let opaque = o
        .document
        .tree
        .preorder(o.document.tree.root())
        .find(|n| matches!(o.document.tree.kind(*n), Some(NodeKind::Opaque(_))))
        .expect("read");
    let under = o
        .document
        .tree
        .preorder(opaque)
        .filter(|n| !matches!(o.document.tree.kind(*n), Some(NodeKind::Attr(_))))
        .count();
    // The opaque node, two paths and the group.
    assert_eq!(under, 4);
}

#[test]
fn key_stops_finer_than_8_bits_are_a_fixed_point_from_the_first_re_save() {
    // A profiled ramp's key stops are written as 8-bit sRGB; the baked
    // stops are sampled from the key as written (XARA-T-0110), so the
    // reload re-derives the same bytes.
    let mut b = xarast_doc::builder::skeleton(BuildLimits::default()).unwrap();
    b.node(triangle(50_000, 50_000)).unwrap();
    b.push_scope().unwrap();
    let mut ramp = Ramp::new();
    ramp.profile = BiasGain::new(0.3, -0.2);
    b.attribute(AttrValue::Fill(FillGeometry::Linear {
        start: Point::raw(50_000, 50_000),
        end: Point::raw(150_000, 100_000),
        persp: None,
        from: Colour::Direct(ColourValue::rgb(0.1, 0.9, 0.1)),
        to: Colour::Direct(ColourValue::rgb(0.33, 0.21, 0.77)),
        ramp,
    }))
    .unwrap();
    b.pop_scope();
    let doc = b.finish().unwrap().0;
    let first = package(&doc, SvgOptions::default());
    let mut o = open(&first);
    assert_eq!(normal_form(&o.document), normal_form(&doc));
    let second = resave(&mut o);
    let mut o2 = open(&second);
    assert_eq!(resave(&mut o2), second);
}

#[test]
fn hoisted_paint_and_classes_read_like_inline_paint() {
    // XARA-T-0107: passes 4 and 5 on and off give the same model.
    let doc = fixture();
    let plain = open(&package(
        &doc,
        SvgOptions {
            hoist: false,
            classes: false,
            ..SvgOptions::default()
        },
    ));
    let styled = open(&package(&doc, SvgOptions::default()));
    assert_eq!(normal_form(&plain.document), normal_form(&styled.document));
    assert_eq!(normal_form(&plain.document), normal_form(&doc));
}

/// The resolved fill of the `n`th path of a document, as 8-bit sRGB.
fn fill_of(doc: &Document, n: usize) -> Option<Rgba8> {
    let id = doc
        .tree
        .preorder(doc.tree.root())
        .filter(|i| {
            matches!(
                doc.tree.kind(*i),
                Some(NodeKind::Path(_) | NodeKind::Shape(_))
            )
        })
        .nth(n)?;
    let r = resolve_uncached(&doc.tree, id, &doc.defaults);
    match r.get(AttrSlot::FillGeometry) {
        AttrValue::Fill(FillGeometry::Flat { value }) => {
            Some(value.resolve(&doc.resources.colours).to_rgba8())
        }
        _ => None,
    }
}

fn read(svg: &str) -> xarast_format::svg::SvgRead {
    let mut none = |_: &str| None;
    read_svg(svg.as_bytes(), &ReadOptions::default(), &mut none).expect("reads")
}

const HEAD: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xarast="https://xarast.org/ns/document/1.0" viewBox="0 0 100 100">"##;

fn rgb8(r: u8, g: u8, b: u8) -> Rgba8 {
    Rgba8 { r, g, b, a: 255 }
}

#[test]
fn paint_inherits_through_two_groups_and_an_own_value_wins() {
    let svg = format!(
        r##"{HEAD}<g fill="#f00"><g stroke="#00f"><path d="M0 0h10v10z"/><path d="M0 0h10v10z" fill="#0f0"/></g></g></svg>"##
    );
    let r = read(&svg);
    assert_eq!(fill_of(&r.document, 0), Some(rgb8(255, 0, 0)));
    assert_eq!(fill_of(&r.document, 1), Some(rgb8(0, 255, 0)));
    // Hoisted paint is not an attribute of the group: only the leaves.
    let groups_with_attrs = r
        .document
        .tree
        .preorder(r.document.tree.root())
        .filter(|g| matches!(r.document.tree.kind(*g), Some(NodeKind::Group(_))))
        .filter(|g| {
            r.document
                .tree
                .children(*g)
                .any(|c| matches!(r.document.tree.kind(c), Some(NodeKind::Attr(_))))
        })
        .count();
    assert_eq!(groups_with_attrs, 0);
}

fn palette_svg(body: &str, style: &str) -> String {
    format!(
        r##"{HEAD}<defs>{style}<xarast:palette xarast:id="doc"><xarast:colour xarast:id="c-1" xarast:model="rgb" xarast:components="1 0 0 0" xarast:srgb="#f00"/><xarast:colour xarast:id="c-2" xarast:model="rgb" xarast:components="0 0 1 0" xarast:srgb="#00f"/></xarast:palette></defs>{body}</svg>"##
    )
}

fn is_palette_ref(doc: &Document, n: usize) -> bool {
    let id = doc
        .tree
        .preorder(doc.tree.root())
        .filter(|i| matches!(doc.tree.kind(*i), Some(NodeKind::Path(_))))
        .nth(n)
        .unwrap();
    let r = resolve_uncached(&doc.tree, id, &doc.defaults);
    matches!(
        r.get(AttrSlot::FillGeometry),
        AttrValue::Fill(FillGeometry::Flat {
            value: Colour::Indexed { .. }
        })
    )
}

#[test]
fn a_palette_twin_inherits_from_a_group() {
    let svg = palette_svg(
        r##"<g fill="#f00" xarast:fill-ref="#c-1"><path d="M0 0h1v1z"/><path d="M0 0h1v1z" fill="#0f0"/></g>"##,
        "",
    );
    let r = read(&svg);
    assert!(is_palette_ref(&r.document, 0));
    // A different own colour does not take the inherited reference.
    assert!(!is_palette_ref(&r.document, 1));
    assert_eq!(fill_of(&r.document, 1), Some(rgb8(0, 255, 0)));
}

#[test]
fn classes_carry_paint_and_their_twins_whatever_the_prefix() {
    for name in ["c1", "xc7", "xarast-c2"] {
        let style = format!(
            r##"<style type="text/css">.{name}{{fill:#00f;stroke:#f00;stroke-width:2}}</style><xarast:paint-class xarast:class="{name}" xarast:fill-ref="#c-2"/>"##
        );
        let svg = palette_svg(
            &format!(r##"<path d="M0 0h1v1z" class="{name}"/>"##),
            &style,
        );
        let r = read(&svg);
        assert!(is_palette_ref(&r.document, 0), "{name}");
        assert_eq!(fill_of(&r.document, 0), Some(rgb8(0, 0, 255)), "{name}");
        let id = r
            .document
            .tree
            .preorder(r.document.tree.root())
            .find(|i| matches!(r.document.tree.kind(*i), Some(NodeKind::Path(_))))
            .unwrap();
        let a = resolve_uncached(&r.document.tree, id, &r.document.defaults);
        assert_eq!(
            a.get(AttrSlot::LineWidth),
            &AttrValue::LineWidth(Mp::new(2_000))
        );
        // Our class is consumed, not kept as foreign data.
        assert!(r.document.tree.foreign(id).is_none(), "{name}");
    }
}

#[test]
fn a_foreign_class_is_kept() {
    let svg = format!(r##"{HEAD}<path d="M0 0h1v1z" class="inkscape-thing"/></svg>"##);
    let r = read(&svg);
    let id = r
        .document
        .tree
        .preorder(r.document.tree.root())
        .find(|i| matches!(r.document.tree.kind(*i), Some(NodeKind::Path(_))))
        .unwrap();
    let bag = r.document.tree.foreign(id).expect("baggage");
    assert!(
        bag.attrs
            .iter()
            .any(|a| &*a.local == "class" && &*a.value == "inkscape-thing")
    );
}

/// Injects foreign data the way another editor would: namespaced
/// attributes, an unknown element, a comment and a PI on a path.
fn edited_by_hand(svg: &str) -> String {
    let svg = svg.replacen("<svg", "<svg xmlns:acme=\"urn:acme\"", 1);
    // The first path element after the header.
    let at = svg.find("<path id=").expect("a path");
    let end = at + svg[at..].find('>').unwrap();
    let self_closing = svg[..end].ends_with('/');
    let tag_end = if self_closing { end - 1 } else { end };
    let mut out = String::new();
    out.push_str(&svg[..tag_end]);
    out.push_str(" acme:state=\"reviewed\" sodipodi:nodetypes=\"cccc\"");
    if self_closing {
        out.push_str(
            "><!-- hand note --><acme:meta a=\"1\"><acme:x/></acme:meta><?acme pi?></path>",
        );
    } else {
        out.push_str("><!-- hand note --><acme:meta a=\"1\"><acme:x/></acme:meta><?acme pi?>");
    }
    out.push_str(&svg[end + 1..]);
    out
}

fn repack(original: &[u8], svg: &str) -> Vec<u8> {
    let mut r = XarastReader::open(Cursor::new(original.to_vec())).unwrap();
    let mut res = ResourceIndex::from_package(&r);
    let meta = r.meta_bytes().unwrap();
    let mut w = PackageWriter::new(WriteOptions::deterministic());
    w.set_document(svg.as_bytes().to_vec());
    w.set_meta(meta);
    res.begin_recount();
    for rec in res.records().map(|r| r.path()).collect::<Vec<_>>() {
        res.count_path(&rec);
    }
    w.add_resources(&res);
    w.carry_from(&r);
    let mut out = Cursor::new(Vec::new());
    w.finish_with_source(&mut out, Some(&mut r)).unwrap();
    out.into_inner()
}

#[test]
fn foreign_data_added_by_another_editor_survives_in_place() {
    let doc = fixture();
    let first = package(&doc, SvgOptions::default());
    let edited = edited_by_hand(&svg_of(&first));
    let pkg = repack(&first, &edited);
    let mut o = open(&pkg);
    // Something was added: not a loss.
    assert_eq!(o.preservation.lost(), 0);
    let second = resave(&mut o);
    let svg2 = svg_of(&second);
    for needle in [
        "acme:state=\"reviewed\"",
        "sodipodi:nodetypes=\"cccc\"",
        "<!-- hand note -->",
        "<acme:meta xmlns:acme=\"urn:acme\" a=\"1\"><acme:x/></acme:meta>",
        "<?acme pi?>",
        "fut:review-state=\"approved &amp; &lt;done&gt;\"",
        "<fut:between xmlns:fut=\"urn:test:future\"/>",
    ] {
        assert!(svg2.contains(needle), "{needle} lost");
    }
    // In place: the fragments are still inside the same element.
    let at = svg2.find("acme:state").unwrap();
    let open_end = at + svg2[at..].find('>').unwrap();
    let close = open_end + svg2[open_end..].find("</path>").unwrap();
    assert!(svg2[open_end..close].contains("<!-- hand note -->"));
    // And the next save is a fixed point.
    let mut o2 = open(&second);
    assert!(o2.preservation.intact(), "{:?}", o2.preservation);
    assert_eq!(resave(&mut o2), second);
}

#[test]
fn the_digest_notices_foreign_data_another_program_dropped() {
    let doc = fixture();
    let first = package(&doc, SvgOptions::default());
    let svg = svg_of(&first);
    assert!(svg.contains("xarast:foreign-count"));
    let stripped = svg.replacen(" fut:review-state=\"approved &amp; &lt;done&gt;\"", "", 1);
    assert_ne!(stripped, svg);
    let o = open(&repack(&first, &stripped));
    assert_eq!(o.preservation.lost(), 1);
    assert!(
        o.diagnostics
            .iter()
            .any(|d| d.message.contains("1 item(s) of data")),
        "{:?}",
        o.diagnostics
    );
}

#[test]
fn active_content_is_stripped() {
    let svg = format!(
        r##"{HEAD}<script>alert(1)</script><path d="M0 0h1v1z" onclick="evil()"><acme:x xmlns:acme="urn:a"><script/></acme:x></path><foreignObject/><a href="javascript:x()"/></svg>"##
    );
    let r = read(&svg);
    let text = normal_form(&r.document);
    assert!(!text.contains("script") && !text.contains("onclick") && !text.contains("javascript"));
    assert!(r.stats.stripped >= 4, "{:?}", r.stats);
}

#[test]
fn hostile_xml_is_refused() {
    let mut none = |_: &str| None;
    let opts = ReadOptions::fuzz();
    for bad in [
        "<!DOCTYPE svg [<!ENTITY x SYSTEM \"file:///etc/passwd\">]><svg xmlns=\"http://www.w3.org/2000/svg\">&x;</svg>",
        "<svg xmlns=\"http://www.w3.org/2000/svg\">&lol;</svg>",
        "<html/>",
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><g></svg>",
    ] {
        assert!(read_svg(bad.as_bytes(), &opts, &mut none).is_err(), "{bad}");
    }
    let deep = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\">{}{}</svg>",
        "<g>".repeat(300),
        "</g>".repeat(300)
    );
    assert!(read_svg(deep.as_bytes(), &ReadOptions::default(), &mut none).is_err());
}

#[test]
fn duplicate_ids_are_reassigned_with_a_warning() {
    let svg = format!(
        r##"{HEAD}<g xarast:kind="spread" xarast:origin="0 100" xarast:size="100 100" id="x2"><g xarast:kind="layer" id="x3"><path id="x5" d="M0 0h1v1z"/><path id="x5" d="M0 0h2v2z"/><path id="rect12" d="M0 0h3v3z"/></g></g></svg>"##
    );
    let r = read(&svg);
    assert_eq!(r.stats.ids_reassigned, 2);
    let tags: Vec<u32> = r
        .document
        .tree
        .preorder(r.document.tree.root())
        .filter(|i| matches!(r.document.tree.kind(*i), Some(NodeKind::Path(_))))
        .map(|i| r.document.tree.get(i).unwrap().tag.0)
        .collect();
    assert_eq!(tags.first(), Some(&5));
    let mut unique = tags.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), tags.len());
}

#[test]
fn orphan_baked_geometry_is_kept_as_objects() {
    let svg = format!(
        r##"{HEAD}<g xarast:generated="blend" xarast:generated-by="x99" xarast:base-authoritative="true"><path d="M0 0h1v1z"/><path d="M0 0h2v2z"/></g></svg>"##
    );
    let r = read(&svg);
    assert_eq!(r.stats.generated_orphaned, 1);
    let paths = r
        .document
        .tree
        .preorder(r.document.tree.root())
        .filter(|i| matches!(r.document.tree.kind(*i), Some(NodeKind::Path(_))))
        .count();
    assert_eq!(paths, 2, "never delete art without a replacement");
    assert!(
        r.diagnostics
            .iter()
            .any(|d| d.message.contains("effect itself is lost"))
    );
}

#[test]
fn a_plain_svg_reads_into_one_layer() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="0 0 200 100"><rect x="10" y="10" width="30" height="20" style="fill:#123456"/><g transform="translate(50 0)"><circle cx="20" cy="20" r="10" fill="red"/><path d="M0 50 q 20 -20 40 0 a 10 10 0 0 1 20 0 z" stroke="blue"/></g><polygon points="0,0 10,0 5,8"/></svg>"##;
    let r = read(svg);
    let d = &r.document;
    let layers = d
        .tree
        .preorder(d.tree.root())
        .filter(|i| matches!(d.tree.kind(*i), Some(NodeKind::Layer(_))))
        .count();
    assert_eq!(layers, 1);
    assert_eq!(fill_of(d, 0), Some(rgb8(0x12, 0x34, 0x56)));
    // The circle, moved by its group's transform: a shape at x 60..80.
    let circle = d
        .tree
        .preorder(d.tree.root())
        .filter_map(|i| match d.tree.kind(i) {
            Some(NodeKind::Shape(s)) if s.shape == ShapeKind::Ellipse => Some(s.origin),
            _ => None,
        })
        .next()
        .unwrap();
    assert_eq!(circle.x, Mp::new(60_000));
    assert_eq!(circle.y, Mp::new(100_000 - 30_000));
}

#[test]
fn opening_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("doc.xarast");
    std::fs::write(&path, package(&fixture(), SvgOptions::default())).unwrap();
    let before = std::fs::metadata(&path).unwrap().modified().unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let names = || {
        let mut v: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        v.sort();
        v
    };
    let listing = names();
    {
        let o = xarast_format::open(&path).unwrap();
        drop(o);
    }
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        before
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(names(), listing, "no lock, no temporary");
}

#[test]
fn unknown_entries_and_resources_named_only_by_foreign_data_survive_a_re_save() {
    let doc = fixture();
    let first = package(&doc, SvgOptions::default());
    // Add an unknown entry, an extension, and a resource only a foreign
    // fragment refers to.
    let mut r = XarastReader::open(Cursor::new(first.clone())).unwrap();
    let mut res = ResourceIndex::from_package(&r);
    let extra = res
        .insert(
            xarast_format::ResourceKind::Blob,
            "bin",
            Arc::<[u8]>::from(&b"only foreign"[..]),
        )
        .unwrap();
    let extra_path = res.get(extra).unwrap().path();
    let svg = svg_of(&first).replacen(
        "<fut:between",
        &format!("<fut:ref xmlns:fut=\"urn:test:future\" href=\"{extra_path}\"/><fut:between"),
        1,
    );
    let mut w = PackageWriter::new(WriteOptions::deterministic());
    w.set_document(svg.into_bytes());
    w.set_meta(r.meta_bytes().unwrap());
    w.add_resources(&res);
    w.add_entry(
        "extensions/acme/state.json",
        "application/json",
        xarast_format::Role::Unknown,
        Arc::<[u8]>::from(&b"{}"[..]),
    )
    .unwrap();
    w.add_entry(
        "NOTES.txt",
        "text/plain",
        xarast_format::Role::Unknown,
        Arc::<[u8]>::from(&b"hello"[..]),
    )
    .unwrap();
    w.carry_from(&r);
    let mut out = Cursor::new(Vec::new());
    w.finish_with_source(&mut out, Some(&mut r)).unwrap();
    let pkg = out.into_inner();

    let mut o = open(&pkg);
    let second = resave(&mut o);
    let mut r2 = XarastReader::open(Cursor::new(second)).unwrap();
    assert_eq!(r2.entry("extensions/acme/state.json").unwrap(), b"{}");
    assert_eq!(r2.entry("NOTES.txt").unwrap(), b"hello");
    assert_eq!(r2.entry(&extra_path).unwrap(), b"only foreign");
}
