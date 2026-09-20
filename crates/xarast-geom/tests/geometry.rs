//! Points, rectangles, matrices and profiles.

use proptest::prelude::*;
use xarast_geom::{BiasGain, Fixed16, Matrix, Mp, Point, Rect, Vector};

#[test]
fn sizes_are_as_specified() {
    assert_eq!(size_of::<Point>(), 8);
    assert_eq!(size_of::<Vector>(), 8);
    assert_eq!(size_of::<Rect>(), 16);
    assert_eq!(size_of::<xarast_geom::Verb>(), 1);
    assert_eq!(size_of::<xarast_geom::PointFlags>(), 1);
}

#[test]
fn empty_rect_is_the_union_identity() {
    let r = Rect::raw(10, 20, 30, 40);
    assert_eq!(Rect::EMPTY.union(r), r);
    assert_eq!(r.union(Rect::EMPTY), r);
    assert!(Rect::EMPTY.is_empty());
    assert_eq!(
        Rect::EMPTY.union_point(Point::raw(5, 5)),
        Rect::from_point(Point::raw(5, 5))
    );
    assert_eq!(Rect::EMPTY.width(), Mp::ZERO);
    assert_eq!(Rect::EMPTY.height(), Mp::ZERO);
    assert_eq!(Rect::default(), Rect::EMPTY);
}

#[test]
fn rect_normalises_inverted_corners() {
    let a = Rect::new(Point::raw(30, 40), Point::raw(10, 20));
    assert_eq!(a, Rect::raw(10, 20, 30, 40));
    assert!(!a.is_empty());
}

#[test]
fn rect_geometry() {
    let r = Rect::raw(0, 0, 100, 50);
    assert_eq!(r.width(), Mp::new(100));
    assert_eq!(r.height(), Mp::new(50));
    assert_eq!(r.centre(), Point::raw(50, 25));
    assert!(r.contains(Point::raw(0, 0)));
    assert!(r.contains(Point::raw(100, 50)));
    assert!(!r.contains(Point::raw(101, 0)));
    assert!(r.contains_rect(Rect::raw(10, 10, 20, 20)));
    assert!(r.contains_rect(Rect::EMPTY));
    assert!(!r.contains_rect(Rect::raw(-1, 0, 10, 10)));
    assert_eq!(r.inflated(Mp::new(10)), Rect::raw(-10, -10, 110, 60));
    assert!(r.inflated(Mp::new(-1000)).is_empty());
    assert_eq!(r.translated(Vector::raw(5, 5)), Rect::raw(5, 5, 105, 55));
    assert!(Rect::EMPTY.inflated(Mp::new(5)).is_empty());
    assert!(Rect::EMPTY.translated(Vector::raw(5, 5)).is_empty());
}

#[test]
fn rect_intersection() {
    let a = Rect::raw(0, 0, 100, 100);
    let b = Rect::raw(50, 50, 150, 150);
    assert_eq!(a.intersection(b), Rect::raw(50, 50, 100, 100));
    assert!(a.intersects(b));
    let c = Rect::raw(200, 200, 300, 300);
    assert!(a.intersection(c).is_empty());
    assert!(!a.intersects(c));
    // Touching along an edge counts as intersecting: the shared edge is a
    // real set of points, and culling that dropped it would leave seams.
    assert!(a.intersects(Rect::raw(100, 0, 200, 100)));
}

#[test]
fn y_is_up() {
    // lo is the bottom-left, so a rect built from a "top-left plus size" in a
    // Y-down frame would come out inverted, which is the mistake this test
    // documents.
    let r = Rect::new(Point::raw(0, 0), Point::raw(10, 10));
    assert_eq!(r.lo, Point::raw(0, 0));
    assert_eq!(r.hi, Point::raw(10, 10));
    assert!(r.lo.y < r.hi.y);
}

#[test]
fn points_translate_and_vectors_do_not() {
    let m = Matrix::translate(Vector::raw(100, 200));
    assert_eq!(m.transform_point(Point::raw(1, 2)), Point::raw(101, 202));
    assert_eq!(m.transform_vector(Vector::raw(1, 2)), Vector::raw(1, 2));
}

#[test]
fn composition_order_is_self_then_other() {
    // Scale by two, then translate by ten: the point ends at 2x + 10, not
    // 2(x + 10). Getting this backwards mirrors every nested group.
    let m = Matrix::scale(2.0, 2.0).then(Matrix::translate(Vector::raw(10, 0)));
    assert_eq!(m.transform_point(Point::raw(5, 0)), Point::raw(20, 0));
    let n = Matrix::translate(Vector::raw(10, 0)).then(Matrix::scale(2.0, 2.0));
    assert_eq!(n.transform_point(Point::raw(5, 0)), Point::raw(30, 0));
}

