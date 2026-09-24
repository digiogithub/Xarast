//! Bitmap fill editing (XARA-T-0271, T10.3.1/T10.3.2): the fill tool's
//! handles on a bitmap fill, its tiling and resolution fields, driven
//! through intents exactly as the shell drives them.

use xarast_app::shapes::rectangle;
use xarast_app::{
    DevicePoint, DocumentId, EditCommand, HandleShape, InfobarField, InfobarItem, InfobarValue,
    Intent, OverlayShape, PointerButton, PointerSample, Session, ToolAction, ToolId,
};
use xarast_doc::bitmap_fill::bitmap_virtual_points;
use xarast_doc::fill::{FillGeometry, Paint, Tiling};
use xarast_doc::fill_edit::{FillChannel, FillValue, PaintSlot, fill_in_force};
use xarast_doc::{AttrValue, BitmapId, NodeId};
use xarast_geom::{BiasGain, Point};

/// A 4 × 2 picture of four colours per row, at 96 dpi.
fn picture() -> xarast_app::place::ImageToPlace {
    let px: [[u8; 4]; 4] = [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 0, 255],
    ];
    let row: Vec<u8> = px.iter().flatten().copied().collect();
    let mut bottom = row.clone();
    bottom.reverse();
    let rgba = [row, bottom].concat();
    xarast_app::place::image_from_rgba(4, 2, &rgba).unwrap()
}

/// A rectangle 100 × 60 pt centred on (200, 200) pt with a bitmap fill
/// 80 × 40 pt about its centre, selected, the fill tool in force.
fn fixture() -> (Session, NodeId, BitmapId) {
    let mut s = Session::new_empty(DocumentId(7));
    let spread = s.doc.active_spread();
    let layer = s.doc.active_layer(spread).unwrap();
    let image = s.doc.resources.insert_bitmap(picture().resource);
    let fill = FillGeometry::Bitmap {
        image,
        origin: Point::raw(160_000, 180_000),
        axis_x: Point::raw(240_000, 180_000),
        axis_y: Point::raw(160_000, 220_000),
        persp: None,
        tiling: Tiling::None,
        dpi: 96,
        contone: None,
        profile: BiasGain::IDENTITY,
    };
    s.apply_edit(EditCommand::CreateShape {
        layer,
        shape: Box::new(rectangle(Point::raw(200_000, 200_000), 50_000.0, 30_000.0)),
        attrs: vec![AttrValue::Fill(fill)],
    })
    .unwrap();
    s.bus.history_mut().clear(&mut s.doc);
    let n = xarast_app::edit::selectable_objects(&s.doc).next().unwrap();
    s.apply(Intent::Select {
        nodes: vec![n],
        mode: xarast_app::SelectMode::Replace,
    })
    .unwrap();
    s.apply(Intent::ChooseTool(ToolId::Fill)).unwrap();
    (s, n, image)
}

fn fill(s: &Session, n: NodeId) -> Paint {
    match fill_in_force(&s.doc, n, PaintSlot::Fill, FillChannel::Colour) {
        FillValue::Colour(g) => g,
        FillValue::Transparency(_) => unreachable!(),
    }
}

fn corners(g: &Paint) -> (Point, Point, Point) {
    match g {
        FillGeometry::Bitmap {
            origin,
            axis_x,
            axis_y,
            ..
        } => (*origin, *axis_x, *axis_y),
        other => panic!("not a bitmap fill: {other:?}"),
    }
}

fn sample(at: DevicePoint) -> PointerSample {
    PointerSample {
        at,
        pressure: None,
        time_ms: 0,
    }
}

fn press(s: &mut Session, p: Point) {
    let at = s.viewport.doc_to_device(p);
    s.apply(Intent::PointerMove(sample(at))).unwrap();
    s.apply(Intent::PointerDown {
        button: PointerButton::Primary,
        sample: sample(at),
    })
    .unwrap();
}

fn move_to(s: &mut Session, p: Point) {
    let at = s.viewport.doc_to_device(p);
    s.apply(Intent::PointerMove(sample(at))).unwrap();
}

fn release(s: &mut Session, p: Point) {
    let at = s.viewport.doc_to_device(p);
    s.apply(Intent::PointerUp {
        button: PointerButton::Primary,
        sample: sample(at),
    })
    .unwrap();
}

