//! Text on a path (W9.5): the story is laid out as usual, on a straight
//! baseline as long as the path, and each character is then carried onto
//! the path by arc length.
//!
//! This is "spike A" of `docs/phases/phase-09-text.md` W9.5, which the
//! measurement of T9.5.3 chose (`docs/memory/text.md`, "Text on a path"):
//! the original fits text to a path the same way, after formatting, so a
//! layout-aware pass would only move glyphs away from where it puts them.
//!
//! The rule, as documented facts about the original (not its code):
//!
//! * The line is as long as the path; the left and right indents are its
//!   physical margins, so alignment and justification work along the path.
//! * A character is placed by the **centre of its advance**: that distance
//!   along the path gives a point and the tangent there; the character's
//!   baseline centre goes to the point and it turns by the tangent's angle
//!   about that centre (none when characters stay upright).
//! * On a closed path the distance wraps around; off either end of an open
//!   path the path continues as a straight line along its end tangent.
//! * Lines after the first are **not** offset along each point's normal:
//!   the whole line moves by its baseline distance along the normal at the
//!   path's start.
//! * The pre-fit character transform (reflection and shear) applies to each
//!   character before it turns.
//!
//! Everything is in story space, millipoints as `f64`, y up.

use kurbo::{Affine, BezPath, ParamCurve, ParamCurveArclen, ParamCurveDeriv, PathSeg, Point, Vec2};
use xarast_geom::Mp;

use crate::layout::{LaidCluster, LaidLine};
use crate::style::StoryMode;

/// Accuracy of arc-length inversion, in millipoints.
const ARCLEN_ACCURACY: f64 = 0.01;

/// A path parameterised by arc length.
#[derive(Clone, Debug)]
pub struct TextPath {
    segs: Vec<PathSeg>,
    /// Arc length at the start of each segment.
    starts: Vec<f64>,
    length: f64,
    closed: bool,
    start_dir: Vec2,
    end_dir: Vec2,
}

impl TextPath {
    /// Parameterises `path` (story space). With `reversed` the text runs
    /// from the path's end to its start. `None` when the path has no
    /// length.
    #[must_use]
    pub fn new(path: &BezPath, reversed: bool) -> Option<TextPath> {
        let closed = matches!(path.elements().last(), Some(kurbo::PathEl::ClosePath));
        let mut segs: Vec<PathSeg> = path
            .segments()
            .filter(|s| s.arclen(ARCLEN_ACCURACY) > 1e-9)
            .collect();
        if reversed {
            segs.reverse();
            for s in &mut segs {
                *s = s.reverse();
            }
        }
        let mut starts = Vec::with_capacity(segs.len());
        let mut length = 0.0;
        for s in &segs {
            starts.push(length);
            length += s.arclen(ARCLEN_ACCURACY);
        }
        if !length.is_finite() || length <= 0.0 {
            return None;
        }
        let start_dir = tangent(segs.first()?, 0.0);
        let end_dir = tangent(segs.last()?, 1.0);
        Some(TextPath {
            segs,
            starts,
            length,
            closed,
            start_dir,
            end_dir,
        })
    }

    /// The path's length.
    #[must_use]
    pub fn length(&self) -> f64 {
        self.length
    }

    /// Whether the path is closed (text wraps around it).
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// The point `dist` along the path and the tangent's angle there, in
    /// radians. A closed path wraps; an open one extends along its end
    /// tangents.
    #[must_use]
    pub fn at(&self, dist: f64) -> (Point, f64) {
        let dist = if self.closed {
            dist.rem_euclid(self.length)
        } else {
            dist
        };
        let angle = |v: Vec2| v.y.atan2(v.x);
        if dist < 0.0 {
            let p0 = self.segs[0].start();
            return (p0 + self.start_dir * dist, angle(self.start_dir));
        }
        if dist > self.length {
            let Some(last) = self.segs.last() else {
                return (Point::ORIGIN, 0.0);
            };
            return (
                last.end() + self.end_dir * (dist - self.length),
                angle(self.end_dir),
            );
        }
        let i = match self.starts.binary_search_by(|s| s.total_cmp(&dist)) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let seg = &self.segs[i];
        let t = seg.inv_arclen(dist - self.starts[i], ARCLEN_ACCURACY);
        (seg.eval(t), angle(tangent(seg, t)))
    }

    /// The unit normal to the left of the path's direction at its start:
    /// where lines above the first would go (they have positive baselines).
    #[must_use]
    pub fn start_normal(&self) -> Vec2 {
        Vec2::new(-self.start_dir.y, self.start_dir.x)
    }
}

/// The unit tangent of `seg` at `t`, falling back to a nearby difference
/// and then to the chord where the derivative vanishes (a collapsed
/// control handle).
fn tangent(seg: &PathSeg, t: f64) -> Vec2 {
    let d = match seg {
        PathSeg::Line(l) => l.p1 - l.p0,
        PathSeg::Quad(q) => q.deriv().eval(t).to_vec2(),
        PathSeg::Cubic(c) => c.deriv().eval(t).to_vec2(),
    };
    let d = if d.hypot2() > 1e-18 {
        d
    } else {
        let (a, b) = ((t - 1e-3).max(0.0), (t + 1e-3).min(1.0));
        let e = seg.eval(b) - seg.eval(a);
        if e.hypot2() > 1e-18 {
            e
        } else {
            seg.end() - seg.start()
        }
    };
    let n = d.hypot();
    if n > 0.0 && n.is_finite() {
        d / n
    } else {
        Vec2::new(1.0, 0.0)
    }
}