#[test]
fn identity_and_inversion() {
    assert!(Matrix::IDENTITY.is_identity());
    assert!(Matrix::translate(Vector::raw(1, 1)).is_translation_only());
    assert!(!Matrix::scale(2.0, 1.0).is_translation_only());
    // A singular matrix has no inverse.
    assert!(Matrix::scale(0.0, 1.0).invert().is_none());
    assert!(Matrix::scale(1.0, 0.0).invert().is_none());
    let m = Matrix::rotate(0.7).then(Matrix::translate(Vector::raw(500, -300)));
    let inv = m.invert().expect("rotation and translation are invertible");
    let p = Point::raw(1234, -5678);
    assert_eq!(inv.transform_point(m.transform_point(p)), p);
}

#[test]
fn max_scale_is_the_largest_singular_value() {
    assert!((Matrix::IDENTITY.max_scale() - 1.0).abs() < 1e-12);
    assert!((Matrix::scale(3.0, 1.0).max_scale() - 3.0).abs() < 1e-12);
    assert!((Matrix::scale(1.0, 4.0).max_scale() - 4.0).abs() < 1e-12);
    // Rotation preserves lengths.
    assert!((Matrix::rotate(1.1).max_scale() - 1.0).abs() < 1e-12);
    // And rotating a scale does not change the largest stretch.
    let m = Matrix::scale(3.0, 1.0).then(Matrix::rotate(0.4));
    assert!((m.max_scale() - 3.0).abs() < 1e-9);
}

#[test]
fn transform_rect_bounds_a_rotation() {
    let r = Rect::raw(0, 0, 1000, 1000);
    let m = Matrix::rotate(std::f64::consts::FRAC_PI_4);
    let out = m.transform_rect(r);
    let diag = (1000.0 * std::f64::consts::SQRT_2).round() as i32;
    assert_eq!(out.width().raw(), diag);
    assert!(m.transform_rect(Rect::EMPTY).is_empty());
}

#[test]
fn fixed16_quantisation_happens_only_on_demand() {
    let m = Matrix::rotate(0.123_456_789);
    // The unquantised matrix keeps full f64 precision.
    assert_ne!(m.a, Fixed16::quantise(m.a));
    let q = m.quantise_fixed16();
    assert_eq!(q.a, Fixed16::quantise(m.a));
    // Quantising is idempotent, so a value read from a file survives a
    // second pass unchanged.
    assert_eq!(q.quantise_fixed16(), q);
}

#[test]
fn unquantised_composition_does_not_drift() {
    // Risk 6: composing a thousand rotations must not accumulate error in
    // the f64 path. Quantising each step would visibly drift.
    let step = std::f64::consts::TAU / 1000.0;
    let mut m = Matrix::IDENTITY;
    for _ in 0..1000 {
        m = m.then(Matrix::rotate(step));
    }
    assert!((m.a - 1.0).abs() < 1e-9, "a drifted to {}", m.a);
    assert!(m.b.abs() < 1e-9, "b drifted to {}", m.b);
}

#[test]
fn fixed16_codec() {
    assert_eq!(Fixed16::ONE.to_f64(), 1.0);
    assert_eq!(Fixed16::from_f64_round(1.0), Fixed16::ONE);
    assert_eq!(Fixed16::from_f64_round(f64::NAN), Fixed16::ZERO);
    assert_eq!(Fixed16::from_f64_round(1e300), Fixed16(i32::MAX));
    assert_eq!(Fixed16::from_f64_round(-1e300), Fixed16(i32::MIN));
}

#[test]
fn bias_gain_identity_and_endpoints() {
    let id = BiasGain::IDENTITY;
    for i in 0..=10 {
        let t = i as f64 / 10.0;
        assert!((id.map(t) - t).abs() < 1e-9, "identity moved {t}");
    }
    // Endpoints are fixed for every profile, which is what keeps a gradient's
    // end colours at its ends.
    for b in [-1.0, -0.5, 0.0, 0.5, 1.0] {
        for g in [-1.0, -0.5, 0.0, 0.5, 1.0] {
            let p = BiasGain::new(b, g);
            assert!(p.map(0.0).abs() < 1e-6, "bias {b} gain {g} moved 0");
            assert!((p.map(1.0) - 1.0).abs() < 1e-6, "bias {b} gain {g} moved 1");
        }
    }
    // Out-of-range and NaN parameters are clamped rather than propagated.
    assert_eq!(
        BiasGain::new(5.0, -5.0),
        BiasGain {
            bias: 1.0,
            gain: -1.0
        }
    );
    assert_eq!(BiasGain::new(f64::NAN, f64::NAN), BiasGain::IDENTITY);
}

#[test]
fn bias_gain_lut() {
    let p = BiasGain::new(0.3, -0.2);
    let lut = p.lut(5);
    assert_eq!(lut.len(), 5);
    assert!((lut[0] - p.map(0.0) as f32).abs() < 1e-6);
    assert!((lut[4] - p.map(1.0) as f32).abs() < 1e-6);
    assert!(BiasGain::IDENTITY.lut(0).is_empty());
    assert_eq!(BiasGain::IDENTITY.lut(1).len(), 1);
}

fn small_mp() -> impl Strategy<Value = Mp> {
    (-1_000_000i32..1_000_000).prop_map(Mp::new)
}

fn any_point() -> impl Strategy<Value = Point> {
    (small_mp(), small_mp()).prop_map(|(x, y)| Point::new(x, y))
}

