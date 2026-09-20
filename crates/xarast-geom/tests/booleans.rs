//! Boolean operations: algebra, degeneracy, determinism and robustness.

mod corpus;

use corpus::{corpus, small_corpus};
use proptest::prelude::*;
use xarast_geom::{BoolOp, FillRule, Mp, Path, Point, Rect, Tolerance, boolean, self_union};

/// A closed square.
fn square(x0: i32, y0: i32, x1: i32, y1: i32) -> Path {
    let mut b = Path::builder();
    b.rect(Rect::raw(x0, y0, x1, y1));
    b.build()
}

const TOL: Tolerance = Tolerance::BOOLEAN;

/// How much area error to allow in the conservation identities.
///
/// Two terms. The relative one is the phase document's `1e-4` of the result
/// area. The absolute one is the price of integer millipoints: the engine
/// places every intersection on the lattice, so a vertex can move by up to a
/// millipoint, and a one-millipoint band around a shape of area `A` has area
/// of order `4 * sqrt(A)`. Below that the identity is measuring the
/// quantisation, not the engine.
fn area_slack(area: f64) -> f64 {
    1e-4 * area.abs().max(1.0) + 4.0 * area.abs().sqrt().max(1.0)
}

/// The same, plus the flattening term, for identities that compare results
/// obtained by different routes through the pipeline.
///
/// Flattening at tolerance `t` can move the boundary by `t`, so it can change
/// an enclosed area by up to `t` times the perimeter. Demanding better than
/// that of a curved input would be demanding accuracy the polygonal engine
/// cannot have; the term matters only for shapes that are mostly curve, and
/// it dominates exactly where it should.
fn curve_slack(area: f64, a: &Path, b: &Path) -> f64 {
    let perimeter = xarast_geom::arclen(a, 1.0) + xarast_geom::arclen(b, 1.0);
    area_slack(area) + 2.0 * TOL.0 * perimeter
}

#[test]
fn union_of_disjoint_squares_keeps_both() {
    let a = square(0, 0, 1000, 1000);
    let b = square(5000, 0, 6000, 1000);
    let u = boolean(&a, &b, BoolOp::Union, FillRule::NonZero, TOL);
    assert_eq!(u.subpaths().count(), 2);
    assert!((u.signed_area() - 2_000_000.0).abs() < 1.0);
}

#[test]
fn union_of_overlapping_squares_merges_them() {
    let a = square(0, 0, 1000, 1000);
    let b = square(500, 0, 1500, 1000);
    let u = boolean(&a, &b, BoolOp::Union, FillRule::NonZero, TOL);
    assert_eq!(u.subpaths().count(), 1);
    assert!(
        (u.signed_area() - 1_500_000.0).abs() < 1.0,
        "{}",
        u.signed_area()
    );
}

#[test]
fn intersection_difference_and_xor() {
    let a = square(0, 0, 1000, 1000);
    let b = square(500, 500, 1500, 1500);
    let i = boolean(&a, &b, BoolOp::Intersection, FillRule::NonZero, TOL);
    assert!(
        (i.signed_area() - 250_000.0).abs() < 1.0,
        "{}",
        i.signed_area()
    );
    let d = boolean(&a, &b, BoolOp::Difference, FillRule::NonZero, TOL);
    assert!(
        (d.signed_area() - 750_000.0).abs() < 1.0,
        "{}",
        d.signed_area()
    );
    let x = boolean(&a, &b, BoolOp::Xor, FillRule::NonZero, TOL);
    assert!(
        (x.signed_area() - 1_500_000.0).abs() < 1.0,
        "{}",
        x.signed_area()
    );
}

#[test]
fn difference_producing_a_hole() {
    let a = square(0, 0, 10_000, 10_000);
    let b = square(2_000, 2_000, 8_000, 8_000);
    let d = boolean(&a, &b, BoolOp::Difference, FillRule::NonZero, TOL);
    assert_eq!(d.subpaths().count(), 2, "an outer and a hole");
    assert!((d.signed_area() - (100_000_000.0 - 36_000_000.0)).abs() < 1.0);
}

#[test]
fn the_empty_path_is_the_union_identity() {
    for case in small_corpus() {
        let u = boolean(
            &case.path,
            &Path::new(),
            BoolOp::Union,
            FillRule::NonZero,
            TOL,
        );
        // Compared against `self_union`, not against the input: the boolean
        // engine resolves self-intersections and a self-intersecting input
        // therefore has a different area before and after, correctly.
        let n = self_union(&case.path, FillRule::NonZero, TOL);
        assert_eq!(u, n, "{}", case.name);
    }
}

#[test]
fn difference_with_itself_is_empty() {
    for case in small_corpus() {
        let d = boolean(
            &case.path,
            &case.path,
            BoolOp::Difference,
            FillRule::NonZero,
            TOL,
        );
        assert!(
            d.is_empty(),
            "{} left {} verbs behind",
            case.name,
            d.verbs().len()
        );
    }
}

