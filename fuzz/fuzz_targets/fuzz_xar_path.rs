//! One record payload, decoded as a path, both ways.
//!
//! The four things being hunted: the `% 9` check letting a bad size
//! through, the interleave reading past its eight bytes, the delta sign
//! wrapping an `i32`, and a coordinate escaping the document extent.

#![no_main]

use libfuzzer_sys::fuzz_target;
use xarast_geom::{Mp, Point};
use xarast_xar::{Cur, DiagSink, decode_absolute, decode_relative};

fuzz_target!(|data: &[u8]| {
    // A non-zero origin, so that the translation is exercised rather than
    // being a no-op the way it is for every real file.
    let origin = Point::raw(1_234, -5_678);
    let mut diags = DiagSink::new();

    let mut cur = Cur::new(data);
    if let Ok(path) = decode_relative(&mut cur, origin, &mut diags, (1, 116)) {
        path.validate().expect("a decoded path is well formed");
        for p in path.points() {
            assert!(p.x >= Mp::EXTENT_MIN && p.x <= Mp::EXTENT_MAX);
            assert!(p.y >= Mp::EXTENT_MIN && p.y <= Mp::EXTENT_MAX);
        }
        // The relative decoder consumes the whole payload or none of it.
        assert!(cur.remaining() == 0 || path.is_empty());
    }

    let mut cur = Cur::new(data);
    if let Ok(path) = decode_absolute(&mut cur, origin, &mut diags, (1, 100)) {
        path.validate().expect("a decoded path is well formed");
    }

    // Flags of an arbitrary length must never desynchronise the path.
    let mut cur = Cur::new(data);
    if let Ok(mut path) = decode_relative(&mut cur, origin, &mut diags, (1, 116)) {
        let points = path.points().len();
        xarast_xar::apply_path_flags(&mut path, data, &mut diags, (1, 111));
        assert_eq!(path.points().len(), points);
        path.validate().expect("flags did not break the path");
    }
});