/// How characters are carried onto a path.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PathFitStyle {
    /// Characters turn with the path (`false`: they stay upright).
    pub tangential: bool,
    /// Characters are reflected about their baseline: the text hangs on
    /// the other side of the path, and later lines go the other way.
    pub reflected: bool,
    /// Shear of every character, radians.
    pub shear: f64,
    /// The left indent: where the line starts along the path.
    pub left_indent: Mp,
    /// The right indent: how far before the path's end the line ends.
    pub right_indent: Mp,
}

impl Default for PathFitStyle {
    fn default() -> PathFitStyle {
        PathFitStyle {
            tangential: true,
            reflected: false,
            shear: 0.0,
            left_indent: Mp::ZERO,
            right_indent: Mp::ZERO,
        }
    }
}

/// A laid-out story carried onto a path.
#[derive(Clone, Debug)]
pub struct PathFit {
    path: TextPath,
    style: PathFitStyle,
}

impl PathFit {
    /// Fits onto `path` with `style`.
    #[must_use]
    pub fn new(path: TextPath, style: PathFitStyle) -> PathFit {
        PathFit { path, style }
    }

    /// The path.
    #[must_use]
    pub fn path(&self) -> &TextPath {
        &self.path
    }

    /// The flow to lay the story out with before fitting: a column as long
    /// as the path between its indents, which does not wrap. The layout's
    /// x = 0 is then the left indent.
    #[must_use]
    pub fn story_mode(&self) -> StoryMode {
        let len = Mp::from_f64_round(self.path.length);
        let width = len
            .saturating_sub(self.style.left_indent)
            .saturating_sub(self.style.right_indent);
        StoryMode::Column {
            width: width.max(Mp::ZERO),
            wrap: false,
        }
    }