#[test]
fn intersection_with_itself_is_itself() {
    for case in small_corpus() {
        let i = boolean(
            &case.path,
            &case.path,
            BoolOp::Intersection,
            FillRule::NonZero,
            TOL,
        );
        let u = boolean(
            &case.path,
            &case.path,
            BoolOp::Union,
            FillRule::NonZero,
            TOL,
        );
        assert!(
            (i.signed_area() - u.signed_area()).abs()
                <= curve_slack(u.signed_area(), &case.path, &case.path),
            "{}: {} vs {}",
            case.name,
            i.signed_area(),
            u.signed_area()
        );
    }
}

#[test]
fn inclusion_exclusion_holds_over_the_corpus() {
    // |A| + |B| == |A u B| + |A n B|, the identity every correct boolean
    // engine satisfies and the strongest cheap check there is.
    let cases = small_corpus();
    for a in &cases {
        for b in &cases {
            let u = boolean(&a.path, &b.path, BoolOp::Union, FillRule::NonZero, TOL);
            let i = boolean(
                &a.path,
                &b.path,
                BoolOp::Intersection,
                FillRule::NonZero,
                TOL,
            );
            let sa = self_union(&a.path, FillRule::NonZero, TOL).signed_area();
            let sb = self_union(&b.path, FillRule::NonZero, TOL).signed_area();
            let err = (u.signed_area() + i.signed_area() - sa - sb).abs();
            let scale = u.signed_area().abs().max(1.0);
            assert!(
                err <= curve_slack(scale, &a.path, &b.path),
                "{} vs {}: error {err} over area {scale}",
                a.name,
                b.name
            );
        }
    }
}

#[test]
fn xor_equals_difference_of_union_and_intersection() {
    let cases = small_corpus();
    for a in cases.iter().take(12) {
        for b in cases.iter().take(12) {
            let x = boolean(&a.path, &b.path, BoolOp::Xor, FillRule::NonZero, TOL);
            let u = boolean(&a.path, &b.path, BoolOp::Union, FillRule::NonZero, TOL);
            let i = boolean(
                &a.path,
                &b.path,
                BoolOp::Intersection,
                FillRule::NonZero,
                TOL,
            );
            let d = boolean(&u, &i, BoolOp::Difference, FillRule::NonZero, TOL);
            let err = (x.signed_area() - d.signed_area()).abs();
            assert!(
                err <= curve_slack(x.signed_area(), &a.path, &b.path),
                "{} vs {}: {} != {}",
                a.name,
                b.name,
                x.signed_area(),
                d.signed_area()
            );
        }
    }
}

#[test]
fn degenerate_inputs_do_not_panic() {
    let cases = corpus();
    let ops = [
        BoolOp::Union,
        BoolOp::Intersection,
        BoolOp::Difference,
        BoolOp::Xor,
    ];
    let rules = [
        FillRule::NonZero,
        FillRule::EvenOdd,
        FillRule::Positive,
        FillRule::Negative,
    ];
    for a in cases.iter().filter(|c| c.path.segment_count() <= 64) {
        for b in cases.iter().filter(|c| c.path.segment_count() <= 64) {
            for op in ops {
                for rule in rules {
                    let out = boolean(&a.path, &b.path, op, rule, TOL);
                    assert_eq!(
                        out.validate(),
                        Ok(()),
                        "{} {op:?} {} under {rule:?} produced an invalid path",
                        a.name,
                        b.name
                    );
                }
            }
        }
    }
}

#[test]
fn large_inputs_complete() {
    let cases = corpus();
    let big: Vec<_> = cases
        .iter()
        .filter(|c| c.path.segment_count() >= 1_000)
        .collect();
    assert!(!big.is_empty(), "the corpus must contain large cases");
    for c in big {
        let out = boolean(&c.path, &c.path, BoolOp::Union, FillRule::NonZero, TOL);
        assert_eq!(out.validate(), Ok(()), "{}", c.name);
        assert!(!out.is_empty(), "{} vanished", c.name);
    }
}

#[test]
fn results_are_bit_reproducible() {
    // The integer engine sees exact millipoints, so the same inputs give the
    // same output every time, in the same order. Golden tests and undo/redo
    // both depend on this.
    let cases = small_corpus();
    for a in &cases {
        for b in cases.iter().take(8) {
            let one = boolean(&a.path, &b.path, BoolOp::Union, FillRule::NonZero, TOL);
            let two = boolean(&a.path, &b.path, BoolOp::Union, FillRule::NonZero, TOL);
            assert_eq!(
                one.to_svg_path_data(),
                two.to_svg_path_data(),
                "{} u {} is not reproducible",
                a.name,
                b.name
            );
        }
    }
}

