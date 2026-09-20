//! The rectangular grid: settings, the visible lines, and snapping.
//!
//! Phase 5 owns display and interaction only; snapping a *drag* to the grid
//! belongs to Phase 7, but the arithmetic that says where the nearest grid
//! intersection is lives here, next to the grid itself, so that the tool
//! phase gets it rather than inventing a second version.
//!
//! Isometric grids are out of scope for this phase (`research/04 §1.18`).

use xarast_geom::Mp;

use crate::model::ViewTransform;

/// A rectangular grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GridSettings {
    /// Whether the grid is drawn.
    pub visible: bool,
    /// The distance between two major divisions.
    #[serde(with = "crate::serde_mp")]
    pub spacing: Mp,
    /// How many subdivisions a major division is split into. One means no
    /// minor lines at all.
    pub subdivisions: u32,
    /// Where the grid starts, usually the page corner.
    #[serde(with = "crate::serde_mp::pair")]
    pub origin: (Mp, Mp),
}

impl Default for GridSettings {
    fn default() -> Self {
        GridSettings {
            visible: false,
            // 10 mm majors with 10 minors is the metric default; the unit
            // the user works in does not change the grid, only its label.
            spacing: Mp::from_mm(10.0),
            subdivisions: 10,
            origin: (Mp::ZERO, Mp::ZERO),
        }
    }
}

impl GridSettings {
    /// The distance between two minor lines, never zero.
    pub fn minor_spacing(&self) -> Mp {
        let subs = self.subdivisions.max(1) as i32;
        Mp::new((self.spacing.raw() / subs).max(1))
    }

    /// Rounds a document coordinate to the nearest minor grid line.
    ///
    /// Phase 7 will decide *when* to call this; the arithmetic is here.
    pub fn snap_x(&self, x: Mp) -> Mp {
        snap_to(x, self.origin.0, self.minor_spacing())
    }

    /// The vertical half of [`GridSettings::snap_x`].
    pub fn snap_y(&self, y: Mp) -> Mp {
        snap_to(y, self.origin.1, self.minor_spacing())
    }
}

fn snap_to(v: Mp, origin: Mp, step: Mp) -> Mp {
    let step = step.raw().max(1) as f64;
    let offset = (v.raw() - origin.raw()) as f64;
    let snapped = (offset / step).round() * step;
    Mp::new(origin.raw().saturating_add(snapped.round() as i32))
}

/// One grid line to draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridLine {
    /// Where the line sits, in canvas-relative logical points.
    pub position: f64,
    /// True for a major division, false for a subdivision.
    pub major: bool,
}

/// The grid lines crossing one axis of a canvas region.
///
/// `length` is the canvas extent along that axis, in logical points, and
/// the result is in the same coordinates. The function refuses to produce
/// an unreasonable number of lines: below roughly two points of separation
/// a grid is a grey wash that costs time and tells the user nothing, so the
/// minor lines are dropped first and then the grid itself.
pub fn grid_lines(
    grid: &GridSettings,
    view: &ViewTransform,
    horizontal: bool,
    length: f64,
) -> Vec<GridLine> {
    let mut out = Vec::new();
    if !grid.visible || length <= 0.0 || !length.is_finite() {
        return out;
    }
    let major = grid.spacing.to_pt() * view.zoom;
    if !(major.is_finite() && major > 0.0) {
        return out;
    }
    let minor = grid.minor_spacing().to_pt() * view.zoom;
    let draw_minor = minor >= MIN_LINE_SEPARATION;
    let step = if draw_minor { minor } else { major };
    if step < MIN_LINE_SEPARATION {
        return out;
    }

    let origin_doc = if horizontal {
        grid.origin.0
    } else {
        grid.origin.1
    };
    let origin_view = if horizontal {
        view.doc_to_view_x(origin_doc)
    } else {
        view.doc_to_view_y(origin_doc)
    };
    // The index of the first line at or after the region's leading edge.
    let first = ((0.0 - origin_view) / step).ceil();
    let count = ((length - (origin_view + first * step)) / step).floor() as i64 + 1;
    let count = count.clamp(0, MAX_LINES as i64);
    let subs = grid.subdivisions.max(1) as i64;
    for i in 0..count {
        let index = first as i64 + i;
        let position = origin_view + index as f64 * step;
        let major_line = !draw_minor || index.rem_euclid(subs) == 0;
        out.push(GridLine {
            position,
            major: major_line,
        });
    }
    out
}