    /// The transform that takes `cluster`'s glyphs (as placed on `line`
    /// by a layout made with [`PathFit::story_mode`]) onto the path.
    #[must_use]
    pub fn cluster_transform(&self, line: &LaidLine, cluster: &LaidCluster) -> Affine {
        let scale = if self.style.reflected { -1.0 } else { 1.0 };
        let pen = cluster.pen.to_f64();
        let base = line.baseline_y.to_f64();
        let half = cluster.advance.to_f64() / 2.0;
        let dist = pen + self.style.left_indent.to_f64() + half;
        let (at, angle) = self.path.at(dist);
        let angle = if self.style.tangential { angle } else { 0.0 };
        let offset = self.path.start_normal() * (base * scale);
        let shear = scale * self.style.shear.tan();
        let shear = if shear.is_finite() { shear } else { 0.0 };
        let local = Affine::translate((-pen, -base));
        let pre = Affine::new([1.0, 0.0, shear, scale, 0.0, 0.0]);
        let turn = Affine::translate((half, 0.0))
            * Affine::rotate(angle)
            * Affine::translate((-half, 0.0));
        let place = Affine::translate((at.x - half + offset.x, at.y + offset.y));
        place * turn * pre * local
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(from: (f64, f64), to: (f64, f64)) -> BezPath {
        let mut p = BezPath::new();
        p.move_to(from);
        p.line_to(to);
        p
    }

    #[test]
    fn a_straight_path_is_parameterised_exactly() {
        let t = TextPath::new(&line((0.0, 0.0), (300.0, 400.0)), false).unwrap();
        assert!((t.length() - 500.0).abs() < 1e-9);
        let (p, a) = t.at(250.0);
        assert!((p - Point::new(150.0, 200.0)).hypot() < 1e-6);
        assert!((a - (400f64).atan2(300.0)).abs() < 1e-12);
        // Off the ends it continues straight.
        let (p, _) = t.at(-50.0);
        assert!((p - Point::new(-30.0, -40.0)).hypot() < 1e-6);
        let (p, _) = t.at(550.0);
        assert!((p - Point::new(330.0, 440.0)).hypot() < 1e-6);
    }

    #[test]
    fn reversing_runs_from_the_end() {
        let t = TextPath::new(&line((0.0, 0.0), (100.0, 0.0)), true).unwrap();
        let (p, a) = t.at(10.0);
        assert!((p - Point::new(90.0, 0.0)).hypot() < 1e-9);
        assert!((a.abs() - std::f64::consts::PI).abs() < 1e-12);
    }

    #[test]
    fn a_closed_path_wraps_around() {
        let mut p = BezPath::new();
        p.move_to((0.0, 0.0));
        p.line_to((100.0, 0.0));
        p.line_to((100.0, 100.0));
        p.line_to((0.0, 100.0));
        p.close_path();
        let t = TextPath::new(&p, false).unwrap();
        assert!(t.is_closed());
        assert!((t.length() - 400.0).abs() < 1e-9);
        let (a, _) = t.at(450.0);
        assert!((a - Point::new(50.0, 0.0)).hypot() < 1e-9);
        let (b, _) = t.at(-50.0);
        assert!((b - Point::new(0.0, 50.0)).hypot() < 1e-9);
    }

    #[test]
    fn a_collapsed_handle_still_has_a_direction() {
        let mut p = BezPath::new();
        p.move_to((0.0, 0.0));
        p.curve_to((0.0, 0.0), (100.0, 0.0), (100.0, 100.0));
        let t = TextPath::new(&p, false).unwrap();
        let n = t.start_normal();
        assert!((n.hypot() - 1.0).abs() < 1e-9, "{n:?}");
    }

    #[test]
    fn a_point_or_an_empty_path_has_nothing_to_follow() {
        assert!(TextPath::new(&BezPath::new(), false).is_none());
        assert!(TextPath::new(&line((5.0, 5.0), (5.0, 5.0)), false).is_none());
    }

    fn cluster(pen: i32, advance: i32) -> LaidCluster {
        LaidCluster {
            range: 0..1,
            x: Mp::new(pen),
            width: Mp::new(advance),
            pen: Mp::new(pen),
            advance: Mp::new(advance),
            rtl: false,
        }
    }

    fn laid_line(baseline: i32) -> LaidLine {
        LaidLine {
            logical_range: 0..1,
            paragraph: 0,
            ends_paragraph: true,
            baseline_y: Mp::new(baseline),
            descent_line: Mp::ZERO,
            ascent: Mp::ZERO,
            descent: Mp::ZERO,
            size: Mp::ZERO,
            base_rtl: false,
            x: Mp::ZERO,
            width: Mp::ZERO,
            runs: Vec::new(),
            clusters: Vec::new(),
        }
    }

    #[test]
    fn a_straight_horizontal_path_changes_nothing_but_the_start() {
        let fit = PathFit::new(
            TextPath::new(&line((1000.0, 2000.0), (90_000.0, 2000.0)), false).unwrap(),
            PathFitStyle::default(),
        );
        let xf = fit.cluster_transform(&laid_line(0), &cluster(5000, 800));
        let p = xf * Point::new(5000.0, 0.0);
        assert!((p - Point::new(6000.0, 2000.0)).hypot() < 1e-6, "{p:?}");
        // A second line goes below, along the start normal.
        let xf = fit.cluster_transform(&laid_line(-12_000), &cluster(5000, 800));
        let p = xf * Point::new(5000.0, -12_000.0);
        assert!((p - Point::new(6000.0, -10_000.0)).hypot() < 1e-6, "{p:?}");
    }

    #[test]
    fn a_character_turns_about_its_centre_and_sits_on_the_path() {
        // A vertical path going up: the text reads bottom to top.
        let fit = PathFit::new(
            TextPath::new(&line((0.0, 0.0), (0.0, 100_000.0)), false).unwrap(),
            PathFitStyle::default(),
        );
        let c = cluster(10_000, 2_000);
        let xf = fit.cluster_transform(&laid_line(0), &c);
        // The baseline centre lands on the path at 11 000.
        let p = xf * Point::new(11_000.0, 0.0);
        assert!((p - Point::new(0.0, 11_000.0)).hypot() < 1e-6, "{p:?}");
        // The top of the character points left of the path.
        let top = xf * Point::new(11_000.0, 5_000.0);
        assert!(
            (top - Point::new(-5_000.0, 11_000.0)).hypot() < 1e-6,
            "{top:?}"
        );
        // Upright characters do not turn.
        let upright = PathFit::new(
            fit.path().clone(),
            PathFitStyle {
                tangential: false,
                ..PathFitStyle::default()
            },
        );
        let top = upright.cluster_transform(&laid_line(0), &c) * Point::new(11_000.0, 5_000.0);
        assert!((top - Point::new(0.0, 16_000.0)).hypot() < 1e-6, "{top:?}");
    }

    #[test]
    fn indents_become_margins_and_reflection_flips_the_side() {
        let path = TextPath::new(&line((0.0, 0.0), (100_000.0, 0.0)), false).unwrap();
        let style = PathFitStyle {
            left_indent: Mp::new(10_000),
            right_indent: Mp::new(5_000),
            reflected: true,
            ..PathFitStyle::default()
        };
        let fit = PathFit::new(path, style);
        assert_eq!(
            fit.story_mode(),
            StoryMode::Column {
                width: Mp::new(85_000),
                wrap: false
            }
        );
        let xf = fit.cluster_transform(&laid_line(0), &cluster(0, 1_000));
        let p = xf * Point::new(500.0, 0.0);
        assert!((p - Point::new(10_500.0, 0.0)).hypot() < 1e-6, "{p:?}");
        let top = xf * Point::new(500.0, 3_000.0);
        assert!(
            (top - Point::new(10_500.0, -3_000.0)).hypot() < 1e-6,
            "{top:?}"
        );
    }
}