#[test]
fn boolean_output_is_canonically_oriented() {
    let a = square(0, 0, 10_000, 10_000);
    let b = square(2_000, 2_000, 8_000, 8_000);
    let d = boolean(&a, &b, BoolOp::Difference, FillRule::NonZero, TOL);
    let areas: Vec<f64> = d
        .subpaths()
        .map(|sp| {
            d.subpath_segments(&sp)
                .iter()
                .map(|s| {
                    use kurbo::ParamCurveArea;
                    s.to_kurbo().signed_area()
                })
                .sum()
        })
        .collect();
    assert!(
        areas.iter().any(|a| *a > 0.0),
        "an outer contour: {areas:?}"
    );
    assert!(areas.iter().any(|a| *a < 0.0), "a hole: {areas:?}");
}

#[test]
fn curves_survive_a_boolean_that_does_not_cut_them() {
    // The refit step exists so that a circle unioned with a distant square
    // still has cubics in it afterwards. Without it, every boolean would
    // turn every curve in the document into a polyline, cumulatively.
    let mut cb = Path::builder();
    cb.ellipse(Point::ORIGIN, Mp::new(10_000), Mp::new(10_000));
    let circle = cb.build();
    let far = square(100_000, 100_000, 110_000, 110_000);
    let u = boolean(&circle, &far, BoolOp::Union, FillRule::NonZero, TOL);
    let cubics = u
        .verbs()
        .iter()
        .filter(|v| **v == xarast_geom::Verb::CubicTo)
        .count();
    assert_eq!(
        cubics,
        4,
        "the circle's four cubics must be restored: {:?}",
        u.verbs()
    );
}

#[test]
fn repeated_booleans_do_not_degrade_the_curve() {
    // Risk 1: apply twenty booleans in sequence and check the area holds.
    let mut cb = Path::builder();
    cb.ellipse(Point::ORIGIN, Mp::new(100_000), Mp::new(100_000));
    let circle = cb.build();
    let expected = circle.signed_area();
    let mut cur = circle.clone();
    for _ in 0..20 {
        cur = boolean(&cur, &circle, BoolOp::Union, FillRule::NonZero, TOL);
    }
    let err = (cur.signed_area() - expected).abs() / expected.abs();
    assert!(err < 1e-4, "area drifted by {err} over twenty unions");
}

#[test]
fn self_union_resolves_a_self_intersection() {
    // A figure eight under the non-zero rule resolves into two lobes with
    // opposite winding; under even-odd it is two separate positive lobes.
    let eight = {
        let mut b = Path::builder();
        b.move_to(Point::raw(0, 0));
        b.line_to(Point::raw(1000, 1000));
        b.line_to(Point::raw(0, 1000));
        b.line_to(Point::raw(1000, 0));
        b.close();
        b.build()
    };
    let out = self_union(&eight, FillRule::EvenOdd, TOL);
    assert_eq!(out.validate(), Ok(()));
    assert!(
        out.signed_area() > 0.0,
        "even-odd leaves two positive lobes"
    );
    assert!(self_union(&Path::new(), FillRule::NonZero, TOL).is_empty());
}

#[test]
fn self_union_is_idempotent_in_area() {
    for case in small_corpus() {
        let once = self_union(&case.path, FillRule::NonZero, TOL);
        let twice = self_union(&once, FillRule::NonZero, TOL);
        assert!(
            (once.signed_area() - twice.signed_area()).abs()
                <= curve_slack(once.signed_area(), &case.path, &once),
            "{}",
            case.name
        );
    }
}

fn any_square() -> impl Strategy<Value = Path> {
    (-5_000i32..5_000, -5_000i32..5_000, 1i32..5_000, 1i32..5_000)
        .prop_map(|(x, y, w, h)| square(x, y, x + w, y + h))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2_000))]

    /// Inclusion-exclusion, as a property rather than over a fixed corpus.
    #[test]
    fn areas_are_conserved(a in any_square(), b in any_square()) {
        let u = boolean(&a, &b, BoolOp::Union, FillRule::NonZero, TOL);
        let i = boolean(&a, &b, BoolOp::Intersection, FillRule::NonZero, TOL);
        let err = (u.signed_area() + i.signed_area() - a.signed_area() - b.signed_area()).abs();
        prop_assert!(err <= area_slack(u.signed_area()), "error {}", err);
    }

    /// Union with nothing changes nothing.
    #[test]
    fn union_with_empty_is_identity(a in any_square()) {
        let u = boolean(&a, &Path::new(), BoolOp::Union, FillRule::NonZero, TOL);
        prop_assert_eq!(u, a.normalised());
    }

    /// Difference with itself leaves nothing.
    #[test]
    fn difference_with_self_is_empty(a in any_square()) {
        prop_assert!(boolean(&a, &a, BoolOp::Difference, FillRule::NonZero, TOL).is_empty());
    }

    /// Every output is a valid path under every operation and rule.
    #[test]
    fn output_always_validates(a in any_square(), b in any_square()) {
        for op in [BoolOp::Union, BoolOp::Intersection, BoolOp::Difference, BoolOp::Xor] {
            for rule in [FillRule::NonZero, FillRule::EvenOdd] {
                prop_assert_eq!(boolean(&a, &b, op, rule, TOL).validate(), Ok(()));
            }
        }
    }
}