/// Below this separation in logical points a grid stops being a grid.
const MIN_LINE_SEPARATION: f64 = 3.0;

/// A hard ceiling on the lines produced for one axis in one frame, so that a
/// pathological zoom cannot turn into an unbounded allocation.
const MAX_LINES: usize = 4_096;

#[cfg(test)]
mod tests {
    use super::*;

    fn visible_grid() -> GridSettings {
        GridSettings {
            visible: true,
            spacing: Mp::from_pt(100.0),
            subdivisions: 10,
            origin: (Mp::ZERO, Mp::ZERO),
        }
    }

    #[test]
    fn a_hidden_grid_produces_nothing() {
        let g = GridSettings::default();
        assert!(!g.visible);
        assert!(grid_lines(&g, &ViewTransform::default(), true, 1000.0).is_empty());
    }

    #[test]
    fn major_and_minor_lines_alternate_correctly() {
        let lines = grid_lines(&visible_grid(), &ViewTransform::default(), true, 400.0);
        assert!(!lines.is_empty());
        let majors: Vec<_> = lines.iter().filter(|l| l.major).collect();
        assert_eq!(majors.len(), 5, "0, 100, 200, 300, 400");
        for m in majors {
            assert!((m.position % 100.0).abs() < 1e-6);
        }
    }

    #[test]
    fn minor_lines_disappear_before_the_grid_becomes_a_wash() {
        let zoomed_out = ViewTransform {
            zoom: 0.05,
            ..Default::default()
        };
        let lines = grid_lines(&visible_grid(), &zoomed_out, true, 800.0);
        assert!(lines.iter().all(|l| l.major), "only majors survive");
        let much_further = ViewTransform {
            zoom: 0.005,
            ..Default::default()
        };
        assert!(grid_lines(&visible_grid(), &much_further, true, 800.0).is_empty());
    }

    #[test]
    fn the_line_count_is_bounded_whatever_the_zoom() {
        let huge = ViewTransform {
            zoom: 200.0,
            ..Default::default()
        };
        let lines = grid_lines(&visible_grid(), &huge, true, 1e7);
        assert!(lines.len() <= MAX_LINES);
    }

    #[test]
    fn lines_follow_a_pan() {
        let view = ViewTransform::default().panned(37.0, 0.0);
        let lines = grid_lines(&visible_grid(), &view, true, 400.0);
        let first_major = lines.iter().find(|l| l.major).unwrap();
        assert!((first_major.position - 37.0).abs() < 1e-6);
    }

    #[test]
    fn snapping_goes_to_the_nearest_minor_line() {
        let g = visible_grid();
        assert_eq!(g.minor_spacing(), Mp::from_pt(10.0));
        assert_eq!(g.snap_x(Mp::from_pt(14.0)), Mp::from_pt(10.0));
        assert_eq!(g.snap_x(Mp::from_pt(16.0)), Mp::from_pt(20.0));
        assert_eq!(g.snap_y(Mp::from_pt(-4.0)), Mp::ZERO);
    }

    #[test]
    fn snapping_respects_a_shifted_origin() {
        let g = GridSettings {
            origin: (Mp::from_pt(5.0), Mp::from_pt(5.0)),
            ..visible_grid()
        };
        assert_eq!(g.snap_x(Mp::from_pt(14.0)), Mp::from_pt(15.0));
    }

    #[test]
    fn a_degenerate_grid_cannot_divide_by_zero() {
        let g = GridSettings {
            visible: true,
            spacing: Mp::new(0),
            subdivisions: 0,
            origin: (Mp::ZERO, Mp::ZERO),
        };
        assert!(g.minor_spacing().raw() >= 1);
        assert!(grid_lines(&g, &ViewTransform::default(), true, 500.0).is_empty());
        let _ = g.snap_x(Mp::from_pt(3.0));
    }

    #[test]
    fn grid_settings_round_trip_through_serde() {
        let g = visible_grid();
        let json = serde_json::to_string(&g).unwrap();
        assert_eq!(serde_json::from_str::<GridSettings>(&json).unwrap(), g);
    }
}
