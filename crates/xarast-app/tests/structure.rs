//! Structure operations (XARA-US-0035): group and ungroup preserving
//! appearance, z-order, align and distribute, duplicate, and the
//! clipboard round trip — each undone exactly (canonical digest).

use std::sync::Arc;

use xarast_app::headless::{self, HeadlessFrame, HeadlessOptions};
use xarast_app::structure::{AlignSpec, AlignTarget, AxisAlign, ZOrder};
use xarast_app::{AppState, DeviceSize, Intent, PlatformRequest, SelectMode, Session};
use xarast_color::{Colour, ColourValue};
use xarast_doc::{
    Attach, AttrNode, AttrSlot, AttrValue, Command, Document, EditError, NodeId, NodeKind,
    ShapeKind, ShapeNode, Tx,
};
use xarast_geom::{Point, Rect, Vector};

fn fill(r: f32, g: f32, b: f32) -> AttrValue {
    AttrValue::Fill(xarast_doc::fill::Paint::Flat {
        value: Colour::Direct(ColourValue::rgbt(r, g, b, 0.0)),
    })
}

/// One item of the fixture: a square, or a loose attribute on the layer.
#[derive(Debug, Clone)]
enum Item {
    Square(i32, i32, i32, Option<AttrValue>),
    Attr(AttrValue),
}

#[derive(Debug)]
struct Build(Vec<Item>);

impl Command for Build {
    fn label(&self) -> &'static str {
        "Fixture"
    }
    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        let layer = tx.doc().active_layer(tx.doc().active_spread()).unwrap();
        for it in &self.0 {
            match it {
                Item::Square(x, y, side, own) => {
                    let n = tx.create(NodeKind::Shape(Box::new(ShapeNode {
                        shape: ShapeKind::Rect,
                        origin: Point::raw(*x, *y),
                        major: Vector::raw(*side, 0),
                        minor: Vector::raw(0, *side),
                    })))?;
                    tx.attach(n, layer, Attach::LastChild)?;
                    if let Some(v) = own {
                        let a = tx.create(NodeKind::Attr(Box::new(AttrNode::new(v.clone()))))?;
                        tx.attach(a, n, Attach::LastChild)?;
                    }
                }
                Item::Attr(v) => {
                    let a = tx.create(NodeKind::Attr(Box::new(AttrNode::new(v.clone()))))?;
                    tx.attach(a, layer, Attach::LastChild)?;
                }
            }
        }
        Ok(())
    }
}

/// On the active layer: a loose red fill, A, a loose blue fill, B, C with
/// its own green fill, D. A inherits red, B and D blue.
fn fixture() -> (Session, [NodeId; 4]) {
    let mut s = Session::new_empty(xarast_app::DocumentId(1));
    s.dispatch(&Build(vec![
        Item::Attr(fill(0.9, 0.1, 0.1)),
        Item::Square(0, 0, 100_000, None),
        Item::Attr(fill(0.1, 0.1, 0.9)),
        Item::Square(150_000, 0, 100_000, None),
        Item::Square(300_000, 0, 100_000, Some(fill(0.1, 0.8, 0.1))),
        Item::Square(0, 150_000, 100_000, None),
    ]))
    .unwrap();
    s.bus.history_mut().clear(&mut s.doc);
    let objs: Vec<NodeId> = xarast_app::edit::selectable_objects(&s.doc).collect();
    (s, [objs[0], objs[1], objs[2], objs[3]])
}

fn pixels(s: &Session) -> Vec<u8> {
    let r = headless::render(
        s,
        &HeadlessOptions {
            size: DeviceSize::new(240, 180),
            frame: HeadlessFrame::Fit(Rect::new(
                Point::raw(-20_000, -20_000),
                Point::raw(420_000, 270_000),
            )),
            ..HeadlessOptions::default()
        },
    )
    .unwrap();
    r.surface.data().to_vec()
}

/// The session's incrementally kept pick index equals a fresh one.
fn index_fresh(s: &Session) {
    assert_eq!(
        s.picker().dump(&s.doc),
        xarast_app::tool::Picker::new().dump(&s.doc)
    );
}

