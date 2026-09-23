//! Rulers: choosing the tick interval, and drawing the strip.
//!
//! The interval is chosen in *document* units and the ticks are then mapped
//! to the screen, never the other way round, so that a label always reads
//! `10mm` and never `9.87mm`. Positions are snapped to device pixels
//! **after** the `f64` transform (see [`crate::scale`]), and nothing is
//! cached across a scale change.

use xarast_geom::Mp;

use crate::guides::Axis;
use crate::model::ViewTransform;
use crate::scale::Scale;
use crate::theme::{RULER_THICKNESS, ThemeTokens};
use crate::units::{Unit, format_measure};

/// One tick of a ruler.
#[derive(Debug, Clone, PartialEq)]
pub struct Tick {
    /// Where it sits, in canvas-relative logical points.
    pub position: f64,
    /// The document coordinate it marks.
    pub value: Mp,
    /// True when the tick carries a label and a full-height mark.
    pub major: bool,
    /// The label, on major ticks only.
    pub label: Option<String>,
}

/// The candidate steps of a ruler, as multiples of the unit.
///
/// A 1-2-5 progression with the typographic and imperial subdivisions the
/// respective units actually use: a ruler in inches that offers 0.25 is
/// useful, one that offers 0.2 is not.
fn candidate_steps(unit: Unit) -> &'static [f64] {
    match unit {
        Unit::Inch => &[
            0.0625, 0.125, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0,
        ],
        Unit::Pica => &[
            0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0,
        ],
        _ => &[
            0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0,
            1000.0, 2000.0, 5000.0, 10000.0,
        ],
    }
}

/// How many logical points a labelled tick needs so labels do not collide.
const MIN_MAJOR_SEPARATION: f64 = 48.0;

/// How many logical points a minor tick needs to stay distinguishable.
const MIN_MINOR_SEPARATION: f64 = 5.0;

/// A hard ceiling on ticks per axis per frame.
const MAX_TICKS: usize = 2_048;

/// Chooses the labelled interval for a ruler, in the given unit.
///
/// Returns the step in document millipoints. The smallest step whose
/// on-screen separation clears the label-collision distance wins, so zooming in
/// subdivides and zooming out coarsens, and the label values stay round.
pub fn major_step(unit: Unit, view: &ViewTransform) -> Mp {
    let steps = candidate_steps(unit);
    for &s in steps {
        let mp = unit.to_mp(s);
        if mp.raw() <= 0 {
            continue;
        }
        if mp.to_pt() * view.zoom >= MIN_MAJOR_SEPARATION {
            return mp;
        }
    }
    unit.to_mp(*steps.last().unwrap_or(&1.0))
}

/// The ticks of one ruler.
///
/// `length` is the canvas extent along the ruler's axis in logical points.
/// Minor ticks are dropped automatically when they would crowd.
pub fn ticks(axis: Axis, unit: Unit, view: &ViewTransform, length: f64) -> Vec<Tick> {
    let mut out = Vec::new();
    if length <= 0.0 || !length.is_finite() || !view.zoom.is_finite() || view.zoom <= 0.0 {
        return out;
    }
    let major = major_step(unit, view);
    let major_pts = major.to_pt() * view.zoom;
    if !(major_pts.is_finite() && major_pts >= 1.0) {
        return out;
    }
    // Five subdivisions when they fit, otherwise none.
    let subdivisions = if major_pts / 5.0 >= MIN_MINOR_SEPARATION {
        5
    } else if major_pts / 2.0 >= MIN_MINOR_SEPARATION {
        2
    } else {
        1
    };
    let step_pts = major_pts / subdivisions as f64;

    let origin = match axis {
        Axis::Vertical => view.origin_y,
        Axis::Horizontal => view.origin_x,
    };
    let flip = axis == Axis::Vertical && view.y_up;
    let first = ((0.0 - origin) / step_pts).ceil();
    let count = (((length - (origin + first * step_pts)) / step_pts).floor() as i64 + 1)
        .clamp(0, MAX_TICKS as i64);
    for i in 0..count {
        let index = first as i64 + i;
        let position = origin + index as f64 * step_pts;
        let is_major = index.rem_euclid(subdivisions as i64) == 0;
        let magnitude = Mp::new(
            major
                .raw()
                .saturating_mul(index.div_euclid(subdivisions as i64) as i32)
                .saturating_add(if is_major {
                    0
                } else {
                    // Minor ticks carry their exact value too, so a tooltip
                    // or a snap can use it.
                    (major.raw() / subdivisions)
                        .saturating_mul(index.rem_euclid(subdivisions as i64) as i32)
                }),
        );
        // Down the screen is down the document when `y` points up.
        let value = if flip {
            Mp::new(magnitude.raw().saturating_neg())
        } else {
            magnitude
        };
        out.push(Tick {
            position,
            value,
            major: is_major,
            label: is_major.then(|| format_measure(value, unit)),
        });
    }
    out
}