/// Matrices built from the primitives, so that the generated set is the set
/// the editor can actually produce.
fn any_matrix() -> impl Strategy<Value = Matrix> {
    prop_oneof![
        (0.1f64..10.0, 0.1f64..10.0).prop_map(|(x, y)| Matrix::scale(x, y)),
        (-6.3f64..6.3).prop_map(Matrix::rotate),
        (small_mp(), small_mp()).prop_map(|(x, y)| Matrix::translate(Vector::new(x, y))),
        (-1.0f64..1.0, -1.0f64..1.0).prop_map(|(x, y)| Matrix::skew(x, y)),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// Composition followed by inversion returns the identity, in the linear
    /// part to 1e-9 and in the translation to a millipoint.
    #[test]
    fn matrix_inverse_round_trip(m in any_matrix()) {
        let Some(inv) = m.invert() else { return Ok(()) };
        let id = m.then(inv);
        prop_assert!((id.a - 1.0).abs() < 1e-9, "a = {}", id.a);
        prop_assert!(id.b.abs() < 1e-9, "b = {}", id.b);
        prop_assert!(id.c.abs() < 1e-9, "c = {}", id.c);
        prop_assert!((id.d - 1.0).abs() < 1e-9, "d = {}", id.d);
        prop_assert!(id.e.abs() <= Mp::new(1), "e = {:?}", id.e);
        prop_assert!(id.f.abs() <= Mp::new(1), "f = {:?}", id.f);
    }

    /// Composition is associative in the linear part, and associative in the
    /// translation up to the rounding the millipoint storage forces.
    ///
    /// The translation is stored as `Mp`, so each composition rounds it to a
    /// whole millipoint and a later scale multiplies that rounding by its own
    /// factor. The bound below is that error, not slack: it is why a long
    /// transform chain should be composed once and applied once rather than
    /// applied step by step.
    #[test]
    fn composition_is_associative(a in any_matrix(), b in any_matrix(), c in any_matrix()) {
        let l = a.then(b).then(c);
        let r = a.then(b.then(c));
        prop_assert!((l.a - r.a).abs() < 1e-9);
        prop_assert!((l.b - r.b).abs() < 1e-9);
        prop_assert!((l.c - r.c).abs() < 1e-9);
        prop_assert!((l.d - r.d).abs() < 1e-9);
        let slack = Mp::from_f64_round(2.0 * (1.0 + c.max_scale() * (1.0 + b.max_scale())));
        prop_assert!((l.e - r.e).abs() <= slack, "{:?} vs {:?}", l.e, r.e);
        prop_assert!((l.f - r.f).abs() <= slack, "{:?} vs {:?}", l.f, r.f);
    }

    /// Composing then transforming equals transforming twice.
    #[test]
    fn composition_matches_sequential_application(
        a in any_matrix(),
        b in any_matrix(),
        p in any_point(),
    ) {
        let one = a.then(b).transform_point(p);
        let two = b.transform_point(a.transform_point(p));
        // Applying step by step rounds the intermediate point to a whole
        // millipoint, and `b` then scales that rounding up. Composing first
        // is therefore the more accurate order as well as the faster one.
        let slack = Mp::from_f64_round(2.0 * (1.0 + b.max_scale()));
        prop_assert!((one.x - two.x).abs() <= slack, "{:?} vs {:?}", one, two);
        prop_assert!((one.y - two.y).abs() <= slack, "{:?} vs {:?}", one, two);
    }

    /// Union is commutative, associative and contains both arguments.
    #[test]
    fn rect_union_is_a_join(a in any_point(), b in any_point(), c in any_point()) {
        let (r, s) = (Rect::new(a, b), Rect::from_point(c));
        prop_assert_eq!(r.union(s), s.union(r));
        prop_assert!(r.union(s).contains_rect(r));
        prop_assert!(r.union(s).contains_rect(s));
        prop_assert_eq!(r.union(r), r);
    }

    /// Intersection is contained in both arguments.
    #[test]
    fn rect_intersection_is_a_meet(a in any_point(), b in any_point(), c in any_point(), d in any_point()) {
        let (r, s) = (Rect::new(a, b), Rect::new(c, d));
        let i = r.intersection(s);
        prop_assert_eq!(i.is_empty(), s.intersection(r).is_empty());
        if !i.is_empty() {
            prop_assert!(r.contains_rect(i));
            prop_assert!(s.contains_rect(i));
        }
    }

    /// A bias/gain profile is monotone and stays inside the unit interval —
    /// the two properties that make it safe to use as a gradient ramp.
    #[test]
    fn bias_gain_is_monotone(bias in -1.0f64..1.0, gain in -1.0f64..1.0) {
        let p = BiasGain::new(bias, gain);
        let mut prev = p.map(0.0);
        for i in 1..=64 {
            let v = p.map(i as f64 / 64.0);
            prop_assert!(v.is_finite());
            prop_assert!((-1e-9..=1.0 + 1e-9).contains(&v), "{v} left the unit interval");
            prop_assert!(v >= prev - 1e-9, "not monotone at {i}: {prev} then {v}");
            prev = v;
        }
    }
}