fn drag_open(s: &mut Session, from: Point, to: Point) {
    press(s, from);
    for i in 1..=10 {
        let k = f64::from(i) / 10.0;
        let (x0, y0) = from.to_f64();
        let (x1, y1) = to.to_f64();
        move_to(
            s,
            Point::from_f64_round(x0 + (x1 - x0) * k, y0 + (y1 - y0) * k),
        );
    }
}

/// A headless render at draft quality around the object.
fn pixels(s: &Session) -> Vec<u8> {
    let r = xarast_app::headless::render(
        s,
        &xarast_app::HeadlessOptions {
            size: xarast_app::DeviceSize::new(160, 120),
            frame: xarast_app::HeadlessFrame::Fit(xarast_geom::Rect::new(
                Point::raw(120_000, 140_000),
                Point::raw(280_000, 260_000),
            )),
            quality: xarast_render::RenderQuality::Draft,
            ..xarast_app::HeadlessOptions::default()
        },
    )
    .unwrap();
    r.surface.data().to_vec()
}

fn handles(s: &Session) -> Vec<(Point, HandleShape)> {
    s.overlay()
        .into_iter()
        .filter_map(|o| match o {
            OverlayShape::Handle { at, shape } => Some((at.into(), shape)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_bitmap_fill_shows_its_centre_and_edge_handles() {
    let (s, _, _) = fixture();
    let h = handles(&s);
    let at: Vec<Point> = h.iter().map(|(p, _)| *p).collect();
    assert_eq!(
        at,
        vec![
            Point::raw(200_000, 200_000),
            Point::raw(240_000, 200_000),
            Point::raw(200_000, 220_000),
        ]
    );
    assert!(matches!(h[0].1, HandleShape::FillCentre));
    let arrows = s
        .overlay()
        .iter()
        .filter(|o| matches!(o, OverlayShape::Arrow { .. }))
        .count();
    assert_eq!(arrows, 2, "arms from the centre to both edge handles");
}

#[test]
fn an_edge_drag_previews_then_commits_one_exact_undo_step() {
    let (mut s, n, _) = fixture();
    let digest = s.doc.canonical_digest();
    let before = pixels(&s);
    let from = Point::raw(240_000, 200_000);
    let to = Point::raw(260_000, 210_000);
    drag_open(&mut s, from, to);
    // Nothing is written before the release, but the preview moves.
    assert_eq!(s.doc.canonical_digest(), digest);
    assert!(s.bus.history().is_empty());
    let preview = pixels(&s);
    assert_ne!(preview, before, "the drag previews");
    release(&mut s, to);
    assert_eq!(s.bus.history().len(), 1);
    assert_eq!(s.undo_label(), Some("Move Fill Handle"));
    let committed = pixels(&s);
    assert!(
        preview == committed,
        "the preview is what the commit renders"
    );
    // The centre stayed; the dragged handle landed where it was dropped.
    let (o, x, y) = corners(&fill(&s, n));
    let [c, mx, _] = bitmap_virtual_points(o, x, y);
    assert_eq!(c, Point::raw(200_000, 200_000));
    assert_eq!(mx, to);
    // Undo is exact; redo restores the edit.
    s.apply(Intent::Undo).unwrap();
    assert_eq!(s.doc.canonical_digest(), digest);
    assert_eq!(pixels(&s), before);
    s.apply(Intent::Redo).unwrap();
    assert_eq!(pixels(&s), committed);
}

#[test]
fn the_centre_moves_the_whole_fill_and_esc_leaves_nothing() {
    let (mut s, n, _) = fixture();
    let digest = s.doc.canonical_digest();
    drag_open(
        &mut s,
        Point::raw(200_000, 200_000),
        Point::raw(210_000, 190_000),
    );
    s.apply(Intent::Cancel).unwrap();
    assert_eq!(s.doc.canonical_digest(), digest);
    assert!(s.bus.history().is_empty());

    drag_open(
        &mut s,
        Point::raw(200_000, 200_000),
        Point::raw(210_000, 190_000),
    );
    release(&mut s, Point::raw(210_000, 190_000));
    assert_eq!(
        corners(&fill(&s, n)),
        (
            Point::raw(170_000, 170_000),
            Point::raw(250_000, 170_000),
            Point::raw(170_000, 210_000)
        )
    );
    assert_eq!(s.bus.history().len(), 1);
}

#[test]
fn adjust_locks_the_aspect_of_an_edge_drag() {
    let (mut s, n, _) = fixture();
    // The x-axis handle turned a quarter turn about the centre and
    // stretched to 60 pt: locked, the y axis turns and scales with it.
    let to = Point::raw(200_000, 260_000);
    s.apply(Intent::ModifiersChanged(xarast_app::Modifiers {
        adjust: true,
        ..xarast_app::Modifiers::default()
    }))
    .unwrap();
    drag_open(&mut s, Point::raw(240_000, 200_000), to);
    release(&mut s, to);
    let (o, x, y) = corners(&fill(&s, n));
    let [c, mx, my] = bitmap_virtual_points(o, x, y);
    assert_eq!((c, mx), (Point::raw(200_000, 200_000), to));
    // Was 20 pt, scaled by 60/40 and turned: now 30 pt along −x.
    assert_eq!(my, Point::raw(170_000, 200_000));
    assert_eq!(s.bus.history().len(), 1);
}

fn infobar_choice(s: &Session, field: InfobarField) -> Option<(Vec<&'static str>, Option<usize>)> {
    s.infobar().items.into_iter().find_map(|i| match i {
        InfobarItem::Choice {
            field: f,
            options,
            selected,
        } if f == field => Some((options, selected)),
        _ => None,
    })
}

fn dpi_field(s: &Session) -> Option<f64> {
    s.infobar().items.into_iter().find_map(|i| match i {
        InfobarItem::Scalar {
            field: InfobarField::BitmapDpi,
            value,
            ..
        } => value,
        _ => None,
    })
}

#[test]
fn tiling_offers_repeat_inverted_and_writes_both_places() {
    let (mut s, n, _) = fixture();
    let (options, selected) = infobar_choice(&s, InfobarField::FillTiling).unwrap();
    assert_eq!(options, ["Simple", "Repeating", "Repeat inverted"]);
    assert_eq!(selected, Some(1), "unset renders as repeat");
    let digest = s.doc.canonical_digest();
    s.apply(Intent::InfobarEdit {
        field: InfobarField::FillTiling,
        value: InfobarValue::Choice(2),
    })
    .unwrap();
    assert_eq!(s.bus.history().len(), 1);
    assert_eq!(s.undo_label(), Some("Fill Tiling"));
    assert!(matches!(
        fill(&s, n),
        FillGeometry::Bitmap {
            tiling: Tiling::RepeatInverted,
            ..
        }
    ));
    assert_eq!(
        infobar_choice(&s, InfobarField::FillTiling).unwrap().1,
        Some(2)
    );
    s.apply(Intent::Undo).unwrap();
    assert_eq!(s.doc.canonical_digest(), digest);
}

#[test]
fn resolution_and_natural_size_resize_about_the_centre() {
    let (mut s, n, _) = fixture();
    // 4 px across 80 pt: 3.6 dpi.
    assert_eq!(dpi_field(&s), Some(4.0));
    let digest = s.doc.canonical_digest();
    s.apply(Intent::ToolAction(ToolAction::NaturalSize))
        .unwrap();
    assert_eq!(s.undo_label(), Some("Natural Size"));
    // 4 × 2 px at 96 dpi: 3 × 1.5 pt, about the same centre.
    let (o, x, y) = corners(&fill(&s, n));
    assert_eq!(o.distance_to(x), 3_000.0);
    assert_eq!(o.distance_to(y), 1_500.0);
    assert_eq!(
        bitmap_virtual_points(o, x, y)[0],
        Point::raw(200_000, 200_000)
    );
    assert_eq!(dpi_field(&s), Some(96.0));
    s.apply(Intent::InfobarEdit {
        field: InfobarField::BitmapDpi,
        value: InfobarValue::Real(48.0),
    })
    .unwrap();
    assert_eq!(s.undo_label(), Some("Bitmap Resolution"));
    let (o, x, _) = corners(&fill(&s, n));
    assert_eq!(o.distance_to(x), 6_000.0);
    assert_eq!(s.bus.history().len(), 2);
    s.apply(Intent::Undo).unwrap();
    s.apply(Intent::Undo).unwrap();
    assert_eq!(s.doc.canonical_digest(), digest);
}