/// One ruler strip, ready to draw.
///
/// A struct rather than six arguments: the ruler needs the axis, the
/// unit, the view, the scale, the theme and where the canvas region
/// starts, and a call site that passes those positionally is one
/// transposition away from a silently mirrored ruler.
#[derive(Debug, Clone, Copy)]
pub struct Ruler<'a> {
    /// Which edge this is.
    pub axis: Axis,
    /// The unit its labels are written in.
    pub unit: Unit,
    /// The view transform of the document it measures.
    pub view: &'a ViewTransform,
    /// The frame's one scale factor.
    pub scale: Scale,
    /// The theme to draw with.
    pub tokens: &'a ThemeTokens,
    /// Where the canvas region's leading edge is along this axis, in
    /// logical points, so that the ruler's zero lines up with the
    /// document's.
    pub origin_along: f32,
}

impl Ruler<'_> {
    /// Draws the strip into `strip`.
    ///
    /// Positions are snapped to device pixels after the `f64` transform,
    /// so a tick is one crisp pixel at any fractional scale, and nothing
    /// is cached across a scale change.
    pub fn draw(&self, ui: &egui::Ui, strip: egui::Rect) {
        let painter = ui.painter_at(strip);
        painter.rect_filled(strip, 0.0, self.tokens.surface_raised);

        let hairline = self.scale.hairline_width() as f32;
        let length = match self.axis {
            Axis::Horizontal => strip.width(),
            Axis::Vertical => strip.height(),
        } as f64;
        let stroke_major = egui::Stroke::new(hairline, self.tokens.text_muted);
        let stroke_minor = egui::Stroke::new(hairline, self.tokens.border);
        let font = egui::FontId::proportional(9.0);

        for tick in ticks(self.axis, self.unit, self.view, length) {
            let along = self.origin_along as f64 + tick.position;
            let snapped = self.scale.snap_hairline(along) as f32;
            let depth = if tick.major {
                RULER_THICKNESS * 0.55
            } else {
                RULER_THICKNESS * 0.3
            };
            let stroke = if tick.major {
                stroke_major
            } else {
                stroke_minor
            };
            match self.axis {
                Axis::Horizontal => {
                    painter.line_segment(
                        [
                            egui::pos2(snapped, strip.max.y - depth),
                            egui::pos2(snapped, strip.max.y),
                        ],
                        stroke,
                    );
                    if let Some(label) = tick.label {
                        painter.text(
                            egui::pos2(snapped + 2.0, strip.min.y),
                            egui::Align2::LEFT_TOP,
                            label,
                            font.clone(),
                            self.tokens.text_muted,
                        );
                    }
                }
                Axis::Vertical => {
                    painter.line_segment(
                        [
                            egui::pos2(strip.max.x - depth, snapped),
                            egui::pos2(strip.max.x, snapped),
                        ],
                        stroke,
                    );
                    if let Some(label) = tick.label {
                        // A horizontal label under the tick is legible in
                        // an 18-point strip and needs no rotated text
                        // pass, which egui would rasterise per frame.
                        painter.text(
                            egui::pos2(strip.min.x + 1.0, snapped + 1.0),
                            egui::Align2::LEFT_TOP,
                            label,
                            egui::FontId::proportional(8.0),
                            self.tokens.text_muted,
                        );
                    }
                }
            }
        }

        // The edge between the ruler and the canvas, on a device pixel.
        let edge = match self.axis {
            Axis::Horizontal => {
                let y = self.scale.snap_hairline(strip.max.y as f64) as f32;
                [egui::pos2(strip.min.x, y), egui::pos2(strip.max.x, y)]
            }
            Axis::Vertical => {
                let x = self.scale.snap_hairline(strip.max.x as f64) as f32;
                [egui::pos2(x, strip.min.y), egui::pos2(x, strip.max.y)]
            }
        };
        painter.line_segment(edge, egui::Stroke::new(hairline, self.tokens.border));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_y_up_vertical_ruler_counts_upwards_and_agrees_with_the_view() {
        // Document origin 600 points down the canvas, y up: the ruler must
        // read positive above the origin and each tick must sit exactly
        // where the view puts its value.
        let view = ViewTransform {
            zoom: 3.3,
            origin_x: 0.0,
            origin_y: 600.0,
            y_up: true,
        };
        let t = ticks(Axis::Vertical, Unit::Point, &view, 800.0);
        assert!(t.iter().any(|k| !k.major), "minor ticks are exercised");
        for tick in &t {
            let at = view.doc_to_view_y(tick.value);
            assert!(
                (at - tick.position).abs() < 1e-6,
                "{tick:?} is drawn at {at}"
            );
        }
        let above: Vec<_> = t.iter().filter(|k| k.position < 600.0 && k.major).collect();
        assert!(above.iter().all(|k| k.value.raw() > 0), "{above:?}");
        assert!(
            t.windows(2).all(|w| w[0].value > w[1].value),
            "values fall as the ruler goes down the screen"
        );
        // The horizontal ruler is not affected by the vertical flip.
        let h = ticks(Axis::Horizontal, Unit::Point, &view, 800.0);
        assert!(h.windows(2).all(|w| w[0].value < w[1].value));
    }

    #[test]
    fn minor_ticks_left_of_the_origin_carry_their_own_value() {
        // Truncating division once gave the minor tick one step left of
        // zero the value of four steps right of it.
        let view = ViewTransform {
            zoom: 3.3,
            origin_x: 400.0,
            ..Default::default()
        };
        let t = ticks(Axis::Horizontal, Unit::Point, &view, 800.0);
        assert!(t.iter().any(|k| !k.major && k.value.raw() < 0));
        for tick in &t {
            let at = view.doc_to_view_x(tick.value);
            assert!(
                (at - tick.position).abs() < 1e-3,
                "{tick:?} is drawn at {at}"
            );
        }
    }

    #[test]
    fn labels_are_round_numbers_in_the_chosen_unit() {
        for unit in Unit::ALL {
            for zoom in [0.1, 0.5, 1.0, 2.0, 7.3, 50.0] {
                let view = ViewTransform {
                    zoom,
                    ..Default::default()
                };
                let step = major_step(unit, &view);
                let in_unit = unit.from_mp(step);
                let candidates = candidate_steps(unit);
                assert!(
                    candidates.iter().any(|c| (c - in_unit).abs() < 0.02 * c),
                    "{unit:?} at {zoom}×: step {in_unit} is not a candidate"
                );
            }
        }
    }

    #[test]
    fn labelled_ticks_never_crowd() {
        for unit in Unit::ALL {
            for zoom in [0.05, 0.3, 1.0, 3.0, 20.0] {
                let view = ViewTransform {
                    zoom,
                    ..Default::default()
                };
                let majors: Vec<_> = ticks(Axis::Horizontal, unit, &view, 1200.0)
                    .into_iter()
                    .filter(|t| t.major)
                    .collect();
                for pair in majors.windows(2) {
                    let gap = pair[1].position - pair[0].position;
                    assert!(
                        gap >= MIN_MAJOR_SEPARATION - 1e-6,
                        "{unit:?} at {zoom}×: labels {gap} apart"
                    );
                }
            }
        }
    }

    #[test]
    fn zooming_in_subdivides_and_zooming_out_coarsens() {
        let close = major_step(
            Unit::Millimetre,
            &ViewTransform {
                zoom: 8.0,
                ..Default::default()
            },
        );
        let far = major_step(
            Unit::Millimetre,
            &ViewTransform {
                zoom: 0.1,
                ..Default::default()
            },
        );
        assert!(close.raw() < far.raw());
    }

    #[test]
    fn the_origin_tick_sits_on_the_document_origin() {
        let view = ViewTransform {
            zoom: 1.0,
            origin_x: 137.0,
            origin_y: 0.0,
            y_up: false,
        };
        let t = ticks(Axis::Horizontal, Unit::Point, &view, 900.0);
        let zero = t
            .iter()
            .find(|t| t.value == Mp::ZERO)
            .expect("the origin is visible");
        assert!((zero.position - 137.0).abs() < 1e-6);
    }

    #[test]
    fn the_ruler_origin_lands_within_half_a_device_pixel_at_every_scale() {
        // Acceptance criterion 8 of the phase document, in the only form
        // that can be checked without a display: the snapped position of
        // the origin tick is within half a device pixel of the exact one.
        let view = ViewTransform {
            zoom: 1.0,
            origin_x: 137.3,
            origin_y: 41.7,
            y_up: false,
        };
        for s in [1.0, 1.25, 1.5, 2.0] {
            let scale = Scale::new(s);
            for axis in [Axis::Horizontal, Axis::Vertical] {
                let exact = match axis {
                    Axis::Horizontal => view.origin_x,
                    Axis::Vertical => view.origin_y,
                };
                let error = (scale.snap_hairline(exact) - exact).abs() * s;
                assert!(
                    error <= 0.5 + 1e-9,
                    "scale {s}, {axis:?}: {error} device px"
                );
            }
        }
    }

    #[test]
    fn tick_counts_stay_bounded_at_absurd_zooms() {
        for zoom in [1e-6, 1e6] {
            let view = ViewTransform {
                zoom,
                ..Default::default()
            };
            let t = ticks(Axis::Horizontal, Unit::Millimetre, &view, 1e6);
            assert!(t.len() <= MAX_TICKS, "{} ticks at {zoom}×", t.len());
        }
    }

    #[test]
    fn a_degenerate_view_produces_no_ticks_instead_of_panicking() {
        let bad = ViewTransform {
            zoom: 0.0,
            origin_x: f64::NAN,
            origin_y: 0.0,
            y_up: false,
        };
        assert!(ticks(Axis::Horizontal, Unit::Point, &bad, 500.0).is_empty());
        assert!(
            ticks(
                Axis::Horizontal,
                Unit::Point,
                &ViewTransform::default(),
                -5.0
            )
            .is_empty()
        );
    }

    #[test]
    fn minor_ticks_appear_between_labels_when_there_is_room() {
        let view = ViewTransform {
            zoom: 4.0,
            ..Default::default()
        };
        let t = ticks(Axis::Horizontal, Unit::Millimetre, &view, 600.0);
        assert!(t.iter().any(|t| !t.major), "expected minor ticks");
        assert!(t.iter().any(|t| t.major), "expected major ticks");
    }
}
