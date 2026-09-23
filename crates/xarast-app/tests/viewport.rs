//! The view transform, in the properties the rest of the application
//! relies on.

use xarast_app::{DevicePoint, DeviceSize, DocPoint, MAX_ZOOM, MIN_ZOOM, Viewport, ZoomTarget};
use xarast_doc::Document;
use xarast_geom::{Mp, Point, Rect};

fn viewport() -> Viewport {
    let mut vp = Viewport::new(DeviceSize::new(800, 600));
    vp.set_centre(kurbo::Point::new(100_000.0, 200_000.0));
    vp.set_zoom(1.5);
    vp
}

#[test]
fn device_and_document_are_inverses() {
    let vp = viewport();
    for (x, y) in [(0.0, 0.0), (800.0, 600.0), (13.5, 411.25), (-40.0, 900.0)] {
        let p = DevicePoint::new(x, y);
        let back = vp.doc_to_device_f64(vp.device_to_doc_f64(p));
        assert!((back.x - p.x).abs() < 1e-9, "{back:?} vs {p:?}");
        assert!((back.y - p.y).abs() < 1e-9, "{back:?} vs {p:?}");
    }
}

#[test]
fn the_document_y_axis_points_up_and_the_device_one_points_down() {
    let vp = viewport();
    let low = vp.doc_to_device(DocPoint::new(Mp::ZERO, Mp::ZERO));
    let high = vp.doc_to_device(DocPoint::new(Mp::ZERO, Mp::from_pt(100.0)));
    assert!(
        high.y < low.y,
        "a document point higher up must land nearer the top of the surface"
    );
}

#[test]
fn zooming_keeps_the_anchor_under_the_pointer() {
    let mut vp = viewport();
    let anchor = DevicePoint::new(613.0, 91.0);
    let before = vp.device_to_doc_f64(anchor);
    vp.zoom_about(2.5, anchor);
    let after = vp.device_to_doc_f64(anchor);
    assert!((before.x - after.x).abs() < 1e-6, "{before:?} {after:?}");
    assert!((before.y - after.y).abs() < 1e-6, "{before:?} {after:?}");
}

#[test]
fn the_zoom_is_clamped_at_both_ends() {
    let mut vp = viewport();
    vp.set_zoom(1e9);
    assert!((vp.zoom() - MAX_ZOOM).abs() < 1e-9);
    vp.set_zoom(1e-9);
    assert!((vp.zoom() - MIN_ZOOM).abs() < 1e-9);
}

#[test]
fn a_degenerate_or_hostile_input_never_poisons_the_transform() {
    let mut vp = viewport();
    let before = vp.clone();
    vp.pan_by(f64::NAN, 0.0);
    vp.zoom_about(f64::INFINITY, DevicePoint::ZERO);
    vp.zoom_about(0.0, DevicePoint::ZERO);
    vp.zoom_about(2.0, DevicePoint::new(f64::NAN, 0.0));
    vp.set_dpi(0.0);
    vp.set_dpi(f64::NAN);
    vp.fit_rect(Rect::EMPTY);
    assert_eq!(vp, before);
    assert!(vp.scale().is_finite() && vp.scale() > 0.0);
}

#[test]
fn fitting_a_rectangle_frames_it_with_a_margin() {
    let mut vp = Viewport::new(DeviceSize::new(800, 600));
    let r = Rect::new(
        Point::new(Mp::from_pt(0.0), Mp::from_pt(0.0)),
        Point::new(Mp::from_pt(400.0), Mp::from_pt(200.0)),
    );
    vp.fit_rect(r);
    let visible = vp.visible_doc_rect();
    assert!(
        visible.contains_rect(r),
        "the fitted rectangle must be visible: {visible:?} does not contain {r:?}"
    );
    assert!(visible.width().to_f64() < r.width().to_f64() * 1.3);
}

#[test]
fn pan_moves_the_drawing_with_the_pointer() {
    let mut vp = viewport();
    let p = DocPoint::new(Mp::from_pt(10.0), Mp::from_pt(10.0));
    let before = vp.doc_to_device(p);
    vp.pan_by(30.0, -20.0);
    let after = vp.doc_to_device(p);
    assert!((after.x - (before.x + 30.0)).abs() < 1e-6);
    assert!((after.y - (before.y - 20.0)).abs() < 1e-6);
}

#[test]
fn zoom_targets_frame_what_they_name() {
    let doc = Document::new_empty();
    let mut vp = Viewport::new(DeviceSize::new(800, 600));
    vp.fit_bounds_to(&doc);

    vp.zoom_to(ZoomTarget::Page, &doc, Rect::EMPTY);
    let page = xarast_app::viewport::page_rect(&doc);
    assert!(vp.visible_doc_rect().contains_rect(page));

    let at_page = vp.zoom();
    vp.zoom_to(ZoomTarget::Spread, &doc, Rect::EMPTY);
    assert!(
        vp.zoom() <= at_page,
        "the spread is larger than the page, so framing it zooms out"
    );

    vp.zoom_to(ZoomTarget::Percent100, &doc, Rect::EMPTY);
    assert!((vp.zoom() - 1.0).abs() < 1e-9);

    vp.zoom_to(ZoomTarget::Previous, &doc, Rect::EMPTY);
    assert!(
        (vp.zoom() - 1.0).abs() > 1e-9,
        "previous must restore a different zoom"
    );
}