fn select(s: &mut Session, nodes: &[NodeId]) {
    s.apply(Intent::Select {
        nodes: nodes.to_vec(),
        mode: SelectMode::Replace,
    })
    .unwrap();
}

fn objects(s: &Session) -> Vec<NodeId> {
    xarast_app::edit::selectable_objects(&s.doc).collect()
}

fn resolved_fill(doc: &Document, n: NodeId) -> AttrValue {
    xarast_doc::attr::resolve_uncached(&doc.tree, n, &doc.defaults)
        .get(AttrSlot::FillGeometry)
        .clone()
}

#[test]
fn group_then_ungroup_preserves_appearance_and_order_and_undoes_exactly() {
    let (mut s, [a, b, c, d]) = fixture();
    let digest = s.doc.canonical_digest();
    let before = pixels(&s);
    index_fresh(&s);
    let built = s.picker().rebuilds();
    // A and B straddle the loose blue fill: grouping moves A past it.
    select(&mut s, &[b, a]);
    s.apply(Intent::Group).unwrap();
    assert_eq!(s.undo_label(), Some("Group"));
    let g = s.edit.selection().next().unwrap();
    assert!(matches!(s.doc.tree.kind(g), Some(NodeKind::Group(_))));
    assert_eq!(s.doc.tree.links(a).parent, Some(g));
    assert_eq!(resolved_fill(&s.doc, a), fill(0.9, 0.1, 0.1), "A keeps red");
    assert_eq!(
        resolved_fill(&s.doc, b),
        fill(0.1, 0.1, 0.9),
        "B keeps blue"
    );
    assert_eq!(objects(&s), vec![g, c, d]);
    assert_eq!(pixels(&s), before, "grouping changed no pixel");
    index_fresh(&s);
    s.apply(Intent::Ungroup).unwrap();
    index_fresh(&s);
    assert_eq!(objects(&s), vec![a, b, c, d], "z-order restored");
    assert_eq!(pixels(&s), before, "ungrouping changed no pixel");
    let sel: Vec<NodeId> = s.edit.selection().collect();
    assert_eq!(sel, vec![a, b]);
    s.undo();
    s.undo();
    assert_eq!(s.doc.canonical_digest(), digest);
    s.redo();
    s.redo();
    assert_eq!(pixels(&s), before);
    index_fresh(&s);
    assert_eq!(
        s.picker().rebuilds(),
        built,
        "group and ungroup were incremental"
    );
}

#[test]
fn a_common_fill_is_factored_onto_the_group() {
    let mut s = Session::new_empty(xarast_app::DocumentId(1));
    let red = fill(0.9, 0.1, 0.1);
    s.dispatch(&Build(vec![
        Item::Square(0, 0, 50_000, Some(red.clone())),
        Item::Square(60_000, 0, 50_000, Some(red.clone())),
    ]))
    .unwrap();
    let objs = objects(&s);
    select(&mut s, &objs);
    s.apply(Intent::Group).unwrap();
    let g = s.edit.selection().next().unwrap();
    let first = s.doc.tree.children(g).next().unwrap();
    assert!(
        matches!(s.doc.tree.kind(first), Some(NodeKind::Attr(x)) if x.value == red),
        "the shared fill moved up"
    );
    for o in &objs {
        assert!(
            !s.doc
                .tree
                .children(*o)
                .any(|c| matches!(s.doc.tree.kind(c), Some(NodeKind::Attr(_)))),
            "and left the members"
        );
        assert_eq!(resolved_fill(&s.doc, *o), red);
    }
}

