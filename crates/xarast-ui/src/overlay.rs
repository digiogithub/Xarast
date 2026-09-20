//! The overlay: handles, marquees and page edges drawn over the document.
//!
//! The overlay never touches the document. It is a list of items in
//! *document* coordinates that is rebuilt every frame from whatever the
//! application currently wants shown — a selection's handles, a rubber-band
//! rectangle, the page edge, the guides — and painted on top of the canvas
//! region. Phase 7 fills it with tool handles; Phase 5 gives it the page
//! and the guides, and the hit-testing that both will need.
//!
//! Xara drew handles with an XOR blit (`research/04 §3` item 3). We do not:
//! XOR is unavailable on a modern composited surface and looks wrong over
//! antialiased content. Instead handles are drawn with a light and a dark
//! outline, which stays visible on any background — the property XOR was
//! bought for — without the artefacts.

use xarast_geom::Mp;

use crate::model::ViewTransform;
use crate::scale::Scale;
use crate::theme::ThemeTokens;

/// What a handle means, which decides how it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HandleKind {
    /// A corner or edge handle of a bounding box.
    Bounds,
    /// A rotation or skew handle, drawn hollow.
    Rotate,
    /// A path control point.
    Node,
    /// A fill or transparency handle, drawn as a diamond.
    Fill,
    /// The centre of rotation.
    Centre,
}

impl HandleKind {
    /// The side of the handle, in logical points.
    ///
    /// Seven points is the size Xara used and is still right: large enough
    /// to grab, small enough not to hide the artwork underneath.
    pub fn size(self) -> f32 {
        match self {
            HandleKind::Centre => 9.0,
            _ => 7.0,
        }
    }
}

/// One thing drawn over the document.
#[derive(Debug, Clone, PartialEq)]
pub enum OverlayItem {
    /// A grab handle at a document point.
    Handle {
        /// Document x.
        x: Mp,
        /// Document y.
        y: Mp,
        /// What it means.
        kind: HandleKind,
        /// Whether it is the one under the pointer or being dragged.
        active: bool,
    },
    /// A straight line between two document points: a gradient arrow, a
    /// constraint line, a measurement.
    Line {
        /// Start, document coordinates.
        from: (Mp, Mp),
        /// End, document coordinates.
        to: (Mp, Mp),
        /// Drawn dashed, for a construction line.
        dashed: bool,
    },
    /// An outlined rectangle: a bounding box or a rubber band.
    Rect {
        /// Document coordinates: left, top, right, bottom.
        bounds: (Mp, Mp, Mp, Mp),
        /// Drawn dashed, for a rubber band.
        dashed: bool,
    },
}

/// Draws the overlay items over a canvas region.
///
/// Everything is snapped to device pixels after the transform, so a handle
/// is a crisp square at 1×, at 1.25× and at 2×, and nothing is cached
/// across a scale change.
#[derive(Debug)]
pub struct OverlayPainter<'a> {
    /// Where the canvas region sits, in logical points.
    pub region: egui::Rect,
    /// The view transform of the document shown there.
    pub view: &'a ViewTransform,
    /// The frame's one scale factor.
    pub scale: Scale,
    /// The theme tokens to draw with.
    pub tokens: &'a ThemeTokens,
}