#[test]
fn an_empty_selection_falls_back_to_the_drawing() {
    let doc = Document::new_empty();
    let mut a = Viewport::new(DeviceSize::new(800, 600));
    let mut b = a.clone();
    a.zoom_to(ZoomTarget::Selection, &doc, Rect::EMPTY);
    b.zoom_to(ZoomTarget::Drawing, &doc, Rect::EMPTY);
    assert_eq!(a, b);
}

#[test]
fn scrolling_is_bounded_but_lets_a_corner_reach_the_middle() {
    let doc = Document::new_empty();
    let mut vp = Viewport::new(DeviceSize::new(800, 600));
    vp.fit_bounds_to(&doc);
    let bounds = vp.scroll_bounds();
    assert!(!bounds.is_empty());

    vp.set_centre(kurbo::Point::new(1e12, -1e12));
    let range = vp.scroll_range();
    let c = vp.centre();
    assert!(c.x <= range.hi.x.to_f64() + 1.0 && c.x >= range.lo.x.to_f64() - 1.0);
    assert!(c.y <= range.hi.y.to_f64() + 1.0 && c.y >= range.lo.y.to_f64() - 1.0);
    assert!(range.width().to_f64() > bounds.width().to_f64());
}

#[test]
fn a_zero_sized_viewport_does_not_divide_by_zero() {
    let mut vp = Viewport::new(DeviceSize::new(0, 0));
    vp.fit_rect(Rect::new(
        Point::new(Mp::ZERO, Mp::ZERO),
        Point::new(Mp::from_pt(10.0), Mp::from_pt(10.0)),
    ));
    assert!(vp.scale().is_finite());
    assert!(vp.visible_doc_rect().is_empty() || vp.visible_doc_rect().width() == Mp::ZERO);
}

fn small_drawing() -> Document {
    xarast_doc::synthetic_document(xarast_doc::SynthSpec {
        nodes: 400,
        ..xarast_doc::SynthSpec::default()
    })
}

#[test]
fn the_drawing_rect_leaves_the_pages_out() {
    use xarast_app::viewport::{content_rect, drawing_or_page_rect, drawing_rect, page_rect};
    let doc = small_drawing();
    let (drawing, page) = (drawing_rect(&doc), page_rect(&doc));
    assert!(!drawing.is_empty());
    assert!(
        page.contains_rect(drawing) && drawing != page,
        "{drawing:?} is the ink, strictly inside the page {page:?}"
    );
    assert_eq!(drawing_or_page_rect(&doc), drawing);
    // The superset culling uses does include the page.
    assert!(content_rect(&doc).contains_rect(page.union(drawing)));

    // Nothing drawn: the drawing is empty and fitting it frames the page.
    let empty = Document::new_empty();
    assert!(drawing_rect(&empty).is_empty());
    assert_eq!(drawing_or_page_rect(&empty), page_rect(&empty));
}

#[test]
fn headless_takes_a_fixed_zoom_and_dpi_and_reports_the_walk() {
    use xarast_app::viewport::drawing_rect;
    use xarast_app::{DocumentId, HeadlessFrame, HeadlessOptions, Session, headless};
    let doc = small_drawing();
    let centre_on = drawing_rect(&doc);
    let session = Session::adopt(DocumentId(1), doc, None);
    let out = headless::render(
        &session,
        &HeadlessOptions {
            size: DeviceSize::new(200, 100),
            frame: HeadlessFrame::Fixed {
                zoom: 1.0,
                centre_on,
            },
            dpi: Some(192.0),
            ..HeadlessOptions::default()
        },
    )
    .unwrap();
    assert!((out.zoom - 1.0).abs() < 1e-12);
    assert!((out.view.dpi - 192.0).abs() < 1e-12);
    // One inch of document (72 000 mp) is 192 pixels at 100 %.
    let a = out.view.transform.to_affine().as_coeffs();
    assert!((a[0] * 72_000.0 - 192.0).abs() < 1e-6, "{a:?}");
    // Centred on the drawing's middle.
    let mid = centre_on.to_kurbo().center();
    let at = out.view.transform.to_affine() * mid;
    assert!(
        (at.x - 100.0).abs() < 1e-6 && (at.y - 50.0).abs() < 1e-6,
        "{at:?}"
    );
    // The walk's findings come back with the pixels.
    assert!(out.walk.visited > 400, "{:?}", out.walk);
    assert!(out.scene.primitives() > 0);
    // The session's own viewport was not touched.
    assert!((session.viewport.dpi() - 96.0).abs() < 1e-12);
}