#[test]
fn z_order_moves_keep_appearance_and_undo() {
    let (mut s, [a, b, c, d]) = fixture();
    let digest = s.doc.canonical_digest();
    index_fresh(&s);
    select(&mut s, &[a]);
    s.apply(Intent::Arrange(ZOrder::BringToFront)).unwrap();
    assert_eq!(objects(&s), vec![b, c, d, a]);
    index_fresh(&s);
    assert_eq!(resolved_fill(&s.doc, a), fill(0.9, 0.1, 0.1));
    assert_eq!(s.undo_label(), Some("Bring to Front"));
    s.apply(Intent::Arrange(ZOrder::SendBackward)).unwrap();
    assert_eq!(objects(&s), vec![b, c, a, d]);
    s.apply(Intent::Arrange(ZOrder::SendToBack)).unwrap();
    assert_eq!(objects(&s), vec![a, b, c, d]);
    s.apply(Intent::Arrange(ZOrder::BringForward)).unwrap();
    assert_eq!(objects(&s), vec![b, a, c, d]);
    // Already at the back: nothing to do, no undo step.
    select(&mut s, &[b]);
    let steps = s.bus.history().len();
    s.apply(Intent::Arrange(ZOrder::SendToBack)).unwrap();
    assert_eq!(s.bus.history().len(), steps);
    while s.undo().is_some() {}
    assert_eq!(s.doc.canonical_digest(), digest);
}

#[test]
fn layer_up_and_down_move_to_the_next_editable_layer() {
    let (mut s, [a, ..]) = fixture();
    let layer = s.edit.active_layer().unwrap();
    let before = pixels(&s);
    // A second layer above.
    let spread = s.doc.active_spread();
    s.dispatch(&xarast_app::commands::AddLayer {
        spread,
        name: "Top".into(),
    })
    .unwrap();
    let top = s
        .doc
        .tree
        .links(layer)
        .next
        .expect("the new layer is above");
    select(&mut s, &[a]);
    s.apply(Intent::Arrange(ZOrder::LayerUp)).unwrap();
    assert_eq!(s.doc.tree.links(a).parent, Some(top));
    assert_eq!(resolved_fill(&s.doc, a), fill(0.9, 0.1, 0.1));
    s.apply(Intent::Arrange(ZOrder::LayerDown)).unwrap();
    assert_eq!(s.doc.tree.links(a).parent, Some(layer));
    s.undo();
    s.undo();
    assert_eq!(pixels(&s), before);
}

#[test]
fn align_and_distribute_land_exactly() {
    let (mut s, [a, b, c, d]) = fixture();
    let bounds = |s: &Session, n: NodeId| xarast_app::viewport::nodes_rect(&s.doc, [n]);
    select(&mut s, &[a, b, d]);
    s.apply(Intent::Align(AlignSpec {
        x: AxisAlign::None,
        y: AxisAlign::Max,
        to: AlignTarget::Selection,
    }))
    .unwrap();
    assert_eq!(s.undo_label(), Some("Align"));
    let top = bounds(&s, d).hi.y;
    assert_eq!(bounds(&s, a).hi.y, top);
    assert_eq!(bounds(&s, b).hi.y, top);
    s.undo();
    // Distribute centres of A, B, C horizontally after nudging B.
    select(&mut s, &[b]);
    s.apply_edit(xarast_app::EditCommand::translate(
        vec![b],
        Vector::raw(-40_000, 0),
    ))
    .unwrap();
    select(&mut s, &[a, b, c]);
    s.apply(Intent::Align(AlignSpec {
        x: AxisAlign::DistributeCentre,
        y: AxisAlign::None,
        to: AlignTarget::Selection,
    }))
    .unwrap();
    assert_eq!(s.undo_label(), Some("Distribute"));
    let cx = |n| {
        let r = bounds(&s, n);
        (i64::from(r.lo.x.raw()) + i64::from(r.hi.x.raw())) / 2
    };
    assert_eq!(cx(b) - cx(a), cx(c) - cx(b));
    // To the page, first selected: the first stays put.
    let a_box = bounds(&s, a);
    select(&mut s, &[a, c]);
    s.apply(Intent::Align(AlignSpec {
        x: AxisAlign::Min,
        y: AxisAlign::Min,
        to: AlignTarget::FirstSelected,
    }))
    .unwrap();
    assert_eq!(bounds(&s, a), a_box);
    assert_eq!(bounds(&s, c).lo, a_box.lo);
}