impl OverlayPainter<'_> {
    /// Maps a document point to a logical screen position.
    pub fn to_screen(&self, x: Mp, y: Mp) -> egui::Pos2 {
        egui::pos2(
            (self.region.min.x as f64 + self.view.doc_to_view_x(x)) as f32,
            (self.region.min.y as f64 + self.view.doc_to_view_y(y)) as f32,
        )
    }

    /// Maps a logical screen position back to document coordinates.
    pub fn to_doc(&self, p: egui::Pos2) -> (Mp, Mp) {
        (
            self.view
                .view_to_doc_x(p.x as f64 - self.region.min.x as f64),
            self.view
                .view_to_doc_y(p.y as f64 - self.region.min.y as f64),
        )
    }

    /// Paints every item.
    pub fn paint(&self, painter: &egui::Painter, items: &[OverlayItem]) {
        for item in items {
            match item {
                OverlayItem::Handle { x, y, kind, active } => {
                    self.paint_handle(painter, *x, *y, *kind, *active)
                }
                OverlayItem::Line { from, to, dashed } => {
                    let a = self.snap_point(self.to_screen(from.0, from.1));
                    let b = self.snap_point(self.to_screen(to.0, to.1));
                    self.paint_line(painter, a, b, *dashed);
                }
                OverlayItem::Rect { bounds, dashed } => {
                    let a = self.snap_point(self.to_screen(bounds.0, bounds.1));
                    let b = self.snap_point(self.to_screen(bounds.2, bounds.3));
                    let r = egui::Rect::from_two_pos(a, b);
                    self.paint_line(painter, r.left_top(), r.right_top(), *dashed);
                    self.paint_line(painter, r.right_top(), r.right_bottom(), *dashed);
                    self.paint_line(painter, r.right_bottom(), r.left_bottom(), *dashed);
                    self.paint_line(painter, r.left_bottom(), r.left_top(), *dashed);
                }
            }
        }
    }

    fn snap_point(&self, p: egui::Pos2) -> egui::Pos2 {
        egui::pos2(
            self.scale.snap_hairline(p.x as f64) as f32,
            self.scale.snap_hairline(p.y as f64) as f32,
        )
    }

    fn paint_line(&self, painter: &egui::Painter, a: egui::Pos2, b: egui::Pos2, dashed: bool) {
        let w = self.scale.hairline_width() as f32;
        if dashed {
            // Two passes: a light dash over a dark one is visible on both a
            // white page and dark artwork, which is what XOR used to buy.
            painter.line_segment([a, b], egui::Stroke::new(w, self.tokens.surface_sunken));
            for seg in dashes(a, b, 4.0) {
                painter.line_segment(seg, egui::Stroke::new(w, self.tokens.page));
            }
        } else {
            painter.line_segment([a, b], egui::Stroke::new(w, self.tokens.accent));
        }
    }

    fn paint_handle(&self, painter: &egui::Painter, x: Mp, y: Mp, kind: HandleKind, active: bool) {
        let centre = self.to_screen(x, y);
        let size = kind.size();
        // A handle is an odd number of device pixels wide, so it has a
        // true centre pixel: snap the centre to a pixel centre and the
        // edges land on pixel boundaries.
        let c = egui::pos2(
            self.scale.snap_hairline(centre.x as f64) as f32,
            self.scale.snap_hairline(centre.y as f64) as f32,
        );
        let half = size / 2.0;
        let rect = egui::Rect::from_center_size(c, egui::vec2(size, size));
        let fill = if active {
            self.tokens.accent
        } else {
            self.tokens.page
        };
        let w = self.scale.hairline_width() as f32;
        match kind {
            HandleKind::Rotate | HandleKind::Centre => {
                painter.circle_stroke(c, half, egui::Stroke::new(w, self.tokens.surface_sunken));
                painter.circle_stroke(c, half - w, egui::Stroke::new(w, fill));
            }
            HandleKind::Fill => {
                let d = [
                    egui::pos2(c.x, c.y - half),
                    egui::pos2(c.x + half, c.y),
                    egui::pos2(c.x, c.y + half),
                    egui::pos2(c.x - half, c.y),
                ];
                painter.add(egui::Shape::convex_polygon(
                    d.to_vec(),
                    fill,
                    egui::Stroke::new(w, self.tokens.surface_sunken),
                ));
            }
            HandleKind::Bounds | HandleKind::Node => {
                painter.rect_filled(rect, 0.0, fill);
                painter.rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(w, self.tokens.surface_sunken),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }
}

fn dashes(a: egui::Pos2, b: egui::Pos2, dash: f32) -> Vec<[egui::Pos2; 2]> {
    let delta = b - a;
    let len = delta.length();
    if len <= f32::EPSILON || !len.is_finite() {
        return Vec::new();
    }
    let dir = delta / len;
    let mut out = Vec::new();
    let mut t = 0.0;
    while t < len {
        let end = (t + dash).min(len);
        out.push([a + dir * t, a + dir * end]);
        t += dash * 2.0;
    }
    out
}

/// Finds the handle under a pointer, in logical screen coordinates.
///
/// Later items win, because they are drawn on top. The tolerance is the
/// handle's own size, which is what makes a seven-point handle grabbable
/// without a pixel hunt.
pub fn handle_at(
    items: &[OverlayItem],
    painter: &OverlayPainter<'_>,
    pointer: egui::Pos2,
) -> Option<usize> {
    let mut found = None;
    for (i, item) in items.iter().enumerate() {
        if let OverlayItem::Handle { x, y, kind, .. } = item {
            let c = painter.to_screen(*x, *y);
            let half = kind.size() / 2.0 + 1.0;
            if (pointer.x - c.x).abs() <= half && (pointer.y - c.y).abs() <= half {
                found = Some(i);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{ResolvedTheme, ThemeTokens};

    fn setup() -> (ViewTransform, ThemeTokens) {
        (
            ViewTransform {
                zoom: 2.0,
                origin_x: 10.0,
                origin_y: 20.0,
            },
            ThemeTokens::of(ResolvedTheme::Dark),
        )
    }

    #[test]
    fn screen_and_document_coordinates_round_trip() {
        let (view, tokens) = setup();
        let p = OverlayPainter {
            region: egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(800.0, 600.0)),
            view: &view,
            scale: Scale::new(1.5),
            tokens: &tokens,
        };
        let doc = (Mp::from_pt(30.0), Mp::from_pt(-12.0));
        let screen = p.to_screen(doc.0, doc.1);
        let back = p.to_doc(screen);
        assert!((back.0.raw() - doc.0.raw()).abs() <= 2);
        assert!((back.1.raw() - doc.1.raw()).abs() <= 2);
    }

    #[test]
    fn a_handle_is_grabbable_over_its_whole_square() {
        let (view, tokens) = setup();
        let p = OverlayPainter {
            region: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)),
            view: &view,
            scale: Scale::new(1.0),
            tokens: &tokens,
        };
        let items = vec![OverlayItem::Handle {
            x: Mp::from_pt(50.0),
            y: Mp::from_pt(50.0),
            kind: HandleKind::Bounds,
            active: false,
        }];
        let centre = p.to_screen(Mp::from_pt(50.0), Mp::from_pt(50.0));
        assert_eq!(handle_at(&items, &p, centre), Some(0));
        let edge = centre + egui::vec2(3.0, 3.0);
        assert_eq!(handle_at(&items, &p, edge), Some(0));
        let miss = centre + egui::vec2(20.0, 0.0);
        assert_eq!(handle_at(&items, &p, miss), None);
    }

    #[test]
    fn the_topmost_of_two_stacked_handles_wins() {
        let (view, tokens) = setup();
        let p = OverlayPainter {
            region: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)),
            view: &view,
            scale: Scale::new(1.0),
            tokens: &tokens,
        };
        let h = |kind| OverlayItem::Handle {
            x: Mp::ZERO,
            y: Mp::ZERO,
            kind,
            active: false,
        };
        let items = vec![h(HandleKind::Bounds), h(HandleKind::Node)];
        assert_eq!(
            handle_at(&items, &p, p.to_screen(Mp::ZERO, Mp::ZERO)),
            Some(1)
        );
    }

    #[test]
    fn a_line_is_dashed_into_segments_and_never_loops_forever() {
        let segs = dashes(egui::pos2(0.0, 0.0), egui::pos2(20.0, 0.0), 4.0);
        assert_eq!(segs.len(), 3);
        assert!(dashes(egui::pos2(1.0, 1.0), egui::pos2(1.0, 1.0), 4.0).is_empty());
        assert!(dashes(egui::pos2(0.0, 0.0), egui::pos2(f32::NAN, 0.0), 4.0).is_empty());
    }

    #[test]
    fn painting_every_item_kind_is_pure_and_headless() {
        let (view, tokens) = setup();
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let p = OverlayPainter {
                    region: ui.max_rect(),
                    view: &view,
                    scale: Scale::new(1.25),
                    tokens: &tokens,
                };
                let items = vec![
                    OverlayItem::Handle {
                        x: Mp::ZERO,
                        y: Mp::ZERO,
                        kind: HandleKind::Bounds,
                        active: true,
                    },
                    OverlayItem::Handle {
                        x: Mp::from_pt(10.0),
                        y: Mp::ZERO,
                        kind: HandleKind::Rotate,
                        active: false,
                    },
                    OverlayItem::Handle {
                        x: Mp::from_pt(20.0),
                        y: Mp::ZERO,
                        kind: HandleKind::Fill,
                        active: false,
                    },
                    OverlayItem::Handle {
                        x: Mp::from_pt(30.0),
                        y: Mp::ZERO,
                        kind: HandleKind::Centre,
                        active: false,
                    },
                    OverlayItem::Line {
                        from: (Mp::ZERO, Mp::ZERO),
                        to: (Mp::from_pt(40.0), Mp::from_pt(40.0)),
                        dashed: true,
                    },
                    OverlayItem::Rect {
                        bounds: (Mp::ZERO, Mp::ZERO, Mp::from_pt(60.0), Mp::from_pt(40.0)),
                        dashed: false,
                    },
                ];
                p.paint(ui.painter(), &items);
            });
        });
    }
}
