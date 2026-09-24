//! Text on a path in the SVG base (T9.5.6, XARA-T-0252): the placer puts
//! each character where the walker draws it along the path and turns it
//! with the path, so browsers, resvg and Inkscape show it on its curve.

use std::path::Path;
use std::sync::Arc;

use xarast_app::fonts::FontService;
use xarast_app::svg_text::SvgTextPlacer;
use xarast_doc::builder::{BuildLimits, skeleton};
use xarast_doc::{
    AttrValue, CharsTransform, NodeKind, TextItem, TextLayout, TextStoryNode, TypefaceRef,
};
use xarast_format::ResourceIndex;
use xarast_format::svg::{Placer, SvgDialect, SvgOptions, write_svg};
use xarast_geom::{Matrix, Mp, Point, Vector};

fn fonts() -> Arc<FontService> {
    FontService::from_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../xarast-text/tests/fonts"))
}

/// "Up" at 10 pt on a path from the story's anchor straight up 200 pt.
fn story_up_a_path(chars: CharsTransform) -> xarast_doc::Document {
    let mut b = skeleton(BuildLimits::default()).unwrap();
    b.node(NodeKind::TextStory(Box::new(TextStoryNode {
        transform: Matrix::translate(Vector::new(Mp::new(100_000), Mp::new(200_000))),
        layout: TextLayout::OnPath {
            reversed: false,
            tangential: true,
            left_indent: Mp::ZERO,
            right_indent: Mp::ZERO,
            chars,
        },
        ..TextStoryNode::default()
    })))
    .unwrap();
    b.push_scope().unwrap();
    b.attribute(AttrValue::FontTypeface(Arc::new(TypefaceRef {
        full_name: Arc::from("Noto Sans"),
        family: Arc::from("Noto Sans"),
        panose: None,
    })))
    .unwrap();
    b.attribute(AttrValue::FontSize(Mp::new(10_000))).unwrap();
    let mut pb = xarast_geom::Path::builder();
    pb.move_to(Point::raw(100_000, 200_000))
        .line_to(Point::raw(100_000, 400_000));
    let mut p = xarast_doc::PathNode::new(pb.build());
    p.filled = false;
    b.node(NodeKind::Path(Box::new(p))).unwrap();
    b.node(NodeKind::TextLine(Box::default())).unwrap();
    b.push_scope().unwrap();
    for c in "Up".chars() {
        b.node(NodeKind::TextItem(TextItem::Char(c))).unwrap();
    }
    b.node(NodeKind::TextItem(TextItem::LineBreak(true)))
        .unwrap();
    b.pop_scope();
    b.pop_scope();
    b.finish().unwrap().0
}

fn export(doc: &xarast_doc::Document) -> xarast_format::svg::SvgDocument {
    let opts = SvgOptions {
        text: Some(Placer(Arc::new(SvgTextPlacer::new(fonts())))),
        dialect: SvgDialect::Interchange,
        ..SvgOptions::default()
    };
    write_svg(doc, &mut ResourceIndex::new(), &opts)
}

/// The run's `x`, `y` and `rotate` lists.
fn lists(svg: &str) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let run = svg
        .split("<tspan")
        .find(|t| t.contains("rotate="))
        .unwrap_or_else(|| panic!("no turned run\n{svg}"));
    let list = |name: &str| -> Vec<f64> {
        let at = run.find(&format!(" {name}=\"")).unwrap() + name.len() + 3;
        let end = at + run[at..].find('"').unwrap();
        run[at..end]
            .split_ascii_whitespace()
            .map(|v| v.parse().unwrap())
            .collect()
    };
    (list("x"), list("y"), list("rotate"))
}

#[test]
fn text_up_a_path_is_placed_on_it_and_turned_a_quarter() {
    let out = export(&story_up_a_path(CharsTransform::default()));
    assert_eq!(out.stats.text_on_path, 0);
    let (xs, ys, turns) = lists(&out.svg);
    assert_eq!((xs.len(), ys.len(), turns.len()), (2, 2, 2), "{}", out.svg);
    // In the story's frame (y down) the path runs from (0, 0) to
    // (0, -200): every glyph origin is on it, climbing, turned -90°.
    for x in &xs {
        assert!(x.abs() < 0.01, "{xs:?}");
    }
    assert!(ys[0] <= 0.0 && ys[1] < ys[0], "{ys:?}");
    assert!(turns.iter().all(|t| (t + 90.0).abs() < 1e-3), "{turns:?}");
}

#[test]
fn reflected_text_on_a_path_stays_straight_and_is_counted() {
    let out = export(&story_up_a_path(CharsTransform {
        reflected: true,
        ..CharsTransform::default()
    }));
    assert_eq!(out.stats.text_on_path, 1);
    assert!(!out.svg.contains("rotate="), "{}", out.svg);
}