#[test]
fn nine_anchor_alignment_to_the_page() {
    let (s0, [a, ..]) = fixture();
    let page = xarast_app::viewport::page_rect(&s0.doc);
    for x in [AxisAlign::Min, AxisAlign::Centre, AxisAlign::Max] {
        for y in [AxisAlign::Min, AxisAlign::Centre, AxisAlign::Max] {
            let (mut s, _) = fixture();
            select(&mut s, &[a]);
            s.apply(Intent::Align(AlignSpec {
                x,
                y,
                to: AlignTarget::Page,
            }))
            .unwrap();
            let r = xarast_app::viewport::nodes_rect(&s.doc, [a]);
            let pick = |how, lo: i32, hi: i32, rlo: i32, rhi: i32| match how {
                AxisAlign::Min => (rlo, lo),
                AxisAlign::Max => (rhi, hi),
                _ => (
                    (rlo + rhi).div_euclid(2),
                    (i64::from(lo) + i64::from(hi)).div_euclid(2) as i32,
                ),
            };
            let (got, want) = pick(
                x,
                page.lo.x.raw(),
                page.hi.x.raw(),
                r.lo.x.raw(),
                r.hi.x.raw(),
            );
            assert!((got - want).abs() <= 1, "{x:?} x: {got} vs {want}");
            let (got, want) = pick(
                y,
                page.lo.y.raw(),
                page.hi.y.raw(),
                r.lo.y.raw(),
                r.hi.y.raw(),
            );
            assert!((got - want).abs() <= 1, "{y:?} y: {got} vs {want}");
        }
    }
}

#[test]
fn duplicate_offsets_selects_and_undoes() {
    let (mut s, [a, b, ..]) = fixture();
    let digest = s.doc.canonical_digest();
    select(&mut s, &[a, b]);
    s.apply(Intent::Duplicate).unwrap();
    assert_eq!(s.undo_label(), Some("Duplicate"));
    let copies: Vec<NodeId> = s.edit.selection().collect();
    assert_eq!(copies.len(), 2);
    let off = xarast_app::structure::DUPLICATE_OFFSET;
    for (orig, copy) in [a, b].iter().zip(&copies) {
        let (ro, rc) = (
            xarast_app::viewport::nodes_rect(&s.doc, [*orig]),
            xarast_app::viewport::nodes_rect(&s.doc, [*copy]),
        );
        assert_eq!(rc.lo, ro.lo + off);
        assert_eq!(resolved_fill(&s.doc, *copy), resolved_fill(&s.doc, *orig));
        assert_eq!(s.doc.tree.links(*orig).next, Some(*copy), "right above");
    }
    s.undo();
    assert_eq!(s.doc.canonical_digest(), digest);
}

fn app_with_fixture() -> (AppState, [NodeId; 4]) {
    let mut app = AppState::new();
    let (s, ids) = fixture();
    // The fixture's session is document 1: keep the counter past it.
    let _ = app.docs.next_id();
    let id = app.docs.insert(s);
    app.active = Some(id);
    (app, ids)
}

/// Geometry and resolved fill of every object, for comparing copies.
fn signature(doc: &Document) -> Vec<(String, AttrValue)> {
    xarast_app::edit::selectable_objects(doc)
        .map(|n| {
            (
                format!("{:?}", doc.tree.kind(n).unwrap()),
                resolved_fill(doc, n),
            )
        })
        .collect()
}

