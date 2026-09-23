//! Nearest point on transformed paths, through the object index
//! (XARA-T-0153): what snapping to outlines uses.

use xarast_geom::{
    HitIndex, Matrix, Mp, Path, Point, nearest_in_index, nearest_point, nearest_point_transformed,
};

fn square(x: i32, y: i32, side: i32) -> Path {
    let mut b = Path::builder();
    b.move_to(Point::raw(x, y))
        .line_to(Point::raw(x + side, y))
        .line_to(Point::raw(x + side, y + side))
        .line_to(Point::raw(x, y + side))
        .close();
    b.build()
}

#[test]
fn identity_agrees_with_the_untransformed_nearest_point() {
    let p = square(0, 0, 10_000);
    let q = Point::raw(4_000, -300);
    let a = nearest_point(&p, q, 1e-3).unwrap();
    let b = nearest_point_transformed(&p, Matrix::IDENTITY, q, Mp::new(1_000), 1e-3).unwrap();
    assert_eq!(a.point, b.point);
    assert!((a.distance - b.distance).abs() < 1e-9);
    assert_eq!(b.point, Point::raw(4_000, 0));
}

#[test]
fn the_transform_is_applied_before_measuring_and_the_radius_is_respected() {
    let p = square(0, 0, 10_000);
    // Rotated a quarter turn about the origin: the square now spans
    // x ∈ [-10 000, 0].
    let m = Matrix::rotate(std::f64::consts::FRAC_PI_2);
    let q = Point::raw(-5_000, 10_400);
    let n = nearest_point_transformed(&p, m, q, Mp::new(500), 1e-3).unwrap();
    assert_eq!(n.point, Point::raw(-5_000, 10_000));
    assert!((n.distance - 400.0).abs() < 1e-6);
    assert!(nearest_point_transformed(&p, m, q, Mp::new(399), 1e-3).is_none());
    // A curve: an ellipse-ish cubic scaled by 3 on x.
    let mut b = Path::builder();
    b.move_to(Point::raw(0, 0)).cubic_to(
        Point::raw(0, 5_000),
        Point::raw(5_000, 5_000),
        Point::raw(5_000, 0),
    );
    let c = b.build();
    let s = Matrix::scale(3.0, 1.0);
    let top = Point::raw(7_500, 3_750 + 100);
    let n = nearest_point_transformed(&c, s, top, Mp::new(1_000), 1e-3).unwrap();
    assert!((n.distance - 100.0).abs() < 2.0, "{n:?}");
}

#[test]
fn the_index_query_returns_the_nearest_outline_within_the_radius() {
    let paths = [square(0, 0, 10_000), square(12_000, 0, 10_000)];
    let index: HitIndex<usize> = HitIndex::from_entries(
        paths
            .iter()
            .enumerate()
            .map(|(i, p)| (i, p.bounds(), (i as u64) << 16)),
    );
    // Between the two: 1 000 from the first's right edge, 1 000 from the
    // second's left edge minus 200.
    let q = Point::raw(11_200, 5_000);
    let (k, n) = nearest_in_index(&index, q, Mp::new(2_000), |i| {
        nearest_point_transformed(&paths[i], Matrix::IDENTITY, q, Mp::new(2_000), 1e-3)
    })
    .unwrap();
    assert_eq!(k, 1);
    assert_eq!(n.point, Point::raw(12_000, 5_000));
    assert!(
        nearest_in_index(&index, Point::raw(50_000, 50_000), Mp::new(2_000), |_| None).is_none()
    );
}