#[test]
fn copy_then_paste_in_place_in_another_document_is_structurally_equal() {
    let (mut app, [a, b, c, _]) = app_with_fixture();
    let s = app.active_mut().unwrap();
    select(s, &[a, b, c]);
    let want: Vec<_> = signature(&app.active().unwrap().doc)
        .into_iter()
        .take(3)
        .collect();
    app.apply(Intent::Copy).unwrap();
    let svg = match app.take_requests().as_slice() {
        [PlatformRequest::SetClipboardText(t)] => t.clone(),
        other => panic!("expected a clipboard write, got {other:?}"),
    };
    assert!(svg.contains("<svg"), "the SVG flavour");
    // A second, empty document.
    app.new_document();
    app.apply(Intent::Paste { in_place: true }).unwrap();
    assert_eq!(
        app.take_requests(),
        vec![PlatformRequest::ReadClipboard { in_place: true }]
    );
    // The shell answers with our own text: the internal fragment is used.
    app.apply(Intent::PasteText {
        text: Some(svg.clone()),
        in_place: true,
    })
    .unwrap();
    let s = app.active().unwrap();
    assert_eq!(signature(&s.doc), want);
    index_fresh(s);
    assert_eq!(s.edit.selection_len(), 3, "the pasted objects are selected");
    assert_eq!(s.undo_label(), Some("Paste"));
    // Undo is exact.
    let empty = Document::new_empty().canonical_digest();
    let s = app.active_mut().unwrap();
    s.undo();
    assert_eq!(s.doc.canonical_digest(), empty);
}

#[test]
fn the_svg_flavour_alone_pastes_the_same_drawing() {
    let (mut app, [a, b, c, _]) = app_with_fixture();
    select(app.active_mut().unwrap(), &[a, b, c]);
    let want: Vec<_> = signature(&app.active().unwrap().doc)
        .into_iter()
        .take(3)
        .collect();
    let frag = app.active().unwrap().copy_selection().unwrap();
    let svg = xarast_app::structure::fragment_svg(&frag);
    // A different process: no internal clipboard, only the text.
    let mut other = AppState::new();
    other.new_document();
    other
        .apply(Intent::PasteText {
            text: Some(svg),
            in_place: true,
        })
        .unwrap();
    let got = signature(&other.active().unwrap().doc);
    assert_eq!(got.len(), 3);
    // The SVG profile may bring a rectangle back as a path: compare the
    // extents and the resolved colour, not the node kind.
    let rects = |doc: &Document| -> Vec<xarast_app::DocRect> {
        xarast_app::edit::selectable_objects(doc)
            .map(|n| xarast_app::viewport::nodes_rect(doc, [n]))
            .collect()
    };
    let src = rects(&frag);
    let dst = rects(&other.active().unwrap().doc);
    assert_eq!(dst, src, "same places, pasted in place");
    // SVG carries 8-bit colour.
    let rgba = |v: &AttrValue| match v {
        AttrValue::Fill(xarast_doc::fill::Paint::Flat { value }) => {
            value.resolve(&xarast_color::ColourTable::new()).to_rgba8()
        }
        other => panic!("not a flat fill: {other:?}"),
    };
    for ((_, gf), (_, wf)) in got.iter().zip(&want) {
        assert_eq!(rgba(gf), rgba(wf), "same fill");
    }
    // Text that is not a drawing pastes nothing and says so.
    let n = other.diagnostics.entries().len();
    other
        .apply(Intent::PasteText {
            text: Some("hello".into()),
            in_place: false,
        })
        .unwrap();
    assert_eq!(other.diagnostics.entries().len(), n + 1);
}

#[test]
fn cut_copies_then_deletes_as_one_step_and_paste_centres_in_the_view() {
    let (mut app, [a, ..]) = app_with_fixture();
    select(app.active_mut().unwrap(), &[a]);
    app.apply(Intent::Cut).unwrap();
    let s = app.active().unwrap();
    assert!(!s.doc.tree.is_reachable(a));
    assert_eq!(s.undo_label(), Some("Cut"));
    app.take_requests();
    // No system clipboard: the internal copy is pasted.
    app.apply(Intent::PasteText {
        text: None,
        in_place: false,
    })
    .unwrap();
    let s = app.active().unwrap();
    let pasted = s.edit.selection().next().unwrap();
    let r = xarast_app::viewport::nodes_rect(&s.doc, [pasted]);
    let view = s.viewport.visible_doc_rect();
    let (c1, c2) = (r.centre(), view.centre());
    assert!((c1.x.raw() - c2.x.raw()).abs() <= 1 && (c1.y.raw() - c2.y.raw()).abs() <= 1);
    let _ = Arc::new(());
}
